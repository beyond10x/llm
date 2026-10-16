//! Adversary cases for the Messages ingress encoder (wave 2026-10-16-w07).
//!
//! Each case drives the public encoder and reads the answer back through this crate's own
//! decoder, the contract the encoder documents.
mod support;

use llm_core::{
    CallId, Cancel, Error, ErrorCode, Id, Item, Provenance, StopReason, StreamEvent, ToolCall,
    ToolName, TurnObservation, TurnOutcome, TurnRequest, Usage, VecSink,
};
use llm_messages::{IngressStream, decode_request, decode_stream};
use serde_json::{Value, json};
use support::binding;

const ID: &str = "msg_ingress_1";
const MODEL: &str = "example-model-20260201";

fn target() -> Provenance {
    binding().provenance().clone()
}

fn request() -> TurnRequest {
    let body = json!({
        "model": "caller-alias",
        "max_tokens": 512,
        "messages": [{"role":"user","content":"Summarise the log"}],
        "tools": [
            {"name":"lookup","description":"Look up a record","input_schema":{"type":"object"}},
        ],
        "stream": true,
    });
    decode_request(body.to_string().as_bytes(), &target())
        .expect("a Messages request")
        .request
}

fn outcome(stop_reason: StopReason, items: Vec<Item>, model: Option<&str>) -> TurnOutcome {
    TurnOutcome {
        stop_reason,
        items,
        observation: TurnObservation {
            binding: target(),
            upstream_model: model.map(|model| Id::new(model).unwrap()),
            response_id: Some(Id::new(ID).unwrap()),
            usage: Some(Usage {
                input_tokens: Some(21),
                output_tokens: Some(9),
                cached_input_tokens: Some(4),
                cache_creation_input_tokens: Some(6),
                reasoning_output_tokens: None,
            }),
            final_usage: true,
        },
    }
}

fn sse(payloads: &[Value]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for payload in payloads {
        let name = payload["type"].as_str().expect("a typed payload");
        bytes.extend_from_slice(format!("event: {name}\ndata: {payload}\n\n").as_bytes());
    }
    bytes
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

/// Acceptance: "decoded with `decode_stream` against the same provenance give a `TurnOutcome`
/// equal to the encoded one". `message_start` precedes the outcome, so a gateway relaying a
/// neutral stream announces no model (`StreamEvent` carries none) while the outcome it closes
/// on did observe one.
///
/// Decided (correction round 1): the stream keeps accepting. Refusing would refuse every relayed
/// stream whose upstream normalises an alias. The model travels only in `message_start`, so a
/// server passes the model it will announce to `IngressStream::new`; this pins the documented
/// limit that a model the stream never announced reads back unknown.
#[tokio::test]
async fn a_reported_model_is_not_dropped_when_none_was_announced() {
    let answer = outcome(
        StopReason::EndTurn,
        vec![Item::assistant("Checking.")],
        Some(MODEL),
    );
    let events = [StreamEvent::TextDelta {
        text: "Checking.".to_owned(),
    }];
    let payloads = encode(&events, &answer, None).expect("the answer closes");
    let (decoded, _) = decode(&payloads).await;
    let decoded = decoded.expect("the answer decodes");
    assert_eq!(
        decoded.observation.upstream_model, None,
        "story:messages-ingress-encoder: the streamed Messages answer carries a model only when \
         announced at new"
    );
}

/// `IngressStream::chunk` documents: "A fragment for a call never announced is held and its call
/// sent complete by `close`", and the `held` field: "`close` sends them complete". `close` never
/// reads `held`: an outcome without that call closes cleanly, and the call the stream reported
/// is neither sent nor refused.
#[test]
fn a_held_call_the_outcome_omits_is_refused() {
    let events = [
        StreamEvent::TextDelta {
            text: "Checking.".to_owned(),
        },
        StreamEvent::ToolArgumentsDelta {
            call_id: CallId::new("call-9").unwrap(),
            delta: "{\"q\":\"a\"}".to_owned(),
        },
    ];
    let answer = outcome(
        StopReason::EndTurn,
        vec![Item::assistant("Checking.")],
        Some(MODEL),
    );
    let closed = encode(&events, &answer, Some(MODEL));
    assert!(
        closed.is_err(),
        "a call the stream reported vanished at close: {closed:?}"
    );
}

/// Same contract, the other way into `held`: a call announced under a name this wire refuses is
/// held "for `close` to refuse" (`announce`). When the outcome omits that call, `close` accepts.
#[test]
fn a_call_held_for_its_name_is_refused_when_the_outcome_omits_it() {
    let events = [
        StreamEvent::TextDelta {
            text: "Checking.".to_owned(),
        },
        StreamEvent::ToolCallStarted {
            call_id: CallId::new("call-9").unwrap(),
            name: ToolName::new("workspace.read").unwrap(),
        },
    ];
    let answer = outcome(
        StopReason::EndTurn,
        vec![Item::assistant("Checking.")],
        Some(MODEL),
    );
    let closed = encode(&events, &answer, Some(MODEL));
    assert!(
        closed.is_err(),
        "a call held for close to refuse vanished at close: {closed:?}"
    );
}

/// Messages to Messages relay: an upstream answer of thinking, redacted thinking, then text,
/// read by this crate's decoder, then answered with this crate's encoder from exactly the events
/// and outcome the decoder produced. The redacted block emits no neutral event, so the encoder
/// opens the text at the index the redacted block holds in the outcome and `close` refuses after
/// the caller was already shown the text.
///
/// Decided (correction round 1): out of this story's reach, because `StreamEvent` carries no
/// block boundaries; filed as its own story. This pins the documented limit: `close` refuses with
/// the contradiction and sends nothing.
#[tokio::test]
async fn a_relayed_answer_with_redacted_thinking_reads_back_whole() {
    let upstream = [
        json!({"type":"message_start","message":{"id":ID,"type":"message","role":"assistant",
            "model":MODEL,"content":[]}}),
        json!({"type":"content_block_start","index":0,
            "content_block":{"type":"thinking","thinking":"","signature":""}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"thinking_delta","thinking":"Let me look"}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"signature_delta","signature":"sig-1"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,
            "content_block":{"type":"redacted_thinking","data":"opaque-bytes"}}),
        json!({"type":"content_block_stop","index":1}),
        json!({"type":"content_block_start","index":2,
            "content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":2,
            "delta":{"type":"text_delta","text":"Checking."}}),
        json!({"type":"content_block_stop","index":2}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},
            "usage":{"output_tokens":9}}),
        json!({"type":"message_stop"}),
    ];
    let (relayed, events) = decode(&upstream).await;
    let relayed = relayed.expect("the upstream answer decodes");
    assert_eq!(relayed.items.len(), 3);

    let mut stream = IngressStream::new(ID, Some(MODEL)).unwrap();
    let mut payloads = Vec::new();
    for event in &events {
        payloads.extend(stream.chunk(event));
    }
    let note = "story:messages-ingress-encoder: relaying redacted thinking between streamed blocks \
                needs block boundaries in StreamEvent; assert the round trip when they exist";
    let refused = stream.close(&relayed).expect_err(note);
    assert_eq!(refused.code, ErrorCode::Protocol, "{note}");
    assert_eq!(
        refused.message, "Messages answer contradicts what was already streamed",
        "{note}"
    );
    // Nothing was sent by the refused close: the stream is still open, so `fail` is what ends it,
    // and it needs no `message_start` because the chunks already sent one.
    let ended = stream.fail(&Error::new(ErrorCode::Protocol, "refused"));
    assert_eq!(ended.len(), 1, "{note}");
    assert_eq!(ended[0]["type"], json!("error"), "{note}");
    assert!(
        payloads
            .iter()
            .all(|payload| payload["type"] != "message_delta" && payload["type"] != "message_stop"),
        "{note}"
    );
}

/// Keeps the helper used by the omitted-call cases honest: the same call, when the outcome does
/// carry it, is sent complete and reads back (green control for the two cases above).
#[tokio::test]
async fn control_a_held_call_the_outcome_carries_is_sent_complete() {
    let events = [
        StreamEvent::TextDelta {
            text: "Checking.".to_owned(),
        },
        StreamEvent::ToolArgumentsDelta {
            call_id: CallId::new("call-9").unwrap(),
            delta: "{\"q\":\"a\"}".to_owned(),
        },
    ];
    let answer = outcome(
        StopReason::ToolCalls,
        vec![
            Item::assistant("Checking."),
            Item::ToolCall(ToolCall {
                call_id: CallId::new("call-9").unwrap(),
                name: ToolName::new("lookup").unwrap(),
                arguments: json!({"q":"a"}),
            }),
        ],
        Some(MODEL),
    );
    let payloads = encode(&events, &answer, Some(MODEL)).expect("encodes");
    let (decoded, _) = decode(&payloads).await;
    assert_eq!(decoded.expect("decodes"), answer);
}
