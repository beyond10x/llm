//! Adversary cases for the Messages ingress encoder, second pass (wave 2026-10-16-w07).
//!
//! Each case drives the public encoder and reads the answer back through this crate's own
//! decoder against the same binding, the contract the encoder documents.
mod support;

use llm_core::{
    CallId, Cancel, Error, Id, Item, Provenance, StopReason, StreamEvent, ToolCall, ToolName,
    TurnObservation, TurnOutcome, TurnRequest, Usage, VecSink,
};
use llm_messages::{IngressStream, decode_request, decode_stream};
use serde_json::{Value, json};
use support::binding;

const ID: &str = "msg_ingress_2";
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
            {"name":"search","description":"Search the log","input_schema":{"type":"object"}},
        ],
        "stream": true,
    });
    decode_request(body.to_string().as_bytes(), &target())
        .expect("a Messages request")
        .request
}

fn outcome(stop_reason: StopReason, items: Vec<Item>, usage: Option<Usage>) -> TurnOutcome {
    TurnOutcome {
        stop_reason,
        items,
        observation: TurnObservation {
            binding: target(),
            upstream_model: Some(Id::new(MODEL).unwrap()),
            response_id: Some(Id::new(ID).unwrap()),
            usage,
            final_usage: true,
        },
    }
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

fn encode(events: &[StreamEvent], outcome: &TurnOutcome) -> Result<Vec<Value>, Error> {
    let mut stream = IngressStream::new(ID, Some(MODEL))?;
    let mut payloads = Vec::new();
    for event in events {
        payloads.extend(stream.chunk(event));
    }
    payloads.extend(stream.close(outcome)?);
    Ok(payloads)
}

/// A Chat Completions upstream on a local reasoning endpoint (vLLM's reasoning parser,
/// `reasoning_content`) streams its reasoning as `ReasoningDelta` and finishes with an outcome
/// that holds only the answer text: `crates/llm-chat/src/incoming.rs:131-139` ("never joins the
/// assistant text"), measured end to end in `crates/llm-chat/tests/local_endpoint.rs:95-105`
/// with exactly these strings, `items == [assistant("Guten Tag")]` and `usage == None`.
///
/// A gateway relays that neutral stream to a Messages caller. The outcome has a faithful
/// Messages answer, one text block, and the story's Acceptance requires the decoded outcome to
/// equal the encoded one. `IngressStream` opens a `thinking` block for the first delta, which can
/// only be signed by an opaque block the outcome never has, so `close` refuses every such turn
/// after the caller was already shown the whole answer.
///
/// Decided (correction round 2): a server whose upstream cannot sign reasoning builds its stream
/// `without_reasoning_relay`; the reasoning is then not relayed, and the turn reads back whole.
#[tokio::test]
async fn a_local_reasoning_turn_relayed_from_chat_is_answered_and_reads_back() {
    let events = [reasoning("The user greets me."), text("Guten Tag")];
    let answer = outcome(
        StopReason::EndTurn,
        vec![Item::assistant("Guten Tag")],
        None,
    );
    let mut stream = IngressStream::new(ID, Some(MODEL))
        .expect("a stream")
        .without_reasoning_relay();
    assert!(
        stream.chunk(&events[0]).is_empty(),
        "reasoning that is not relayed sends nothing"
    );
    let mut payloads = stream.chunk(&events[1]);
    payloads.extend(stream.close(&answer).expect(
        "a neutral turn from this repository's Chat decoder has a Messages answer, one text \
         block; close refused it after streaming the whole answer",
    ));
    let (decoded, shown) = decode(&payloads).await;
    assert_eq!(decoded.expect("the answer decodes"), answer);
    assert_eq!(shown, [text("Guten Tag")]);
}

/// Interleaved reasoning and text: two thinking blocks stay open at once while the text blocks
/// after each open and stop, and both are signed at close.
#[tokio::test]
async fn interleaved_thinking_and_text_read_back_whole() {
    let events = [
        reasoning("first "),
        reasoning("thought"),
        text("One."),
        reasoning("second thought"),
        text("Two."),
    ];
    let answer = outcome(
        StopReason::EndTurn,
        vec![
            thinking("first thought", "sig-a"),
            Item::assistant("One."),
            thinking("second thought", "sig-b"),
            Item::assistant("Two."),
        ],
        Some(Usage {
            output_tokens: Some(12),
            reasoning_output_tokens: Some(7),
            ..Usage::default()
        }),
    );
    let payloads = encode(&events, &answer).expect("the answer encodes");
    let (decoded, shown) = decode(&payloads).await;
    assert_eq!(decoded.expect("the answer decodes"), answer);
    assert_eq!(shown, events);
}

/// A call whose block was stopped by the next call before any fragment arrived was shown with
/// the opening `{}`. With empty arguments in the outcome it reads back as itself; with any other
/// arguments the caller assembled something else, and `close` must refuse.
#[tokio::test]
async fn a_call_stopped_before_any_fragment_reads_back_only_as_empty_arguments() {
    let events = [
        started("call-1", "lookup"),
        started("call-2", "search"),
        fragment("call-2", "{\"q\":\"b\"}"),
    ];
    let empty = outcome(
        StopReason::ToolCalls,
        vec![
            call("call-1", "lookup", json!({})),
            call("call-2", "search", json!({"q":"b"})),
        ],
        None,
    );
    let payloads = encode(&events, &empty).expect("the answer encodes");
    let (decoded, _) = decode(&payloads).await;
    assert_eq!(decoded.expect("the answer decodes"), empty);

    let filled = outcome(
        StopReason::ToolCalls,
        vec![
            call("call-1", "lookup", json!({"q":"a"})),
            call("call-2", "search", json!({"q":"b"})),
        ],
        None,
    );
    assert!(
        encode(&events, &filled).is_err(),
        "the caller assembled {{}} for call-1; arguments it never saw cannot follow its stop"
    );
}

/// Text that looks like SSE framing, CRLF, a NUL, a line separator and astral characters split
/// across deltas: each delta is one JSON string, so the framing cannot be broken and the text
/// reads back byte for byte.
#[tokio::test]
async fn text_resembling_framing_and_astral_characters_reads_back_exactly() {
    let deltas = [
        "Gr\u{fc}\u{df}e\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        "\r\n\u{0}\u{2028}",
        "\u{1f642}",
        "\u{1f469}\u{200d}\u{1f52c}",
    ];
    let events: Vec<StreamEvent> = deltas.iter().map(|delta| text(delta)).collect();
    let answer = outcome(
        StopReason::EndTurn,
        vec![Item::assistant(deltas.concat())],
        None,
    );
    let payloads = encode(&events, &answer).expect("the answer encodes");
    let (decoded, shown) = decode(&payloads).await;
    assert_eq!(decoded.expect("the answer decodes"), answer);
    assert_eq!(shown, events);
}
