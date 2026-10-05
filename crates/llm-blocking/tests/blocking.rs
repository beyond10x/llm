//! The blocking turn adapter, driven from plain threads that own no runtime (parity rows R40, M41).
//!
//! Every fake server is a `std::net` listener on `127.0.0.1` run by a plain thread, so nothing in
//! this file is asynchronous unless a case says it is. No provider is contacted, and every
//! credential comes from an injected resolver; nothing is read from this machine.

use llm_blocking::{BlockingModel, BlockingSink};
use llm_core::{
    AuthKind, BillingKind, BoxFuture, CallId, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id,
    Item, Model, Protocol, StopReason, StreamEvent, ToolCall, ToolChoice, ToolName, ToolSpec,
    TurnRequest,
};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_http::{HttpClient, Limits};
use llm_messages::MessagesClient;
use llm_providers::{
    Account, ApiKeyHeader, BaseUrl, Binding, BindingDocument, Endpoint, Provider, ServedModel,
    ServingModel,
};
use llm_responses::ResponsesClient;
use serde_json::{Value, json};
use std::{
    io::{ErrorKind, Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

const SSE_HEAD: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";

/// The transport bound of every client here. A wait in a case is always shorter than this, so a
/// case that times out on its own wait has not been answered by a transport deadline instead.
const TRANSPORT: Duration = Duration::from_secs(10);

/// How long a fake server waits for something the case says must already be happening.
const PROMPTLY: Duration = Duration::from_secs(5);

/// One Responses answer carrying a single `function_call`, as the wire streams it.
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

/// The opening of a Messages answer, up to and including one text delta carrying `text`.
fn messages_opening(text: &str) -> String {
    format!(
        "event: message_start\n\
data: {{\"type\":\"message_start\",\"message\":{{\"id\":\"msg_014a\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"example-model-20260201\",\"content\":[],\"usage\":{{\"input_tokens\":11,\"output_tokens\":1}}}}}}\n\n\
event: content_block_start\n\
data: {{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{{\"type\":\"text\",\"text\":\"\"}}}}\n\n\
event: content_block_delta\n\
data: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"{text}\"}}}}\n\n"
    )
}

/// A whole Messages answer whose only text is `text`.
fn messages_stream(text: &str) -> String {
    messages_opening(text)
        + "event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"input_tokens\":11,\"output_tokens\":8}}\n\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\n"
}

fn id(value: &str) -> Id {
    Id::new(value).expect("fixture identifier")
}

fn capabilities() -> Capabilities {
    Capabilities {
        tools: true,
        tool_choice: true,
        temperature: true,
        top_p: true,
        reasoning_efforts: vec!["medium".to_owned()],
        context_window: 32_768,
        max_output_tokens: 2_048,
    }
}

/// A fixture binding at `base_url` speaking `protocol`: a bearer for Responses, an API-key header
/// for Messages. The account names the credential by reference only.
fn binding(base_url: &str, protocol: Protocol) -> Binding {
    let (auth_kind, api_key_header) = match protocol {
        Protocol::Messages => (
            AuthKind::ApiKey,
            Some(ApiKeyHeader::new("x-api-key").expect("header name")),
        ),
        _ => (AuthKind::Bearer, None),
    };
    BindingDocument::new(
        Provider {
            id: id("my-lab"),
            category: id("hosted"),
        },
        Account {
            id: id("local"),
            provider_id: id("my-lab"),
            auth_kind,
            billing_kind: BillingKind::Metered,
            secret_reference_id: Some(SecretRef::new("fixture-key").expect("reference")),
            api_key_header,
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

/// An injected resolver holding one fixture credential.
struct StaticResolver;

impl SecretResolver for StaticResolver {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async move {
            Ok(ResolvedSecret {
                secret: Secret::new(b"fixture-token".to_vec())?,
                version: SecretVersion::new("generation-1".to_owned())?,
            })
        })
    }
}

fn transport() -> HttpClient {
    HttpClient::new(Limits {
        response_headers: TRANSPORT,
        idle: TRANSPORT,
        total: TRANSPORT,
    })
    .expect("bounded transport")
}

/// One Responses client. Built on a thread with no runtime, as a synchronous loop would build it.
fn responses(url: &str) -> Arc<dyn Model> {
    Arc::new(
        ResponsesClient::new(
            binding(url, Protocol::Responses),
            transport(),
            Arc::new(StaticResolver),
        )
        .expect("a Responses binding"),
    )
}

/// One Messages client. Built on a thread with no runtime, as a synchronous loop would build it.
fn messages(url: &str) -> Arc<dyn Model> {
    Arc::new(
        MessagesClient::new(
            binding(url, Protocol::Messages),
            transport(),
            Arc::new(StaticResolver),
        )
        .expect("a Messages binding"),
    )
}

/// A turn that forces one named tool.
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

/// A plain text turn whose user message is `prompt`, so a server can tell two turns apart.
fn text_request(prompt: &str) -> TurnRequest {
    let mut request = TurnRequest::new("small", vec![Item::user(prompt)]);
    request.max_output_tokens = Some(256);
    request
}

fn listen() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
    let url = format!("http://{}/v1", listener.local_addr().expect("an address"));
    listener.set_nonblocking(true).expect("a pollable listener");
    (listener, url)
}

/// The next connection within `within`, or `None`. Polled, because `std` has no accept timeout.
fn accept(listener: &TcpListener, within: Duration) -> Option<TcpStream> {
    let deadline = Instant::now() + within;
    loop {
        match listener.accept() {
            Ok((socket, _)) => {
                socket.set_nonblocking(false).expect("a blocking socket");
                socket
                    .set_read_timeout(Some(TRANSPORT))
                    .expect("a bounded read");
                return Some(socket);
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return None;
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("accept failed: {error}"),
        }
    }
}

/// Reads one whole request: its head and the body its `content-length` declares.
fn read_request(socket: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0; 1024];
    loop {
        let length = socket.read(&mut buffer).expect("a readable socket");
        assert!(length > 0, "the client closed before sending a request");
        bytes.extend_from_slice(&buffer[..length]);
        assert!(bytes.len() < 256 * 1024);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
            let length: usize = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .expect("a bounded body")
                .trim()
                .parse()
                .expect("a numeric content-length");
            if bytes.len() >= end + 4 + length {
                return String::from_utf8(bytes).expect("UTF-8 request");
            }
        }
    }
}

fn body(request: &str) -> Value {
    serde_json::from_str(request.split_once("\r\n\r\n").expect("a head and a body").1)
        .expect("a JSON body")
}

fn text(events: &[StreamEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            StreamEvent::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

/// Runs one text turn through `model` and returns what its own sink saw.
fn text_turn(model: &BlockingModel, prompt: &str) -> String {
    let mut events = Vec::new();
    let mut sink = |event: StreamEvent| -> Result<(), Error> {
        events.push(event);
        Ok(())
    };
    let outcome = model
        .turn(&text_request(prompt), &mut sink, &Cancel::new())
        .unwrap_or_else(|error| panic!("the {prompt} turn failed: {error}"));
    assert_eq!(outcome.stop_reason, StopReason::EndTurn);
    text(&events)
}

/// A Messages server that answers nothing until **both** turns are in flight, then answers the
/// later one first. A caller that runs its turns one after another never gets a second
/// connection while the first waits for its answer, and this server fails it.
fn serve_two_in_flight(listener: TcpListener) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut first = accept(&listener, TRANSPORT).expect("the first turn never connected");
        let first_request = read_request(&mut first);
        let mut second = accept(&listener, PROMPTLY)
            .expect("the second turn never reached the server while the first was in flight");
        let second_request = read_request(&mut second);
        for (mut socket, request) in [(second, second_request), (first, first_request)] {
            let prompt = body(&request)["messages"][0]["content"][0]["text"]
                .as_str()
                .expect("a text prompt")
                .to_owned();
            socket.write_all(SSE_HEAD).expect("head written");
            socket
                .write_all(messages_stream(&format!("answer-to-{prompt}")).as_bytes())
                .expect("stream written");
            socket.shutdown(Shutdown::Both).expect("closed");
        }
    })
}

/// R40: a synchronous loop runs one Responses turn to completion, with no runtime of its own.
#[test]
fn a_blocking_responses_turn_returns_its_function_call_without_a_caller_runtime() {
    assert!(
        tokio::runtime::Handle::try_current().is_err(),
        "the case must start outside any runtime"
    );
    let (listener, url) = listen();
    let server = thread::spawn(move || {
        let mut socket = accept(&listener, TRANSPORT).expect("the turn never connected");
        let captured = read_request(&mut socket);
        socket.write_all(SSE_HEAD).expect("head written");
        socket.write_all(RESPONSES_STREAM).expect("stream written");
        socket.shutdown(Shutdown::Both).expect("closed");
        assert!(
            accept(&listener, Duration::from_millis(150)).is_none(),
            "the adapter attempted the request a second time"
        );
        captured
    });
    let model = BlockingModel::new(responses(&url)).expect("an adapter owning its runtime");
    let mut events = Vec::new();
    let mut sink = |event: StreamEvent| -> Result<(), Error> {
        events.push(event);
        Ok(())
    };
    let outcome = model
        .turn(&tool_request(), &mut sink, &Cancel::new())
        .expect("a decoded turn");
    let captured = server.join().expect("the fixture server");

    assert!(
        captured
            .to_ascii_lowercase()
            .starts_with("post /v1/responses "),
        "{captured}"
    );
    assert_eq!(body(&captured)["tools"][0]["name"], json!("file_read"));
    assert_eq!(outcome.stop_reason, StopReason::ToolCalls);
    assert_eq!(
        outcome.tool_calls().cloned().collect::<Vec<_>>(),
        vec![ToolCall {
            call_id: CallId::new("call_1").expect("call id"),
            name: ToolName::new("file_read").expect("tool name"),
            arguments: json!({"path": "README.md"}),
        }]
    );
    assert_eq!(
        events,
        [
            StreamEvent::ToolCallStarted {
                call_id: CallId::new("call_1").expect("call id"),
                name: ToolName::new("file_read").expect("tool name"),
            },
            StreamEvent::ToolArgumentsDelta {
                call_id: CallId::new("call_1").expect("call id"),
                delta: "{\"path\":".to_owned(),
            },
            StreamEvent::ToolArgumentsDelta {
                call_id: CallId::new("call_1").expect("call id"),
                delta: "\"README.md\"}".to_owned(),
            },
        ]
    );
    assert_eq!(model.provenance().protocol, Protocol::Responses);
}

/// The sink is called as each event is decoded, not once the turn has ended: the server holds
/// back the rest of the stream until the blocking sink has seen the call open.
#[test]
fn the_blocking_sink_sees_each_event_while_the_turn_is_still_running() {
    let marker = b"event: response.function_call_arguments.delta\n";
    let split = RESPONSES_STREAM
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("the fixture carries an argument delta");
    let (seen_tx, seen_rx) = mpsc::channel::<()>();
    let (listener, url) = listen();
    let server = thread::spawn(move || {
        let mut socket = accept(&listener, TRANSPORT).expect("the turn never connected");
        read_request(&mut socket);
        socket.write_all(SSE_HEAD).expect("head written");
        socket
            .write_all(&RESPONSES_STREAM[..split])
            .expect("prefix written");
        socket.flush().expect("prefix flushed");
        let live = seen_rx.recv_timeout(PROMPTLY).is_ok();
        if live {
            socket
                .write_all(&RESPONSES_STREAM[split..])
                .expect("rest written");
        }
        socket.shutdown(Shutdown::Both).expect("closed");
        live
    });
    let model = BlockingModel::new(responses(&url)).expect("an adapter owning its runtime");
    let mut sink = move |event: StreamEvent| -> Result<(), Error> {
        if matches!(event, StreamEvent::ToolCallStarted { .. }) {
            // The server may already have given up; the assertion below reports that.
            let _ = seen_tx.send(());
        }
        Ok(())
    };
    let result = model.turn(&tool_request(), &mut sink, &Cancel::new());
    assert!(
        server.join().expect("the fixture server"),
        "the sink saw nothing while the server held back the end of the stream"
    );
    assert_eq!(
        result.expect("a decoded turn").stop_reason,
        StopReason::ToolCalls
    );
}

/// M41: two synchronous loops share one adapter over one Messages client, and both turns are in
/// flight at once. Each loop's sink sees only its own turn's text.
#[test]
fn two_concurrent_turns_run_on_one_shared_messages_client() {
    let (listener, url) = listen();
    let server = serve_two_in_flight(listener);
    let model = BlockingModel::new(messages(&url)).expect("an adapter owning its runtime");
    let (first, second) = thread::scope(|scope| {
        let first = scope.spawn(|| text_turn(&model, "first"));
        let second = scope.spawn(|| text_turn(&model, "second"));
        (
            first.join().expect("the first loop"),
            second.join().expect("the second loop"),
        )
    });
    server.join().expect("the fixture server");
    assert_eq!(first, "answer-to-first");
    assert_eq!(second, "answer-to-second");
}

/// M41, the Harness `fork` shape: a fork is an owned handle on the same client and runtime, so it
/// can move to a thread of its own and run beside the original.
#[test]
fn a_fork_runs_its_turn_on_its_own_thread_beside_the_original() {
    let (listener, url) = listen();
    let server = serve_two_in_flight(listener);
    let model = BlockingModel::new(messages(&url)).expect("an adapter owning its runtime");
    let fork = model.fork();
    assert_eq!(fork.provenance(), model.provenance());
    let forked = thread::spawn(move || text_turn(&fork, "forked"));
    let original = text_turn(&model, "original");
    let forked = forked.join().expect("the forked loop");
    server.join().expect("the fixture server");
    assert_eq!(original, "answer-to-original");
    assert_eq!(forked, "answer-to-forked");
}

/// A turn blocked mid-stream ends as `Cancelled` when another thread cancels it, well inside the
/// transport bound and while the server still holds the stream open.
#[test]
fn another_thread_cancels_a_turn_blocked_mid_stream() {
    let (seen_tx, seen_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let (listener, url) = listen();
    let server = thread::spawn(move || {
        let mut socket = accept(&listener, TRANSPORT).expect("the turn never connected");
        read_request(&mut socket);
        socket.write_all(SSE_HEAD).expect("head written");
        socket
            .write_all(messages_opening("Hel").as_bytes())
            .expect("opening written");
        socket.flush().expect("opening flushed");
        // Holds the stream open: the turn can end only by cancellation or by the transport bound.
        let _ = release_rx.recv_timeout(TRANSPORT * 2);
    });
    let cancel = Cancel::new();
    let remote = cancel.clone();
    let canceller = thread::spawn(move || {
        seen_rx
            .recv_timeout(PROMPTLY)
            .expect("the sink never saw the first delta");
        remote.cancel();
    });
    let model = BlockingModel::new(messages(&url)).expect("an adapter owning its runtime");
    let mut events = Vec::new();
    let mut sink = |event: StreamEvent| -> Result<(), Error> {
        events.push(event);
        let _ = seen_tx.send(());
        Ok(())
    };
    let started = Instant::now();
    let result = model.turn(&text_request("wait"), &mut sink, &cancel);
    let elapsed = started.elapsed();
    let _ = release_tx.send(());
    canceller.join().expect("the cancelling thread");
    server.join().expect("the fixture server");

    let error = result.expect_err("a cancelled turn is a failure");
    assert_eq!(error.code, ErrorCode::Cancelled, "{error}");
    assert_eq!(error.dispatch, Dispatch::Accepted, "{error}");
    assert!(
        elapsed < PROMPTLY,
        "cancellation took {elapsed:?}, as long as a transport bound"
    );
    assert_eq!(text(&events), "Hel");
}

/// A sink that refuses an event ends the turn with the sink's own failure, not a dropped event.
#[test]
fn a_sink_refusal_ends_the_turn_with_the_sinks_own_failure() {
    let (listener, url) = listen();
    let server = thread::spawn(move || {
        let mut socket = accept(&listener, TRANSPORT).expect("the turn never connected");
        read_request(&mut socket);
        // The client may close first once the sink refuses; a failed write is expected then.
        let _ = socket.write_all(SSE_HEAD);
        let _ = socket.write_all(messages_stream("Hello").as_bytes());
        let _ = socket.shutdown(Shutdown::Both);
    });
    let model = BlockingModel::new(messages(&url)).expect("an adapter owning its runtime");
    let mut sink = |_event: StreamEvent| -> Result<(), Error> {
        Err(Error::new(ErrorCode::Refused, "the loop stopped reading"))
    };
    let error = model
        .turn(&text_request("refuse"), &mut sink, &Cancel::new())
        .expect_err("a refused event fails the turn");
    server.join().expect("the fixture server");
    assert_eq!(error.code, ErrorCode::Refused, "{error}");
    assert_eq!(error.message, "the loop stopped reading");
}

/// The adapter can borrow a caller's multi-thread runtime instead of owning one.
#[test]
fn a_borrowed_multi_thread_runtime_runs_the_turn() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("the caller's runtime");
    let (listener, url) = listen();
    let server = thread::spawn(move || {
        let mut socket = accept(&listener, TRANSPORT).expect("the turn never connected");
        read_request(&mut socket);
        socket.write_all(SSE_HEAD).expect("head written");
        socket
            .write_all(messages_stream("borrowed").as_bytes())
            .expect("stream written");
        socket.shutdown(Shutdown::Both).expect("closed");
    });
    let model = BlockingModel::on_runtime(messages(&url), runtime.handle().clone())
        .expect("a multi-thread runtime is accepted");
    assert_eq!(text_turn(&model, "borrowed"), "borrowed");
    server.join().expect("the fixture server");
}

/// A current-thread runtime is driven only by its own `block_on`; a handle on one would leave
/// the turn's I/O undriven, so the adapter refuses it when built rather than hanging a turn.
#[test]
fn a_current_thread_runtime_is_refused_because_nothing_would_drive_its_io() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("the caller's runtime");
    let Err(error) =
        BlockingModel::on_runtime(messages("http://127.0.0.1:9/v1"), runtime.handle().clone())
    else {
        panic!("a current-thread handle was accepted");
    };
    assert_eq!(error.code, ErrorCode::Unsupported, "{error}");
    assert_eq!(error.dispatch, Dispatch::NotSent, "{error}");
}

/// A blocking turn called from inside an asynchronous task would block that runtime's thread
/// (and Tokio panics on a nested `block_on`). It is refused with a typed failure instead, before
/// anything is sent.
#[test]
fn a_turn_started_inside_an_async_runtime_is_refused_without_contacting_the_server() {
    let (listener, url) = listen();
    let model = BlockingModel::new(messages(&url)).expect("an adapter owning its runtime");
    let caller = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("an asynchronous caller");
    let result = caller.block_on(async {
        let mut sink = |_event: StreamEvent| -> Result<(), Error> { Ok(()) };
        model.turn(&text_request("nested"), &mut sink, &Cancel::new())
    });
    let error = result.expect_err("a nested blocking turn is refused");
    assert_eq!(error.code, ErrorCode::Unsupported, "{error}");
    assert_eq!(error.dispatch, Dispatch::NotSent, "{error}");
    assert!(
        accept(&listener, Duration::from_millis(150)).is_none(),
        "a refused turn reached the server"
    );
}

/// The sink trait is object safe and a closure is one, so a loop can hold any sink behind
/// `&mut dyn BlockingSink`.
#[test]
fn a_closure_is_a_blocking_sink() {
    let mut seen = Vec::new();
    let mut closure = |event: StreamEvent| -> Result<(), Error> {
        seen.push(event);
        Ok(())
    };
    let sink: &mut dyn BlockingSink = &mut closure;
    sink.emit(StreamEvent::TextDelta {
        text: "one".to_owned(),
    })
    .expect("accepted");
    assert_eq!(text(&seen), "one");
}
