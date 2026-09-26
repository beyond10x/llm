//! Disposable fixture construction around real adapters. No scenario names or
//! expected assertions are accessible here. Native user credential stores are never opened.
use keyring_core::{api::CredentialStoreApi, mock};
use llm_core::Id;
use llm_credentials::{
    ResolvedSecret, SecretError, SecretRef, SecretResolver,
    file::FileResolver,
    keychain::{KeychainEntry, KeychainResolver},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    error::Error,
    fmt::Write,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
};

const CANARY: &str = "llm-fixture-private-marker";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Common {
    content: String,
    replacement: Option<String>,
    known_reference: bool,
    oversized: bool,
}
#[derive(Deserialize)]
enum Shape {
    Regular,
    Missing,
    Symlink,
    ParentSymlink,
    Hardlink,
    Directory,
    Fifo,
}
#[derive(Deserialize)]
struct FileInput {
    #[serde(flatten)]
    common: Common,
    shape: Shape,
    public_readable: bool,
    parent_writable: bool,
}
#[derive(Deserialize)]
struct KeychainInput {
    #[serde(flatten)]
    common: Common,
    entry_present: bool,
    backend_fault: bool,
}

fn code(error: SecretError) -> &'static str {
    match error {
        SecretError::Missing => "missing",
        SecretError::UnsafeSource => "unsafe-source",
        SecretError::TooLarge => "too-large",
        SecretError::RefreshUnsupported => "refresh-unsupported",
        SecretError::Unavailable => "unavailable",
        SecretError::UnsupportedPlatform => "unsupported-platform",
        SecretError::InvalidReference => "invalid-reference",
        SecretError::Expired => "expired",
        SecretError::RefreshRejected => "refresh-rejected",
        SecretError::RefreshUncertain => "refresh-uncertain",
        SecretError::TooManyReferences => "too-many-references",
    }
}
fn bytes(input: &Common) -> Vec<u8> {
    if input.oversized {
        // Authored 1 MiB contract boundary, independent of the implementation's
        // exported bound so changing that bound cannot move this fixture with it.
        vec![b'x'; 1_048_577]
    } else {
        input.content.as_bytes().to_vec()
    }
}
fn protected(path: &Path, content: &[u8]) -> Result<(), Box<dyn Error>> {
    fs::write(path, content)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

async fn observe(
    resolver: &dyn SecretResolver,
    input: &Common,
    rotate: impl FnOnce(&[u8]) -> Result<(), Box<dyn Error>>,
) -> Result<Value, Box<dyn Error>> {
    let mut facts = json!({"error_code":null, "reread_error":null,
        "matches_content":null, "matches_replacement":null,
        "version_changed":null, "refresh_error":null, "diagnostics_safe":true});
    let reference = SecretRef::new(if input.known_reference {
        "selected"
    } else {
        "unbound"
    })?;
    let first = resolver.resolve(&reference).await;
    let mut diagnostics = format!("{first:?}");
    let mut redacted = true;
    match first {
        Err(error) => facts["error_code"] = json!(code(error)),
        Ok(first) => {
            redacted &= debug_redacted(&first);
            facts["matches_content"] = json!(first.secret.expose() == bytes(input));
            let refresh = resolver.refresh(&reference, &first.version).await;
            write!(diagnostics, "{refresh:?}")?;
            facts["refresh_error"] = json!(refresh.err().map(code));
            if let Some(replacement) = &input.replacement {
                rotate(replacement.as_bytes())?;
            }
            let second = resolver.resolve(&reference).await;
            write!(diagnostics, "{second:?}")?;
            match second {
                Err(error) => facts["reread_error"] = json!(code(error)),
                Ok(second) => {
                    redacted &= debug_redacted(&second);
                    facts["version_changed"] = json!(first.version != second.version);
                    facts["matches_replacement"] = json!(
                        second.secret.expose()
                            == input
                                .replacement
                                .as_ref()
                                .map_or(input.content.as_bytes(), String::as_bytes)
                    );
                }
            }
        }
    }
    facts["diagnostics_safe"] = json!(redacted && !diagnostics.contains(CANARY));
    Ok(facts)
}

fn debug_redacted(value: &ResolvedSecret) -> bool {
    format!("{:?}", value.secret) == "Secret([REDACTED])"
        && format!("{:?}", value.version) == "SecretVersion([REDACTED])"
}

pub fn file(input: Value) -> Result<Value, Box<dyn Error>> {
    let input: FileInput = serde_json::from_value(input)?;
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let real = root.join("credential");
    let mut path = real.clone();
    if !matches!(input.shape, Shape::Missing | Shape::Directory | Shape::Fifo) {
        protected(&real, &bytes(&input.common))?;
        if input.public_readable {
            fs::set_permissions(&real, fs::Permissions::from_mode(0o644))?;
        }
    }
    match input.shape {
        Shape::Regular | Shape::Missing => (),
        Shape::Symlink => {
            path = root.join("link");
            symlink(&real, &path)?;
        }
        Shape::ParentSymlink => {
            let alias = root.join("alias");
            symlink(&root, &alias)?;
            path = alias.join("credential");
        }
        Shape::Hardlink => fs::hard_link(&real, root.join("hardlink"))?,
        Shape::Directory => fs::create_dir(&real)?,
        Shape::Fifo => rustix::fs::mknodat(
            rustix::fs::CWD,
            &real,
            rustix::fs::FileType::Fifo,
            rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
            0,
        )?,
    }
    if input.parent_writable {
        fs::set_permissions(&root, fs::Permissions::from_mode(0o777))?;
    }
    let resolver = FileResolver::new(BTreeMap::from([(SecretRef::new("selected")?, path)]))?;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(observe(&resolver, &input.common, |replacement| {
            protected(&real, replacement)
        }))
}

pub fn keychain(input: Value) -> Result<Value, Box<dyn Error>> {
    let input: KeychainInput = serde_json::from_value(input)?;
    let store = mock::Store::new()?;
    let entry = store.build("fixture-service", "selected-entry", None)?;
    store
        .build("other-service", "selected-entry", None)?
        .set_secret(b"wrong-service")?;
    store
        .build("fixture-service", "other-entry", None)?
        .set_secret(b"wrong-entry")?;
    if input.entry_present {
        entry.set_secret(&bytes(&input.common))?;
    }
    if input.backend_fault {
        entry
            .as_any()
            .downcast_ref::<mock::Cred>()
            .ok_or("mock fixture credential type")?
            .set_error(keyring_core::Error::Invalid(CANARY.into(), CANARY.into()));
    }
    let resolver = KeychainResolver::new(
        store,
        BTreeMap::from([(
            SecretRef::new("selected")?,
            KeychainEntry {
                service: Id::new("fixture-service")?,
                entry: Id::new("selected-entry")?,
            },
        )]),
    )?;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(observe(&resolver, &input.common, |replacement| {
            entry.set_secret(replacement)?;
            Ok(())
        }))
}
