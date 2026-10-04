#![cfg(all(feature = "codex-auth-file", unix))]
//! Second adversary pass on the Codex auth file resolver. Fixture files only: no case reads a
//! real Codex login.
use llm_credentials::{SecretError, SecretRef, SecretResolver, codex::CodexAuthFile};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime},
};

/// The caller's clock in most cases: 2026-10-03T00:00:00Z.
const NOW: u64 = 1_790_985_600;

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

/// A fixture JWT whose payload is `claims` verbatim.
fn jwt(claims: &str) -> String {
    format!(
        "{}.{}.{}",
        base64url(br#"{"alg":"RS256","typ":"JWT"}"#),
        base64url(claims.as_bytes()),
        base64url(b"adversary2-signature"),
    )
}

fn live_jwt() -> String {
    jwt(&format!(r#"{{"exp":{}}}"#, NOW + 3600))
}

fn login(access_token: &str) -> String {
    serde_json::json!({
        "OPENAI_API_KEY": null,
        "tokens": {
            "id_token": "fixture-id-token",
            "access_token": access_token,
            "refresh_token": "fixture-refresh-token",
            "account_id": "fixture-account"
        },
        "last_refresh": "2026-10-03T00:00:00Z"
    })
    .to_string()
}

fn write_fixture(path: &Path, content: &str) {
    fs::write(path, content).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

fn resolver(path: &Path) -> CodexAuthFile {
    CodexAuthFile::new(reference(), path)
        .with_clock(|| SystemTime::UNIX_EPOCH + Duration::from_secs(NOW))
}

fn fixture_dir() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().canonicalize().unwrap().join("auth.json");
    (temp, path)
}

/// A FIFO at the configured path, with no writer, is not a login. The sibling `file` adapter
/// opens with `O_NONBLOCK` and refuses anything but a regular file; this one must at least return.
/// A resolve that never returns also holds one of the resolver's eight blocking-read permits for
/// good, and blocks runtime shutdown.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[tokio::test]
async fn a_fifo_at_the_path_is_refused_without_hanging() {
    use std::os::unix::fs::OpenOptionsExt;
    const O_NONBLOCK: i32 = 0o4000;

    let (_temp, path) = fixture_dir();
    let made = std::process::Command::new("mkfifo")
        .arg(&path)
        .status()
        .unwrap();
    assert!(made.success(), "mkfifo failed");
    let resolver = resolver(&path);
    let outcome =
        tokio::time::timeout(Duration::from_secs(3), resolver.resolve(&reference())).await;
    if outcome.is_err() {
        // Release the open still blocked on the blocking pool, so the runtime can shut down.
        for _ in 0..50 {
            let writer = fs::OpenOptions::new()
                .write(true)
                .custom_flags(O_NONBLOCK)
                .open(&path);
            if writer.is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    let refusal = outcome
        .expect("resolve against a FIFO with no writer did not return within 3 s")
        .unwrap_err();
    assert!(
        matches!(
            refusal,
            SecretError::Unavailable | SecretError::UnsafeSource | SecretError::Missing
        ),
        "{refusal:?}"
    );
}

/// docs/local-secrets.md: "a token whose integer `exp` (negative included) is not after [the
/// clock] is refused as `Expired`". With the caller's clock 100 s before the Unix epoch, an `exp`
/// of -50 is after the clock and must resolve; -100 and -150 are not, and are `Expired`.
#[tokio::test]
async fn a_clock_before_the_epoch_judges_negative_exp_against_that_instant() {
    let (_temp, path) = fixture_dir();
    let resolver = CodexAuthFile::new(reference(), &path)
        .with_clock(|| SystemTime::UNIX_EPOCH - Duration::from_secs(100));

    for exp in [-100, -150] {
        write_fixture(&path, &login(&jwt(&format!(r#"{{"exp":{exp}}}"#))));
        assert_eq!(
            resolver.resolve(&reference()).await.unwrap_err(),
            SecretError::Expired,
            "exp {exp}"
        );
    }
    for exp in [-50, 0, 1] {
        let token = jwt(&format!(r#"{{"exp":{exp}}}"#));
        write_fixture(&path, &login(&token));
        let outcome = resolver.resolve(&reference()).await;
        assert_eq!(
            outcome.as_ref().map(|value| value.secret.expose()),
            Ok(token.as_bytes()),
            "exp {exp} is after a clock of -100 s and was refused"
        );
    }
}

/// The story: "resolves one `SecretRef` to the `/tokens/access_token` of the given `auth.json`".
/// A JSON array has no member named `tokens` or `access_token`, so neither document below has
/// that pointer, and neither may resolve to a token.
#[tokio::test]
async fn an_array_in_place_of_an_object_has_no_tokens_pointer() {
    let (_temp, path) = fixture_dir();
    let resolver = resolver(&path);
    let token = live_jwt();

    for document in [
        format!(r#"[{{"access_token":"{token}"}}]"#),
        format!(r#"{{"tokens":["{token}"]}}"#),
    ] {
        write_fixture(&path, &document);
        let outcome = resolver.resolve(&reference()).await;
        assert!(
            matches!(
                outcome,
                Err(SecretError::Missing | SecretError::Unavailable)
            ),
            "{document} resolved a token at no `/tokens/access_token`: {:?}",
            outcome.map(|value| value.secret.expose() == token.as_bytes())
        );
    }
}

/// docs/local-secrets.md: "a token whose `exp` is absent ... is `Unavailable`". A JWT payload that
/// is a JSON array carries no `exp` claim, whatever its first element is.
#[tokio::test]
async fn a_jwt_payload_that_is_an_array_has_no_exp_claim() {
    let (_temp, path) = fixture_dir();
    let resolver = resolver(&path);
    write_fixture(&path, &login(&jwt(&format!("[{}]", NOW + 3600))));
    let outcome = resolver.resolve(&reference()).await;
    assert_eq!(
        outcome.map(|value| value.secret.expose().len()),
        Err(SecretError::Unavailable)
    );
}

/// The file is replaced atomically (written aside, renamed over) while many resolves run at once,
/// as an atomic writer would replace it. The two tokens differ in length, so a size taken from the
/// path rather than from the opened file would read one file with the other's length. Every
/// resolve sees exactly one whole file: one of the two tokens, with that token's version.
#[tokio::test]
async fn an_atomic_replace_mid_stream_yields_one_whole_token_and_its_version() {
    let (temp, path) = fixture_dir();
    let short = live_jwt();
    let long = jwt(&format!(
        r#"{{"exp":{},"sub":"{}"}}"#,
        NOW + 3600,
        "s".repeat(8192)
    ));
    let resolver = Arc::new(resolver(&path));
    write_fixture(&path, &login(&short));
    let short_version = resolver.resolve(&reference()).await.unwrap().version;
    write_fixture(&path, &login(&long));
    let long_version = resolver.resolve(&reference()).await.unwrap().version;
    assert_ne!(short_version, long_version);

    let stop = Arc::new(AtomicBool::new(false));
    let writer = {
        let stop = stop.clone();
        let (path, stage) = (path.clone(), temp.path().join("auth.json.next"));
        let documents = [login(&short), login(&long)];
        std::thread::spawn(move || {
            let mut index = 0;
            while !stop.load(Ordering::Relaxed) {
                write_fixture(&stage, &documents[index % 2]);
                fs::rename(&stage, &path).unwrap();
                index += 1;
            }
            index
        })
    };

    let mut outcomes = Vec::new();
    for _ in 0..100 {
        let mut batch = tokio::task::JoinSet::new();
        for _ in 0..32 {
            let resolver = resolver.clone();
            batch.spawn(async move { resolver.resolve(&reference()).await });
        }
        outcomes.extend(batch.join_all().await);
    }
    stop.store(true, Ordering::Relaxed);
    let replaced = writer.join().unwrap();
    assert!(
        replaced > 10,
        "the writer replaced the file {replaced} times"
    );

    let mut refused = Vec::new();
    for outcome in outcomes {
        match outcome {
            Ok(value) if value.secret.expose() == short.as_bytes() => {
                assert_eq!(value.version, short_version);
            }
            Ok(value) if value.secret.expose() == long.as_bytes() => {
                assert_eq!(value.version, long_version);
            }
            Ok(value) => panic!(
                "a resolve returned neither token ({} bytes)",
                value.secret.expose().len()
            ),
            Err(error) => refused.push(error),
        }
    }
    assert!(
        refused.is_empty(),
        "{} of 3200 resolves were refused during atomic replaces: {:?}",
        refused.len(),
        refused.first()
    );
}
