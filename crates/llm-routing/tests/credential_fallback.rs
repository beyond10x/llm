//! A credential whose JSON-pointer document is not in its declared form is a configuration
//! error. Through `prepare_auth` it becomes `Unauthorized`, `NotSent`, and the route does not fall
//! back to another target, as Harness refuses it `Unauthorized` without fallback. A source that is
//! merely unavailable still falls back, which shows the fixture can.
use llm_core::{
    AuthKind, BoxFuture, Cancel, Error, ErrorCode, Id, Item, Model, StopReason, StreamSink,
    TurnObservation, TurnOutcome, TurnRequest, VecSink,
};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
    pointer::JsonPointerResolver,
};
use llm_providers::Binding;
use llm_routing::{Catalog, CatalogDocument, FallbackPolicy, FallbackRun, Halt, Models, Ports};
use std::{
    collections::BTreeMap,
    future::Future,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
};

const CANARY: &str = "llm-fixture-private-marker";

fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
    }
}

/// The example catalog with fallback on, and its primary target's account authenticated by the
/// reference `lab-login`.
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
    local.secret_reference_id = Some(SecretRef::new("lab-login").unwrap());
    doc.validate().unwrap()
}

/// The credential store document behind the reference, or the store's own refusal.
struct Store(Result<&'static [u8], SecretError>);
impl SecretResolver for Store {
    fn resolve<'a>(
        &'a self,
        _: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async move {
            let document = self.0?;
            Ok(ResolvedSecret {
                secret: Secret::new(document.to_vec())?,
                version: SecretVersion::new("store-1".to_owned())?,
            })
        })
    }
}

/// A model that authenticates through its binding before every turn, then answers.
struct Authenticating<'c> {
    binding: &'c Binding,
    resolver: JsonPointerResolver,
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

/// Runs the route with the primary's credential store answering `store`.
fn run_route(store: Result<&'static [u8], SecretError>) -> (FallbackRun, usize, usize) {
    let catalog = catalog();
    let source = Arc::new(Store(store));
    let fleet = Fleet {
        primary: Authenticating {
            binding: catalog.binding(&id("local-small")).unwrap(),
            resolver: JsonPointerResolver::new(
                source,
                BTreeMap::from([(
                    SecretRef::new("lab-login").unwrap(),
                    "/claudeAiOauth/accessToken".to_owned(),
                )]),
            )
            .unwrap(),
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
    };
    let run =
        block_on(catalog.run_turn(&request, Some(100), FallbackPolicy::default(), ports)).unwrap();
    (
        run,
        fleet.primary.calls.load(Ordering::SeqCst),
        fleet.secondary.calls.load(Ordering::SeqCst),
    )
}

#[test]
fn a_malformed_pointer_document_is_unauthorized_and_never_falls_back() {
    for document in [
        &br#"{"claudeAiOauth":{"accessToken":17}}"#[..],
        br#"{"claudeAiOauth":{"accessToken":{"value":"llm-fixture-private-marker"}}}"#,
        b"llm-fixture-private-marker\n",
    ] {
        let (run, primary, secondary) = run_route(Ok(document));
        let shown = String::from_utf8_lossy(document);
        assert_eq!(run.halt, Halt::IneligibleFailure, "{shown}");
        assert_eq!((primary, secondary), (1, 0), "{shown}");
        let error = run.result.unwrap_err();
        assert_eq!(error.code, ErrorCode::Unauthorized, "{shown}");
        assert!(error.message.contains("`lab-login`"), "{}", error.message);
        assert!(!format!("{error} {error:?}").contains(CANARY), "{shown}");
    }
}

#[test]
fn an_unavailable_store_still_falls_back_and_a_good_one_answers() {
    let (run, primary, secondary) = run_route(Err(SecretError::Unavailable));
    assert_eq!(run.halt, Halt::Succeeded);
    assert_eq!((primary, secondary), (1, 1));

    let (run, primary, secondary) = run_route(Ok(
        br#"{"claudeAiOauth":{"accessToken":"llm-fixture-token"}}"#,
    ));
    assert_eq!(run.halt, Halt::Succeeded);
    assert_eq!((primary, secondary), (1, 0));
}
