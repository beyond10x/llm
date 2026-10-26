//! Adversary, wave 2026-10-26-w20, unit messages-usage-cost, pass 1
//! (story:messages-usage-cost).
//!
//! The story promises that a streamed usage `cost` is carried "verbatim", and `docs/messages.md`
//! narrows that to "the same JSON value rather than the same bytes". These cases hold the
//! implementation to the narrower promise for numbers, and probe the edges the unit's own suite
//! leaves open: the byte bound's first refused length, where the name stays refused, a held
//! `message_start` cost against every stream error before answering and a cut, and the order of
//! a held warning against a preserved unknown event. Local sockets only.
mod support;

use llm_core::{
    Cancel, Error, ErrorCode, Item, Model, StreamEvent, TurnOutcome, TurnRequest, VecSink,
};
use llm_http::{HttpClient, Limits, SseEvent};
use llm_messages::{MessagesClient, StreamDecoder, USAGE_EXTENSION, decode_stream};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use support::{StaticResolver, binding, binding_with, capabilities};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

fn request() -> TurnRequest {
    TurnRequest::new("internal-model", vec![Item::user("Summarise the log")])
}

/// The SSE body for `events`, each payload written exactly as given: a number's spelling in
/// `raw` reaches the decoder untouched by any `serde_json` round trip in this test.
fn sse_raw(events: &[String]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for data in events {
        let parsed: Value = serde_json::from_str(data).expect("a JSON payload");
        let name = parsed["type"].as_str().expect("event type");
        bytes.extend_from_slice(format!("event: {name}\ndata: {data}\n\n").as_bytes());
    }
    bytes
}

const START: &str = r#"{"type":"message_start","message":{"id":"msg_014a","type":"message","role":"assistant","model":"example-model-20260201","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":11,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"output_tokens":1}}}"#;
const BLOCK_START: &str =
    r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#;
const BLOCK_DELTA: &str =
    r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ok"}}"#;
const BLOCK_STOP: &str = r#"{"type":"content_block_stop","index":0}"#;
const STOP: &str = r#"{"type":"message_stop"}"#;

/// A message delta whose usage carries `cost` spelled exactly as `raw`.
fn delta_with_raw_cost(raw: &str) -> String {
    format!(
        r#"{{"type":"message_delta","delta":{{"stop_reason":"end_turn","stop_sequence":null}},"usage":{{"input_tokens":11,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"output_tokens":8,"cost":{raw}}}}}"#
    )
}

fn raw_turn(raw_cost: &str) -> Vec<String> {
    vec![
        START.to_owned(),
        BLOCK_START.to_owned(),
        BLOCK_DELTA.to_owned(),
        BLOCK_STOP.to_owned(),
        delta_with_raw_cost(raw_cost),
        STOP.to_owned(),
    ]
}

async fn decode_bytes(body: &[u8]) -> (Result<TurnOutcome, Error>, Vec<StreamEvent>) {
    let mut sink = VecSink::new(64, 64 * 1024);
    let outcome = decode_stream(
        body,
        &request(),
        binding().provenance(),
        &mut sink,
        &Cancel::new(),
    )
    .await;
    (outcome, sink.into_events())
}

async fn decode(events: &[Value]) -> (Result<TurnOutcome, Error>, Vec<StreamEvent>) {
    let raw: Vec<String> = events.iter().map(Value::to_string).collect();
    decode_bytes(&sse_raw(&raw)).await
}

fn extensions(events: &[StreamEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            StreamEvent::Warning { code, message } if code == USAGE_EXTENSION => {
                Some(message.clone())
            }
            _ => None,
        })
        .collect()
}

/// The one warning a turn whose delta carries `raw` shows, after asserting the turn decoded.
async fn carried(raw: &str) -> String {
    let (outcome, events) = decode_bytes(&sse_raw(&raw_turn(raw))).await;
    outcome.expect("a streamed cost is admitted");
    let warnings = extensions(&events);
    assert_eq!(warnings.len(), 1, "{events:?}");
    warnings[0].clone()
}

/// A cost a producer printed as the shortest text of an `f64`, as JavaScript's `JSON.stringify`
/// and Python's `json.dumps` do. Without `serde_json`'s `float_roundtrip` feature, about one such
/// number in nine parses to a neighbouring `f64` and is carried as a different number (measured
/// on 2,000,000 random shortest-text costs between 1e-7 and 1e2 with `serde_json` 1.0.151: 230,167
/// changed value). `llm-messages` does not enable the feature; a `cargo test --workspace` build
/// gets it by feature unification from `ess-primitives`, a consumer's build does not.
#[tokio::test]
async fn adversary_w20_a_shortest_f64_cost_is_carried_as_the_same_number() {
    for raw in [
        "0.009027474764568267",
        "19.813640638684994",
        "1.8233521453552406e-5",
    ] {
        let message = carried(raw).await;
        let value = message
            .strip_prefix("message_delta.usage.cost: ")
            .unwrap_or_else(|| panic!("the warning names its path: {message}"));
        let sent: f64 = raw.parse().expect("a Rust-parsed f64");
        let shown: f64 = value.parse().expect("the warning carries a number");
        assert_eq!(
            shown.to_bits(),
            sent.to_bits(),
            "the cost {raw} was carried as {value}: not the same JSON value"
        );
    }
}

/// A decimal cost with more significant digits than an `f64` holds, as a producer computing in
/// a decimal type writes it, is carried as its nearest double in that double's shortest form.
///
/// This replaced the adversary's original assertion, that the warning carries `raw` itself
/// ("the same JSON value"). No `f64` holds this number, and `serde_json`'s `arbitrary_precision`
/// would change every JSON number in the workspace, so the coordinator corrected the promise
/// instead (`docs/messages.md`, `llm_messages::USAGE_EXTENSION`): a number is carried as the
/// nearest double in its shortest form, the value and not its bytes.
#[tokio::test]
async fn adversary_w20_a_decimal_cost_wider_than_a_double_is_carried_as_its_nearest_double() {
    let raw = "0.000420000000000000001";
    let nearest: f64 = raw.parse().expect("a Rust-parsed f64");
    assert_ne!(
        nearest.to_string(),
        raw,
        "the decimal is wider than a double"
    );
    let message = carried(raw).await;
    assert_eq!(message, "message_delta.usage.cost: 0.00042");
    assert_eq!(message, format!("message_delta.usage.cost: {nearest:?}"));
}

/// 1025 bytes is the first refused length; 1024 is carried (the unit's own case). The unit's
/// suite refuses only a value of about 1036 bytes, so a bound moved anywhere up to it passes.
#[tokio::test]
async fn adversary_w20_a_cost_of_exactly_1025_bytes_is_refused() {
    let over = json!("x".repeat(1023));
    assert_eq!(over.to_string().len(), 1025);
    let raw = over.to_string();
    let (outcome, events) = decode_bytes(&sse_raw(&raw_turn(&raw))).await;
    let error = outcome.expect_err("one byte over the bound is refused");
    assert_eq!(error.code, ErrorCode::TooLarge);
    assert_eq!(
        error.message,
        "Messages usage extension exceeds its bound: message_delta.usage.cost"
    );
    assert!(extensions(&events).is_empty(), "{events:?}");
}

/// The bound counts bytes of compact JSON: non-ASCII text is written unescaped, so 512 two-byte
/// characters plus quotes are 1026 bytes and refused, while 511 are 1024 and carried.
#[tokio::test]
async fn adversary_w20_the_bound_counts_utf8_bytes_not_characters() {
    let refused = json!("\u{e9}".repeat(512)).to_string();
    let (outcome, _) = decode_bytes(&sse_raw(&raw_turn(&refused))).await;
    assert_eq!(
        outcome.expect_err("over in bytes").code,
        ErrorCode::TooLarge
    );
    let fits = json!("\u{e9}".repeat(511)).to_string();
    assert_eq!(fits.len(), 1024);
    let message = carried(&fits).await;
    assert_eq!(message, format!("message_delta.usage.cost: {fits}"));
}

fn start_usage() -> Value {
    json!({"input_tokens":11,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,
        "output_tokens":1})
}

fn message_start(usage: &Value) -> Value {
    json!({"type":"message_start","message":{"id":"msg_014a","type":"message",
        "role":"assistant","model":"example-model-20260201","content":[],
        "stop_reason":null,"stop_sequence":null,"usage":usage}})
}

fn text_turn_with_delta(delta: Value) -> Vec<Value> {
    vec![
        message_start(&start_usage()),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ok"}}),
        json!({"type":"content_block_stop","index":0}),
        delta,
        json!({"type":"message_stop"}),
    ]
}

/// `cost` is admitted on the two usage objects only: under any nested usage object, beside the
/// usage at message level, or inside the delta, it is refused by its own path.
#[tokio::test]
async fn adversary_w20_cost_anywhere_but_the_usage_object_is_refused_by_its_path() {
    let usage = json!({"input_tokens":11,"output_tokens":8});
    let cases = [
        (
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},
                "usage":{"input_tokens":11,"output_tokens":8,
                    "cache_creation":{"ephemeral_5m_input_tokens":0,"cost":1}}}),
            "message_delta.usage.cache_creation.cost",
        ),
        (
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},
                "usage":{"input_tokens":11,"output_tokens":8,
                    "output_tokens_details":{"thinking_tokens":0,"cost":1}}}),
            "message_delta.usage.output_tokens_details.cost",
        ),
        (
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},
                "usage":{"input_tokens":11,"output_tokens":8,
                    "server_tool_use":{"web_search_requests":0,"cost":1}}}),
            "message_delta.usage.server_tool_use.cost",
        ),
        (
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn","cost":1},
                "usage":usage.clone()}),
            "message_delta.delta.cost",
        ),
        (
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},
                "usage":usage.clone(),"cost":1}),
            "message_delta.cost",
        ),
    ];
    for (delta, at) in cases {
        let (outcome, events) = decode(&text_turn_with_delta(delta)).await;
        let error = outcome.expect_err("cost is admitted on the usage object only");
        assert_eq!(
            error.code,
            ErrorCode::Unsupported,
            "{at}: {}",
            error.message
        );
        assert_eq!(
            error.message,
            format!("Messages field is outside the declared subset: {at}")
        );
        assert!(extensions(&events).is_empty(), "{at}: {events:?}");
    }
    let mut start = message_start(&start_usage());
    start["message"]["cost"] = json!(1);
    let (outcome, events) = decode(&[start]).await;
    assert_eq!(
        outcome.expect_err("refused").message,
        "Messages field is outside the declared subset: message_start.message.cost"
    );
    assert!(events.is_empty(), "{events:?}");
}

/// A held `message_start` cost against every stream error before answering: the attempt is as
/// retriable as the same stream without the cost, with the same code, and shows nothing.
#[tokio::test]
async fn adversary_w20_a_held_cost_changes_no_stream_error_before_answering() {
    for kind in [
        "overloaded_error",
        "api_error",
        "rate_limit_error",
        "timeout_error",
        "invalid_request_error",
    ] {
        let error = json!({"type":"error","error":{"type":kind,"message":"x"}});
        let mut costed_usage = start_usage();
        costed_usage["cost"] = json!(0.0001);
        let (plain, plain_events) = decode(&[
            message_start(&start_usage()),
            json!({"type":"ping"}),
            error.clone(),
        ])
        .await;
        let (costed, costed_events) =
            decode(&[message_start(&costed_usage), json!({"type":"ping"}), error]).await;
        let (plain, costed) = (plain.expect_err(kind), costed.expect_err(kind));
        assert_eq!(
            (costed.code, costed.retriable, costed.may_retry()),
            (plain.code, plain.retriable, plain.may_retry()),
            "{kind}"
        );
        assert!(plain_events.is_empty(), "{kind}: {plain_events:?}");
        assert!(costed_events.is_empty(), "{kind}: {costed_events:?}");
    }
}

/// A held cost is shown before the unknown event that answered the stream, and the event is
/// still preserved and reported.
#[tokio::test]
async fn adversary_w20_a_held_cost_is_shown_before_a_preserved_unknown_event() {
    let mut decoder = StreamDecoder::new(binding().provenance().clone());
    let mut sink = VecSink::new(64, 64 * 1024);
    let cancel = Cancel::new();
    let mut costed_usage = start_usage();
    costed_usage["cost"] = json!(0.0001);
    for data in [
        message_start(&costed_usage),
        json!({"type":"vendor_note","note":"hi"}),
    ] {
        let event = SseEvent::Payload {
            event: data["type"].as_str().map(str::to_owned),
            data,
        };
        decoder
            .apply(&event, &mut sink, &cancel)
            .await
            .expect("admitted");
    }
    assert!(decoder.has_answered());
    let codes: Vec<&str> = sink
        .events()
        .iter()
        .map(|event| match event {
            StreamEvent::Warning { code, .. } => code.as_str(),
            _ => "other",
        })
        .collect();
    assert_eq!(codes, [USAGE_EXTENSION, "unknown-stream-event"]);
}

const SSE_HEAD: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";

const PARTIAL: &str = "event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\"";

fn client(url: &str) -> MessagesClient {
    let bound = Duration::from_secs(20);
    MessagesClient::new(
        binding_with(url, capabilities()),
        HttpClient::new(Limits {
            response_headers: bound,
            idle: bound,
            total: Duration::from_secs(30),
        })
        .expect("bounded transport"),
        Arc::new(StaticResolver::new("fixture-key")),
    )
    .expect("a Messages binding")
}

async fn turn(body: String) -> (Result<TurnOutcome, Error>, VecSink) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local port");
    let url = format!("http://{}/v1", listener.local_addr().expect("an address"));
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("an accepted connection");
        let mut seen = Vec::new();
        let mut buffer = vec![0; 1 << 16];
        while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
            let length = socket.read(&mut buffer).await.expect("a readable socket");
            assert!(length > 0, "the client closed before sending a request");
            seen.extend_from_slice(&buffer[..length]);
        }
        socket.write_all(SSE_HEAD).await.expect("head written");
        socket
            .write_all(body.as_bytes())
            .await
            .expect("body written");
        let _ = socket.shutdown().await;
    });
    let mut request = request();
    request.max_output_tokens = Some(512);
    let mut sink = VecSink::new(64, 64 * 1024);
    let result = client(&url).turn(&request, &mut sink, &Cancel::new()).await;
    server.await.expect("the fixture server");
    (result, sink)
}

/// A cut right after a `message_start` that carried a cost is exactly as retriable as the same
/// cut without it, through the client a route reaches, and shows nothing.
#[tokio::test]
async fn adversary_w20_a_cut_after_a_costed_start_stays_as_retriable_as_without_it() {
    let mut costed_usage = start_usage();
    costed_usage["cost"] = json!({"total":0.0001,"currency":"USD"});
    let mut results = Vec::new();
    for start in [message_start(&start_usage()), message_start(&costed_usage)] {
        let body = format!("event: message_start\ndata: {start}\n\n{PARTIAL}");
        let (result, sink) = turn(body).await;
        let error = result.expect_err("a cut turn is a refusal");
        assert!(sink.events().is_empty(), "{:?}", sink.events());
        results.push((
            error.code,
            error.dispatch,
            error.retriable,
            error.may_retry(),
        ));
    }
    assert_eq!(results[1], results[0]);
    assert!(
        results[0].3,
        "a cut before answering stays retriable: {results:?}"
    );
}

/// A delta whose usage is accepted and whose stop reason is refused shows no warning: the cost
/// is "shown with that delta, once the whole delta was accepted" (`docs/messages.md`). The
/// unit's suite refuses a delta only inside its usage object, so emitting the warning before the
/// stop reason is checked passes it.
#[tokio::test]
async fn adversary_w20_a_delta_refused_for_its_stop_reason_shows_no_cost() {
    for (reasons, message) in [
        (
            vec![json!("")],
            "Messages terminal reason is not a bounded name",
        ),
        (
            vec![json!("end_turn"), json!("end_turn")],
            "Messages reported two terminal reasons",
        ),
    ] {
        let mut events = vec![
            message_start(&start_usage()),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ok"}}),
            json!({"type":"content_block_stop","index":0}),
        ];
        let last = reasons.len() - 1;
        for (n, reason) in reasons.into_iter().enumerate() {
            let cost = if n == last {
                json!(0.00042)
            } else {
                Value::Null
            };
            events.push(json!({"type":"message_delta",
                "delta":{"stop_reason":reason,"stop_sequence":null},
                "usage":{"input_tokens":11,"output_tokens":8,"cost":cost}}));
        }
        events.push(json!({"type":"message_stop"}));
        let (outcome, shown) = decode(&events).await;
        let error = outcome.expect_err("the delta is refused");
        assert_eq!(error.message, message);
        assert!(extensions(&shown).is_empty(), "{message}: {shown:?}");
    }
}
