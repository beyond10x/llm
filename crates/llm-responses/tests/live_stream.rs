//! Live delivery (row R30) and kept text (row R36) for the Responses client, over local sockets.
//! No provider is contacted; every answer is a literal fixture served from `127.0.0.1`.
//!
//! The claims are the ones `spec/domains/responses.yaml` declares under "Live delivery",
//! "Retry agreement" and "Kept text": each payload's events reach the caller's sink before the
//! next payload is read (Harness `harness-responses/src/lib.rs:353`-`359`); a sink refusal or a
//! cancel ends the turn at once with the evidence decoded so far; the transport's bounds hold
//! while events are handed over; a cut after the answer was shown is final; text the caller
//! was shown stays in the turn when the terminal `output` omits it; and every event is final:
//! the turn's text is the text the caller was shown, and a server's own text that differed is
//! replaced and reported with `shown-text-kept`.

use llm_core::{
    AuthKind, BillingKind, BoxFuture, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, Item,
    Model, Protocol, StopReason, StreamEvent, StreamSink, TurnOutcome, TurnRequest, VecSink,
};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_http::{HttpClient, Limits, MAX_EVENT_BYTES};
use llm_providers::{
    Account, BaseUrl, Binding, BindingDocument, Endpoint, Provider, ServedModel, ServingModel,
};
use llm_responses::{IngressStream, ResponsesClient, decode_stream};
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
};

const UPSTREAM_MODEL: &str = "example/Small-Model";

const SSE_HEAD: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";

/// A `200` with no content type, as the Codex backend answers (`story:codex-stream`).
const UNTYPED_HEAD: &[u8] = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n";

/// How long a fixture server waits for something the client should do at once. A live client
/// does it in milliseconds; a client that reads the whole stream first never does it.
const PATIENCE: Duration = Duration::from_secs(3);

/// How long a fixture server holds a connection open with nothing more to send.
const HOLD: Duration = Duration::from_secs(6);

fn id(value: &str) -> Id {
    Id::new(value).expect("fixture identifier")
}

fn binding(base_url: &str) -> Binding {
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
            secret_reference_id: Some(SecretRef::new("responses-key").expect("reference")),
            api_key_header: None,
        },
        Endpoint {
            id: id("local-endpoint"),
            account_id: id("local"),
            base_url: BaseUrl::new(base_url).expect("fixture URL"),
        },
        ServedModel {
            id: id("small"),
            upstream_name: id(UPSTREAM_MODEL),
        },
        ServingModel {
            id: id("serving"),
            endpoint_id: id("local-endpoint"),
            model_id: id("small"),
            protocol: Protocol::Responses,
            capabilities: Capabilities {
                tools: true,
                tool_choice: true,
                temperature: true,
                top_p: true,
                reasoning_efforts: vec!["medium".to_owned()],
                context_window: 32_768,
                max_output_tokens: 2_048,
                output_streaming: true,
                input_during_output: false,
            },
        },
    )
    .bind()
    .expect("a valid fixture binding")
}

struct Resolver;

impl SecretResolver for Resolver {
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

/// Every limit is longer than [`HOLD`], so no case ends on a transport limit by accident.
fn client(url: &str) -> ResponsesClient {
    let limit = Duration::from_secs(10);
    ResponsesClient::new(
        binding(url),
        HttpClient::new(Limits {
            response_headers: limit,
            idle: limit,
            total: limit,
        })
        .expect("bounded transport"),
        Arc::new(Resolver),
    )
    .expect("a Responses binding")
}

fn request() -> TurnRequest {
    TurnRequest::new("small", vec![Item::user("Say hello")])
}

fn created() -> Value {
    json!({"type": "response.created",
        "response": {"id": "resp_1", "status": "in_progress", "output": []}})
}

fn text_delta(item_id: &str, output_index: u64, delta: &str) -> Value {
    json!({"type": "response.output_text.delta", "item_id": item_id,
        "output_index": output_index, "content_index": 0, "delta": delta})
}

fn completed(output: &Value) -> Value {
    json!({"type": "response.completed", "response": {
        "id": "resp_1", "status": "completed", "model": UPSTREAM_MODEL, "output": output,
        "usage": {"input_tokens": 20, "output_tokens": 9}}})
}

fn hello_message() -> Value {
    json!([{"type": "message", "id": "msg_1", "role": "assistant",
        "content": [{"type": "output_text", "text": "Hello"}]}])
}

fn sse(events: &[Value]) -> Vec<u8> {
    let mut body = Vec::new();
    for event in events {
        let kind = event["type"].as_str().expect("an event type");
        body.extend_from_slice(format!("event: {kind}\ndata: {event}\n\n").as_bytes());
    }
    body
}

fn text(event: &str) -> StreamEvent {
    StreamEvent::TextDelta {
        text: event.to_owned(),
    }
}

async fn listener() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local port");
    let url = format!("http://{}/v1", listener.local_addr().expect("an address"));
    (listener, url)
}

/// Accepts the one connection and reads the request through its body.
async fn accept(listener: &TcpListener) -> TcpStream {
    let (mut socket, _) = tokio::time::timeout(Duration::from_secs(10), listener.accept())
        .await
        .expect("the client never connected")
        .expect("an accepted connection");
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let length = socket.read(&mut buffer).await.expect("a readable socket");
        assert!(length > 0, "the client closed before sending a request");
        bytes.extend_from_slice(&buffer[..length]);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
            let length: usize = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .map_or(0, |value| value.trim().parse().expect("a numeric length"));
            if bytes.len() >= end + 4 + length {
                return socket;
            }
        }
    }
}

async fn send(socket: &mut TcpStream, bytes: &[u8]) {
    socket
        .write_all(bytes)
        .await
        .expect("fixture bytes written");
    socket.flush().await.expect("fixture bytes flushed");
}

/// Holds the connection open with nothing more to send, until the client closes it or
/// [`HOLD`] passes. Returns whether the client closed it first.
async fn hold(socket: &mut TcpStream) -> bool {
    let mut buffer = [0; 1024];
    let closed = tokio::time::timeout(HOLD, async {
        loop {
            match socket.read(&mut buffer).await {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
        }
    })
    .await
    .is_ok();
    let _ = socket.shutdown().await;
    closed
}

/// A sink that records every event and reports each one to the fixture server as it arrives.
struct WatchedSink {
    events: Vec<StreamEvent>,
    seen: mpsc::UnboundedSender<StreamEvent>,
}

impl StreamSink for WatchedSink {
    fn emit(&mut self, event: StreamEvent) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            self.events.push(event.clone());
            let _ = self.seen.send(event);
            Ok(())
        })
    }
}

/// A sink that cancels the turn when its first event arrives, and accepts it.
struct CancellingSink {
    cancel: Cancel,
    events: Vec<StreamEvent>,
}

impl StreamSink for CancellingSink {
    fn emit(&mut self, event: StreamEvent) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            self.events.push(event);
            self.cancel.cancel();
            Ok(())
        })
    }
}

fn assert_accepted_failure(error: &Error, model: &ResponsesClient, case: &str) {
    assert_eq!(error.dispatch, Dispatch::Accepted, "{case}: {error:?}");
    assert_eq!(
        error.validate_for(model.provenance()),
        Ok(()),
        "{case}: Error::validate_for refuses {error:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// R30: each payload's events reach the sink before the next payload is read.
// ---------------------------------------------------------------------------------------------

/// The server sends the first delta and then waits for the caller to have been shown it before
/// it sends the rest. A client that reads the whole stream before it emits shows nothing until
/// the server gives up waiting.
#[tokio::test]
async fn each_delta_reaches_the_sink_before_the_server_sends_the_next() {
    let (listener, url) = listener().await;
    let (seen, mut watched) = mpsc::unbounded_channel();
    let server = tokio::spawn(async move {
        let mut socket = accept(&listener).await;
        send(&mut socket, SSE_HEAD).await;
        send(
            &mut socket,
            &sse(&[created(), text_delta("msg_1", 0, "Hel")]),
        )
        .await;
        let shown = tokio::time::timeout(PATIENCE, watched.recv())
            .await
            .ok()
            .flatten();
        send(
            &mut socket,
            &sse(&[text_delta("msg_1", 0, "lo"), completed(&hello_message())]),
        )
        .await;
        let _ = socket.shutdown().await;
        shown
    });
    let model = client(&url);
    let mut sink = WatchedSink {
        events: Vec::new(),
        seen,
    };
    let outcome = model
        .turn(&request(), &mut sink, &Cancel::new())
        .await
        .expect("a completed turn");
    let shown = server.await.expect("the fixture server");
    assert_eq!(
        shown,
        Some(text("Hel")),
        "the first delta had not reached the sink while the server held the rest of the stream \
         for {PATIENCE:?}: the caller sees nothing until the stream ends"
    );
    assert_eq!(sink.events, vec![text("Hel"), text("lo")]);
    assert_eq!(outcome.items, vec![Item::assistant("Hello")]);
    assert_eq!(outcome.stop_reason, StopReason::EndTurn);
}

/// The caller cancels as soon as it is shown the first delta, while the server is still
/// sending. The turn ends at once, without waiting for the rest of the stream; nothing after
/// the cancel is shown; and counters that were never read stay absent.
#[tokio::test]
async fn a_cancel_while_the_server_is_still_sending_ends_the_turn_at_once() {
    let (listener, url) = listener().await;
    let server = tokio::spawn(async move {
        let mut socket = accept(&listener).await;
        send(&mut socket, SSE_HEAD).await;
        send(
            &mut socket,
            &sse(&[created(), text_delta("msg_1", 0, "Hel")]),
        )
        .await;
        hold(&mut socket).await
    });
    let model = client(&url);
    let cancel = Cancel::new();
    let mut sink = CancellingSink {
        cancel: cancel.clone(),
        events: Vec::new(),
    };
    let started = Instant::now();
    let error = model
        .turn(&request(), &mut sink, &cancel)
        .await
        .expect_err("the caller cancelled");
    let elapsed = started.elapsed();
    let closed = server.await.expect("the fixture server");
    assert_eq!(sink.events, vec![text("Hel")], "{error:?}");
    assert_eq!(
        error.code,
        ErrorCode::Cancelled,
        "the cancel issued while the first delta was shown did not end the turn: {error:?}"
    );
    assert!(
        elapsed < PATIENCE && closed,
        "the turn waited {elapsed:?} for a server still sending (client closed: {closed})"
    );
    assert!(!error.may_retry(), "{error:?}");
    assert_accepted_failure(&error, &model, "cancel");
    let observation = error.observation.as_deref().expect("the binding");
    assert_eq!(observation.usage, None, "{observation:?}");
}

/// The sink refuses its second event while the server is still sending. The turn ends at once
/// with the sink's refusal and the evidence decoded so far.
#[tokio::test]
async fn a_sink_refusal_while_the_server_is_still_sending_ends_the_turn_at_once() {
    let (listener, url) = listener().await;
    let server = tokio::spawn(async move {
        let mut socket = accept(&listener).await;
        send(&mut socket, SSE_HEAD).await;
        send(
            &mut socket,
            &sse(&[
                created(),
                text_delta("msg_1", 0, "Hel"),
                text_delta("msg_1", 0, "lo"),
            ]),
        )
        .await;
        hold(&mut socket).await
    });
    let model = client(&url);
    // One event accepted, the second refused.
    let mut sink = VecSink::new(1, 4096);
    let started = Instant::now();
    let error = model
        .turn(&request(), &mut sink, &Cancel::new())
        .await
        .expect_err("the sink refused its second event");
    let elapsed = started.elapsed();
    let closed = server.await.expect("the fixture server");
    assert_eq!(sink.events(), [text("Hel")], "{error:?}");
    assert_eq!(error.code, ErrorCode::TooLarge, "{error:?}");
    assert!(
        elapsed < PATIENCE && closed,
        "the turn waited {elapsed:?} for a server still sending (client closed: {closed})"
    );
    assert_accepted_failure(&error, &model, "sink refusal");
    let observation = error.observation.as_deref().expect("the binding");
    assert_eq!(observation.usage, None, "{observation:?}");
}

// ---------------------------------------------------------------------------------------------
// Bounds hold while streaming.
// ---------------------------------------------------------------------------------------------

/// A line over the per-event bound follows a delta. The delta was shown; the bound still ends
/// the turn, and the refusal is final.
#[tokio::test]
async fn a_line_over_its_bound_ends_the_turn_after_the_delta_before_it_was_shown() {
    let (listener, url) = listener().await;
    let server = tokio::spawn(async move {
        let mut socket = accept(&listener).await;
        send(&mut socket, SSE_HEAD).await;
        let mut body = sse(&[created(), text_delta("msg_1", 0, "Hel")]);
        body.extend_from_slice(b"data: ");
        body.extend(std::iter::repeat_n(b'x', MAX_EVENT_BYTES + 1));
        body.extend_from_slice(b"\n\n");
        // Writing may fail once the client has stopped reading; the bound is what is checked.
        let _ = socket.write_all(&body).await;
        let _ = socket.shutdown().await;
    });
    let model = client(&url);
    let mut sink = VecSink::new(64, 64 * 1024);
    let error = model
        .turn(&request(), &mut sink, &Cancel::new())
        .await
        .expect_err("a line over its bound");
    server.await.expect("the fixture server");
    assert_eq!(error.code, ErrorCode::TooLarge, "{error:?}");
    assert_eq!(
        sink.events(),
        [text("Hel")],
        "the delta that arrived before the over-long line was not shown"
    );
    assert!(!error.may_retry(), "{error:?}");
    assert_accepted_failure(&error, &model, "line bound");
}

/// More events than a stream may carry follow a delta. The delta was shown; the event-count
/// bound still ends the turn.
#[tokio::test]
async fn an_event_count_over_its_bound_ends_the_turn_after_the_delta_before_it_was_shown() {
    let (listener, url) = listener().await;
    let server = tokio::spawn(async move {
        let mut socket = accept(&listener).await;
        send(&mut socket, SSE_HEAD).await;
        let mut body = sse(&[created(), text_delta("msg_1", 0, "Hel")]);
        // Every keepalive is an event; together they exceed the transport's event-count bound.
        for _ in 0..65_536 {
            body.extend_from_slice(b"data: {\"type\":\"keepalive\"}\n\n");
        }
        // Writing may fail once the client has stopped reading; the bound is what is checked.
        let _ = socket.write_all(&body).await;
        let _ = socket.shutdown().await;
    });
    let model = client(&url);
    let mut sink = VecSink::new(64, 64 * 1024);
    let error = model
        .turn(&request(), &mut sink, &Cancel::new())
        .await
        .expect_err("more events than the bound");
    server.await.expect("the fixture server");
    assert_eq!(error.code, ErrorCode::TooLarge, "{error:?}");
    assert_eq!(error.message, "SSE event count exceeds bound", "{error:?}");
    assert_eq!(
        sink.events(),
        [text("Hel")],
        "the delta that arrived before the bound was crossed was not shown"
    );
    assert_accepted_failure(&error, &model, "event bound");
}

// ---------------------------------------------------------------------------------------------
// Retry agreement: the client's answered rule and routing's visible-output rule.
// ---------------------------------------------------------------------------------------------

/// The answer is streamed and the connection is then cut inside the next event. The caller was
/// shown the answer, so routing's visible-output rule sees it; and the client's own rule makes
/// the cut final. Both say the same thing.
#[tokio::test]
async fn a_cut_after_the_answer_was_shown_is_final_and_the_answer_was_shown() {
    let (listener, url) = listener().await;
    let server = tokio::spawn(async move {
        let mut socket = accept(&listener).await;
        send(&mut socket, SSE_HEAD).await;
        let mut body = sse(&[
            created(),
            text_delta("msg_1", 0, "Hel"),
            text_delta("msg_1", 0, "lo"),
        ]);
        body.extend_from_slice(
            b"event: response.output_text.delta\ndata: {\"type\":\"response.out",
        );
        send(&mut socket, &body).await;
        let _ = socket.shutdown().await;
    });
    let model = client(&url);
    let mut sink = VecSink::new(64, 64 * 1024);
    let error = model
        .turn(&request(), &mut sink, &Cancel::new())
        .await
        .expect_err("the stream was cut before response.completed");
    server.await.expect("the fixture server");
    assert_eq!(
        sink.events(),
        [text("Hel"), text("lo")],
        "the answer streamed before the cut was not shown to the caller"
    );
    assert!(
        !error.may_retry(),
        "an answered turn was offered for another attempt: {error:?}"
    );
    assert_accepted_failure(&error, &model, "cut");
}

// ---------------------------------------------------------------------------------------------
// R36: text the caller was shown stays in the turn.
// ---------------------------------------------------------------------------------------------

/// The Codex shape: an untyped `200`, text deltas with no `response.output_item.done`, and an
/// empty terminal `output`. The caller is shown the text as it arrives, and the turn it gets
/// back carries that text.
#[tokio::test]
async fn a_codex_text_turn_keeps_the_text_it_showed() {
    let (listener, url) = listener().await;
    let (seen, mut watched) = mpsc::unbounded_channel();
    let server = tokio::spawn(async move {
        let mut socket = accept(&listener).await;
        send(&mut socket, UNTYPED_HEAD).await;
        send(
            &mut socket,
            &sse(&[created(), text_delta("msg_1", 0, "The answer is 42.")]),
        )
        .await;
        let shown = tokio::time::timeout(PATIENCE, watched.recv())
            .await
            .ok()
            .flatten();
        send(
            &mut socket,
            &sse(&[
                json!({"type": "response.output_text.done", "item_id": "msg_1",
                    "output_index": 0, "content_index": 0, "text": "The answer is 42."}),
                completed(&json!([])),
            ]),
        )
        .await;
        let _ = socket.shutdown().await;
        shown
    });
    let model = client(&url);
    let mut sink = WatchedSink {
        events: Vec::new(),
        seen,
    };
    let outcome = model
        .turn(&request(), &mut sink, &Cancel::new())
        .await
        .expect("a completed turn");
    let shown = server.await.expect("the fixture server");
    assert_eq!(
        outcome.items,
        vec![Item::assistant("The answer is 42.")],
        "the caller was shown text the turn does not carry ({:?})",
        outcome.stop_reason
    );
    assert_eq!(outcome.stop_reason, StopReason::EndTurn);
    assert_eq!(sink.events, vec![text("The answer is 42.")]);
    assert_eq!(
        shown,
        Some(text("The answer is 42.")),
        "the delta was not shown while the server held the terminal object"
    );
    let usage = outcome.observation.usage.expect("the terminal counters");
    assert_eq!(usage.input_tokens, Some(20));
    assert_eq!(usage.output_tokens, Some(9));
}

// ---------------------------------------------------------------------------------------------
// Every event is final: the turn's text is the text the caller was shown.
// ---------------------------------------------------------------------------------------------

/// The fixed code of the warning that says a server's own text gave way to the shown text.
const SHOWN_TEXT_KEPT: &str = "shown-text-kept";

/// What a server wrote in a finished message that no delta showed. No warning may quote it.
const SERVER_TEXT: &str = "the final text";

fn decoder_binding() -> llm_responses::Binding {
    llm_responses::Binding::new(
        binding("http://127.0.0.1:9/v1").provenance().clone(),
        id(UPSTREAM_MODEL),
    )
}

fn message(id: Option<&str>, text: &str) -> Value {
    let mut message = json!({"type": "message", "role": "assistant",
        "content": [{"type": "output_text", "text": text}]});
    if let Some(id) = id {
        message["id"] = json!(id);
    }
    message
}

fn message_done(id: &str, output_index: u64, text: &str) -> Value {
    json!({"type": "response.output_item.done", "output_index": output_index,
        "item": message(Some(id), text)})
}

fn reasoning_delta(output_index: u64, delta: &str) -> Value {
    json!({"type": "response.reasoning_summary_text.delta", "item_id": "rs_1",
        "output_index": output_index, "summary_index": 0, "delta": delta})
}

/// Every text delta the caller was shown, joined.
fn shown_text(events: &[StreamEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            StreamEvent::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

/// The text of every assistant message the turn carries, joined.
fn carried_text(outcome: &TurnOutcome) -> String {
    outcome
        .items
        .iter()
        .filter_map(|item| match item {
            Item::AssistantText { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

/// Each warning as its code and its message.
fn warnings(events: &[StreamEvent]) -> Vec<(&str, &str)> {
    events
        .iter()
        .filter_map(|event| match event {
            StreamEvent::Warning { code, message } => Some((code.as_str(), message.as_str())),
            _ => None,
        })
        .collect()
}

/// One stream, the items its turn must carry, and whether a server's own text gave way.
type ShownTextCase = (&'static str, Vec<Value>, Vec<Item>, bool);

/// The ways a server's message can meet the text the caller was shown, and the ways it cannot.
fn shown_text_cases() -> [ShownTextCase; 7] {
    [
        (
            "a terminal message under the deltas' id",
            vec![
                created(),
                text_delta("msg_1", 0, "a dr"),
                text_delta("msg_1", 0, "aft"),
                completed(&json!([message(Some("msg_1"), SERVER_TEXT)])),
            ],
            vec![Item::assistant("a draft")],
            true,
        ),
        (
            "a terminal message that names no id",
            vec![
                created(),
                text_delta("msg_1", 0, "a draft"),
                completed(&json!([message(None, SERVER_TEXT)])),
            ],
            vec![Item::assistant("a draft")],
            true,
        ),
        (
            "a streamed message before an empty terminal output",
            vec![
                created(),
                text_delta("msg_1", 0, "a draft"),
                message_done("msg_1", 0, SERVER_TEXT),
                completed(&json!([])),
            ],
            vec![Item::assistant("a draft")],
            true,
        ),
        (
            "two messages, the second one rewritten",
            vec![
                created(),
                text_delta("msg_a", 0, "A"),
                reasoning_delta(1, "weighing"),
                text_delta("msg_b", 2, "B"),
                completed(&json!([
                    message(Some("msg_a"), "A"),
                    message(Some("msg_b"), SERVER_TEXT)
                ])),
            ],
            vec![Item::assistant("A"), Item::assistant("B")],
            true,
        ),
        (
            "a terminal message equal to its deltas",
            vec![
                created(),
                text_delta("msg_1", 0, "Hel"),
                text_delta("msg_1", 0, "lo"),
                completed(&hello_message()),
            ],
            vec![Item::assistant("Hello")],
            false,
        ),
        (
            "deltas and an empty terminal output (R36)",
            vec![
                created(),
                text_delta("msg_1", 0, "a draft"),
                completed(&json!([])),
            ],
            vec![Item::assistant("a draft")],
            false,
        ),
        (
            "a message no delta showed",
            vec![
                created(),
                completed(&json!([message(Some("msg_1"), SERVER_TEXT)])),
            ],
            vec![Item::assistant(SERVER_TEXT)],
            false,
        ),
    ]
}

/// On a successful turn each assistant message carries exactly the text its deltas showed,
/// paired by `id` where both sides name one and by `output_index` otherwise, whichever way the
/// server's own message reached the decoder: in the terminal `output`, or streamed before an
/// empty one. A message no delta showed carries the server's text. `shown-text-kept` appears
/// exactly when a server's text gave way, once, after every delta, quoting nothing either side
/// wrote. The same events, relayed by the ingress encoder, close on the turn: a gateway relaying
/// this stream can answer it.
#[test]
fn the_turn_carries_the_text_the_caller_was_shown_and_says_when_the_server_wrote_other_text() {
    for (case, payloads, items, replaced) in shown_text_cases() {
        let decoded = decode_stream(&decoder_binding(), &payloads);
        let outcome = decoded
            .result
            .unwrap_or_else(|error| panic!("{case}: {error:?}"));
        assert_eq!(
            outcome.items, items,
            "{case}: the turn's text is not the text the caller was shown"
        );
        let shown = shown_text(&decoded.events);
        assert!(
            carried_text(&outcome).starts_with(&shown),
            "{case}: the caller was shown {shown:?}, the turn carries {:?}",
            outcome.items
        );
        let warnings = warnings(&decoded.events);
        let codes: Vec<&str> = warnings.iter().map(|(code, _)| *code).collect();
        let expected: &[&str] = if replaced { &[SHOWN_TEXT_KEPT] } else { &[] };
        assert_eq!(codes, expected, "{case}: {:?}", decoded.events);
        for (_, message) in &warnings {
            assert!(
                !message.contains(SERVER_TEXT) && !message.contains("draft"),
                "{case}: the warning quotes the text: {message:?}"
            );
        }
        if replaced {
            assert!(
                matches!(decoded.events.last(), Some(StreamEvent::Warning { .. })),
                "{case}: the warning comes before a delta: {:?}",
                decoded.events
            );
        }
        let mut relay = IngressStream::new("resp_gateway_1", 1_700_000_000);
        for event in &decoded.events {
            relay.chunk(event);
        }
        relay.close(&outcome).unwrap_or_else(|error| {
            panic!("{case}: the relayed stream cannot close on the turn: {error:?}")
        });
    }
}

/// Where shown text could be more than one message and none carries its exact text, the decoder
/// cannot tell which message was shown: it overwrites none, keeps the server's messages as sent,
/// adds no message of its own, and says so with `shown-text-unpaired`. Where exactly one of them
/// carries the shown text, that one is the message shown and nothing is said.
#[test]
fn shown_text_no_single_message_can_be_leaves_the_server_messages_as_sent_and_says_so() {
    let ambiguous = [
        created(),
        json!({"type": "response.output_text.delta", "item_id": "msg_2", "content_index": 0,
            "delta": "answer"}),
        completed(&json!([
            message(None, "preamble"),
            message(None, SERVER_TEXT)
        ])),
    ];
    let decoded = decode_stream(&decoder_binding(), &ambiguous);
    let outcome = decoded.result.expect("a completed turn");
    assert_eq!(
        outcome.items,
        vec![Item::assistant("preamble"), Item::assistant(SERVER_TEXT)],
        "a message no delta showed was overwritten, or the shown text was added beside them"
    );
    let codes: Vec<&str> = warnings(&decoded.events)
        .iter()
        .map(|(code, _)| *code)
        .collect();
    assert_eq!(codes, ["shown-text-unpaired"], "{:?}", decoded.events);
    for (_, message) in warnings(&decoded.events) {
        assert!(
            !message.contains("answer") && !message.contains(SERVER_TEXT),
            "the warning quotes the text: {message:?}"
        );
    }

    let resolved = [
        created(),
        json!({"type": "response.output_text.delta", "item_id": "msg_2", "content_index": 0,
            "delta": "answer"}),
        completed(&json!([message(None, "preamble"), message(None, "answer")])),
    ];
    let decoded = decode_stream(&decoder_binding(), &resolved);
    let outcome = decoded.result.expect("a completed turn");
    assert_eq!(
        outcome.items,
        vec![Item::assistant("preamble"), Item::assistant("answer")]
    );
    assert!(warnings(&decoded.events).is_empty(), "{:?}", decoded.events);
}

/// The live client hands the warning to the caller's sink before it returns, and the turn it
/// returns carries the text the sink was shown, not the text the server finished with.
#[tokio::test]
async fn the_live_client_says_so_before_it_returns_a_turn_whose_server_text_gave_way() {
    let (listener, url) = listener().await;
    let server = tokio::spawn(async move {
        let mut socket = accept(&listener).await;
        send(&mut socket, SSE_HEAD).await;
        send(
            &mut socket,
            &sse(&[
                created(),
                text_delta("msg_1", 0, "a draft"),
                completed(&json!([message(Some("msg_1"), SERVER_TEXT)])),
            ]),
        )
        .await;
        let _ = socket.shutdown().await;
    });
    let model = client(&url);
    let mut sink = VecSink::new(64, 64 * 1024);
    let outcome = model
        .turn(&request(), &mut sink, &Cancel::new())
        .await
        .expect("a completed turn");
    server.await.expect("the fixture server");
    assert_eq!(outcome.items, vec![Item::assistant("a draft")]);
    assert_eq!(sink.text(), "a draft");
    assert!(
        matches!(
            sink.events(),
            [StreamEvent::TextDelta { .. }, StreamEvent::Warning { code, .. }]
                if code == SHOWN_TEXT_KEPT
        ),
        "{:?}",
        sink.events()
    );
}
