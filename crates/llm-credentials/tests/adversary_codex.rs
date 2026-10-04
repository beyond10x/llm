#![cfg(all(feature = "codex-auth-file", unix))]
//! Adversary cases for the Codex auth file resolver. Fixture files only: no case reads a real
//! Codex login.
use llm_credentials::{SecretError, SecretRef, SecretResolver, codex::CodexAuthFile};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    time::{Duration, SystemTime},
};

/// The caller's clock in every case: 2026-10-03T00:00:00Z.
const NOW: u64 = 1_790_985_600;
const MIB: usize = 1024 * 1024;

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
        base64url(b"adversary-signature"),
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

fn resolver(path: &Path) -> CodexAuthFile {
    CodexAuthFile::new(reference(), path)
        .with_clock(|| SystemTime::UNIX_EPOCH + Duration::from_secs(NOW))
}

fn fixture_dir() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().canonicalize().unwrap().join("auth.json");
    (temp, path)
}

/// The spec names the field `absolute_path`, the docs say the crate "performs no ambient
/// lookup", and the sibling `file` adapter refuses a relative path as `UnsafeSource`. A relative
/// path must therefore not be resolved against the process working directory.
#[tokio::test]
async fn a_relative_path_is_not_resolved_against_the_working_directory() {
    let (temp, path) = fixture_dir();
    write_fixture(&path, &login(&jwt(&format!(r#"{{"exp":{}}}"#, NOW + 3600))));
    std::env::set_current_dir(path.parent().unwrap()).unwrap();
    let relative = resolver(Path::new("auth.json"));
    let outcome = relative.resolve(&reference()).await;
    assert!(
        outcome.is_err(),
        "a relative path resolved a token through the working directory: {outcome:?}"
    );
    drop(temp);
}

/// docs/local-secrets.md: "a token whose `exp` is not after it is refused as `Expired`" and only
/// "a token without an integer `exp` is `Unavailable`". -1 is an integer not after the clock.
#[tokio::test]
async fn a_negative_integer_exp_is_expired_as_the_docs_say() {
    let (_temp, path) = fixture_dir();
    let resolver = resolver(&path);
    write_fixture(&path, &login(&jwt(r#"{"exp":-1}"#)));
    assert_eq!(
        resolver.resolve(&reference()).await.unwrap_err(),
        SecretError::Expired
    );
}

/// Exactly 1 MiB is read; one byte more is `TooLarge`, even when the first MiB alone would be a
/// valid document (a `take(limit)` mutant would accept it).
#[tokio::test]
async fn the_one_mebibyte_bound_is_on_the_whole_file() {
    let (_temp, path) = fixture_dir();
    let resolver = resolver(&path);
    let token = jwt(&format!(r#"{{"exp":{}}}"#, NOW + 3600));
    let document = login(&token);
    let at_bound = format!("{document}{}", " ".repeat(MIB - document.len()));
    assert_eq!(at_bound.len(), MIB);
    write_fixture(&path, &at_bound);
    assert_eq!(
        resolver
            .resolve(&reference())
            .await
            .unwrap()
            .secret
            .expose(),
        token.as_bytes()
    );
    write_fixture(&path, &format!("{at_bound} "));
    assert_eq!(
        resolver.resolve(&reference()).await.unwrap_err(),
        SecretError::TooLarge
    );
    let refusal = resolver.read(&reference()).await.unwrap_err();
    assert_eq!(refusal.kind(), SecretError::TooLarge);
    assert!(!refusal.to_string().contains(&token));
}

#[tokio::test]
async fn an_empty_access_token_is_missing() {
    let (_temp, path) = fixture_dir();
    let resolver = resolver(&path);
    write_fixture(&path, &login(""));
    assert_eq!(
        resolver.resolve(&reference()).await.unwrap_err(),
        SecretError::Missing
    );
}

/// The `exp` claim and the JWT encoding at their edges.
#[tokio::test]
async fn jwt_edges_resolve_or_refuse_by_the_documented_rule() {
    let (_temp, path) = fixture_dir();
    let resolver = resolver(&path);
    let valid_claims = format!(r#"{{"exp":{}}}"#, NOW + 1);
    let unpadded = jwt(&valid_claims);

    // A padded base64url payload and an unsigned `none` token resolve: the signature is the
    // issuer's to check. A two-segment token resolves as well.
    let mut parts: Vec<String> = unpadded.split('.').map(str::to_owned).collect();
    while !parts[1].len().is_multiple_of(4) {
        parts[1].push('=');
    }
    let padded = parts.join(".");
    let none = format!(
        "{}.{}.",
        base64url(br#"{"alg":"none"}"#),
        base64url(valid_claims.as_bytes())
    );
    for token in [&unpadded, &padded, &none] {
        write_fixture(&path, &login(token));
        assert_eq!(
            resolver
                .resolve(&reference())
                .await
                .unwrap()
                .secret
                .expose(),
            token.as_bytes(),
            "{token}"
        );
    }

    // `exp` exactly at the clock is expired; one second later is not.
    write_fixture(&path, &login(&jwt(&format!(r#"{{"exp":{NOW}}}"#))));
    assert_eq!(
        resolver.resolve(&reference()).await.unwrap_err(),
        SecretError::Expired
    );

    // Not an integer `exp`, or not a JWT at all: `Unavailable`.
    let huge = "18446744073709551616";
    for claims in [
        format!(r#"{{"exp":"{}"}}"#, NOW + 3600),
        format!(r#"{{"exp":{}.5}}"#, NOW + 3600),
        format!(r#"{{"exp":{huge}}}"#),
        r#"{"sub":"no-exp"}"#.to_owned(),
        r#"{"exp":null}"#.to_owned(),
    ] {
        write_fixture(&path, &login(&jwt(&claims)));
        assert_eq!(
            resolver.resolve(&reference()).await.unwrap_err(),
            SecretError::Unavailable,
            "{claims}"
        );
    }
    for token in ["opaque-token", "a.!!!.c", "a.b+/c.d"] {
        write_fixture(&path, &login(token));
        assert_eq!(
            resolver.resolve(&reference()).await.unwrap_err(),
            SecretError::Unavailable,
            "{token}"
        );
    }
}

/// The pointer is `/tokens/access_token` in JSON terms: an escaped key still names it, a
/// top-level key spelled like the pointer does not.
#[tokio::test]
async fn the_pointer_follows_json_not_spelling() {
    let (_temp, path) = fixture_dir();
    let resolver = resolver(&path);
    let token = jwt(&format!(r#"{{"exp":{}}}"#, NOW + 3600));

    write_fixture(
        &path,
        &format!(r#"{{"tokens":{{"access_token":"{token}"}}}}"#),
    );
    assert_eq!(
        resolver
            .resolve(&reference())
            .await
            .unwrap()
            .secret
            .expose(),
        token.as_bytes()
    );

    let escaped = token.replace('.', "\\u002e");
    write_fixture(
        &path,
        &format!(r#"{{"tokens":{{"access_token":"{escaped}"}}}}"#),
    );
    assert_eq!(
        resolver
            .resolve(&reference())
            .await
            .unwrap()
            .secret
            .expose(),
        token.as_bytes()
    );

    write_fixture(
        &path,
        &format!(r#"{{"tokens/access_token":"{token}","tokens":{{}}}}"#),
    );
    assert_eq!(
        resolver.resolve(&reference()).await.unwrap_err(),
        SecretError::Missing
    );
}

/// No refusal's `Display` or `Debug`, and no resolved value's `Debug`, carries a token segment.
#[tokio::test]
async fn no_message_carries_token_material() {
    let (_temp, path) = fixture_dir();
    let resolver = resolver(&path);
    let live = jwt(&format!(
        r#"{{"exp":{},"sub":"adversary-live"}}"#,
        NOW + 3600
    ));
    let expired = jwt(r#"{"exp":1,"sub":"adversary-expired"}"#);
    let unusable = jwt(r#"{"exp":"soon","sub":"adversary-unusable"}"#);
    let segments = |token: &str| -> Vec<String> {
        token
            .split('.')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect()
    };

    write_fixture(&path, &login(&live));
    let value = resolver.resolve(&reference()).await.unwrap();
    let shown = format!("{value:?} {resolver:?}");
    for segment in segments(&live) {
        assert!(!shown.contains(&segment), "{shown}");
    }
    let other = resolver
        .read(&SecretRef::new("another-login").unwrap())
        .await
        .unwrap_err();
    let shown = format!("{other} {other:?}");
    for segment in segments(&live) {
        assert!(!shown.contains(&segment), "{shown}");
    }

    for (token, kind) in [
        (&expired, SecretError::Expired),
        (&unusable, SecretError::Unavailable),
    ] {
        write_fixture(&path, &login(token));
        let refusal = resolver.read(&reference()).await.unwrap_err();
        assert_eq!(refusal.kind(), kind);
        let shown = format!("{refusal} {refusal:?} {:?}", refusal.kind());
        assert!(shown.contains("auth.json"), "{shown}");
        for segment in segments(token) {
            assert!(!shown.contains(&segment), "{shown}");
        }
    }
}

/// The version is the token's identity, not the file's: other fields changing keep it, the
/// token changing moves it, and restoring the token restores it.
#[tokio::test]
async fn the_version_follows_the_token_alone() {
    let (_temp, path) = fixture_dir();
    let resolver = resolver(&path);
    let token = jwt(&format!(r#"{{"exp":{}}}"#, NOW + 3600));
    let other = jwt(&format!(r#"{{"exp":{}}}"#, NOW + 7200));

    write_fixture(&path, &login(&token));
    let first = resolver.resolve(&reference()).await.unwrap().version;
    write_fixture(
        &path,
        &login(&token).replace("2026-10-03T00:00:00Z", "2026-10-03T01:00:00Z"),
    );
    let same = resolver.resolve(&reference()).await.unwrap().version;
    assert_eq!(first, same);
    write_fixture(&path, &login(&other));
    let moved = resolver.resolve(&reference()).await.unwrap().version;
    assert_ne!(first, moved);
    write_fixture(&path, &login(&token));
    assert_eq!(first, resolver.resolve(&reference()).await.unwrap().version);
    assert_eq!(format!("{first:?}"), "SecretVersion([REDACTED])");
}
