#![cfg(all(feature = "codex-auth-file", unix))]
//! A Codex login resolves to its access token, read-only. Fixture files only: no test reads a
//! real Codex login.
use llm_credentials::{SecretError, SecretRef, SecretResolver, codex::CodexAuthFile};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    time::{Duration, SystemTime},
};

/// The caller's clock in every case: 2026-10-03T00:00:00Z.
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

/// An unsigned fixture JWT whose only claim that matters is `exp`.
fn token(exp: u64) -> String {
    format!(
        "{}.{}.{}",
        base64url(br#"{"alg":"none","typ":"JWT"}"#),
        base64url(format!(r#"{{"exp":{exp},"sub":"fixture-subject"}}"#).as_bytes()),
        base64url(b"fixture-signature"),
    )
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

/// The fixture's bytes and mode, to compare after every call.
fn snapshot(path: &Path) -> Option<(Vec<u8>, u32)> {
    let bytes = fs::read(path).ok()?;
    let mode = fs::metadata(path).unwrap().permissions().mode();
    Some((bytes, mode))
}

fn resolver(path: &Path) -> CodexAuthFile {
    CodexAuthFile::new(reference(), path)
        .with_clock(|| SystemTime::UNIX_EPOCH + Duration::from_secs(NOW))
}

fn assert_redacted(resolver: &CodexAuthFile, token: &str) {
    let debug = format!("{resolver:?}");
    assert!(!debug.contains(token), "Debug carries the token: {debug}");
    for segment in token.split('.') {
        assert!(
            !debug.contains(segment),
            "Debug carries a token segment: {debug}"
        );
    }
}

#[tokio::test]
async fn a_codex_login_resolves_its_access_token() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().canonicalize().unwrap().join("auth.json");
    let resolver = resolver(&path);

    // A valid file resolves the token, re-read on every request.
    let valid = token(NOW + 3600);
    write_fixture(&path, &login(&valid));
    let before = snapshot(&path);
    let first = resolver.resolve(&reference()).await.unwrap();
    assert_eq!(first.secret.expose(), valid.as_bytes());
    assert_eq!(snapshot(&path), before, "a resolve changed the fixture");
    assert_redacted(&resolver, &valid);

    let rotated = token(NOW + 7200);
    write_fixture(&path, &login(&rotated));
    let before = snapshot(&path);
    let reread = resolver.resolve(&reference()).await.unwrap();
    assert_eq!(reread.secret.expose(), rotated.as_bytes());
    assert_ne!(reread.version, first.version);
    assert_eq!(snapshot(&path), before, "a resolve changed the fixture");

    // Never refreshes: renewal is the caller's, by running `codex`.
    assert_eq!(
        resolver.refresh(&reference(), &reread.version).await,
        Err(SecretError::RefreshUnsupported)
    );
    assert_eq!(snapshot(&path), before, "a refresh changed the fixture");
    assert_redacted(&resolver, &rotated);

    // An expired token is refused as `Expired`, naming the file and the way to renew it.
    for exp in [NOW, NOW - 1] {
        let expired = token(exp);
        write_fixture(&path, &login(&expired));
        let before = snapshot(&path);
        assert_eq!(
            resolver.resolve(&reference()).await.unwrap_err(),
            SecretError::Expired
        );
        let refusal = resolver.read(&reference()).await.unwrap_err();
        assert_eq!(refusal.kind(), SecretError::Expired);
        assert_eq!(refusal.path(), path.as_path());
        let message = refusal.to_string();
        assert!(message.contains(&path.display().to_string()), "{message}");
        assert!(message.contains("codex"), "{message}");
        assert!(!message.contains(&expired), "{message}");
        assert_eq!(snapshot(&path), before, "a refusal changed the fixture");
        assert_redacted(&resolver, &expired);
    }

    // A missing file, a missing pointer or another reference is refused as `Missing`.
    fs::remove_file(&path).unwrap();
    assert_eq!(
        resolver.resolve(&reference()).await.unwrap_err(),
        SecretError::Missing
    );
    let refusal = resolver.read(&reference()).await.unwrap_err();
    assert_eq!(refusal.kind(), SecretError::Missing);
    assert!(refusal.to_string().contains(&path.display().to_string()));
    assert_eq!(snapshot(&path), None, "a refusal created the fixture");

    for document in [
        serde_json::json!({}),
        serde_json::json!({"tokens": null}),
        serde_json::json!({"tokens": {"refresh_token": "fixture-refresh-token"}}),
        serde_json::json!({"OPENAI_API_KEY": "fixture-api-key", "tokens": {}}),
    ] {
        write_fixture(&path, &document.to_string());
        let before = snapshot(&path);
        assert_eq!(
            resolver.resolve(&reference()).await.unwrap_err(),
            SecretError::Missing,
            "{document}"
        );
        assert_eq!(
            resolver.read(&reference()).await.unwrap_err().kind(),
            SecretError::Missing
        );
        assert_eq!(snapshot(&path), before, "a refusal changed the fixture");
    }

    write_fixture(&path, &login(&valid));
    let before = snapshot(&path);
    assert_eq!(
        resolver
            .resolve(&SecretRef::new("another-login").unwrap())
            .await
            .unwrap_err(),
        SecretError::Missing
    );
    assert_eq!(snapshot(&path), before, "a refusal changed the fixture");
    assert_redacted(&resolver, &valid);
}

/// A path that is not absolute is refused as `Unavailable` before anything is opened, with a
/// message that says so; it is never resolved against the working directory.
#[tokio::test]
async fn a_relative_path_is_refused_as_not_absolute() {
    for relative in ["auth.json", "./auth.json", ".codex/auth.json", ""] {
        let resolver = resolver(Path::new(relative));
        assert_eq!(
            resolver.resolve(&reference()).await.unwrap_err(),
            SecretError::Unavailable,
            "{relative:?}"
        );
        let refusal = resolver.read(&reference()).await.unwrap_err();
        assert_eq!(refusal.kind(), SecretError::Unavailable);
        assert_eq!(refusal.path(), Path::new(relative));
        let message = refusal.to_string();
        assert!(message.contains("not absolute"), "{message}");
        assert!(message.contains(relative), "{message}");
    }
}
