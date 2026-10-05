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

// Codex login renewal over a disposable `auth.json` (under this checkout's `target/conformance`)
// and a scripted token endpoint on 127.0.0.1. Never the operator's login, never a real endpoint.

/// Every view this module answers through `query_view` besides `llm.secrets.LastProbe`.
pub const VIEWS: &[&str] = &["llm.secrets.LastRenewal"];

/// The fixture clock: 2026-10-03T00:00:00Z, and the same instant as `/last_refresh` is written.
const RENEWAL_NOW: i64 = 1_790_985_600;
const RENEWAL_NOW_TEXT: &str = "2026-10-03T00:00:00Z";
const RENEWAL_CLIENT: &str = "llm-fixture-client";
const OLD_REFRESH: &str = "llm-fixture-private-marker-refresh-one";
const NEW_REFRESH: &str = "llm-fixture-private-marker-refresh-two";
const OLD_ID: &str = "llm-fixture-private-marker-id-one";
const NEW_ID: &str = "llm-fixture-private-marker-id-two";
const UNDATED_ACCESS: &str = "llm-fixture-private-marker-opaque-access";

#[derive(Deserialize, Clone, Copy)]
enum RenewalEntry {
    Renew,
    RenewingResolver,
    ReadOnlyResolver,
}
#[derive(Deserialize, Clone, Copy)]
enum AuthLayout {
    Codex,
    RepeatedValue,
}
#[derive(Deserialize, Clone, Copy)]
enum TokenAnswer {
    Rotated,
    Kept,
    AccessOnly,
    EmptyAccess,
    EmptyRefresh,
    NonStringIdToken,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RenewInput {
    entry: RenewalEntry,
    #[serde(default)]
    expires_in_s: Option<f64>,
    margin_s: f64,
    layout: AuthLayout,
    refresh_token_present: bool,
    group_readable: bool,
    answer_status: f64,
    answer: TokenAnswer,
    concurrent_change: bool,
    #[serde(default)]
    fixture: Option<RenewalFixture>,
    #[serde(default)]
    resolve_twice: Option<bool>,
}
/// One departure from an ordinary readable login, applied after it is written.
#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
enum RenewalFixture {
    RelativePath,
    MissingFile,
    NotJson,
    DuplicateKey,
    Oversized,
    Symlink,
    Directory,
    HardLink,
    UnwritableDirectory,
    Truncated,
}

/// An exact integer from an ESS number, which may arrive as `60.0`.
fn integer(value: f64) -> Result<i64, Box<dyn Error>> {
    if value.fract() != 0.0 || value.abs() > 1e15 {
        return Err("fixture number is not an exact integer".into());
    }
    #[expect(clippy::cast_possible_truncation, reason = "checked exact and bounded")]
    Ok(value as i64)
}

/// Unpadded base64url, as a JWT segment is written.
fn segment(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let mut group = [0_u8; 3];
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

/// An unsigned fixture JWT with `exp`.
fn jwt(exp: i64, subject: &str) -> String {
    format!(
        "{}.{}.{}",
        segment(br#"{"alg":"none","typ":"JWT"}"#),
        segment(format!(r#"{{"exp":{exp},"sub":"{subject}"}}"#).as_bytes()),
        segment(b"fixture-signature"),
    )
}

/// The login the way its owner wrote it: an odd indent, its own key order, keys no renewal
/// reads, and in `RepeatedValue` a copy of the refresh token under a second key.
fn auth_document(
    layout: AuthLayout,
    id: &str,
    access: &str,
    refresh: Option<&str>,
    last_refresh: &str,
) -> String {
    let refresh = refresh.map_or(String::new(), |refresh| {
        format!(",\n     \"refresh_token\": \"{refresh}\"")
    });
    let copy = match layout {
        AuthLayout::Codex => String::new(),
        AuthLayout::RepeatedValue => format!("  \"previous_refresh_token\": \"{OLD_REFRESH}\",\n"),
    };
    format!(
        "{{\n{copy}    \"OPENAI_API_KEY\": null,\n  \"tokens\": {{\"id_token\": \"{id}\", \"access_token\":  \"{access}\"{refresh}, \"account_id\": \"fixture-account\"}},\n  \"last_refresh\": \"{last_refresh}\",\n  \"zz_unknown\": [1, 2, {{\"keep\": true}}]\n}}\n"
    )
}

/// What the scripted endpoint saw.
#[derive(Default)]
struct EndpointSeen {
    requests: std::sync::atomic::AtomicUsize,
    received: std::sync::Mutex<Vec<(String, Vec<u8>)>>,
}

async fn read_renewal_request(socket: &mut tokio::net::TcpStream) -> Option<(String, Vec<u8>)> {
    use tokio::io::AsyncReadExt;
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    let end = loop {
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break end;
        }
        if bytes.len() > 64 * 1024 {
            return None;
        }
        let read = socket.read(&mut buffer).await.ok()?;
        if read == 0 {
            return None;
        }
        bytes.extend_from_slice(buffer.get(..read)?);
    };
    let head = String::from_utf8_lossy(bytes.get(..end)?).into_owned();
    let length: usize = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().ok())?
        })
        .unwrap_or(0);
    let mut body = bytes.get(end + 4..)?.to_vec();
    while body.len() < length {
        let read = socket.read(&mut buffer).await.ok()?;
        if read == 0 {
            return None;
        }
        body.extend_from_slice(buffer.get(..read)?);
    }
    Some((head, body))
}

/// Starts the endpoint: every connection is answered with `status` and `answer`, and
/// `before_answer` runs once, after the first request was read and before it is answered.
async fn start_endpoint(
    status: u16,
    answer: Vec<u8>,
    before_answer: impl FnOnce() + Send + 'static,
) -> Result<(String, Arc<EndpointSeen>), Box<dyn Error>> {
    use tokio::io::AsyncWriteExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/oauth/token", listener.local_addr()?);
    let seen = Arc::new(EndpointSeen::default());
    let log = seen.clone();
    tokio::spawn(async move {
        let mut hook = Some(before_answer);
        while let Ok((mut socket, _)) = listener.accept().await {
            log.requests
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let Some(request) = read_renewal_request(&mut socket).await else {
                continue;
            };
            log.received
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(request);
            if let Some(hook) = hook.take() {
                hook();
            }
            let head = format!(
                "HTTP/1.1 {status} Fixture\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                answer.len()
            );
            let _ = socket.write_all(head.as_bytes()).await;
            let _ = socket.write_all(&answer).await;
            let _ = socket.shutdown().await;
        }
    });
    Ok((url, seen))
}

/// Dispatches `llm.secrets.RenewCodexLogin`; `None` for any other command.
pub fn observe_renewal(
    command: &str,
    input: &Value,
) -> Option<Result<crate::target::Observed, ess_conformance::target::TargetError>> {
    if command != "llm.secrets.RenewCodexLogin" {
        return None;
    }
    Some(
        renew(input.clone())
            .map(|facts| crate::target::Observed {
                facts,
                view: "llm.secrets.LastRenewal",
                event: "llm.secrets.Renewed",
                field: "diagnostics_safe",
            })
            .map_err(|error| {
                ess_conformance::target::TargetError::unavailable(
                    "renewal observation",
                    error.to_string(),
                )
            }),
    )
}

#[expect(
    clippy::too_many_lines,
    reason = "one fixture, one call and its facts, in order"
)]
fn renew(input: Value) -> Result<Value, Box<dyn Error>> {
    use llm_credentials::codex::{CodexAuthFile, CodexRenewal, Renewal};
    let input: RenewInput = serde_json::from_value(input)?;
    let margin = std::time::Duration::from_secs(u64::try_from(integer(input.margin_s)?)?);
    let status = u16::try_from(integer(input.answer_status)?)?;
    let stale = match input.expires_in_s {
        Some(seconds) => jwt(RENEWAL_NOW + integer(seconds)?, "stale"),
        None => UNDATED_ACCESS.to_owned(),
    };
    let fresh = jwt(RENEWAL_NOW + 3600, "fresh");

    let scratch = Path::new("target/conformance");
    fs::create_dir_all(scratch)?;
    let temp = tempfile::tempdir_in(scratch)?;
    let path = temp.path().canonicalize()?.join("auth.json");
    let before = auth_document(
        input.layout,
        OLD_ID,
        &stale,
        input.refresh_token_present.then_some(OLD_REFRESH),
        "2026-10-01T00:00:00Z",
    );
    let mode = if input.group_readable { 0o640 } else { 0o600 };
    fs::write(&path, &before)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(mode))?;
    let concurrent = auth_document(
        input.layout,
        "concurrent-id",
        &jwt(RENEWAL_NOW + 7200, "concurrent"),
        Some("concurrent-refresh"),
        "2026-10-02T23:59:00Z",
    );
    let directory = path
        .parent()
        .ok_or("fixture path has no parent")?
        .to_path_buf();
    let mut resolver_path = path.clone();
    let mut rewritten: Option<Vec<u8>> = None;
    match input.fixture {
        None => {}
        Some(RenewalFixture::RelativePath) => {
            resolver_path = std::path::PathBuf::from("relative-fixture/auth.json");
        }
        Some(RenewalFixture::MissingFile) => fs::remove_file(&path)?,
        Some(RenewalFixture::NotJson) => rewritten = Some(b"not json".to_vec()),
        Some(RenewalFixture::DuplicateKey) => {
            let tokens =
                format!("{{\"access_token\": \"{stale}\", \"refresh_token\": \"{OLD_REFRESH}\"}}");
            rewritten =
                Some(format!("{{\"tokens\": {tokens}, \"tokens\": {tokens}}}").into_bytes());
        }
        // Authored 1 MiB boundary plus one, independent of the exported bound.
        Some(RenewalFixture::Oversized) => rewritten = Some(vec![b' '; 1_048_577]),
        Some(RenewalFixture::Symlink) => {
            let real = directory.join("real.json");
            fs::rename(&path, &real)?;
            symlink(&real, &path)?;
        }
        Some(RenewalFixture::Directory) => {
            fs::remove_file(&path)?;
            fs::create_dir(&path)?;
        }
        Some(RenewalFixture::HardLink) => fs::hard_link(&path, directory.join("second.json"))?,
        Some(RenewalFixture::UnwritableDirectory) => {
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o500))?;
        }
        // Cut inside the access token, as a reader sees a login being rewritten in place.
        Some(RenewalFixture::Truncated) => {
            let cut = before.find(&stale).ok_or("fixture holds no access token")? + stale.len() / 2;
            rewritten = Some(before.as_bytes()[..cut].to_vec());
        }
    }
    if let Some(bytes) = &rewritten {
        fs::write(&path, bytes)?;
    }
    let last_written = if let Some(bytes) = rewritten {
        bytes
    } else if input.concurrent_change {
        concurrent.clone().into_bytes()
    } else {
        before.clone().into_bytes()
    };

    let answer = if (200..300).contains(&status) {
        match input.answer {
            TokenAnswer::Rotated => {
                json!({"access_token": fresh, "refresh_token": NEW_REFRESH, "id_token": NEW_ID})
            }
            TokenAnswer::Kept => json!({"access_token": fresh, "refresh_token": OLD_REFRESH}),
            TokenAnswer::AccessOnly => json!({"access_token": fresh}),
            TokenAnswer::EmptyAccess => json!({"access_token": ""}),
            TokenAnswer::EmptyRefresh => json!({"access_token": fresh, "refresh_token": ""}),
            TokenAnswer::NonStringIdToken => json!({"access_token": fresh, "id_token": 42}),
        }
    } else {
        json!({"error": format!("{CANARY}-endpoint-answer")})
    };
    // What a renewal that changed only the token values and `/last_refresh` leaves on disk.
    let text = |key: &str| answer.get(key).and_then(Value::as_str).map(str::to_owned);
    let spliced = auth_document(
        input.layout,
        &text("id_token").unwrap_or_else(|| OLD_ID.to_owned()),
        &text("access_token").unwrap_or_default(),
        input
            .refresh_token_present
            .then(|| text("refresh_token").unwrap_or_else(|| OLD_REFRESH.to_owned()))
            .as_deref(),
        RENEWAL_NOW_TEXT,
    );

    let runtime = runtime()?;
    let (url, seen) =
        runtime.block_on(start_endpoint(status, answer.to_string().into_bytes(), {
            let (target, bytes) = (path.clone(), concurrent.clone());
            let change = input.concurrent_change;
            move || {
                if change {
                    let _ = fs::write(target, bytes);
                }
            }
        }))?;
    let renewal = CodexRenewal::new()?
        .with_endpoint(url, RENEWAL_CLIENT)
        .with_margin(margin);
    let file = CodexAuthFile::new(SecretRef::new("selected")?, &resolver_path).with_clock(|| {
        std::time::SystemTime::UNIX_EPOCH
            + std::time::Duration::from_secs(RENEWAL_NOW.unsigned_abs())
    });
    let reference = SecretRef::new("selected")?;

    let mut facts = json!({
        "outcome": null, "error_code": null, "resolver_error": null, "exchange_error": null,
        "exchange_retriable": null, "requests": 0, "request_well_formed": null,
        "refresh_token_rotated": null, "file_unchanged": false, "file_spliced": false,
        "mode_kept": false, "stray_files": 0, "resolved_renewed_token": null,
        "refusal_names_pointer": null, "diagnostics_safe": true
    });
    let mut diagnostics = format!("{renewal:?}");
    match input.entry {
        RenewalEntry::Renew => {
            match runtime.block_on(file.renew(&renewal, &llm_core::Cancel::new())) {
                Ok(outcome) => {
                    write!(diagnostics, "{outcome:?}")?;
                    facts["outcome"] = json!(match outcome {
                        Renewal::NotDue => "not-due",
                        Renewal::Undated => "undated",
                        Renewal::Renewed(_) => "renewed",
                    });
                    if let Renewal::Renewed(renewed) = outcome {
                        facts["refresh_token_rotated"] = json!(renewed.refresh_token_rotated);
                    }
                }
                Err(error) => {
                    write!(diagnostics, "{error} {error:?}")?;
                    facts["error_code"] = json!(error.refusal().code());
                    facts["resolver_error"] = json!(code(error.kind()));
                    facts["exchange_error"] =
                        error.exchange().map_or(Value::Null, |e| json!(e.code));
                    facts["exchange_retriable"] =
                        error.exchange().map_or(Value::Null, |e| json!(e.retriable));
                    facts["refusal_names_pointer"] =
                        json!(error.to_string().contains("/tokens/refresh_token"));
                }
            }
        }
        RenewalEntry::RenewingResolver => {
            let renewing = file.renewing(renewal);
            write!(diagnostics, "{renewing:?}")?;
            let attempts = if input.resolve_twice == Some(true) {
                2
            } else {
                1
            };
            for _ in 0..attempts {
                facts["resolver_error"] = Value::Null;
                facts["resolved_renewed_token"] = Value::Null;
                match runtime.block_on(renewing.resolve(&reference)) {
                    Ok(resolved) => {
                        write!(diagnostics, "{resolved:?}")?;
                        facts["resolved_renewed_token"] =
                            json!(resolved.secret.expose() == fresh.as_bytes());
                    }
                    Err(error) => facts["resolver_error"] = json!(code(error)),
                }
            }
        }
        RenewalEntry::ReadOnlyResolver => {
            write!(diagnostics, "{file:?}")?;
            match runtime.block_on(file.resolve(&reference)) {
                Ok(resolved) => {
                    write!(diagnostics, "{resolved:?}")?;
                    facts["resolved_renewed_token"] =
                        json!(resolved.secret.expose() == fresh.as_bytes());
                }
                Err(error) => facts["resolver_error"] = json!(code(error)),
            }
        }
    }

    let received = std::mem::take(
        &mut *seen
            .received
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    facts["requests"] = json!(seen.requests.load(std::sync::atomic::Ordering::SeqCst));
    if let [(head, body)] = received.as_slice() {
        let grant = json!({
            "client_id": RENEWAL_CLIENT, "grant_type": "refresh_token", "refresh_token": OLD_REFRESH
        });
        let json_body = head
            .lines()
            .any(|line| line.eq_ignore_ascii_case("content-type: application/json"));
        facts["request_well_formed"] =
            json!(json_body && serde_json::from_slice::<Value>(body).ok() == Some(grant));
    }
    drop(runtime);
    if input.fixture == Some(RenewalFixture::UnwritableDirectory) {
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
    }

    let after = fs::read(&path).ok();
    facts["file_unchanged"] = json!(after.as_deref() == Some(last_written.as_slice()));
    facts["file_spliced"] = json!(after.as_deref() == Some(spliced.as_bytes()));
    facts["mode_kept"] = json!(
        fs::metadata(&path).is_ok_and(|metadata| metadata.permissions().mode() & 0o7777 == mode)
    );
    facts["stray_files"] = json!(
        fs::read_dir(temp.path())?
            .filter(|entry| entry
                .as_ref()
                .is_ok_and(|entry| entry.file_name() != "auth.json"))
            .count()
    );
    let secrets = [stale.as_str(), fresh.as_str(), CANARY];
    facts["diagnostics_safe"] = json!(
        !secrets.iter().any(|secret| diagnostics.contains(secret))
            && !fresh
                .split('.')
                .chain(stale.split('.'))
                .filter(|part| part.len() > 24)
                .any(|part| diagnostics.contains(part))
    );
    Ok(facts)
}
