//! Adversary, wave 2026-10-20-w11, unit 2 (story:async-clients-without-a-tokio-runtime), pass 1.
//!
//! Two paths the unit's own cases do not drive, each polled by a standard-library executor on a
//! thread that is in no Tokio runtime:
//!
//! - a routing run over a route of three compatible targets with fallback enabled: the refusal is
//!   final, so the run attempts the first target once and stops, resolving and sending nothing;
//! - a turn started inside a runtime and polled to completion outside it, after the caller's sink
//!   held one event back for a single poll (as a sink applying backpressure does). The crate
//!   documentation of `b10x-llm-http` says a turn polled outside a runtime "does not panic".
//!
//! Local sockets only; no provider call.

use llm_core::{
    AuthKind, BillingKind, BoxFuture, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, Item,
    Model, Protocol, StreamEvent, StreamSink, ToolChoice, ToolName, ToolSpec, TurnOutcome,
    TurnRequest, VecSink,
};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_http::{HttpClient, Limits};
use llm_messages::MessagesClient;
use llm_models::CatalogModels;
use llm_providers::{
    Account, BaseUrl, Binding, BindingDocument, Endpoint, Provider, ServedModel, ServingModel,
};
use llm_responses::ResponsesClient;
use llm_routing::{Catalog, FallbackPolicy, Halt, Ports, Selection};
use serde_json::json;
use std::{
    any::Any,
    collections::BTreeMap,
    future::{Future, poll_fn},
    io::{ErrorKind, Read, Write},
    net::{TcpListener, TcpStream},
    panic::{AssertUnwindSafe, catch_unwind},
    pin::{Pin, pin},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    thread::{self, Thread},
    time::{Duration, Instant},
};

/// The refusal's whole message, as `docs/design.md` ("Executor") states it.
const REQUIREMENT: &str = "an llm client needs a Tokio runtime; run the call inside one, or use b10x-llm-blocking for a single model";

const SSE_HEAD: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";

/// One Responses answer carrying a single `function_call` (the stream `b10x-llm-blocking`'s own
/// cases answer with): an opening item and two argument deltas, so more than one event is shown.
const RESPONSES_STREAM: &[u8] = b"event: response.created\n\
data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"status\":\"in_progress\"}}\n\n\
event: response.output_item.added\n\
data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"file_read\",\"arguments\":\"\"}}\n\n\
event: response.function_call_arguments.delta\n\
data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"output_index\":0,\"delta\":\"{\\\"path\\\":\"}\n\n\
event: response.function_call_arguments.delta\n\
data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"output_index\":0,\"delta\":\"\\\"README.md\\\"}\"}\n\n\
event: response.function_call_arguments.done\n\
data: {\"type\":\"response.function_call_arguments.done\",\"item_id\":\"fc_1\",\"output_index\":0,\"arguments\":\"{\\\"path\\\":\\\"README.md\\\"}\"}\n\n\
event: response.output_item.done\n\
data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"file_read\",\"arguments\":\"{\\\"path\\\":\\\"README.md\\\"}\",\"status\":\"completed\"}}\n\n\
event: response.completed\n\
data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"model\":\"example/Small-Model\",\"status\":\"completed\",\"output\":[{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"file_read\",\"arguments\":\"{\\\"path\\\":\\\"README.md\\\"}\",\"status\":\"completed\"}],\"usage\":{\"input_tokens\":20,\"output_tokens\":9}}}\n\n";

/// One Messages text answer in three deltas.
const MESSAGES_STREAM: &[u8] = b"event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_014a\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"example-model-20260201\",\"content\":[],\"usage\":{\"input_tokens\":11,\"output_tokens\":1}}}\n\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Guten \"}}\n\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Tag, \"}}\n\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Welt\"}}\n\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"input_tokens\":11,\"output_tokens\":8}}\n\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\n";

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

fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic without a message".to_owned())
}

fn id(value: &str) -> Id {
    Id::new(value).expect("an identifier")
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

fn http() -> HttpClient {
    HttpClient::new(Limits {
        response_headers: Duration::from_secs(10),
        idle: Duration::from_secs(10),
        total: Duration::from_secs(10),
    })
    .expect("a bounded transport")
}

/// A local endpoint and its base URL; nothing is accepted unless a case serves it.
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

// ---------------------------------------------------------------------------------------------
// A routing run outside a runtime.
// ---------------------------------------------------------------------------------------------

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

/// One bearer account at `url` serving one model over each protocol; route `code` lists all
/// three, Responses first, with fallback enabled.
fn fallback_catalog(url: &str) -> Catalog {
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
        ("lab-responses", "responses"),
        ("lab-messages", "messages"),
        ("lab-chat", "chat-completions"),
    ] {
        source.push_str(&serving_model(serving, protocol));
    }
    source.push_str(
        r#"
[[routes]]
id = "coding"
alias = "code"
fallback_enabled = true

[[targets]]
id = "coding-responses"
route_id = "coding"
serving_model_id = "lab-responses"
position = 0

[[targets]]
id = "coding-messages"
route_id = "coding"
serving_model_id = "lab-messages"
position = 1

[[targets]]
id = "coding-chat"
route_id = "coding"
serving_model_id = "lab-chat"
position = 2
"#,
    );
    Catalog::parse(&source).expect("a valid catalog")
}

/// A routing run is not a client turn, so nothing in the unit drives one outside a runtime. The
/// refusal is `unsupported` and not retriable, so routing must neither retry it nor fall back to
/// the next target: one attempt, `ineligible-failure`, no credential resolved, nothing sent.
#[test]
fn a_routing_run_over_three_targets_outside_a_tokio_runtime_attempts_one_and_stops() {
    let (listener, url) = endpoint();
    let catalog = fallback_catalog(&url);
    let counting = Arc::new(Counting::default());
    let resolvers: BTreeMap<Id, Arc<dyn SecretResolver>> =
        BTreeMap::from([(id("lab"), counting.clone() as Arc<dyn SecretResolver>)]);
    let models = CatalogModels::build(&catalog, &http(), &resolvers).expect("every port builds");
    let mut request = TurnRequest::new("code", vec![Item::user("Hallo")]);
    request.max_output_tokens = Some(64);
    let mut admit = |_: &Selection<'_>| -> Result<(), Error> { Ok(()) };
    let mut sink = VecSink::new(64, 64 * 1024);
    let cancel = Cancel::new();
    let pauses = AtomicUsize::new(0);
    let pause = |_: Duration| -> BoxFuture<'static, ()> {
        pauses.fetch_add(1, Ordering::SeqCst);
        Box::pin(std::future::ready(()))
    };
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        block_on(catalog.run_turn(
            &request,
            Some(16),
            FallbackPolicy::default(),
            Ports {
                models: &models,
                admit: &mut admit,
                sink: &mut sink,
                cancel: &cancel,
                pause: &pause,
            },
        ))
    }))
    .map_err(|payload| panic_message(&*payload));

    let mut found = Vec::new();
    match outcome {
        Err(panic) => found.push(format!("the run panicked: {panic}")),
        Ok(Err(error)) => found.push(format!("the run was refused before any attempt: {error:?}")),
        Ok(Ok(run)) => {
            let open: Vec<_> = run
                .explanation
                .targets
                .iter()
                .filter(|target| target.rejections.is_empty())
                .map(|target| target.target_id.as_str().to_owned())
                .collect();
            if open.len() != 3 {
                found.push(format!(
                    "fixture: only {open:?} could be attempted, so fallback is not exercised"
                ));
            }
            if run.attempts.len() != 1 {
                found.push(format!("{} attempts, not one", run.attempts.len()));
            }
            if run.halt != Halt::IneligibleFailure {
                found.push(format!("halt {:?}", run.halt));
            }
            match &run.result {
                Ok(_) => found.push("the run returned an outcome".to_owned()),
                Err(error) => {
                    if error.code != ErrorCode::Unsupported {
                        found.push(format!("code {:?}", error.code));
                    }
                    if error.dispatch != Dispatch::NotSent {
                        found.push(format!("dispatch {:?}", error.dispatch));
                    }
                    if error.retriable || error.may_retry() {
                        found.push("retriable".to_owned());
                    }
                    if error.message != REQUIREMENT {
                        found.push(format!("message {:?}", error.message));
                    }
                }
            }
        }
    }
    let resolved = counting.0.load(Ordering::SeqCst);
    if resolved > 0 {
        found.push(format!("resolved the credential {resolved} time(s)"));
    }
    let waits = pauses.load(Ordering::SeqCst);
    if waits > 0 {
        found.push(format!("waited {waits} time(s) for a retry"));
    }
    if connected(&listener) {
        found.push("reached the endpoint".to_owned());
    }
    assert!(
        found.is_empty(),
        "a routing run outside a Tokio runtime:\n{}",
        found.join("\n")
    );
}

// ---------------------------------------------------------------------------------------------
// A turn started inside a runtime and polled to completion outside it.
// ---------------------------------------------------------------------------------------------

fn capabilities() -> Capabilities {
    Capabilities {
        tools: true,
        tool_choice: true,
        temperature: true,
        top_p: true,
        reasoning_efforts: vec!["medium".to_owned()],
        context_window: 32_768,
        max_output_tokens: 2_048,
        output_streaming: true,
        input_during_output: false,
    }
}

/// A bearer binding at `base_url` speaking `protocol`.
fn binding(base_url: &str, protocol: Protocol) -> Binding {
    BindingDocument::new(
        Provider {
            id: id("my-lab"),
            category: id("hosted"),
        },
        Account {
            id: id("local"),
            provider_id: id("my-lab"),
            auth_kind: AuthKind::Bearer,
            billing_kind: BillingKind::Metered,
            secret_reference_id: Some(SecretRef::new("fixture-key").expect("reference")),
            api_key_header: None,
        },
        Endpoint {
            id: id("local-endpoint"),
            account_id: id("local"),
            base_url: BaseUrl::new(base_url).expect("fixture URL"),
        },
        ServedModel {
            id: id("small"),
            upstream_name: id("example/Small-Model"),
        },
        ServingModel {
            id: id("serving"),
            endpoint_id: id("local-endpoint"),
            model_id: id("small"),
            protocol,
            capabilities: capabilities(),
        },
    )
    .bind()
    .expect("a valid fixture binding")
}

fn tool_request() -> TurnRequest {
    let mut request = TurnRequest::new("small", vec![Item::user("Read the readme")]);
    request.tools = vec![ToolSpec {
        name: ToolName::new("file_read").expect("tool name"),
        description: "Read one file".to_owned(),
        input_schema: json!({"type": "object"}),
    }];
    request.tool_choice = ToolChoice::Named(ToolName::new("file_read").expect("tool name"));
    request.max_output_tokens = Some(256);
    request
}

fn text_request() -> TurnRequest {
    let mut request = TurnRequest::new("small", vec![Item::user("Hallo")]);
    request.max_output_tokens = Some(256);
    request
}

/// Reads one whole request: its head and the body its `content-length` declares.
fn read_request(socket: &mut TcpStream) {
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let length = socket.read(&mut buffer).expect("a readable socket");
        assert!(length > 0, "the client closed before sending a request");
        bytes.extend_from_slice(&buffer[..length]);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
            let length: usize = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .map_or(0, |value| value.trim().parse().expect("a numeric length"));
            if bytes.len() >= end + 4 + length {
                return;
            }
        }
    }
}

/// Serves one connection on a plain thread: the whole answer, head and every event, in one
/// write, then closes. Answers whether a connection came within ten seconds.
fn serve_once(listener: TcpListener, stream: &'static [u8]) -> thread::JoinHandle<bool> {
    thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut socket = loop {
            match listener.accept() {
                Ok((socket, _)) => break socket,
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return false;
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        };
        socket.set_nonblocking(false).expect("a blocking socket");
        socket
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("a bounded read");
        read_request(&mut socket);
        let mut answer = SSE_HEAD.to_vec();
        answer.extend_from_slice(stream);
        socket.write_all(&answer).expect("the answer written");
        true
    })
}

/// Ready on its second poll; the first wakes the task and holds the event back.
struct OnePending(bool);

impl Future for OnePending {
    type Output = Result<(), Error>;
    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        if self.0 {
            self.0 = false;
            context.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
    }
}

/// A sink applying backpressure once: its first event is accepted on the second poll.
struct HoldsTheFirstEvent {
    held: Arc<AtomicBool>,
    seen: usize,
}

impl StreamSink for HoldsTheFirstEvent {
    fn emit(&mut self, _event: StreamEvent) -> BoxFuture<'_, Result<(), Error>> {
        self.seen += 1;
        let hold = !self.held.swap(true, Ordering::SeqCst);
        Box::pin(OnePending(hold))
    }
}

/// Starts one turn of `model` inside a current-thread runtime and polls it there until the sink
/// has held its first event back; then polls the same turn to completion with [`block_on`] on
/// this thread, which is then in no runtime. The runtime stays alive throughout. A panic is
/// caught and returned as its message.
fn started_inside_finished_outside(
    model: &dyn Model,
    request: &TurnRequest,
) -> Result<Result<TurnOutcome, Error>, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let held = Arc::new(AtomicBool::new(false));
    let mut sink = HoldsTheFirstEvent {
        held: held.clone(),
        seen: 0,
    };
    let cancel = Cancel::new();
    let outcome = {
        let mut turn = model.turn(request, &mut sink, &cancel);
        let inside = runtime.block_on(poll_fn(|context| match turn.as_mut().poll(context) {
            Poll::Ready(result) => Poll::Ready(Some(result)),
            Poll::Pending if held.load(Ordering::SeqCst) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }));
        assert!(
            inside.is_none(),
            "fixture: the turn ended inside the runtime before its sink held an event: {inside:?}"
        );
        assert!(
            tokio::runtime::Handle::try_current().is_err(),
            "fixture: this thread is still in a runtime"
        );
        catch_unwind(AssertUnwindSafe(|| block_on(turn.as_mut())))
            .map_err(|payload| panic_message(&*payload))
    };
    assert!(sink.seen >= 1, "fixture: the sink saw no event");
    drop(runtime);
    outcome
}

/// What a turn finished outside a runtime did that the documentation says it does not: panic,
/// or fail with anything but the runtime refusal. An outcome is not a departure: finishing
/// without needing Tokio again is allowed.
fn departures(what: &str, outcome: &Result<Result<TurnOutcome, Error>, String>) -> Vec<String> {
    match outcome {
        Err(panic) => vec![format!("{what}: panicked: {panic}")],
        Ok(Ok(_)) => Vec::new(),
        Ok(Err(error)) => {
            let mut found = Vec::new();
            if error.code != ErrorCode::Unsupported {
                found.push(format!("{what}: code {:?}", error.code));
            }
            if error.message != REQUIREMENT {
                found.push(format!("{what}: message {:?}", error.message));
            }
            found
        }
    }
}

/// `docs/design.md` ("Executor") and the `b10x-llm-http` crate documentation: a turn polled on a
/// thread in no Tokio runtime is refused, "It does not panic". The check runs once, when the turn
/// starts, and again only where the transport waits for the wire. A turn the caller started
/// inside a runtime and finishes outside one creates the next Tokio timer of its own turn
/// deadline (Responses `bounded`, Messages `emit`) without asking again, and the queued events
/// of an already-read chunk need no wire read to reach that timer.
#[test]
fn a_turn_started_inside_a_runtime_and_finished_outside_is_refused_not_a_panic() {
    let mut found = Vec::new();

    let (listener, url) = endpoint();
    let server = serve_once(listener, RESPONSES_STREAM);
    let responses = ResponsesClient::new(
        binding(&url, Protocol::Responses),
        http(),
        Arc::new(Counting::default()),
    )
    .expect("a Responses client");
    found.extend(departures(
        "Responses",
        &started_inside_finished_outside(&responses, &tool_request()),
    ));
    assert!(
        server.join().expect("the Responses server"),
        "fixture: the Responses turn never connected"
    );

    let (listener, url) = endpoint();
    let server = serve_once(listener, MESSAGES_STREAM);
    let messages = MessagesClient::new(
        binding(&url, Protocol::Messages),
        http(),
        Arc::new(Counting::default()),
    )
    .expect("a Messages client");
    found.extend(departures(
        "Messages",
        &started_inside_finished_outside(&messages, &text_request()),
    ));
    assert!(
        server.join().expect("the Messages server"),
        "fixture: the Messages turn never connected"
    );

    assert!(
        found.is_empty(),
        "a turn started inside a runtime and finished outside one:\n{}",
        found.join("\n")
    );
}
