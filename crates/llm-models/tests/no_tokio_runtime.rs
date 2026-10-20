//! A turn driven by an executor that is not Tokio (story:async-clients-without-a-tokio-runtime).
//!
//! Every protocol client and every catalog port, polled by [`block_on`] below on a thread that is
//! in no Tokio runtime, is refused with the typed error that names the requirement and the
//! adapter that serves such a caller, before it resolves a credential or opens a connection, and
//! without a panic. Each test runs every wire before it asserts, so a failure reports all of them.
//! `docs/design.md` ("Executor") records what the same calls did before. Local sockets only; no
//! provider call.

use llm_chat::ChatClient;
use llm_core::{
    BoxFuture, Cancel, Dispatch, Error, ErrorCode, Id, Item, Model, Protocol, TurnOutcome,
    TurnRequest, VecSink,
};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_http::{HttpClient, Limits};
use llm_messages::MessagesClient;
use llm_models::{CatalogModels, port};
use llm_providers::Binding;
use llm_responses::ResponsesClient;
use llm_routing::{Catalog, Models};
use std::{
    any::Any,
    collections::BTreeMap,
    future::Future,
    io::ErrorKind,
    net::TcpListener,
    panic::{AssertUnwindSafe, catch_unwind},
    pin::pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    thread::{self, Thread},
};

/// The error's whole message: the requirement, and the adapter a caller without Tokio uses.
const REQUIREMENT: &str =
    "an llm client needs a Tokio runtime; a caller without one uses b10x-llm-blocking";

/// Wakes the thread [`block_on`] parked.
struct Unpark(Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// An executor that is not Tokio: polls `future` on the calling thread and parks the thread
/// until the future's waker is called. Standard library only.
fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
        thread::park();
    }
}

/// A resolver that answers every reference with a fixed token, and counts the calls.
#[derive(Default)]
struct Counting(AtomicUsize);

impl SecretResolver for Counting {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            Ok(ResolvedSecret {
                secret: Secret::new(b"fixture-token".to_vec())?,
                version: SecretVersion::new("fixture-1".to_owned())?,
            })
        })
    }
}

fn id(value: &str) -> Id {
    Id::new(value).expect("an identifier")
}

fn http() -> HttpClient {
    HttpClient::new(Limits::default()).expect("a bounded transport")
}

/// A local endpoint that nothing is expected to reach, and its base URL.
fn endpoint() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
    listener
        .set_nonblocking(true)
        .expect("a non-blocking listener");
    let url = format!("http://{}/v1", listener.local_addr().expect("an address"));
    (listener, url)
}

/// Whether anything connected to `listener` since it was last asked.
fn connected(listener: &TcpListener) -> bool {
    match listener.accept() {
        Ok(_) => true,
        Err(error) if error.kind() == ErrorKind::WouldBlock => false,
        Err(error) => panic!("the fixture listener failed: {error}"),
    }
}

/// One serving model of the `lab-models` endpoint, over `protocol`.
fn serving_model(serving: &str, protocol: &str) -> String {
    format!(
        r#"
[[serving_models]]
id = "{serving}"
endpoint_id = "lab-models"
model_id = "small"
protocol = "{protocol}"
[serving_models.capabilities]
tools = false
tool_choice = false
temperature = false
top_p = false
reasoning_efforts = []
context_window = 4096
max_output_tokens = 1024
"#
    )
}

/// One bearer account at `url`, serving one model over each of the three protocols.
fn catalog(url: &str) -> Catalog {
    let mut source = format!(
        r#"format = "llm.catalog/1"

[[providers]]
id = "my-lab"
category = "self-hosted"

[[accounts]]
id = "lab"
provider_id = "my-lab"
auth_kind = "bearer"
billing_kind = "metered"
secret_reference_id = "lab-token"

[[endpoints]]
id = "lab-models"
account_id = "lab"
base_url = "{url}"

[[models]]
id = "small"
upstream_name = "example/Small-Model"
"#
    );
    for (serving, protocol) in [
        ("lab-chat", "chat-completions"),
        ("lab-responses", "responses"),
        ("lab-messages", "messages"),
    ] {
        source.push_str(&serving_model(serving, protocol));
    }
    source.push_str(
        r#"
[[routes]]
id = "coding"
alias = "code"
fallback_enabled = false

[[targets]]
id = "coding-primary"
route_id = "coding"
serving_model_id = "lab-chat"
position = 0
"#,
    );
    Catalog::parse(&source).expect("a valid catalog")
}

const SERVING: [&str; 3] = ["lab-chat", "lab-responses", "lab-messages"];

/// The protocol client a caller builds without the catalog.
fn client(binding: Binding, resolver: Arc<dyn SecretResolver>) -> Box<dyn Model> {
    match binding.provenance().protocol {
        Protocol::ChatCompletions => Box::new(ChatClient::new(binding, http(), resolver)),
        Protocol::Responses => {
            Box::new(ResponsesClient::new(binding, http(), resolver).expect("a Responses client"))
        }
        Protocol::Messages => {
            Box::new(MessagesClient::new(binding, http(), resolver).expect("a Messages client"))
        }
    }
}

/// One turn of `model`, polled by [`block_on`] on this thread, which is in no Tokio runtime. A
/// panic is caught and returned as its message.
fn turn_without_tokio(model: &dyn Model) -> Result<Result<TurnOutcome, Error>, String> {
    let mut request = TurnRequest::new("small", vec![Item::user("Hallo")]);
    request.max_output_tokens = Some(64);
    let mut sink = VecSink::new(64, 64 * 1024);
    let cancel = Cancel::new();
    catch_unwind(AssertUnwindSafe(|| {
        block_on(model.turn(&request, &mut sink, &cancel))
    }))
    .map_err(|payload| panic_message(&*payload))
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic without a message".to_owned())
}

/// Every way `outcome` departs from the refusal a call outside a Tokio runtime gets: typed,
/// final, sent nothing, and naming both the requirement and `b10x-llm-blocking`.
fn departures(outcome: &Result<Result<TurnOutcome, Error>, String>) -> Vec<String> {
    let error = match outcome {
        Err(panic) => return vec![format!("panicked: {panic}")],
        Ok(Ok(_)) => return vec!["returned an outcome".to_owned()],
        Ok(Err(error)) => error,
    };
    let mut found = Vec::new();
    if error.code != ErrorCode::Unsupported {
        found.push(format!("code {:?}", error.code));
    }
    if error.dispatch != Dispatch::NotSent {
        found.push(format!("dispatch {:?}", error.dispatch));
    }
    if error.retriable || error.may_retry() {
        found.push("retriable".to_owned());
    }
    if error.observation.is_some() {
        found.push("carries an observation".to_owned());
    }
    if error.message != REQUIREMENT {
        found.push(format!("message {:?}", error.message));
    }
    found
}

/// What one turn outside a runtime did that it must not have: its departures from the refusal,
/// a credential resolved, a connection made.
fn findings(
    what: &str,
    model: &dyn Model,
    counting: Option<&Counting>,
    listener: &TcpListener,
) -> Vec<String> {
    let mut found = departures(&turn_without_tokio(model));
    if let Some(resolved) = counting
        .map(|counting| counting.0.swap(0, Ordering::SeqCst))
        .filter(|resolved| *resolved > 0)
    {
        found.push(format!("resolved the credential {resolved} time(s)"));
    }
    if connected(listener) {
        found.push("reached the endpoint".to_owned());
    }
    found
        .into_iter()
        .map(|finding| format!("{what}: {finding}"))
        .collect()
}

fn assert_no_findings(found: &[String]) {
    assert!(
        found.is_empty(),
        "outside a Tokio runtime:\n{}",
        found.join("\n")
    );
}

/// Each protocol client built directly, as a caller without the catalog builds it, is refused
/// before its resolver is asked for the credential and before anything connects.
#[test]
fn every_protocol_client_outside_a_tokio_runtime_is_refused_before_it_resolves_or_sends() {
    let (listener, url) = endpoint();
    let catalog = catalog(&url);
    let counting = Arc::new(Counting::default());
    let mut found = Vec::new();
    for serving in SERVING {
        let binding = catalog.binding(&id(serving)).expect("declared").clone();
        let model = client(binding, counting.clone());
        found.extend(findings(serving, &*model, Some(&counting), &listener));
    }
    assert_no_findings(&found);
}

/// The same through `b10x-llm-models`: a port built for one serving model, and the port the
/// catalog builder serves to routing.
#[test]
fn every_catalog_port_outside_a_tokio_runtime_is_refused_before_it_resolves_or_sends() {
    let (listener, url) = endpoint();
    let catalog = catalog(&url);
    let counting = Arc::new(Counting::default());
    let resolvers: BTreeMap<Id, Arc<dyn SecretResolver>> =
        BTreeMap::from([(id("lab"), counting.clone() as Arc<dyn SecretResolver>)]);
    let models = CatalogModels::build(&catalog, &http(), &resolvers).expect("every port builds");
    let mut found = Vec::new();
    for serving in SERVING {
        let built = port(&catalog, &id(serving), http(), &resolvers).expect("a port");
        found.extend(findings(
            &format!("port {serving}"),
            &built,
            Some(&counting),
            &listener,
        ));
        let served = Models::model(&models, &id(serving)).expect("a served port");
        found.extend(findings(
            &format!("catalog port {serving}"),
            served,
            Some(&counting),
            &listener,
        ));
    }
    assert_no_findings(&found);
}

/// A file credential is read on Tokio's blocking pool. Outside a runtime it is never reached.
#[cfg(feature = "file")]
#[test]
fn a_file_credential_is_never_read_outside_a_tokio_runtime() {
    use llm_credentials::file::FileResolver;
    use std::path::PathBuf;
    let (listener, url) = endpoint();
    let catalog = catalog(&url);
    let token = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("no-tokio-runtime-token");
    let resolver = Arc::new(
        FileResolver::new(BTreeMap::from([(
            SecretRef::new("lab-token").expect("a reference"),
            token,
        )]))
        .expect("an absolute path"),
    );
    let mut found = Vec::new();
    for serving in SERVING {
        let binding = catalog.binding(&id(serving)).expect("declared").clone();
        let model = client(binding, resolver.clone());
        found.extend(findings(serving, &*model, None, &listener));
    }
    assert_no_findings(&found);
}
