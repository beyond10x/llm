#![cfg(all(feature = "codex-renewal", unix))]
//! A Codex login renewed through its token endpoint and written back atomically, matching Harness
//! `harness-credential/src/renewal.rs` (C9-C22). Fixture files under `CARGO_TARGET_TMPDIR` and a
//! scripted token endpoint on 127.0.0.1 only: no test reads a real Codex login or contacts a real
//! token endpoint.
use llm_core::{Cancel, ErrorCode};
use llm_credentials::{
    SecretError, SecretRef, SecretResolver,
    codex::{
        CODEX_CLIENT_ID, CODEX_TOKEN_URL, CodexAuthFile, CodexRenewal, DEFAULT_RENEWAL_MARGIN,
        Renewal, RenewalRefusal,
    },
};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

/// The caller's clock in every case: 2026-10-03T00:00:00Z.
const NOW: u64 = 1_790_985_600;
/// The same instant as `/last_refresh` is written.
const NOW_RFC3339: &str = "2026-10-03T00:00:00Z";
const MARGIN: Duration = Duration::from_mins(15);
const CLIENT: &str = "fixture-client";
const OLD_REFRESH: &str = "fixture-refresh-token-one";
const NEW_REFRESH: &str = "fixture-refresh-token-two";
const OLD_ID: &str = "fixture-id-token-one";
const NEW_ID: &str = "fixture-id-token-two";
/// A marker only a failing answer body carries.
const ANSWER_MARKER: &str = "llm-fixture-token-endpoint-private-answer";

fn reference() -> SecretRef {
    SecretRef::new("codex-login").unwrap()
}

fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let mut group = [0u8; 3];
        group[..chunk.len()].copy_from_slice(chunk);
        let bits = (u32::from(group[0]) << 16) | (u32::from(group[1]) << 8) | u32::from(group[2]);
        for index in 0..=chunk.len() {
            out.push(char::from(
                ALPHABET[(bits >> (18 - 6 * index)) as usize & 63],
            ));
        }
    }
    out
}

/// An unsigned fixture JWT with `exp` seconds after the epoch; `subject` keeps tokens apart.
fn token(exp: i64, subject: &str) -> String {
    format!(
        "{}.{}.{}",
        base64url(br#"{"alg":"none","typ":"JWT"}"#),
        base64url(format!(r#"{{"exp":{exp},"sub":"{subject}"}}"#).as_bytes()),
        base64url(b"fixture-signature"),
    )
}

fn at(offset: i64) -> i64 {
    i64::try_from(NOW).unwrap() + offset
}

/// A login laid out the way its owner wrote it, not the way serde would: odd indents, a key order
/// that is not alphabetical, and keys no renewal reads. Every byte of it but the token values must
/// survive a renewal.
fn layout(id: &str, access: &str, refresh: &str, last_refresh: &str) -> String {
    format!(
        "{{\n    \"OPENAI_API_KEY\": null,\n  \"tokens\": {{\"id_token\": \"{id}\", \"access_token\":  \"{access}\",\n     \"refresh_token\": \"{refresh}\", \"account_id\": \"fixture-account\"}},\n  \"last_refresh\": \"{last_refresh}\",\n  \"zz_unknown\": [1, 2, {{\"keep\": true}}]\n}}\n"
    )
}

/// The same layout with a copy of the refresh token under a second key, which must not move.
fn repeated_layout(id: &str, access: &str, refresh: &str, last_refresh: &str) -> String {
    format!(
        "{{\"previous_refresh_token\": \"{OLD_REFRESH}\",\n\"tokens\": {{\"id_token\": \"{id}\", \"access_token\": \"{access}\", \"refresh_token\": \"{refresh}\"}},\n\"last_refresh\": \"{last_refresh}\"}}"
    )
}

/// A fresh fixture directory under this target's scratch space.
fn fixture_dir() -> tempfile::TempDir {
    tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap()
}

fn write_fixture(path: &Path, content: &str, mode: u32) {
    fs::write(path, content).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

/// Every name in the login's directory other than `auth.json`.
fn strays(path: &Path) -> Vec<String> {
    fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name != "auth.json")
        .collect()
}

fn resolver(path: &Path) -> CodexAuthFile {
    CodexAuthFile::new(reference(), path)
        .with_clock(|| SystemTime::UNIX_EPOCH + Duration::from_secs(NOW))
}

fn renewal(url: &str) -> CodexRenewal {
    CodexRenewal::new()
        .unwrap()
        .with_endpoint(url, CLIENT)
        .with_margin(MARGIN)
}
type Requests = Arc<Mutex<Vec<(String, Vec<u8>)>>>;
/// A scripted token endpoint on 127.0.0.1 that answers every connection with one status and
/// body, records each request, and runs `before_answer` once, after it has read the first request
/// and before it answers it.
struct Endpoint {
    url: String,
    requests: Arc<AtomicUsize>,
    received: Requests,
}

impl Endpoint {
    async fn start(
        status: &'static str,
        answer: String,
        before_answer: impl FnOnce() + Send + 'static,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/oauth/token", listener.local_addr().unwrap());
        let requests = Arc::new(AtomicUsize::new(0));
        let received = Arc::new(Mutex::new(Vec::new()));
        let (count, log) = (requests.clone(), received.clone());
        tokio::spawn(async move {
            let mut hook = Some(before_answer);
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                count.fetch_add(1, Ordering::SeqCst);
                let Some(request) = read_request(&mut socket).await else {
                    continue;
                };
                log.lock().unwrap().push(request);
                if let Some(hook) = hook.take() {
                    hook();
                }
                let head = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    answer.len()
                );
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(answer.as_bytes()).await;
            }
        });
        Self {
            url,
            requests,
            received,
        }
    }

    async fn answering(status: &'static str, answer: Value) -> Self {
        Self::start(status, answer.to_string(), || {}).await
    }

    fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }

    /// The one request's head and its body parsed as JSON.
    fn only_request(&self) -> (String, Value) {
        let received = self.received.lock().unwrap();
        assert_eq!(received.len(), 1, "expected exactly one request");
        let (head, body) = &received[0];
        (head.clone(), serde_json::from_slice(body).unwrap())
    }
}

async fn read_request(socket: &mut TcpStream) -> Option<(String, Vec<u8>)> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    let end = loop {
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break end;
        }
        let length = socket.read(&mut buffer).await.ok()?;
        if length == 0 {
            return None;
        }
        bytes.extend_from_slice(&buffer[..length]);
    };
    let head = String::from_utf8_lossy(&bytes[..end]).into_owned();
    let length: usize = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().ok())?
        })
        .unwrap_or(0);
    let mut body = bytes[end + 4..].to_vec();
    while body.len() < length {
        let read = socket.read(&mut buffer).await.ok()?;
        if read == 0 {
            return None;
        }
        body.extend_from_slice(&buffer[..read]);
    }
    Some((head, body))
}

/// No diagnostic carries a token or the endpoint's answer.
fn assert_no_secret(texts: &[String], secrets: &[&str]) {
    for text in texts {
        for secret in secrets {
            assert!(
                !text.contains(secret),
                "a diagnostic carries a secret: {text}"
            );
            for segment in secret.split('.').filter(|segment| segment.len() > 8) {
                assert!(
                    !text.contains(segment),
                    "a diagnostic carries a token segment: {text}"
                );
            }
        }
    }
}

/// C9, C10, C11, C12, C13: a due login is renewed by one refresh grant (client id, grant type and
/// the refresh token on disk, no `scope`), and only the token values and `/last_refresh` change
/// on disk; every other byte, the key order, the mode and the directory survive.
#[tokio::test]
async fn a_due_login_is_renewed_by_one_refresh_grant_and_written_back_in_place() {
    let dir = fixture_dir();
    let path = dir.path().join("auth.json");
    let (stale, fresh) = (token(at(60), "stale"), token(at(3600), "fresh"));
    let before = layout(OLD_ID, &stale, OLD_REFRESH, "2026-10-01T00:00:00Z");
    write_fixture(&path, &before, 0o600);
    let inode = fs::metadata(&path).unwrap().ino();
    let endpoint = Endpoint::answering(
        "200 OK",
        json!({"access_token": fresh, "refresh_token": NEW_REFRESH, "id_token": NEW_ID, "token_type": "Bearer"}),
    )
    .await;

    let outcome = resolver(&path)
        .renew(&renewal(&endpoint.url), &Cancel::new())
        .await
        .unwrap();

    let Renewal::Renewed(renewed) = outcome else {
        panic!("a due login was not renewed: {outcome:?}");
    };
    assert_eq!(renewed.expires_unix, Some(i128::from(at(3600))));
    assert!(renewed.refresh_token_rotated);

    // What went out.
    assert_eq!(endpoint.requests(), 1);
    let (head, request) = endpoint.only_request();
    assert!(head.starts_with("POST /oauth/token "), "{head}");
    assert!(
        head.lines()
            .any(|line| line.eq_ignore_ascii_case("content-type: application/json")),
        "{head}"
    );
    assert_eq!(
        request,
        json!({"client_id": CLIENT, "grant_type": "refresh_token", "refresh_token": OLD_REFRESH}),
        "exactly the grant, and no scope narrowing what was granted"
    );

    // What is on disk: the four values moved, nothing else did.
    let after = fs::read_to_string(&path).unwrap();
    assert_eq!(after, layout(NEW_ID, &fresh, NEW_REFRESH, NOW_RFC3339));
    let metadata = fs::metadata(&path).unwrap();
    assert_eq!(metadata.permissions().mode() & 0o7777, 0o600);
    assert_ne!(
        metadata.ino(),
        inode,
        "written in place rather than renamed over"
    );
    assert_eq!(strays(&path), Vec::<String>::new());

    // The renewed login now resolves through the read-only resolver.
    let resolved = resolver(&path).resolve(&reference()).await.unwrap();
    assert_eq!(resolved.secret.expose(), fresh.as_bytes());
}

/// C13: a value that also appears under another key is replaced at its own pointer only, so the
/// write stays byte-preserving where a textual search would have refused or reordered the file.
#[tokio::test]
async fn a_repeated_value_is_replaced_at_its_pointer_and_nowhere_else() {
    let dir = fixture_dir();
    let path = dir.path().join("auth.json");
    let (stale, fresh) = (token(at(60), "stale"), token(at(3600), "fresh"));
    let before = repeated_layout(OLD_ID, &stale, OLD_REFRESH, "2026-10-01T00:00:00Z");
    write_fixture(&path, &before, 0o600);
    let endpoint = Endpoint::answering(
        "200 OK",
        json!({"access_token": fresh, "refresh_token": NEW_REFRESH, "id_token": NEW_ID}),
    )
    .await;
    let outcome = resolver(&path)
        .renew(&renewal(&endpoint.url), &Cancel::new())
        .await
        .unwrap();
    assert!(matches!(outcome, Renewal::Renewed(_)), "{outcome:?}");
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        repeated_layout(NEW_ID, &fresh, NEW_REFRESH, NOW_RFC3339)
    );
}

/// C12: a group-readable login keeps its mode; the new document is a new file renamed over the
/// old one, and nothing is left beside it.
#[tokio::test]
async fn a_group_readable_login_keeps_its_mode_and_is_replaced_by_rename() {
    let dir = fixture_dir();
    let path = dir.path().join("auth.json");
    let (stale, fresh) = (token(at(60), "stale"), token(at(3600), "fresh"));
    write_fixture(
        &path,
        &layout(OLD_ID, &stale, OLD_REFRESH, "2026-10-01T00:00:00Z"),
        0o640,
    );
    let inode = fs::metadata(&path).unwrap().ino();
    let endpoint = Endpoint::answering("200 OK", json!({"access_token": fresh})).await;
    let outcome = resolver(&path)
        .renew(&renewal(&endpoint.url), &Cancel::new())
        .await
        .unwrap();
    assert!(matches!(outcome, Renewal::Renewed(_)), "{outcome:?}");
    let metadata = fs::metadata(&path).unwrap();
    assert_eq!(metadata.permissions().mode() & 0o7777, 0o640);
    assert_ne!(metadata.ino(), inode);
    assert_eq!(strays(&path), Vec::<String>::new());
}

/// C11: a refresh token the endpoint repeats, or does not return, is not reported rotated and stays
/// on disk; an id token the answer omits stays as it was.
#[tokio::test]
async fn a_kept_or_absent_refresh_token_is_not_reported_rotated() {
    for answer_refresh in [Some(OLD_REFRESH), None] {
        let dir = fixture_dir();
        let path = dir.path().join("auth.json");
        let (stale, fresh) = (token(at(60), "stale"), token(at(3600), "fresh"));
        write_fixture(
            &path,
            &layout(OLD_ID, &stale, OLD_REFRESH, "2026-10-01T00:00:00Z"),
            0o600,
        );
        let mut answer = json!({"access_token": fresh});
        if let Some(refresh) = answer_refresh {
            answer["refresh_token"] = json!(refresh);
        }
        let endpoint = Endpoint::answering("200 OK", answer).await;
        let outcome = resolver(&path)
            .renew(&renewal(&endpoint.url), &Cancel::new())
            .await
            .unwrap();
        let Renewal::Renewed(renewed) = outcome else {
            panic!("not renewed: {outcome:?}");
        };
        assert!(!renewed.refresh_token_rotated, "{answer_refresh:?}");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            layout(OLD_ID, &fresh, OLD_REFRESH, NOW_RFC3339),
            "{answer_refresh:?}"
        );
    }
}

/// C19: renewal is due when `exp` is at or before the clock plus the margin, and not one second
/// later; the default margin is Harness's fifteen minutes. A login that is not due is neither sent
/// nor written.
#[tokio::test]
async fn renewal_is_due_inside_the_margin_and_not_one_second_outside_it() {
    assert_eq!(DEFAULT_RENEWAL_MARGIN, Duration::from_mins(15));
    assert_eq!(
        CodexRenewal::new().unwrap().margin(),
        DEFAULT_RENEWAL_MARGIN
    );

    let dir = fixture_dir();
    let path = dir.path().join("auth.json");
    let fresh = token(at(7200), "fresh");
    let endpoint = Endpoint::answering("200 OK", json!({"access_token": fresh})).await;
    let renewal = renewal(&endpoint.url);

    let outside = layout(
        OLD_ID,
        &token(at(901), "outside"),
        OLD_REFRESH,
        "2026-10-01T00:00:00Z",
    );
    write_fixture(&path, &outside, 0o600);
    let outcome = resolver(&path)
        .renew(&renewal, &Cancel::new())
        .await
        .unwrap();
    assert!(matches!(outcome, Renewal::NotDue), "{outcome:?}");
    assert_eq!(
        endpoint.requests(),
        0,
        "a login outside the margin was sent"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), outside);

    let mut expected_requests = 0;
    for offset in [900, 899, 0, -60] {
        let stale = token(at(offset), "stale");
        write_fixture(
            &path,
            &layout(OLD_ID, &stale, OLD_REFRESH, "2026-10-01T00:00:00Z"),
            0o600,
        );
        let outcome = resolver(&path)
            .renew(&renewal, &Cancel::new())
            .await
            .unwrap();
        expected_requests += 1;
        assert!(
            matches!(outcome, Renewal::Renewed(_)),
            "{offset}: {outcome:?}"
        );
        assert_eq!(endpoint.requests(), expected_requests, "{offset}");
    }
}

/// C18: an access token whose `exp` cannot be read is left alone, so no refresh token is spent on
/// a credential nobody can date, and it is still not sent: llm's read rule refuses it as
/// `Malformed`.
#[tokio::test]
async fn an_undated_access_token_is_left_alone_and_still_not_sent() {
    let dir = fixture_dir();
    let path = dir.path().join("auth.json");
    let endpoint =
        Endpoint::answering("200 OK", json!({"access_token": token(at(3600), "fresh")})).await;
    let undated = [
        "fixture-opaque-access-token".to_owned(),
        "a.b.c".to_owned(),
        format!("h.{}.s", base64url(br#"{"exp":1.5}"#)),
        format!("h.{}.s", base64url(br#"{"sub":"no-exp"}"#)),
    ];
    for access in &undated {
        let before = layout(OLD_ID, access, OLD_REFRESH, "2026-10-01T00:00:00Z");
        write_fixture(&path, &before, 0o600);
        let outcome = resolver(&path)
            .renew(&renewal(&endpoint.url), &Cancel::new())
            .await
            .unwrap();
        assert!(matches!(outcome, Renewal::Undated), "{access}: {outcome:?}");
        let renewing = resolver(&path).renewing(renewal(&endpoint.url));
        assert_eq!(
            renewing.resolve(&reference()).await.unwrap_err(),
            SecretError::Malformed,
            "{access}"
        );
        assert_eq!(endpoint.requests(), 0, "{access}");
        assert_eq!(fs::read_to_string(&path).unwrap(), before, "{access}");
    }
}

/// C14: a login rewritten while the refresh request is in flight is not overwritten; the renewal
/// refuses and its outcome is uncertain, because the endpoint may have retired the refresh token.
#[tokio::test]
async fn a_login_changed_during_renewal_is_not_overwritten() {
    let dir = fixture_dir();
    let path = dir.path().join("auth.json");
    let stale = token(at(60), "stale");
    write_fixture(
        &path,
        &layout(OLD_ID, &stale, OLD_REFRESH, "2026-10-01T00:00:00Z"),
        0o600,
    );
    let concurrent = layout(
        "concurrent-id",
        &token(at(7200), "concurrent"),
        "concurrent-refresh",
        "2026-10-02T23:59:00Z",
    );
    let (target, bytes) = (path.clone(), concurrent.clone());
    let endpoint = Endpoint::start(
        "200 OK",
        json!({"access_token": token(at(3600), "fresh"), "refresh_token": NEW_REFRESH}).to_string(),
        move || fs::write(target, bytes).unwrap(),
    )
    .await;
    let error = resolver(&path)
        .renew(&renewal(&endpoint.url), &Cancel::new())
        .await
        .unwrap_err();
    assert_eq!(
        error.refusal(),
        RenewalRefusal::ChangedDuringRenewal,
        "{error}"
    );
    assert_eq!(error.refusal().code(), "changed-during-renewal");
    assert_eq!(error.kind(), SecretError::RefreshUncertain);
    assert_eq!(endpoint.requests(), 1);
    assert_eq!(fs::read_to_string(&path).unwrap(), concurrent);
    assert_eq!(strays(&path), Vec::<String>::new());
}

/// C15 with H33: a refused exchange quotes neither the endpoint's answer nor the refresh token,
/// writes nothing, and is sent once whatever its status: a 4xx is `refresh-rejected`, and a
/// failure after which the endpoint may have issued tokens is `refresh-uncertain`.
#[tokio::test]
async fn a_refused_exchange_quotes_neither_its_answer_nor_the_refresh_token() {
    for (status, code, kind) in [
        (
            "400 Bad Request",
            ErrorCode::Refused,
            SecretError::RefreshRejected,
        ),
        (
            "401 Unauthorized",
            ErrorCode::Unauthorized,
            SecretError::RefreshRejected,
        ),
        (
            "503 Service Unavailable",
            ErrorCode::Transport,
            SecretError::RefreshUncertain,
        ),
    ] {
        let dir = fixture_dir();
        let path = dir.path().join("auth.json");
        let stale = token(at(60), "stale");
        let before = layout(OLD_ID, &stale, OLD_REFRESH, "2026-10-01T00:00:00Z");
        write_fixture(&path, &before, 0o600);
        let endpoint =
            Endpoint::start(status, format!(r#"{{"error":"{ANSWER_MARKER}"}}"#), || {}).await;
        let error = resolver(&path)
            .renew(&renewal(&endpoint.url), &Cancel::new())
            .await
            .unwrap_err();
        assert_eq!(
            error.refusal(),
            RenewalRefusal::ExchangeFailed,
            "{status}: {error}"
        );
        let exchange = error.exchange().expect("the exchange's own refusal");
        assert_eq!(exchange.code, code, "{status}");
        assert!(!exchange.retriable, "{status}");
        assert_eq!(error.kind(), kind, "{status}");
        assert_eq!(endpoint.requests(), 1, "{status} was sent again");
        assert_eq!(fs::read_to_string(&path).unwrap(), before, "{status}");
        assert_eq!(strays(&path), Vec::<String>::new());
        assert!(error.to_string().contains(&status[..3]), "{error}");
        assert!(
            error.to_string().contains(&path.display().to_string()),
            "{error}"
        );
        assert_no_secret(
            &[error.to_string(), format!("{error:?}")],
            &[ANSWER_MARKER, OLD_REFRESH, &stale, OLD_ID],
        );
    }
}

/// C16: an answer with an empty or missing access token, or an empty or non-string refresh or id
/// token, refuses before anything is written.
#[tokio::test]
async fn an_empty_or_non_string_returned_token_refuses_before_writing() {
    let fresh = token(at(3600), "fresh");
    let cases = [
        (
            json!({"access_token": ""}),
            RenewalRefusal::AnswerWithoutAccessToken,
        ),
        (
            json!({"token_type": "Bearer"}),
            RenewalRefusal::AnswerWithoutAccessToken,
        ),
        (
            json!({"access_token": 42}),
            RenewalRefusal::AnswerWithoutAccessToken,
        ),
        (
            json!({"access_token": fresh, "refresh_token": ""}),
            RenewalRefusal::AnswerInvalidRefreshToken,
        ),
        (
            json!({"access_token": fresh, "refresh_token": 7}),
            RenewalRefusal::AnswerInvalidRefreshToken,
        ),
        (
            json!({"access_token": fresh, "id_token": 42}),
            RenewalRefusal::AnswerInvalidIdToken,
        ),
        (
            json!({"access_token": fresh, "id_token": ""}),
            RenewalRefusal::AnswerInvalidIdToken,
        ),
    ];
    for (answer, refusal) in cases {
        let dir = fixture_dir();
        let path = dir.path().join("auth.json");
        let before = layout(
            OLD_ID,
            &token(at(60), "stale"),
            OLD_REFRESH,
            "2026-10-01T00:00:00Z",
        );
        write_fixture(&path, &before, 0o600);
        let endpoint = Endpoint::answering("200 OK", answer.clone()).await;
        let error = resolver(&path)
            .renew(&renewal(&endpoint.url), &Cancel::new())
            .await
            .unwrap_err();
        assert_eq!(error.refusal(), refusal, "{answer}");
        assert_eq!(error.kind(), SecretError::RefreshUncertain, "{answer}");
        assert_eq!(fs::read_to_string(&path).unwrap(), before, "{answer}");
        assert_eq!(strays(&path), Vec::<String>::new(), "{answer}");
        assert_no_secret(
            &[error.to_string(), format!("{error:?}")],
            &[&fresh, OLD_REFRESH],
        );
    }
}

/// C22: a due login with nothing usable at `/tokens/refresh_token` refuses, naming that pointer,
/// before anything is sent.
#[tokio::test]
async fn a_due_login_without_a_refresh_token_refuses_naming_its_pointer_before_sending() {
    let endpoint =
        Endpoint::answering("200 OK", json!({"access_token": token(at(3600), "fresh")})).await;
    let stale = token(at(60), "stale");
    let documents = [
        json!({"tokens": {"access_token": stale}}).to_string(),
        json!({"tokens": {"access_token": stale, "refresh_token": ""}}).to_string(),
        json!({"tokens": {"access_token": stale, "refresh_token": 42}}).to_string(),
    ];
    for before in documents {
        let dir = fixture_dir();
        let path = dir.path().join("auth.json");
        write_fixture(&path, &before, 0o600);
        let error = resolver(&path)
            .renew(&renewal(&endpoint.url), &Cancel::new())
            .await
            .unwrap_err();
        assert_eq!(error.refusal(), RenewalRefusal::NoRefreshToken, "{before}");
        assert_eq!(error.refusal().code(), "no-refresh-token");
        assert_eq!(error.kind(), SecretError::Missing);
        assert!(
            error.to_string().contains("/tokens/refresh_token"),
            "{error}"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
        assert_no_secret(&[error.to_string(), format!("{error:?}")], &[&stale]);
    }
    assert_eq!(
        endpoint.requests(),
        0,
        "a login without a refresh token was sent"
    );
}

/// Renewal replaces a file, so a symlink at the path is refused rather than replaced by a file,
/// before anything is sent.
#[tokio::test]
async fn a_symlinked_login_is_refused_before_sending() {
    let dir = fixture_dir();
    let target = dir.path().join("elsewhere.json");
    let path = dir.path().join("auth.json");
    let before = layout(
        OLD_ID,
        &token(at(60), "stale"),
        OLD_REFRESH,
        "2026-10-01T00:00:00Z",
    );
    write_fixture(&target, &before, 0o600);
    symlink(&target, &path).unwrap();
    let endpoint =
        Endpoint::answering("200 OK", json!({"access_token": token(at(3600), "fresh")})).await;
    let error = resolver(&path)
        .renew(&renewal(&endpoint.url), &Cancel::new())
        .await
        .unwrap_err();
    assert_eq!(error.refusal(), RenewalRefusal::NotARegularFile, "{error}");
    assert_eq!(endpoint.requests(), 0);
    assert!(
        fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), before);
}

/// C20: the renewing resolver renews a due login before answering, answers the token the endpoint
/// issued, and does not renew again while the new token is outside the margin. The read-only
/// resolver stays available and never renews.
#[tokio::test]
async fn the_renewing_resolver_renews_a_due_login_and_answers_the_new_token() {
    let dir = fixture_dir();
    let path = dir.path().join("auth.json");
    let (expired, fresh) = (token(at(-60), "expired"), token(at(3600), "fresh"));
    let before = layout(OLD_ID, &expired, OLD_REFRESH, "2026-10-01T00:00:00Z");
    write_fixture(&path, &before, 0o600);
    let endpoint = Endpoint::answering(
        "200 OK",
        json!({"access_token": fresh, "refresh_token": NEW_REFRESH}),
    )
    .await;

    // Read-only: refused as expired, nothing sent, nothing written.
    let read_only = resolver(&path);
    assert_eq!(
        read_only.resolve(&reference()).await.unwrap_err(),
        SecretError::Expired
    );
    assert_eq!(endpoint.requests(), 0);
    assert_eq!(fs::read_to_string(&path).unwrap(), before);

    let renewing = resolver(&path).renewing(renewal(&endpoint.url));
    let first = renewing.resolve(&reference()).await.unwrap();
    assert_eq!(first.secret.expose(), fresh.as_bytes());
    assert_eq!(endpoint.requests(), 1);
    let second = renewing.resolve(&reference()).await.unwrap();
    assert_eq!(second.secret.expose(), fresh.as_bytes());
    assert_eq!(second.version, first.version);
    assert_eq!(endpoint.requests(), 1, "a fresh login was renewed again");
    assert_no_secret(
        &[format!("{renewing:?}")],
        &[&fresh, NEW_REFRESH, OLD_REFRESH],
    );
}

/// C20: concurrent resolves through one renewing resolver renew once.
#[tokio::test]
async fn concurrent_resolves_through_one_renewing_resolver_renew_once() {
    let dir = fixture_dir();
    let path = dir.path().join("auth.json");
    let fresh = token(at(3600), "fresh");
    write_fixture(
        &path,
        &layout(
            OLD_ID,
            &token(at(-60), "expired"),
            OLD_REFRESH,
            "2026-10-01T00:00:00Z",
        ),
        0o600,
    );
    let endpoint = Endpoint::answering(
        "200 OK",
        json!({"access_token": fresh, "refresh_token": NEW_REFRESH}),
    )
    .await;
    let renewing = Arc::new(resolver(&path).renewing(renewal(&endpoint.url)));
    let reference = reference();
    let (one, two) = tokio::join!(renewing.resolve(&reference), renewing.resolve(&reference));
    assert_eq!(one.unwrap().secret.expose(), fresh.as_bytes());
    assert_eq!(two.unwrap().secret.expose(), fresh.as_bytes());
    assert_eq!(endpoint.requests(), 1);
}

/// The endpoint is configuration with a default: Codex's own token endpoint and public client id
/// (Harness `harness-cli/src/provider.rs:224`-`230`). Every test overrides it; none contacts it.
#[test]
fn the_default_endpoint_is_codexs_own_and_is_overridable() {
    assert_eq!(CODEX_TOKEN_URL, "https://auth.openai.com/oauth/token");
    assert_eq!(CODEX_CLIENT_ID, "app_EMoamEEZ73f0CkXaXp7hrann");
    let default = CodexRenewal::new().unwrap();
    assert_eq!(default.url(), CODEX_TOKEN_URL);
    assert_eq!(default.client_id(), CODEX_CLIENT_ID);
    let overridden = default.with_endpoint("http://127.0.0.1:9/oauth/token", CLIENT);
    assert_eq!(overridden.url(), "http://127.0.0.1:9/oauth/token");
    assert_eq!(overridden.client_id(), CLIENT);
}

/// No Display or Debug on the renewal path carries a token: the configuration, the outcome, the
/// renewing resolver and every refusal.
#[tokio::test]
async fn no_renewal_diagnostic_carries_a_token() {
    let dir = fixture_dir();
    let path = dir.path().join("auth.json");
    let (stale, fresh) = (token(at(60), "stale"), token(at(3600), "fresh"));
    write_fixture(
        &path,
        &layout(OLD_ID, &stale, OLD_REFRESH, "2026-10-01T00:00:00Z"),
        0o600,
    );
    let endpoint = Endpoint::answering(
        "200 OK",
        json!({"access_token": fresh, "refresh_token": NEW_REFRESH, "id_token": NEW_ID}),
    )
    .await;
    let renewal = renewal(&endpoint.url);
    let outcome = resolver(&path)
        .renew(&renewal, &Cancel::new())
        .await
        .unwrap();
    let renewing = resolver(&path).renewing(renewal.clone());
    let refusal = resolver(&PathBuf::from("relative/auth.json"))
        .renew(&renewal, &Cancel::new())
        .await
        .unwrap_err();
    assert_eq!(refusal.refusal(), RenewalRefusal::NotAbsolute);
    assert_no_secret(
        &[
            format!("{renewal:?}"),
            format!("{outcome:?}"),
            format!("{renewing:?}"),
            refusal.to_string(),
            format!("{refusal:?}"),
        ],
        &[&stale, &fresh, OLD_REFRESH, NEW_REFRESH, OLD_ID, NEW_ID],
    );
}

/// F6: a login with a second name (a hard link) is refused before anything is sent, as the `file`
/// adapter refuses one: the rename would split it and leave the other name holding a refresh token
/// the endpoint retired.
#[tokio::test]
async fn a_hard_linked_login_is_refused_before_sending() {
    let dir = fixture_dir();
    let path = dir.path().join("auth.json");
    let before = layout(
        OLD_ID,
        &token(at(60), "stale"),
        OLD_REFRESH,
        "2026-10-01T00:00:00Z",
    );
    write_fixture(&path, &before, 0o600);
    fs::hard_link(&path, dir.path().join("second-name.json")).unwrap();
    let endpoint =
        Endpoint::answering("200 OK", json!({"access_token": token(at(3600), "fresh")})).await;
    let error = resolver(&path)
        .renew(&renewal(&endpoint.url), &Cancel::new())
        .await
        .unwrap_err();
    assert_eq!(error.refusal(), RenewalRefusal::MultipleLinks, "{error}");
    assert_eq!(error.refusal().code(), "multiple-links");
    assert_eq!(error.kind(), SecretError::UnsafeSource);
    assert_eq!(endpoint.requests(), 0);
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
}

/// F1: a grant the endpoint refused is bound to the bytes it was read from, and a changed file
/// lifts it: after the owner logs in again, the renewing resolver renews the new login.
#[tokio::test]
async fn a_refused_grant_is_lifted_when_the_login_changes() {
    let dir = fixture_dir();
    let path = dir.path().join("auth.json");
    let refused = Endpoint::answering("400 Bad Request", json!({"error": "invalid_grant"})).await;
    write_fixture(
        &path,
        &layout(
            OLD_ID,
            &token(at(-60), "expired"),
            OLD_REFRESH,
            "2026-10-01T00:00:00Z",
        ),
        0o600,
    );
    let renewing = resolver(&path).renewing(renewal(&refused.url));
    for _ in 0..3 {
        assert_eq!(
            renewing.resolve(&reference()).await.unwrap_err(),
            SecretError::RefreshRejected
        );
    }
    assert_eq!(refused.requests(), 1, "a refused grant was presented again");

    // The owner logs in again: a new refresh token, still expired, is presented once.
    write_fixture(
        &path,
        &layout(
            OLD_ID,
            &token(at(-30), "relogged"),
            NEW_REFRESH,
            "2026-10-02T00:00:00Z",
        ),
        0o600,
    );
    assert_eq!(
        renewing.resolve(&reference()).await.unwrap_err(),
        SecretError::RefreshRejected
    );
    assert_eq!(refused.requests(), 2, "a changed login was not renewed");
}
