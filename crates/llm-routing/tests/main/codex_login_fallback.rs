//! A misconfigured Codex login file is a configuration error. Through `prepare_auth` it becomes
//! `Unauthorized` and the route does not fall back to another target, as a malformed JSON-pointer
//! document does not (`credential_fallback.rs`). A login cut off mid-document, which is what a
//! reader sees while the file is rewritten in place, stays `Unavailable` and still falls back,
//! which shows the fixture can. Fixture files only: no test reads a real Codex login.
use llm_core::{
    AuthKind, BoxFuture, Cancel, Error, ErrorCode, Id, Item, Model, StopReason, StreamSink,
    TurnObservation, TurnOutcome, TurnRequest, VecSink,
};
use llm_credentials::{SecretRef, codex::CodexAuthFile};
use llm_providers::Binding;
use llm_routing::{Catalog, CatalogDocument, FallbackPolicy, FallbackRun, Halt, Models, Ports};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, SystemTime},
};

/// The caller's clock in every case: 2026-10-03T00:00:00Z.
const NOW: u64 = 1_790_985_600;
/// Carried in every fixture token, so a diagnostic that quotes one is caught.
const CANARY: &str = "llm-fixture-private-marker";

fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}

fn base64url(bytes: &[u8]) -> String {
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

/// An unsigned fixture JWT whose payload is `claims`.
fn jwt(claims: &str) -> String {
    format!(
        "{}.{}.{}",
        base64url(br#"{"alg":"none","typ":"JWT"}"#),
        base64url(claims.as_bytes()),
        base64url(CANARY.as_bytes()),
    )
}

fn live() -> String {
    jwt(&format!(r#"{{"exp":{},"sub":"{CANARY}"}}"#, NOW + 3600))
}

/// A Codex login in the layout the Codex CLI writes, holding `access_token` as a JSON value.
fn login(access_token: &serde_json::Value) -> String {
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

/// The example catalog with fallback on, and its primary target's account authenticated by the
/// reference `codex-login`.
fn catalog() -> Catalog {
    let path = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../examples/catalog.toml");
    let mut doc = CatalogDocument::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
    doc.routes[0].fallback_enabled = true;
    let local = doc
        .accounts
        .iter_mut()
        .find(|account| account.id.as_str() == "local")
        .unwrap();
    local.auth_kind = AuthKind::Bearer;
    local.secret_reference_id = Some(SecretRef::new("codex-login").unwrap());
    doc.validate().unwrap()
}

/// A model that authenticates through its binding before every turn, then answers.
struct Authenticating<'c> {
    binding: &'c Binding,
    resolver: CodexAuthFile,
    calls: AtomicUsize,
}

/// A model that always answers.
struct Answering<'c> {
    binding: &'c Binding,
    calls: AtomicUsize,
}

fn answer(binding: &Binding) -> TurnOutcome {
    let mut observation = TurnObservation::new(binding.provenance().clone());
    observation.final_usage = true;
    TurnOutcome {
        stop_reason: StopReason::EndTurn,
        items: vec![Item::assistant("done")],
        observation,
    }
}

impl Model for Authenticating<'_> {
    fn provenance(&self) -> &llm_core::Provenance {
        self.binding.provenance()
    }
    fn capabilities(&self) -> &llm_core::Capabilities {
        self.binding.capabilities()
    }
    fn turn<'a>(
        &'a self,
        _: &'a TurnRequest,
        _: &'a mut dyn StreamSink,
        cancel: &'a Cancel,
    ) -> BoxFuture<'a, Result<TurnOutcome, Error>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.binding.prepare_auth(&self.resolver, cancel).await?;
            Ok(answer(self.binding))
        })
    }
}

impl Model for Answering<'_> {
    fn provenance(&self) -> &llm_core::Provenance {
        self.binding.provenance()
    }
    fn capabilities(&self) -> &llm_core::Capabilities {
        self.binding.capabilities()
    }
    fn turn<'a>(
        &'a self,
        _: &'a TurnRequest,
        _: &'a mut dyn StreamSink,
        _: &'a Cancel,
    ) -> BoxFuture<'a, Result<TurnOutcome, Error>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(answer(self.binding))
        })
    }
}

struct Fleet<'c> {
    primary: Authenticating<'c>,
    secondary: Answering<'c>,
}
impl Models for Fleet<'_> {
    fn model(&self, serving_model_id: &Id) -> Option<&dyn Model> {
        match serving_model_id.as_str() {
            "local-small" => Some(&self.primary),
            "remote-large" => Some(&self.secondary),
            _ => None,
        }
    }
}

/// A fresh directory for one case, under this build's test scratch directory.
fn case_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "codex-login-fallback-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// Runs the route with the primary's Codex login file holding `content`.
async fn run_route(case: &str, content: &[u8]) -> (FallbackRun, usize, usize) {
    let dir = case_dir(case);
    let path = dir.join("auth.json");
    std::fs::write(&path, content).unwrap();
    let catalog = catalog();
    let fleet = Fleet {
        primary: Authenticating {
            binding: catalog.binding(&id("local-small")).unwrap(),
            resolver: CodexAuthFile::new(SecretRef::new("codex-login").unwrap(), &path)
                .with_clock(|| SystemTime::UNIX_EPOCH + Duration::from_secs(NOW)),
            calls: AtomicUsize::new(0),
        },
        secondary: Answering {
            binding: catalog.binding(&id("remote-large")).unwrap(),
            calls: AtomicUsize::new(0),
        },
    };
    let mut request = TurnRequest::new("code", vec![Item::user("hello")]);
    request.max_output_tokens = Some(128);
    let cancel = Cancel::new();
    let mut sink = VecSink::new(16, 4096);
    let mut admit = |_: &llm_routing::Selection<'_>| Ok(());
    let ports = Ports {
        models: &fleet,
        admit: &mut admit,
        sink: &mut sink,
        cancel: &cancel,
        pause: &|_| Box::pin(std::future::ready(())),
    };
    let run = catalog
        .run_turn(&request, Some(100), FallbackPolicy::default(), ports)
        .await
        .unwrap();
    let calls = (
        fleet.primary.calls.load(Ordering::SeqCst),
        fleet.secondary.calls.load(Ordering::SeqCst),
    );
    std::fs::remove_dir_all(&dir).unwrap();
    (run, calls.0, calls.1)
}

/// Each way a Codex login read whole can fail to be one: the document, then the token's `exp`.
#[tokio::test]
async fn a_misconfigured_codex_login_is_unauthorized_and_never_falls_back() {
    let token = live();
    let tokens = format!(r#"{{"access_token":"{token}"}}"#);
    let cases: Vec<(&str, String)> = vec![
        ("not-json", format!("{CANARY}\n")),
        (
            "tokens-twice",
            format!(r#"{{"tokens":{tokens},"tokens":{tokens}}}"#),
        ),
        (
            "tokens-not-an-object",
            format!(r#"{{"tokens":["{token}"]}}"#),
        ),
        ("document-not-an-object", format!("[{tokens}]")),
        ("token-not-a-string", login(&serde_json::json!(17))),
        (
            "token-not-a-jwt",
            login(&serde_json::json!(format!("{CANARY}-opaque"))),
        ),
        (
            "exp-a-string",
            login(&serde_json::json!(jwt(&format!(
                r#"{{"exp":"{}","sub":"{CANARY}"}}"#,
                NOW + 3600
            )))),
        ),
        (
            "exp-absent",
            login(&serde_json::json!(jwt(&format!(r#"{{"sub":"{CANARY}"}}"#)))),
        ),
        (
            "exp-a-float",
            login(&serde_json::json!(jwt(&format!(
                r#"{{"exp":{}.5,"sub":"{CANARY}"}}"#,
                NOW + 3600
            )))),
        ),
    ];
    for (case, content) in cases {
        let (run, primary, secondary) = run_route(case, content.as_bytes()).await;
        assert_eq!(run.halt, Halt::IneligibleFailure, "{case}");
        assert_eq!((primary, secondary), (1, 0), "{case}");
        let error = run.result.unwrap_err();
        assert_eq!(error.code, ErrorCode::Unauthorized, "{case}");
        assert!(error.message.contains("`codex-login`"), "{}", error.message);
        assert!(!format!("{error} {error:?}").contains(CANARY), "{case}");
    }
}

/// A login that ends before its document does (an empty file included) is what a reader sees
/// while the file is rewritten in place: still `Unavailable`, so the route still falls back. A
/// good login answers from the primary.
#[tokio::test]
async fn a_truncated_codex_login_still_falls_back_and_a_good_one_answers() {
    let whole = login(&serde_json::json!(live()));
    let cut = whole.find("\"refresh_token\"").unwrap();
    for (case, content) in [
        ("cut-in-tokens", &whole.as_bytes()[..cut]),
        ("empty", &b""[..]),
    ] {
        let (run, primary, secondary) = run_route(case, content).await;
        assert_eq!(run.halt, Halt::Succeeded, "{case}");
        assert_eq!((primary, secondary), (1, 1), "{case}");
    }

    let (run, primary, secondary) = run_route("good", whole.as_bytes()).await;
    assert_eq!(run.halt, Halt::Succeeded);
    assert_eq!((primary, secondary), (1, 0));
}
