//! Disposable fixture construction around real adapters. No scenario names or
//! expected assertions are accessible here. Native user credential stores are never opened.
use keyring_core::{api::CredentialStoreApi, mock};
use llm_core::Id;
use llm_credentials::{
    ReferenceError, ResolvedSecret, SecretError, SecretRef, SecretResolver,
    environment::EnvironmentResolver,
    file::FileResolver,
    keychain::{KeychainEntry, KeychainResolver},
    pointer::JsonPointerResolver,
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
    process::{Command, Stdio},
    sync::Arc,
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
    /// One permission arrangement applied after the switches above, when present.
    #[serde(default)]
    mode: Option<ModeFixture>,
}
/// Each arrangement is one mode away from the private default: a 0700 directory, a 0600 file.
#[derive(Deserialize, Clone, Copy)]
enum ModeFixture {
    ParentGroupWritable,
    ParentOtherWritable,
    FileOwnerExecutable,
    FileGroupReadable,
    FileSetuid,
    FileOwnerReadOnly,
}
impl ModeFixture {
    /// Whether the arrangement is applied to the parent directory, and the bits it sets.
    const fn bits(self) -> (bool, u32) {
        match self {
            Self::ParentGroupWritable => (true, 0o770),
            Self::ParentOtherWritable => (true, 0o702),
            Self::FileOwnerExecutable => (false, 0o700),
            Self::FileGroupReadable => (false, 0o640),
            Self::FileSetuid => (false, 0o4600),
            Self::FileOwnerReadOnly => (false, 0o400),
        }
    }
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
        SecretError::Malformed => "malformed",
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
        "version_changed":null, "refresh_error":null, "refusal_names_reference":null,
        "refusal_names_location":null, "diagnostics_safe":true});
    let reference = reference(input)?;
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

fn reference(input: &Common) -> Result<SecretRef, SecretError> {
    SecretRef::new(if input.known_reference {
        "selected"
    } else {
        "unbound"
    })
}

/// For a refused first resolve, what the source's own `read` refusal says: whether its message
/// names the reference, and whether it names any of `locations` (where the value lives).
fn refusal(
    facts: &mut Value,
    read: Result<ResolvedSecret, ReferenceError>,
    reference: &SecretRef,
    locations: &[&str],
) {
    if facts["error_code"].is_null() {
        return;
    }
    let Err(error) = read else {
        facts["refusal_names_reference"] = json!(false);
        return;
    };
    let rendered = format!("{error} {error:?}");
    facts["refusal_names_reference"] = json!(error.to_string().contains(reference.as_str()));
    facts["refusal_names_location"] =
        json!(locations.iter().any(|location| rendered.contains(location)));
    if rendered.contains(CANARY) {
        facts["diagnostics_safe"] = json!(false);
    }
}

fn runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
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
    // A leaf arrangement over a Missing shape has no leaf to apply to, and is left out.
    if let Some(mode) = input.mode {
        let (parent, bits) = mode.bits();
        let target = if parent { &root } else { &real };
        if fs::symlink_metadata(target).is_ok() {
            fs::set_permissions(target, fs::Permissions::from_mode(bits))?;
        }
    }
    let locations = [
        path.to_str().ok_or("non-UTF8 fixture path")?.to_owned(),
        root.to_str().ok_or("non-UTF8 fixture path")?.to_owned(),
    ];
    let resolver = FileResolver::new(BTreeMap::from([(SecretRef::new("selected")?, path)]))?;
    let runtime = runtime()?;
    let mut facts = runtime.block_on(observe(&resolver, &input.common, |replacement| {
        protected(&real, replacement)
    }))?;
    let reference = reference(&input.common)?;
    refusal(
        &mut facts,
        runtime.block_on(resolver.read(&reference)),
        &reference,
        &locations.each_ref().map(String::as_str),
    );
    Ok(facts)
}

/// The variable the environment child resolves; set only in the child's environment.
const ENVIRONMENT_VARIABLE: &str = "LLM_CONFORMANCE_FIXTURE_SECRET";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvironmentInput {
    content: String,
    variable_set: bool,
    known_reference: bool,
}

/// Starts this binary's hidden `secrets-environment-child` with the fixture variable set (or
/// removed) and returns the facts it prints. Nothing changes this process's environment.
pub fn environment(input: &Value) -> Result<Value, Box<dyn Error>> {
    let parsed: EnvironmentInput = serde_json::from_value(input.clone())?;
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args(["secrets-environment-child", &serde_json::to_string(input)?])
        .env_remove(ENVIRONMENT_VARIABLE)
        .stdin(Stdio::null());
    if parsed.variable_set {
        command.env(ENVIRONMENT_VARIABLE, &parsed.content);
    }
    let output = command.output()?;
    if !output.status.success() {
        return Err(format!(
            "secrets-environment-child failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

/// The child half of [`environment`]: resolves through a real `EnvironmentResolver` bound to
/// [`ENVIRONMENT_VARIABLE`] in whatever environment this process was started with.
pub fn environment_here(input_json: &str) -> Result<Value, Box<dyn Error>> {
    let input: EnvironmentInput = serde_json::from_str(input_json)?;
    let common = Common {
        content: input.content,
        replacement: None,
        known_reference: input.known_reference,
        oversized: false,
    };
    let resolver = EnvironmentResolver::new(BTreeMap::from([(
        SecretRef::new("selected")?,
        ENVIRONMENT_VARIABLE.to_owned(),
    )]))?;
    let runtime = runtime()?;
    let mut facts = runtime.block_on(observe(&resolver, &common, |_| {
        Err("an environment variable is not rotated inside the child".into())
    }))?;
    let reference = reference(&common)?;
    refusal(
        &mut facts,
        runtime.block_on(resolver.read(&reference)),
        &reference,
        &[ENVIRONMENT_VARIABLE],
    );
    Ok(facts)
}

#[derive(Deserialize)]
enum PointerDocument {
    TokenAtPointer,
    PointerAbsent,
    NonStringAtPointer,
    EmptyAtPointer,
    NotJson,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PointerInput {
    token: String,
    pointer: String,
    document: PointerDocument,
    known_reference: bool,
}

/// Builds the JSON document the scenario names around the pointer's (unescaped) member names.
fn pointer_document(input: &PointerInput) -> Result<Vec<u8>, Box<dyn Error>> {
    let leaf = match input.document {
        PointerDocument::NotJson => return Ok(format!("{}\n", input.token).into_bytes()),
        PointerDocument::TokenAtPointer => Some(json!(input.token)),
        PointerDocument::PointerAbsent => None,
        PointerDocument::NonStringAtPointer => Some(json!(17)),
        PointerDocument::EmptyAtPointer => Some(json!("")),
    };
    let members: Vec<String> = input
        .pointer
        .split('/')
        .skip(1)
        .map(|member| member.replace("~1", "/").replace("~0", "~"))
        .collect();
    // A pointer naming no member selects the whole document.
    let Some((last, parents)) = members.split_last() else {
        return Ok(serde_json::to_vec(&leaf.unwrap_or_else(|| json!({})))?);
    };
    let mut value = Value::Object(leaf.map(|leaf| (last.clone(), leaf)).into_iter().collect());
    for member in parents.iter().rev() {
        value = Value::Object(std::iter::once((member.clone(), value)).collect());
    }
    Ok(serde_json::to_vec(&value)?)
}

pub fn pointer(input: Value) -> Result<Value, Box<dyn Error>> {
    let input: PointerInput = serde_json::from_value(input)?;
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let path = root.join("credentials.json");
    protected(&path, &pointer_document(&input)?)?;
    let locations = [
        path.to_str().ok_or("non-UTF8 fixture path")?.to_owned(),
        root.to_str().ok_or("non-UTF8 fixture path")?.to_owned(),
    ];
    let selected = SecretRef::new("selected")?;
    let file = FileResolver::new(BTreeMap::from([(selected.clone(), path)]))?;
    // A pointer the constructor refuses is an observation, not a fixture failure.
    let resolver = match JsonPointerResolver::new(
        Arc::new(file),
        BTreeMap::from([(selected, input.pointer.clone())]),
    ) {
        Ok(resolver) => resolver,
        Err(error) => {
            return Ok(json!({"error_code":code(error), "reread_error":null,
                "matches_content":null, "matches_replacement":null, "version_changed":null,
                "refresh_error":null, "refusal_names_reference":null,
                "refusal_names_location":null, "diagnostics_safe":true}));
        }
    };
    let common = Common {
        content: input.token,
        replacement: None,
        known_reference: input.known_reference,
        oversized: false,
    };
    let runtime = runtime()?;
    let mut facts = runtime.block_on(observe(&resolver, &common, |_| {
        Err("the pointer probe does not rotate its document".into())
    }))?;
    let reference = reference(&common)?;
    refusal(
        &mut facts,
        runtime.block_on(resolver.read(&reference)),
        &reference,
        &locations.each_ref().map(String::as_str),
    );
    Ok(facts)
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
