//! Ingress encoding: a neutral answer re-emitted on the Messages wire reads back through this
//! crate's own decoder as the same answer.
//!
//! The request arrives through `decode_request`, the answer is encoded by `IngressStream` or
//! `encode_ingress_message`, and the bytes are read back by `decode_stream` or `decode_message`
//! against the same binding. Equality of the two outcomes is the whole claim; nothing here
//! re-implements either half.
mod support;

use llm_core::{
    CallId, Cancel, Error, ErrorCode, Id, Item, Protocol, Provenance, StopReason, StreamEvent,
    ToolCall, ToolName, TurnObservation, TurnOutcome, TurnRequest, Usage, VecSink,
};
use llm_messages::{
    IngressStream, decode_message, decode_request, decode_stream, encode_ingress_message,
};
use serde_json::{Value, json};
use support::binding;

const ID: &str = "msg_ingress_1";
const MODEL: &str = "example-model-20260201";

fn target() -> Provenance {
    binding().provenance().clone()
}

/// The caller's request, read the way a gateway reads it.
fn request() -> TurnRequest {
    let body = json!({
        "model": "caller-alias",
        "max_tokens": 512,
        "messages": [{"role":"user","content":"Summarise the log"}],
        "tools": [
            {"name":"lookup","description":"Look up a record","input_schema":{"type":"object"}},
            {"name":"search","description":"Search the log","input_schema":{"type":"object"}},
        ],
        "stream": true,
    });
    let ingress =
        decode_request(body.to_string().as_bytes(), &target()).expect("a Messages request");
    assert!(ingress.stream);
    ingress.request
}

fn thinking(text: &str, signature: &str) -> Item {
    Item::Opaque {
        provenance: target(),
        payload: json!({"type":"thinking","thinking":text,"signature":signature}),
    }
}

fn call(id: &str, name: &str, arguments: Value) -> Item {
    Item::ToolCall(ToolCall {
        call_id: CallId::new(id).unwrap(),
        name: ToolName::new(name).unwrap(),
        arguments,
    })
}

fn started(id: &str, name: &str) -> StreamEvent {
    StreamEvent::ToolCallStarted {
        call_id: CallId::new(id).unwrap(),
        name: ToolName::new(name).unwrap(),
    }
}

fn fragment(id: &str, delta: &str) -> StreamEvent {
    StreamEvent::ToolArgumentsDelta {
        call_id: CallId::new(id).unwrap(),
        delta: delta.to_owned(),
    }
}

fn text(delta: &str) -> StreamEvent {
    StreamEvent::TextDelta {
        text: delta.to_owned(),
    }
}

fn reasoning(delta: &str) -> StreamEvent {
    StreamEvent::ReasoningDelta {
        text: delta.to_owned(),
    }
}

fn usage() -> Usage {
    Usage {
        input_tokens: Some(21),
        output_tokens: Some(9),
        cached_input_tokens: Some(4),
        cache_creation_input_tokens: Some(6),
        reasoning_output_tokens: Some(3),
    }
}

fn outcome(stop_reason: StopReason, items: Vec<Item>) -> TurnOutcome {
    TurnOutcome {
        stop_reason,
        items,
        observation: TurnObservation {
            binding: target(),
            upstream_model: Some(Id::new(MODEL).unwrap()),
            response_id: Some(Id::new(ID).unwrap()),
            usage: Some(usage()),
            final_usage: true,
        },
    }
}

/// Reasoning, then text, then two tool calls, as a producer streams them.
fn calls_events() -> Vec<StreamEvent> {
    vec![
        reasoning("Let me "),
        reasoning("look"),
        text("Check"),
        text("ing."),
        started("call-1", "lookup"),
        fragment("call-1", "{\"q\":"),
        fragment("call-1", "\"a\"}"),
        started("call-2", "search"),
        fragment("call-2", "{\"q\":\"b\"}"),
    ]
}

fn calls_items() -> Vec<Item> {
    vec![
        thinking("Let me look", "sig-1"),
        Item::assistant("Checking."),
        call("call-1", "lookup", json!({"q":"a"})),
        call("call-2", "search", json!({"q":"b"})),
    ]
}

/// `end_turn` with calls is refused by the neutral turn itself (`TurnOutcome::validate_for`), so
/// the completed turn carries reasoning and text only.
fn text_events() -> Vec<StreamEvent> {
    vec![reasoning("Let me look"), text("Check"), text("ing.")]
}

fn text_items() -> Vec<Item> {
    vec![
        thinking("Let me look", "sig-1"),
        Item::assistant("Checking."),
    ]
}

/// Frames each payload as the route does: the event named by its own `type`.
fn sse(payloads: &[Value]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for payload in payloads {
        let name = payload["type"]
            .as_str()
            .expect("every payload names its type");
        bytes.extend_from_slice(format!("event: {name}\ndata: {payload}\n\n").as_bytes());
    }
    bytes
}

fn encode(
    events: &[StreamEvent],
    outcome: &TurnOutcome,
    model: Option<&str>,
) -> Result<Vec<Value>, Error> {
    let mut stream = IngressStream::new(ID, model)?;
    let mut payloads = Vec::new();
    for event in events {
        payloads.extend(stream.chunk(event));
    }
    payloads.extend(stream.close(outcome)?);
    Ok(payloads)
}

async fn decode(payloads: &[Value]) -> (Result<TurnOutcome, Error>, Vec<StreamEvent>) {
    let mut sink = VecSink::new(256, 256 * 1024);
    let decoded = decode_stream(
        &sse(payloads),
        &request(),
        &target(),
        &mut sink,
        &Cancel::new(),
    )
    .await;
    (decoded, sink.into_events())
}

/// The decoded outcome equals the encoded one, and the caller was shown exactly the deltas the
/// producer streamed, in the same order.
async fn round_trip(events: &[StreamEvent], outcome: &TurnOutcome) -> Vec<Value> {
    let payloads = encode(events, outcome, Some(MODEL)).expect("the answer encodes");
    let (decoded, shown) = decode(&payloads).await;
    assert_eq!(&decoded.expect("the answer decodes"), outcome);
    assert_eq!(shown, events);
    payloads
}

fn kinds(payloads: &[Value]) -> Vec<String> {
    payloads
        .iter()
        .map(|payload| {
            let kind = payload["type"].as_str().unwrap();
            match payload.get("index") {
                Some(index) => format!("{kind}:{index}"),
                None => kind.to_owned(),
            }
        })
        .collect()
}

#[tokio::test]
async fn a_streamed_end_turn_round_trips() {
    round_trip(&text_events(), &outcome(StopReason::EndTurn, text_items())).await;
}

#[tokio::test]
async fn a_streamed_tool_call_turn_round_trips() {
    let payloads = round_trip(
        &calls_events(),
        &outcome(StopReason::ToolCalls, calls_items()),
    )
    .await;
    // Each kind change stops the open block, except thinking: its signature is the outcome's, so
    // it is signed and stopped at close, after the blocks that followed it.
    assert_eq!(
        kinds(&payloads),
        [
            "message_start",
            "content_block_start:0",
            "content_block_delta:0",
            "content_block_delta:0",
            "content_block_start:1",
            "content_block_delta:1",
            "content_block_delta:1",
            "content_block_stop:1",
            "content_block_start:2",
            "content_block_delta:2",
            "content_block_delta:2",
            "content_block_stop:2",
            "content_block_start:3",
            "content_block_delta:3",
            "content_block_delta:0",
            "content_block_stop:0",
            "content_block_stop:3",
            "message_delta",
            "message_stop",
        ]
    );
    assert_eq!(
        payloads[14]["delta"],
        json!({"type":"signature_delta","signature":"sig-1"})
    );
    assert_eq!(payloads[0]["message"]["model"], json!(MODEL));
    assert_eq!(payloads[0]["message"]["id"], json!(ID));
}

#[tokio::test]
async fn a_streamed_max_tokens_turn_round_trips() {
    round_trip(
        &calls_events(),
        &outcome(StopReason::MaxOutputTokens, calls_items()),
    )
    .await;
}

#[tokio::test]
async fn a_streamed_incomplete_reason_round_trips_under_its_own_name() {
    let reason = StopReason::Incomplete {
        reason: "refusal".to_owned(),
    };
    let payloads = round_trip(&calls_events(), &outcome(reason, calls_items())).await;
    let delta = payloads
        .iter()
        .find(|payload| payload["type"] == "message_delta")
        .unwrap();
    assert_eq!(delta["delta"]["stop_reason"], json!("refusal"));
}

#[test]
fn the_non_streamed_message_round_trips_for_every_stop_reason() {
    for (reason, items) in [
        (StopReason::EndTurn, text_items()),
        (StopReason::ToolCalls, calls_items()),
        (StopReason::MaxOutputTokens, calls_items()),
        (
            StopReason::Incomplete {
                reason: "pause_turn".to_owned(),
            },
            calls_items(),
        ),
    ] {
        let encoded = outcome(reason, items);
        let message = encode_ingress_message(&encoded, ID).expect("the answer encodes");
        assert_eq!(message["type"], json!("message"));
        let decoded = decode_message(&message, &request(), &target()).expect("it decodes");
        assert_eq!(decoded, encoded);
    }
}

#[tokio::test]
async fn an_unknown_output_count_is_left_out_and_decodes_back_unknown() {
    let mut encoded = outcome(StopReason::EndTurn, text_items());
    encoded.observation.usage = Some(Usage {
        output_tokens: None,
        reasoning_output_tokens: None,
        ..usage()
    });
    let payloads = round_trip(&text_events(), &encoded).await;
    let delta = payloads
        .iter()
        .find(|payload| payload["type"] == "message_delta")
        .unwrap();
    assert_eq!(
        delta["usage"],
        json!({"input_tokens":11,"cache_read_input_tokens":4,"cache_creation_input_tokens":6})
    );
    // Nothing on message_start claims a count either: the counters are known only at close.
    assert!(payloads[0]["message"].get("usage").is_none());

    let message = encode_ingress_message(&encoded, ID).unwrap();
    assert!(message["usage"].get("output_tokens").is_none());
    let decoded = decode_message(&message, &request(), &target()).unwrap();
    assert_eq!(decoded.observation.usage.unwrap().output_tokens, None);
}

#[test]
fn an_input_total_the_wire_cannot_split_is_left_out_never_approximated() {
    // The route reports fresh input disjointly from both cache counters, so the fresh count is
    // known only when both are. With cache creation unknown it is left out, never derived.
    let mut encoded = outcome(StopReason::EndTurn, text_items());
    encoded.observation.usage = Some(Usage {
        cache_creation_input_tokens: None,
        ..usage()
    });
    let message = encode_ingress_message(&encoded, ID).unwrap();
    assert_eq!(
        message["usage"],
        json!({"output_tokens":9,"cache_read_input_tokens":4,
            "output_tokens_details":{"thinking_tokens":3}})
    );
    let decoded = decode_message(&message, &request(), &target()).unwrap();
    assert_eq!(
        decoded.observation.usage,
        Some(Usage {
            input_tokens: None,
            cache_creation_input_tokens: None,
            ..usage()
        })
    );
}

#[tokio::test]
async fn an_answer_with_no_usage_reports_none() {
    let mut encoded = outcome(StopReason::EndTurn, text_items());
    encoded.observation.usage = None;
    let payloads = round_trip(&text_events(), &encoded).await;
    assert!(
        payloads
            .iter()
            .all(|payload| payload.get("usage").is_none())
    );
    let message = encode_ingress_message(&encoded, ID).unwrap();
    assert!(message.get("usage").is_none());
}

#[tokio::test]
async fn fail_produces_an_error_event_the_decoder_reads_as_its_code() {
    for (code, expected) in [
        (ErrorCode::InvalidRequest, ErrorCode::InvalidRequest),
        (ErrorCode::Transport, ErrorCode::Transport),
        (ErrorCode::Unauthorized, ErrorCode::Unauthorized),
        (ErrorCode::RateLimited, ErrorCode::RateLimited),
        (ErrorCode::Refused, ErrorCode::Refused),
        (ErrorCode::TooLarge, ErrorCode::TooLarge),
        (ErrorCode::Deadline, ErrorCode::Deadline),
        (ErrorCode::Unavailable, ErrorCode::Unavailable),
        // No error type of this route names these three; each takes the nearest one the
        // decoder reads, as the encoder documents.
        (ErrorCode::Protocol, ErrorCode::Transport),
        (ErrorCode::Cancelled, ErrorCode::Transport),
        (ErrorCode::Unsupported, ErrorCode::InvalidRequest),
    ] {
        // Nothing streamed yet: `fail` opens the message itself.
        let mut stream = IngressStream::new(ID, Some(MODEL)).unwrap();
        let payloads = stream.fail(&Error::new(code, "the upstream failed"));
        assert_eq!(kinds(&payloads), ["message_start", "error"], "{code:?}");
        assert_eq!(
            payloads[1]["error"]["message"],
            json!("the upstream failed")
        );
        let (decoded, _) = decode(&payloads).await;
        assert_eq!(decoded.expect_err("an error event").code, expected);

        // After content: the error ends the stream where it stands.
        let mut stream = IngressStream::new(ID, Some(MODEL)).unwrap();
        let mut payloads = stream.chunk(&text("partial"));
        payloads.extend(stream.fail(&Error::new(code, "the upstream failed")));
        assert_eq!(payloads.last().unwrap()["type"], json!("error"));
        assert_eq!(
            payloads
                .iter()
                .filter(|p| p["type"] == "message_start")
                .count(),
            1
        );
        let (decoded, shown) = decode(&payloads).await;
        assert_eq!(decoded.expect_err("an error event").code, expected);
        assert_eq!(shown, [text("partial")]);

        // Nothing follows the error.
        assert_eq!(stream.chunk(&text("late")), [] as [Value; 0]);
        assert!(
            stream
                .close(&outcome(StopReason::EndTurn, text_items()))
                .is_err()
        );
    }
}

#[test]
fn an_outcome_contradicting_the_streamed_calls_is_refused() {
    let full = outcome(StopReason::ToolCalls, calls_items());
    let mut missing = full.clone();
    missing.items.pop();
    let mut arguments = full.clone();
    arguments.items[2] = call("call-1", "lookup", json!({"q":"z"}));
    let mut renamed = full.clone();
    renamed.items[3] = call("call-2", "lookup", json!({"q":"b"}));
    let mut retexted = full.clone();
    retexted.items[1] = Item::assistant("Something else.");
    let mut reordered = full.clone();
    reordered.items.swap(2, 3);
    let mut resigned = full;
    resigned.items[0] = thinking("Other thoughts", "sig-1");
    for contradiction in [missing, arguments, renamed, retexted, reordered, resigned] {
        let error = encode(&calls_events(), &contradiction, Some(MODEL))
            .expect_err("an outcome other than the streamed one");
        assert_eq!(error.code, ErrorCode::Protocol, "{contradiction:?}");
    }
}

#[test]
fn a_refused_close_sends_nothing_and_leaves_the_stream_to_fail() {
    let mut stream = IngressStream::new(ID, Some(MODEL)).unwrap();
    for event in calls_events() {
        stream.chunk(&event);
    }
    let mut wrong = outcome(StopReason::ToolCalls, calls_items());
    wrong.items.pop();
    assert!(stream.close(&wrong).is_err());
    let payloads = stream.fail(&Error::new(ErrorCode::Protocol, "refused"));
    assert_eq!(kinds(&payloads), ["error"]);
}

#[test]
fn caller_owned_items_are_refused() {
    for owned in [
        Item::user("Summarise the log"),
        Item::ToolResult {
            call_id: CallId::new("call-1").unwrap(),
            output: json!("found"),
            failed: false,
        },
    ] {
        let mut answer = outcome(StopReason::EndTurn, text_items());
        answer.items.push(owned.clone());
        let error = encode(&text_events(), &answer, Some(MODEL)).expect_err("caller-owned");
        assert_eq!(error.code, ErrorCode::Protocol, "{owned:?}");
        let error = encode_ingress_message(&answer, ID).expect_err("caller-owned");
        assert_eq!(error.code, ErrorCode::Protocol, "{owned:?}");
    }
    let mut answer = outcome(StopReason::EndTurn, text_items());
    answer.items.push(Item::UnattributedOpaque {
        protocol: Protocol::Messages,
        payload: json!({"type":"thinking","thinking":"x","signature":"sig-2"}),
    });
    for error in [
        encode(&text_events(), &answer, Some(MODEL)).expect_err("unattributed"),
        encode_ingress_message(&answer, ID).expect_err("unattributed"),
    ] {
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert_eq!(error.message, Item::UNATTRIBUTED_REFUSAL);
    }
}

#[test]
fn reasoning_without_a_signature_is_refused() {
    // Streamed reasoning the outcome carries no signed block for.
    let unsigned = outcome(StopReason::EndTurn, vec![Item::assistant("Checking.")]);
    assert!(encode(&text_events(), &unsigned, Some(MODEL)).is_err());
    // A thinking block whose signature is empty or missing is one the decoder refuses.
    for payload in [
        json!({"type":"thinking","thinking":"Let me look","signature":""}),
        json!({"type":"thinking","thinking":"Let me look"}),
    ] {
        let mut answer = outcome(StopReason::EndTurn, text_items());
        answer.items[0] = Item::Opaque {
            provenance: target(),
            payload,
        };
        assert!(encode(&text_events(), &answer, Some(MODEL)).is_err());
        assert!(encode_ingress_message(&answer, ID).is_err());
    }
}

/// A stream built `without_reasoning_relay` shows no reasoning delta. Signed thinking the outcome
/// carries is still sent, whole at close, as content the stream never showed, and reads back.
#[tokio::test]
async fn reasoning_not_relayed_still_sends_signed_thinking_whole_at_close() {
    let answer = outcome(StopReason::EndTurn, text_items());
    let mut stream = IngressStream::new(ID, Some(MODEL))
        .unwrap()
        .without_reasoning_relay();
    for event in [reasoning("Let me "), reasoning("look")] {
        assert!(stream.chunk(&event).is_empty(), "{event:?}");
    }
    let payloads = stream.close(&answer).unwrap();
    let (decoded, shown) = decode(&payloads).await;
    assert_eq!(decoded.unwrap(), answer);
    assert_eq!(shown, [reasoning("Let me look"), text("Checking.")]);

    // Unshown content only follows what was streamed: text already relayed at index 0 cannot be
    // preceded by the thinking block the outcome places first.
    let mut stream = IngressStream::new(ID, Some(MODEL))
        .unwrap()
        .without_reasoning_relay();
    for event in text_events() {
        stream.chunk(&event);
    }
    assert!(stream.close(&answer).is_err());
}

#[test]
fn opaque_state_this_wire_cannot_carry_is_refused() {
    let mut foreign = target();
    foreign.protocol = Protocol::Responses;
    for item in [
        Item::Opaque {
            provenance: foreign,
            payload: json!({"type":"thinking","thinking":"x","signature":"sig-2"}),
        },
        Item::Opaque {
            provenance: target(),
            payload: json!({"type":"reasoning","summary":[]}),
        },
    ] {
        let mut answer = outcome(StopReason::EndTurn, vec![Item::assistant("Checking.")]);
        answer.items.push(item.clone());
        assert!(encode(&[], &answer, Some(MODEL)).is_err(), "{item:?}");
        assert!(encode_ingress_message(&answer, ID).is_err(), "{item:?}");
    }
}

#[test]
fn a_terminal_reason_the_decoder_would_read_as_another_is_refused() {
    for reason in ["end_turn", "tool_use", "max_tokens", "", "two words"] {
        let answer = outcome(
            StopReason::Incomplete {
                reason: reason.to_owned(),
            },
            calls_items(),
        );
        assert!(
            encode(&calls_events(), &answer, Some(MODEL)).is_err(),
            "{reason}"
        );
        assert!(encode_ingress_message(&answer, ID).is_err(), "{reason}");
    }
}

#[test]
fn the_announced_model_must_be_the_outcome_model() {
    let answer = outcome(StopReason::EndTurn, text_items());
    let error = encode(&text_events(), &answer, Some("another-model")).expect_err("contradiction");
    assert_eq!(error.code, ErrorCode::Protocol);
    let mut unreported = answer;
    unreported.observation.upstream_model = None;
    assert!(encode(&text_events(), &unreported, Some(MODEL)).is_err());
}

#[tokio::test]
async fn an_unreported_model_is_left_out() {
    let mut answer = outcome(StopReason::EndTurn, text_items());
    answer.observation.upstream_model = None;
    let payloads = encode(&text_events(), &answer, None).unwrap();
    assert!(payloads[0]["message"].get("model").is_none());
    let (decoded, _) = decode(&payloads).await;
    assert_eq!(decoded.unwrap(), answer);
    let message = encode_ingress_message(&answer, ID).unwrap();
    assert!(message.get("model").is_none());
}

#[tokio::test]
async fn close_with_nothing_streamed_emits_the_whole_answer() {
    let answer = outcome(StopReason::ToolCalls, calls_items());
    let payloads = encode(&[], &answer, Some(MODEL)).unwrap();
    assert_eq!(payloads[0]["type"], json!("message_start"));
    let (decoded, _) = decode(&payloads).await;
    assert_eq!(decoded.unwrap(), answer);
}

#[tokio::test]
async fn content_the_stream_never_showed_is_sent_at_close_after_what_it_did() {
    let redacted = Item::Opaque {
        provenance: target(),
        payload: json!({"type":"redacted_thinking","data":"opaque-bytes"}),
    };
    let mut answer = outcome(StopReason::ToolCalls, calls_items());
    answer.items.push(redacted.clone());
    answer.items.push(call("call-3", "lookup", json!({})));
    let payloads = encode(&calls_events(), &answer, Some(MODEL)).unwrap();
    let (decoded, _) = decode(&payloads).await;
    assert_eq!(decoded.unwrap(), answer);

    // Placed before content already streamed, it would be read back in another order.
    let mut misplaced = outcome(StopReason::ToolCalls, calls_items());
    misplaced.items.insert(1, redacted);
    assert!(encode(&calls_events(), &misplaced, Some(MODEL)).is_err());
}

#[tokio::test]
async fn fragments_for_a_call_never_announced_are_held_and_sent_complete() {
    let events = vec![text("Checking."), fragment("call-1", "{\"q\":\"a\"}")];
    let answer = outcome(
        StopReason::ToolCalls,
        vec![
            Item::assistant("Checking."),
            call("call-1", "lookup", json!({"q":"a"})),
        ],
    );
    let payloads = encode(&events, &answer, Some(MODEL)).unwrap();
    let (decoded, shown) = decode(&payloads).await;
    assert_eq!(decoded.unwrap(), answer);
    assert_eq!(
        shown,
        [
            text("Checking."),
            started("call-1", "lookup"),
            fragment("call-1", "{\"q\":\"a\"}"),
        ]
    );
}

#[test]
fn an_identifier_the_decoder_cannot_read_is_refused_before_anything_is_sent() {
    assert!(IngressStream::new("", Some(MODEL)).is_err());
    assert!(IngressStream::new("two words", Some(MODEL)).is_err());
    assert!(IngressStream::new(ID, Some("two words")).is_err());
    let answer = outcome(StopReason::EndTurn, text_items());
    assert!(encode_ingress_message(&answer, "two words").is_err());
}

/// story:chat-counts-every-output-delta. Messages has no block for refusal text, and sending it
/// as text would make it the assistant's answer: an answer or a conversation carrying it is
/// refused by name on both directions, never sent without it.
/// A model that reasons without showing it (Claude Haiku 5.5 at a low effort, observed on Bedrock
/// `InvokeModel`) opens its answer with a signed `thinking` block whose text is empty. Its
/// producer marks the block with an empty reasoning delta; the stream opens the thinking block
/// there, so the answer that follows takes the next index and `close` signs it in place.
fn empty_signed_items(after: Vec<Item>) -> Vec<Item> {
    [vec![thinking("", "sig-empty")], after].concat()
}

#[tokio::test]
async fn signed_reasoning_with_empty_text_then_text_round_trips() {
    let events = [reasoning(""), text("Smoke check "), text("passed.")];
    let answer = outcome(
        StopReason::EndTurn,
        empty_signed_items(vec![Item::assistant("Smoke check passed.")]),
    );
    let payloads = round_trip(&events, &answer).await;
    assert_eq!(
        kinds(&payloads),
        [
            "message_start",
            "content_block_start:0",
            "content_block_start:1",
            "content_block_delta:1",
            "content_block_delta:1",
            "content_block_stop:1",
            "content_block_delta:0",
            "content_block_stop:0",
            "message_delta",
            "message_stop",
        ]
    );
    assert_eq!(
        payloads[1]["content_block"],
        json!({"type":"thinking","thinking":"","signature":""})
    );
    assert_eq!(
        payloads[6]["delta"],
        json!({"type":"signature_delta","signature":"sig-empty"})
    );
}

#[tokio::test]
async fn signed_reasoning_with_empty_text_then_a_tool_call_round_trips() {
    let events = [
        reasoning(""),
        started("call-1", "lookup"),
        fragment("call-1", "{\"q\":\"a\"}"),
    ];
    let answer = outcome(
        StopReason::ToolCalls,
        empty_signed_items(vec![call("call-1", "lookup", json!({"q":"a"}))]),
    );
    let payloads = round_trip(&events, &answer).await;
    assert_eq!(payloads[1]["content_block"]["type"], json!("thinking"));
    assert_eq!(payloads[2]["content_block"]["type"], json!("tool_use"));
}

#[tokio::test]
async fn signed_reasoning_with_empty_text_alone_ending_by_max_tokens_round_trips() {
    let answer = outcome(StopReason::MaxOutputTokens, empty_signed_items(Vec::new()));
    let payloads = round_trip(&[reasoning("")], &answer).await;
    assert_eq!(
        kinds(&payloads),
        [
            "message_start",
            "content_block_start:0",
            "content_block_delta:0",
            "content_block_stop:0",
            "message_delta",
            "message_stop",
        ]
    );
}

/// The whole relay a gateway runs: the upstream's Messages stream (as Bedrock `InvokeModel`
/// carries it, the signature in one delta and no thinking text) is read by this crate's decoder,
/// every event it shows is re-emitted by `IngressStream`, and the client reads back the same
/// answer, its empty thinking block in its place.
#[tokio::test]
async fn an_upstream_answer_opening_with_empty_signed_reasoning_is_relayed_in_place() {
    let upstream = [
        json!({"type":"message_start","message":{"id":ID,"type":"message","role":"assistant",
            "model":MODEL,"content":[],"usage":{"input_tokens":4,"output_tokens":1}}}),
        json!({"type":"content_block_start","index":0,
            "content_block":{"type":"thinking","thinking":"","signature":""}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"signature_delta","signature":"sig-upstream"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":1,
            "delta":{"type":"text_delta","text":"Smoke check passed."}}),
        json!({"type":"content_block_stop","index":1}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},
            "usage":{"output_tokens":28}}),
        json!({"type":"message_stop"}),
    ];
    let (upstream_outcome, shown) = decode(&upstream).await;
    let upstream_outcome = upstream_outcome.expect("the upstream answer decodes");
    assert_eq!(shown, [reasoning(""), text("Smoke check passed.")]);
    assert_eq!(upstream_outcome.items[0], thinking("", "sig-upstream"));

    let mut stream = IngressStream::new(ID, Some(MODEL)).unwrap();
    let mut relayed = Vec::new();
    for event in &shown {
        relayed.extend(stream.chunk(event));
    }
    relayed.extend(
        stream
            .close(&upstream_outcome)
            .expect("the relayed answer does not contradict what was streamed"),
    );
    let (decoded, reshown) = decode(&relayed).await;
    assert_eq!(
        decoded.expect("the relayed answer decodes"),
        upstream_outcome
    );
    assert_eq!(reshown, shown);
}

/// A server that cannot sign reasoning relays none: an empty reasoning delta opens nothing, and
/// text streamed before the signed thinking the outcome places first is still a contradiction,
/// as before.
#[test]
fn without_reasoning_relay_an_empty_reasoning_delta_opens_nothing() {
    let answer = outcome(
        StopReason::EndTurn,
        empty_signed_items(vec![Item::assistant("Checking.")]),
    );
    let mut stream = IngressStream::new(ID, Some(MODEL))
        .unwrap()
        .without_reasoning_relay();
    assert!(stream.chunk(&reasoning("")).is_empty());
    assert_eq!(
        kinds(&stream.chunk(&text("Checking."))),
        [
            "message_start",
            "content_block_start:0",
            "content_block_delta:0"
        ]
    );
    assert!(stream.close(&answer).is_err());
}

#[test]
fn refusal_text_is_refused_by_name_and_never_dropped() {
    let refusal = Item::AssistantRefusal {
        text: "I can't help with that.".to_owned(),
    };
    let mut answer = outcome(StopReason::EndTurn, text_items());
    answer.items.push(refusal.clone());
    let streamed = [
        text_events(),
        vec![StreamEvent::RefusalDelta {
            text: "I can't help with that.".to_owned(),
        }],
    ]
    .concat();
    let mut request = TurnRequest::new("internal-model", vec![Item::user("Hello")]);
    request.items.extend([refusal, Item::user("Again")]);
    for error in [
        encode(&streamed, &answer, Some(MODEL)).expect_err("streamed"),
        encode_ingress_message(&answer, ID).expect_err("whole"),
        llm_messages::encode_request(&request, &binding()).expect_err("egress"),
    ] {
        assert_eq!(
            (error.code, error.message.as_str()),
            (
                ErrorCode::Unsupported,
                "Messages carries no assistant refusal text"
            )
        );
    }
}
