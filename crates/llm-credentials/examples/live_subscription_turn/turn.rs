//! What one live subscription turn does and reports, shared by the example's `main.rs` and its
//! tests (`tests/live_subscription_turn.rs`), which run it on a mock store and a loopback fixture.
//!
//! A catalog built in code holds one `subscription-oauth` account on Messages, billed as
//! `subscription`, whose secret reference is `--name`. The token resolves through
//! [`SecretsResolver::keychain`] in scope `default/<namespace>/default`, which is where
//! `secretsctl put <name> --namespace <namespace>` stores it on the default keychain mount. The
//! report carries names, kinds, counts and versions' equality; never the token or a header value.
use clap::Parser;
use keyring_core::CredentialStore;
use llm_core::{
    AuthKind, BillingKind, BoxFuture, Cancel, Error, ErrorCode, Item, Model, StopReason,
    TurnRequest, Usage, VecSink,
};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
    secrets::{
        SecretsResolver,
        keychain::DEFAULT_SERVICE,
        storage::{Scope, ScopeName, SecretName},
    },
};
use llm_http::{HeaderMap, HttpClient, Limits, ResponseTap};
use llm_messages::{
    ANTHROPIC_VERSION, BETA_HEADER, MessagesClient, SUBSCRIPTION_CLIENT_PREAMBLE, VERSION_HEADER,
    encode_request,
};
use llm_providers::Binding;
use llm_routing::CatalogDocument;
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub const DEFAULT_NAMESPACE: &str = "llm";
pub const DEFAULT_NAME: &str = "anthropic-subscription";
pub const DEFAULT_MODEL: &str = "claude-haiku-4-5";
pub const DEFAULT_ENDPOINT: &str = "https://api.anthropic.com";
pub const DEFAULT_PROMPT: &str = "Reply with the single word: ok";

const FORMAT: &str = "llm.live-subscription-turn/1";
const SERVING: &str = "live-subscription";
const CATALOG_MODEL: &str = "live-model";
const MAX_OUTPUT_TOKENS: u64 = 64;
const MAX_REPLY_CHARS: usize = 400;
const TURN_LIMIT: Duration = Duration::from_secs(120);
/// What this run's request does not use, so the report does not qualify it.
const NOT_EXERCISED: [&str; 6] = [
    "tools",
    "tool-choice",
    "sampling",
    "reasoning-effort",
    "fallback",
    "api-key-route",
];

/// One turn over the operator's own subscription token, resolved from the platform keychain
/// through the secrets library. Prints a JSON report on stdout; never prints the token.
#[derive(Debug, Parser)]
#[command(name = "live_subscription_turn")]
pub struct Args {
    /// Secrets namespace holding the token (tenant and user `default`, as `secretsctl` stores).
    #[arg(long, default_value = DEFAULT_NAMESPACE)]
    pub namespace: String,
    /// Secret name of the token in that namespace.
    #[arg(long, default_value = DEFAULT_NAME)]
    pub name: String,
    /// Upstream model name sent in the request.
    #[arg(long, default_value = DEFAULT_MODEL)]
    pub model: String,
    /// The user message of the turn.
    #[arg(long, default_value = DEFAULT_PROMPT)]
    pub prompt: String,
    /// Origin of the Messages API; the request goes to `<endpoint>/v1/messages`.
    #[arg(long, default_value = DEFAULT_ENDPOINT)]
    pub endpoint: String,
    /// After the first turn, wait for the token to be rotated in the store, run a second turn
    /// and report whether the resolved credential version changed.
    #[arg(long)]
    pub rotate_check: bool,
    /// Write what the route sent back (status line, response headers and the event stream as
    /// it arrived) to this new file, mode 0600 on Unix. Nothing the client sent is written: no
    /// request header and no credential. The file must not exist yet.
    #[arg(long, value_name = "PATH")]
    pub capture_response: Option<PathBuf>,
}

/// Where the token was read from. All of it is non-secret lookup metadata.
#[derive(Debug, Serialize)]
pub struct SecretLocation {
    pub backend: &'static str,
    pub service: &'static str,
    pub tenant: &'static str,
    pub namespace: String,
    pub user: &'static str,
    pub name: String,
}

/// One attempt: its outcome, latency and what the route reported, or the typed error.
#[derive(Debug, Serialize)]
pub struct TurnReport {
    pub outcome: &'static str,
    pub latency_ms: u64,
    pub stop_reason: Option<StopReason>,
    pub usage: Option<Usage>,
    pub upstream_model: Option<String>,
    pub response_id: Option<String>,
    pub reply: Option<String>,
    pub error: Option<Error>,
}

/// The two resolutions of a rotation check: how many there were and whether the versions differ.
#[derive(Debug, Serialize)]
pub struct Rotation {
    pub resolutions: usize,
    pub version_changed: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub format: &'static str,
    pub endpoint: Option<String>,
    pub auth_kind: AuthKind,
    pub billing_kind: BillingKind,
    pub secret: SecretLocation,
    /// The header names the Messages client sends for this binding; values are never reported.
    pub request_headers: Vec<String>,
    pub oauth_beta_sent: bool,
    pub anthropic_version: &'static str,
    pub system_opens_with_preamble: bool,
    pub model: String,
    pub not_exercised: [&'static str; 6],
    /// A refusal before any turn ran: the store, the arguments or the catalog.
    pub refused: Option<Error>,
    pub turns: Vec<TurnReport>,
    pub rotation: Option<Rotation>,
    /// What `--capture-response` wrote; its path is the operator's and is not repeated here.
    pub capture: Option<CaptureReport>,
}

impl Report {
    fn new(args: &Args) -> Self {
        Self {
            format: FORMAT,
            endpoint: None,
            auth_kind: AuthKind::SubscriptionOauth,
            billing_kind: BillingKind::Subscription,
            secret: SecretLocation {
                backend: "secrets keychain",
                service: DEFAULT_SERVICE,
                tenant: ScopeName::DEFAULT,
                namespace: args.namespace.clone(),
                user: ScopeName::DEFAULT,
                name: args.name.clone(),
            },
            request_headers: Vec::new(),
            oauth_beta_sent: false,
            anthropic_version: ANTHROPIC_VERSION,
            system_opens_with_preamble: false,
            model: args.model.clone(),
            not_exercised: NOT_EXERCISED,
            refused: None,
            turns: Vec::new(),
            rotation: None,
            capture: None,
        }
    }

    /// Every turn completed, and a rotation check, when asked for, saw the version change.
    pub fn succeeded(&self) -> bool {
        self.refused.is_none()
            && !self.turns.is_empty()
            && self.turns.iter().all(|turn| turn.error.is_none())
            && self
                .rotation
                .as_ref()
                .is_none_or(|rotation| rotation.version_changed == Some(true))
    }
}

/// The report as the example prints it.
pub fn render(report: &Report) -> String {
    serde_json::to_string_pretty(report).unwrap_or_else(|_| {
        json!({"format": FORMAT, "refused": "the report could not be encoded"}).to_string()
    })
}

/// Resolves through the secrets library and keeps each resolution's version, never its value.
struct Recording {
    inner: SecretsResolver,
    versions: Mutex<Vec<SecretVersion>>,
}

impl SecretResolver for Recording {
    fn resolve<'a>(
        &'a self,
        reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async move {
            let resolved = self.inner.resolve(reference).await?;
            self.versions
                .lock()
                .map_err(|_| SecretError::Unavailable)?
                .push(resolved.version.clone());
            Ok(resolved)
        })
    }
}

/// Stands in for the credential when only the names of the authentication headers are wanted.
struct Placeholder;

impl SecretResolver for Placeholder {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async {
            Ok(ResolvedSecret {
                secret: Secret::new(b"placeholder".to_vec())?,
                version: SecretVersion::new("placeholder".to_owned())?,
            })
        })
    }
}

/// How much of the response `--capture-response` wrote, and whether every write succeeded.
#[derive(Debug, Serialize)]
pub struct CaptureReport {
    pub bytes: u64,
    pub complete: bool,
}

/// The capture file, shown each streamed response by the transport's [`ResponseTap`], which is
/// never shown the request. A turn of the rotation check appends after the one before it.
struct Capture {
    file: Mutex<File>,
    written: Mutex<CaptureReport>,
}

impl Capture {
    /// Creates `path` new, readable and writable by its owner alone on Unix; an existing file is
    /// refused rather than overwritten or left with wider permissions.
    fn create(path: &Path) -> Result<Self, Error> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let file = options.open(path).map_err(|_| {
            Error::invalid("--capture-response could not be created; it must name a new file")
        })?;
        Ok(Self {
            file: Mutex::new(file),
            written: Mutex::new(CaptureReport {
                bytes: 0,
                complete: true,
            }),
        })
    }

    fn write(&self, bytes: &[u8]) {
        let wrote = self
            .file
            .lock()
            .is_ok_and(|mut file| file.write_all(bytes).is_ok());
        if let Ok(mut written) = self.written.lock() {
            if wrote {
                written.bytes += bytes.len() as u64;
            } else {
                written.complete = false;
            }
        }
    }

    fn report(&self) -> CaptureReport {
        self.written.lock().map_or(
            CaptureReport {
                bytes: 0,
                complete: false,
            },
            |written| CaptureReport {
                bytes: written.bytes,
                complete: written.complete,
            },
        )
    }
}

impl ResponseTap for Capture {
    fn head(&self, status_line: &str, headers: &HeaderMap) {
        let mut head = Vec::new();
        head.extend_from_slice(status_line.as_bytes());
        head.extend_from_slice(b"\r\n");
        for (name, value) in headers {
            head.extend_from_slice(name.as_str().as_bytes());
            head.extend_from_slice(b": ");
            head.extend_from_slice(value.as_bytes());
            head.extend_from_slice(b"\r\n");
        }
        head.extend_from_slice(b"\r\n");
        self.write(&head);
    }

    fn chunk(&self, bytes: &[u8]) {
        self.write(bytes);
    }
}

fn catalog_binding(args: &Args) -> Result<Binding, Error> {
    let base_url = format!("{}/v1", args.endpoint.trim_end_matches('/'));
    let document = json!({
        "format": "llm.catalog/1",
        "providers": [{"id": "anthropic", "category": "hosted"}],
        "accounts": [{
            "id": "operator-subscription",
            "provider_id": "anthropic",
            "auth_kind": "subscription-oauth",
            "billing_kind": "subscription",
            "secret_reference_id": args.name,
        }],
        "endpoints": [{
            "id": "anthropic-messages",
            "account_id": "operator-subscription",
            "base_url": base_url,
        }],
        "models": [{"id": CATALOG_MODEL, "upstream_name": args.model}],
        "serving_models": [{
            "id": SERVING,
            "endpoint_id": "anthropic-messages",
            "model_id": CATALOG_MODEL,
            "protocol": "messages",
            "capabilities": {
                "tools": false,
                "tool_choice": false,
                "temperature": false,
                "top_p": false,
                "reasoning_efforts": [],
                "context_window": 200_000,
                "max_output_tokens": 1024,
            },
        }],
        "routes": [{"id": "live", "alias": "live", "fallback_enabled": false}],
        "targets": [{
            "id": "live-target",
            "route_id": "live",
            "serving_model_id": SERVING,
            "position": 0,
        }],
    });
    let catalog = serde_json::from_value::<CatalogDocument>(document)
        .map_err(|_| Error::invalid("--endpoint, --model or --name is not a valid declaration"))?
        .validate()?;
    llm_core::Id::new(SERVING)
        .ok()
        .and_then(|id| catalog.binding(&id).cloned())
        .ok_or_else(|| Error::invalid("the catalog holds no live serving model"))
}

/// The header names the Messages client sends for `binding`: the authentication header its
/// account presents (resolved here with a placeholder, so no credential is read), the fixed
/// content headers, the API version and, for a subscription token, the beta header. The test
/// holds this list to the headers a loopback fixture receives.
async fn header_names(binding: &Binding) -> Result<Vec<String>, Error> {
    let (auth, _) = binding
        .prepare_auth(&Placeholder, &Cancel::new())
        .await?
        .into_parts();
    let mut names: BTreeSet<String> = auth.keys().map(|name| name.as_str().to_owned()).collect();
    names.extend(["accept", "content-type", VERSION_HEADER].map(str::to_owned));
    if binding.declaration().account.auth_kind == AuthKind::SubscriptionOauth {
        names.insert(BETA_HEADER.to_owned());
    }
    Ok(names.into_iter().collect())
}

/// Whether the body the client encodes opens `system` with the preamble as its own block.
fn opens_with_preamble(request: &TurnRequest, binding: &Binding) -> Result<bool, Error> {
    let body: Value = serde_json::from_slice(&encode_request(request, binding)?)
        .map_err(|_| Error::protocol("the encoded request is not JSON"))?;
    Ok(body["system"][0] == json!({"type": "text", "text": SUBSCRIPTION_CLIENT_PREAMBLE}))
}

async fn one_turn(client: &MessagesClient, request: &TurnRequest) -> TurnReport {
    let mut sink = VecSink::new(4096, 64 * 1024);
    let started = Instant::now();
    let result = client.turn(request, &mut sink, &Cancel::new()).await;
    let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    match result {
        Ok(outcome) => TurnReport {
            outcome: "completed",
            latency_ms,
            stop_reason: Some(outcome.stop_reason),
            usage: outcome.observation.usage,
            upstream_model: outcome.observation.upstream_model.map(|id| id.to_string()),
            response_id: outcome.observation.response_id.map(|id| id.to_string()),
            reply: Some(sink.text().chars().take(MAX_REPLY_CHARS).collect()),
            error: None,
        },
        Err(error) => TurnReport {
            outcome: "error",
            latency_ms,
            stop_reason: None,
            usage: None,
            upstream_model: None,
            response_id: None,
            reply: None,
            error: Some(error),
        },
    }
}

async fn turns(
    report: &mut Report,
    args: &Args,
    store: Result<Arc<CredentialStore>, SecretError>,
    rotate: impl AsyncFnOnce(),
) -> Result<(), Error> {
    let store = store.map_err(|error| {
        Error::new(
            ErrorCode::Unavailable,
            format!("the platform keychain could not be opened: {error}"),
        )
    })?;
    let namespace = ScopeName::parse(&args.namespace)
        .map_err(|_| Error::invalid("--namespace is not a secrets namespace name"))?;
    SecretName::parse(&args.name).map_err(|_| Error::invalid("--name is not a secrets name"))?;
    let binding = catalog_binding(args)?;
    report.endpoint = Some(binding.request_url().to_owned());

    let mut request = TurnRequest::new(CATALOG_MODEL, vec![Item::user(args.prompt.clone())]);
    request.max_output_tokens = Some(MAX_OUTPUT_TOKENS);
    report.request_headers = header_names(&binding).await?;
    report.oauth_beta_sent = report
        .request_headers
        .iter()
        .any(|name| name == BETA_HEADER);
    report.system_opens_with_preamble = opens_with_preamble(&request, &binding)?;
    let capture = args
        .capture_response
        .as_deref()
        .map(Capture::create)
        .transpose()?
        .map(Arc::new);

    let resolver = Arc::new(Recording {
        inner: SecretsResolver::keychain(
            store,
            Scope {
                namespace,
                ..Scope::local()
            },
        ),
        versions: Mutex::new(Vec::new()),
    });
    let mut http = HttpClient::new(Limits {
        response_headers: TURN_LIMIT,
        idle: TURN_LIMIT,
        total: TURN_LIMIT,
    })?;
    if let Some(capture) = &capture {
        http = http.with_response_tap(capture.clone());
    }
    let client = MessagesClient::new(binding, http, resolver.clone())?;

    report.turns.push(one_turn(&client, &request).await);
    report.capture = capture.as_ref().map(|capture| capture.report());
    if !args.rotate_check {
        return Ok(());
    }
    if report.turns.iter().all(|turn| turn.error.is_none()) {
        rotate().await;
        report.turns.push(one_turn(&client, &request).await);
        report.capture = capture.as_ref().map(|capture| capture.report());
    }
    let versions = resolver
        .versions
        .lock()
        .map_err(|_| Error::new(ErrorCode::Unavailable, "the version record is poisoned"))?;
    report.rotation = Some(Rotation {
        resolutions: versions.len(),
        version_changed: match versions.as_slice() {
            [first, second] => Some(first != second),
            _ => None,
        },
    });
    Ok(())
}

/// Runs the turn, and with `--rotate-check` calls `rotate` between the first turn and the
/// second. Every failure lands in the report; nothing is printed here.
pub async fn run(
    args: &Args,
    store: Result<Arc<CredentialStore>, SecretError>,
    rotate: impl AsyncFnOnce(),
) -> Report {
    let mut report = Report::new(args);
    if let Err(error) = turns(&mut report, args, store, rotate).await {
        report.refused = Some(error);
    }
    report
}
