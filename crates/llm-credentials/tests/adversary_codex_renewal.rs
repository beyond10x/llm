#![cfg(all(feature = "codex-renewal", unix))]
//! Adversary pass 1, wave 2026-10-05-w31: attacks on the opt-in Codex login renewal. Fixture files
//! under `CARGO_TARGET_TMPDIR` and a scripted token endpoint on 127.0.0.1 only; no test reads a
//! real Codex login or contacts a real token endpoint.
use llm_core::Cancel;
use llm_credentials::{
    SecretError, SecretRef, SecretResolver,
    codex::{CodexAuthFile, CodexRenewal, Renewal, RenewalRefusal},
};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
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

const NOW: u64 = 1_790_985_600;
const MARGIN: Duration = Duration::from_mins(15);
const CLIENT: &str = "fixture-client";
const OLD_REFRESH: &str = "fixture-refresh-token-one";
const NEW_REFRESH: &str = "fixture-refresh-token-two";

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

fn token_with_exp(exp: &str, subject: &str) -> String {
    format!(
        "{}.{}.{}",
        base64url(br#"{"alg":"none","typ":"JWT"}"#),
        base64url(format!(r#"{{"exp":{exp},"sub":"{subject}"}}"#).as_bytes()),
        base64url(b"fixture-signature"),
    )
}

fn token(offset: i64, subject: &str) -> String {
    token_with_exp(&(i64::try_from(NOW).unwrap() + offset).to_string(), subject)
}

fn fixture_dir() -> tempfile::TempDir {
    tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap()
}

fn write_fixture(path: &Path, content: &str, mode: u32) {
    fs::write(path, content).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

fn names(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
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

fn login(access: &str, refresh: &str) -> String {
    format!(
        "{{\"tokens\": {{\"access_token\": \"{access}\", \"refresh_token\": \"{refresh}\"}}, \"last_refresh\": \"2026-10-01T00:00:00Z\"}}\n"
    )
}

/// What the scripted endpoint does with the n-th connection.
#[derive(Clone)]
enum Act {
    Answer(&'static str, String),
    /// Reads the request, then holds the connection open without answering.
    Hang,
}

struct Endpoint {
    url: String,
    requests: Arc<AtomicUsize>,
    bodies: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl Endpoint {
    /// Connection `n` gets `script[n]`, or the last entry once the script runs out.
    async fn scripted(script: Vec<Act>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/oauth/token", listener.local_addr().unwrap());
        let requests = Arc::new(AtomicUsize::new(0));
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let (count, log) = (requests.clone(), bodies.clone());
        tokio::spawn(async move {
            let mut index = 0usize;
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let act = script[index.min(script.len() - 1)].clone();
                index += 1;
                let Some(body) = read_request(&mut socket).await else {
                    continue;
                };
                count.fetch_add(1, Ordering::SeqCst);
                log.lock().unwrap().push(body);
                match act {
                    Act::Answer(status, answer) => {
                        let head = format!(
                            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                            answer.len()
                        );
                        let _ = socket.write_all(head.as_bytes()).await;
                        let _ = socket.write_all(answer.as_bytes()).await;
                    }
                    Act::Hang => {
                        tokio::spawn(async move {
                            tokio::time::sleep(Duration::from_secs(60)).await;
                            drop(socket);
                        });
                    }
                }
            }
        });
        Self {
            url,
            requests,
            bodies,
        }
    }

    async fn answering(status: &'static str, answer: &Value) -> Self {
        Self::scripted(vec![Act::Answer(status, answer.to_string())]).await
    }

    async fn raw(status: &'static str, answer: String) -> Self {
        Self::scripted(vec![Act::Answer(status, answer)]).await
    }

    fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }

    fn body(&self, index: usize) -> Value {
        serde_json::from_slice(&self.bodies.lock().unwrap()[index]).unwrap()
    }
}

async fn read_request(socket: &mut TcpStream) -> Option<Vec<u8>> {
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
    Some(body)
}

// ---------------------------------------------------------------------------------------------
// Red: a renewing resolver repeats an authorization-server mutation the crate's own coordinated
// refresh rule (`llm-credentials/src/lib.rs` `CoordinatedResolver::refresh`) says must not be
// repeated on the same generation.
// ---------------------------------------------------------------------------------------------

/// A refresh grant the endpoint refused (400, `RefreshRejected`) is not sent again by the next
/// resolve of the same, unchanged login: nothing on disk changed, so the same refresh token would
/// be presented to the endpoint again on every resolve.
#[tokio::test]
async fn a_refused_grant_is_not_sent_again_by_the_next_resolve_of_the_same_login() {
    let dir = fixture_dir();
    let path = dir.path().join("auth.json");
    let before = login(&token(-60, "expired"), OLD_REFRESH);
    write_fixture(&path, &before, 0o600);
    let endpoint = Endpoint::answering("400 Bad Request", &json!({"error": "invalid_grant"})).await;
    let renewing = resolver(&path).renewing(renewal(&endpoint.url));

    let first = renewing.resolve(&reference()).await.unwrap_err();
    assert_eq!(first, SecretError::RefreshRejected);
    assert_eq!(endpoint.requests(), 1);
    assert_eq!(fs::read_to_string(&path).unwrap(), before);

    let second = renewing.resolve(&reference()).await.unwrap_err();
    assert_eq!(second, SecretError::RefreshRejected);
    assert_eq!(
        endpoint.requests(),
        1,
        "the refused refresh token was presented to the endpoint again by the next resolve"
    );
}

/// A resolve dropped while its refresh grant is in flight (a caller's timeout) leaves the outcome
/// uncertain: the endpoint may have issued and rotated tokens. The next resolve of the same,
/// unchanged login does not blindly present the same refresh token again; it refuses
/// `RefreshUncertain` until the file changes, as `CoordinatedResolver::refresh` does for a
/// dropped refresh.
#[tokio::test]
async fn a_dropped_resolve_does_not_let_the_next_resolve_resend_the_grant() {
    let dir = fixture_dir();
    let path = dir.path().join("auth.json");
    write_fixture(&path, &login(&token(-60, "expired"), OLD_REFRESH), 0o600);
    let endpoint = Endpoint::scripted(vec![
        Act::Hang,
        Act::Answer(
            "200 OK",
            json!({"access_token": token(3600, "fresh"), "refresh_token": NEW_REFRESH}).to_string(),
        ),
    ])
    .await;
    let renewing = resolver(&path).renewing(renewal(&endpoint.url));

    let dropped =
        tokio::time::timeout(Duration::from_millis(500), renewing.resolve(&reference())).await;
    assert!(dropped.is_err(), "the first resolve was expected to hang");
    assert_eq!(
        endpoint.requests(),
        1,
        "the first grant reached the endpoint"
    );

    let second = renewing.resolve(&reference()).await;
    assert_eq!(
        endpoint.requests(),
        1,
        "the next resolve presented the same refresh token again after an uncertain grant \
         (second resolve: {second:?})"
    );
    assert_eq!(second.unwrap_err(), SecretError::RefreshUncertain);
}

// ---------------------------------------------------------------------------------------------
// Probes expected green: what the attack could not break.
// ---------------------------------------------------------------------------------------------

/// Returned tokens holding quotes, backslashes, a control character, U+2028 and non-ASCII are
/// written as valid JSON with exactly those values, and every byte outside the token spans
/// survives.
#[tokio::test]
async fn hostile_token_characters_are_written_as_valid_json_with_the_right_values() {
    let dir = fixture_dir();
    let path = dir.path().join("auth.json");
    let stale = token(-60, "expired");
    let before = login(&stale, OLD_REFRESH);
    write_fixture(&path, &before, 0o600);
    let access = format!("{}\"\\\u{1}\u{2028}Ã©", token(3600, "fresh"));
    let refresh = "r\"\\n\u{7f}\u{1F600}";
    let endpoint = Endpoint::answering(
        "200 OK",
        &json!({"access_token": access, "refresh_token": refresh, "scope": "x", "expires_in": 3600}),
    )
    .await;
    let outcome = resolver(&path)
        .renew(&renewal(&endpoint.url), &Cancel::new())
        .await
        .unwrap();
    assert!(matches!(outcome, Renewal::Renewed(_)), "{outcome:?}");
    let after = fs::read_to_string(&path).unwrap();
    let parsed: Value = serde_json::from_str(&after).unwrap();
    assert_eq!(parsed["tokens"]["access_token"], json!(access));
    assert_eq!(parsed["tokens"]["refresh_token"], json!(refresh));
    assert!(
        after.starts_with("{\"tokens\": {\"access_token\": \""),
        "{after}"
    );
    assert!(
        after.ends_with("\"}, \"last_refresh\": \"2026-10-03T00:00:00Z\"}\n"),
        "{after}"
    );
}

/// Values and keys stored with JSON escapes on disk: the refresh token is sent unescaped, and
/// the escaped key names are still found and spliced at their own positions.
#[tokio::test]
async fn escaped_keys_and_values_on_disk_are_found_and_sent_unescaped() {
    let dir = fixture_dir();
    let path = dir.path().join("auth.json");
    let stale = token(-60, "expired");
    let before = format!(
        "{{\"tok\\u0065ns\": {{\"access_\\u0074oken\": \"{stale}\", \"refresh_token\": \"fixture\\/refresh\\u002done\"}}, \"keep\": \"\\u00e9\"}}"
    );
    write_fixture(&path, &before, 0o600);
    let fresh = token(3600, "fresh");
    let endpoint = Endpoint::answering("200 OK", &json!({"access_token": fresh})).await;
    let outcome = resolver(&path)
        .renew(&renewal(&endpoint.url), &Cancel::new())
        .await
        .unwrap();
    assert!(matches!(outcome, Renewal::Renewed(_)), "{outcome:?}");
    assert_eq!(
        endpoint.body(0)["refresh_token"],
        json!("fixture/refresh-one")
    );
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        before.replace(&stale, &fresh)
    );
}

/// A symlinked parent directory: the new file is created beside the real one and renamed over
/// it; nothing is left in either directory.
#[tokio::test]
async fn a_symlinked_parent_directory_is_renewed_in_the_real_directory() {
    let dir = fixture_dir();
    let real = dir.path().join("real");
    fs::create_dir(&real).unwrap();
    let linked = dir.path().join("linked");
    symlink(&real, &linked).unwrap();
    write_fixture(
        &real.join("auth.json"),
        &login(&token(-60, "expired"), OLD_REFRESH),
        0o600,
    );
    let fresh = token(3600, "fresh");
    let endpoint = Endpoint::answering("200 OK", &json!({"access_token": fresh})).await;
    let outcome = resolver(&linked.join("auth.json"))
        .renew(&renewal(&endpoint.url), &Cancel::new())
        .await
        .unwrap();
    assert!(matches!(outcome, Renewal::Renewed(_)), "{outcome:?}");
    assert_eq!(names(&real), vec!["auth.json".to_owned()]);
    assert!(
        fs::symlink_metadata(&linked)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        fs::read_to_string(real.join("auth.json"))
            .unwrap()
            .contains(&fresh)
    );
}

/// Owner-read-only and setuid modes are kept exactly, never widened.
#[tokio::test]
async fn unusual_modes_are_kept_exactly() {
    for mode in [0o400, 0o4600, 0o640] {
        let dir = fixture_dir();
        let path = dir.path().join("auth.json");
        write_fixture(&path, &login(&token(-60, "expired"), OLD_REFRESH), mode);
        let endpoint =
            Endpoint::answering("200 OK", &json!({"access_token": token(3600, "fresh")})).await;
        resolver(&path)
            .renew(&renewal(&endpoint.url), &Cancel::new())
            .await
            .unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o7777,
            mode,
            "{mode:o}"
        );
    }
}

/// A directory that cannot take a new file: the renewal refuses `WriteFailed` as uncertain, the
/// file is unchanged and no file is left behind.
#[tokio::test]
async fn an_unwritable_directory_refuses_uncertain_and_leaves_nothing_behind() {
    let dir = fixture_dir();
    let inner = dir.path().join("inner");
    fs::create_dir(&inner).unwrap();
    let path = inner.join("auth.json");
    let before = login(&token(-60, "expired"), OLD_REFRESH);
    write_fixture(&path, &before, 0o600);
    fs::set_permissions(&inner, fs::Permissions::from_mode(0o500)).unwrap();
    let endpoint =
        Endpoint::answering("200 OK", &json!({"access_token": token(3600, "fresh")})).await;
    let result = resolver(&path)
        .renew(&renewal(&endpoint.url), &Cancel::new())
        .await;
    fs::set_permissions(&inner, fs::Permissions::from_mode(0o700)).unwrap();
    let error = result.unwrap_err();
    assert_eq!(error.refusal(), RenewalRefusal::WriteFailed);
    assert_eq!(error.kind(), SecretError::RefreshUncertain);
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
    assert_eq!(names(&inner), vec!["auth.json".to_owned()]);
}

/// A JSON answer that is not an object, and an answer over the bound, refuse before writing and
/// quote nothing of the answer.
#[tokio::test]
async fn odd_answers_refuse_before_writing_and_quote_nothing() {
    let marker = "llm-adversary-answer-marker";
    let big = format!("{{\"access_token\": \"{}\"}}", "a".repeat(64 * 1024));
    for (answer, refusal) in [
        (
            format!("[\"{marker}\"]"),
            RenewalRefusal::AnswerWithoutAccessToken,
        ),
        (
            format!("\"{marker}\""),
            RenewalRefusal::AnswerWithoutAccessToken,
        ),
        ("null".to_owned(), RenewalRefusal::AnswerWithoutAccessToken),
        (
            format!("{{\"access_token\": null, \"m\": \"{marker}\"}}"),
            RenewalRefusal::AnswerWithoutAccessToken,
        ),
        (
            format!("<html>{marker}</html>"),
            RenewalRefusal::ExchangeFailed,
        ),
        (big, RenewalRefusal::ExchangeFailed),
    ] {
        let dir = fixture_dir();
        let path = dir.path().join("auth.json");
        let before = login(&token(-60, "expired"), OLD_REFRESH);
        write_fixture(&path, &before, 0o600);
        let endpoint = Endpoint::raw("200 OK", answer.clone()).await;
        let error = resolver(&path)
            .renew(&renewal(&endpoint.url), &Cancel::new())
            .await
            .unwrap_err();
        assert_eq!(
            error.refusal(),
            refusal,
            "{}",
            &answer[..answer.len().min(60)]
        );
        assert_eq!(endpoint.requests(), 1);
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
        assert_eq!(names(dir.path()), vec!["auth.json".to_owned()]);
        for text in [error.to_string(), format!("{error:?}")] {
            assert!(!text.contains(marker), "{text}");
            assert!(!text.contains(OLD_REFRESH), "{text}");
            assert!(!text.contains("aaaaaaaa"), "{text}");
        }
    }
}

/// Clock edges: an `exp` at the far ends of the integer range, and a clock before the epoch.
#[tokio::test]
async fn far_expiries_and_a_pre_epoch_clock_decide_without_overflow() {
    for (exp, due) in [
        (u64::MAX.to_string(), false),
        (i64::MIN.to_string(), true),
        ("0".to_owned(), true),
    ] {
        let dir = fixture_dir();
        let path = dir.path().join("auth.json");
        write_fixture(
            &path,
            &login(&token_with_exp(&exp, "edge"), OLD_REFRESH),
            0o600,
        );
        let endpoint = Endpoint::answering(
            "200 OK",
            &json!({"access_token": token(3600, "fresh"), "refresh_token": NEW_REFRESH}),
        )
        .await;
        let outcome = resolver(&path)
            .renew(&renewal(&endpoint.url), &Cancel::new())
            .await
            .unwrap();
        assert_eq!(
            matches!(outcome, Renewal::Renewed(_)),
            due,
            "{exp}: {outcome:?}"
        );
        assert_eq!(endpoint.requests(), usize::from(due), "{exp}");
    }
    let dir = fixture_dir();
    let path = dir.path().join("auth.json");
    write_fixture(
        &path,
        &login(&token_with_exp("-10", "edge"), OLD_REFRESH),
        0o600,
    );
    let endpoint =
        Endpoint::answering("200 OK", &json!({"access_token": token(3600, "fresh")})).await;
    let early = CodexAuthFile::new(reference(), &path)
        .with_clock(|| SystemTime::UNIX_EPOCH - Duration::from_secs(1000));
    let outcome = early
        .renew(
            &renewal(&endpoint.url).with_margin(Duration::ZERO),
            &Cancel::new(),
        )
        .await
        .unwrap();
    assert_eq!(outcome, Renewal::NotDue);
    assert_eq!(endpoint.requests(), 0);
}

/// A login that names `tokens` twice is refused before anything is sent, with the kind the read
/// rule gives the same file (`Unavailable`: the document cannot be read), not `Missing` ("no
/// access token ...; run `codex` to log in"), which this file does hold.
#[tokio::test]
async fn a_duplicated_key_is_refused_before_sending_with_the_read_rules_kind() {
    let dir = fixture_dir();
    let path = dir.path().join("auth.json");
    let stale = token(-60, "expired");
    let before = format!(
        "{{\"tokens\": {{\"access_token\": \"{stale}\", \"refresh_token\": \"{OLD_REFRESH}\"}}, \"tokens\": {{\"access_token\": \"{stale}\", \"refresh_token\": \"{OLD_REFRESH}\"}}}}"
    );
    write_fixture(&path, &before, 0o600);
    let endpoint =
        Endpoint::answering("200 OK", &json!({"access_token": token(3600, "fresh")})).await;
    let error = resolver(&path)
        .renew(&renewal(&endpoint.url), &Cancel::new())
        .await
        .unwrap_err();
    assert_eq!(endpoint.requests(), 0);
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
    let read = resolver(&path).read(&reference()).await.unwrap_err().kind();
    assert_eq!(read, SecretError::Unavailable);
    assert_eq!(
        error.kind(),
        read,
        "renewal refused {:?}: {error}",
        error.refusal()
    );
}
