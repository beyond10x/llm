//! Adversary pass 1 on story:retry-classes-harness-retried (wave 2026-10-20-w09).
//!
//! **Through a gateway.** [`IngressStream::fail`] writes a neutral failure under "the error type
//! the decoder reads back as the same code" (`crates/llm-messages/src/ingress.rs`). llm-gateway
//! passes the upstream error's retry mark into it (`crates/llm-gateway-cli/src/translate.rs:809`
//! at llm-gateway `bc872f2`). Before this story no Messages stream error read back retriable, so
//! a failure a gateway marked final (after silent upstream output, after `cancelled`, after a
//! `protocol` refusal, or once its own retries were spent) stayed final one hop on. The
//! coordinator accepted that it no longer does (correction round 1); the gateway case below
//! records today's behaviour until story:relayed-failures-keep-their-retry-decision.
//!
//! **Silent output.** The unit's tests name an empty text block, an empty thinking block and a
//! text delta; these name the rest: a tool-call announcement and its argument delta, a thinking
//! delta, a redacted thinking block, an empty text delta, a block that opened and stopped, a
//! message delta and an event outside the pinned subset.
mod support;

use llm_core::{
    Cancel, Dispatch, Error, ErrorCode, Item, Provenance, TurnOutcome, TurnRequest, VecSink,
};
use llm_messages::{IngressStream, decode_stream};
use serde_json::{Value, json};
use support::binding;

const MODEL: &str = "example-model-20260201";

const PROSE: &str = "SECRET-UPSTREAM-TEXT";

/// The two stream errors marked retriable before output, and the class each reads as.
const TRANSIENT: [(&str, ErrorCode); 2] = [
    ("overloaded_error", ErrorCode::Unavailable),
    ("api_error", ErrorCode::Transport),
];

fn target() -> Provenance {
    binding().provenance().clone()
}

fn request() -> TurnRequest {
    let mut request = TurnRequest::new("internal-model", vec![Item::user("Summarise the log")]);
    request.max_output_tokens = Some(512);
    request
}

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

async fn decode(payloads: &[Value]) -> (Result<TurnOutcome, Error>, VecSink) {
    let mut sink = VecSink::new(64, 64 * 1024);
    let decoded = decode_stream(
        &sse(payloads),
        &request(),
        &target(),
        &mut sink,
        &Cancel::new(),
    )
    .await;
    (decoded, sink)
}

/// Today's behaviour, accepted by the coordinator: a relayed failure reads back by this decoder's
/// rules, so `overloaded_error` and `api_error` read back retriable when nothing was streamed
/// first, whatever the gateway decided. No wire field carries the gateway's decision; carrying it
/// across the hop is story:relayed-failures-keep-their-retry-decision, and this case flips to
/// final when that story lands.
#[tokio::test]
async fn a_failure_a_gateway_marked_final_reads_back_retriable_today() {
    for (code, read_as) in [
        (ErrorCode::Unavailable, ErrorCode::Unavailable),
        (ErrorCode::Transport, ErrorCode::Transport),
        (ErrorCode::Protocol, ErrorCode::Transport),
        (ErrorCode::Cancelled, ErrorCode::Transport),
    ] {
        let mut stream = IngressStream::new("msg_gateway_1", Some(MODEL)).expect("a stream");
        let payloads = stream.fail(&Error::new(code, "the upstream failed").with_retriable(false));
        let (decoded, sink) = decode(&payloads).await;
        let refusal = decoded.expect_err("an error event");
        assert_eq!(
            (refusal.code, refusal.dispatch),
            (read_as, Dispatch::Accepted),
            "{code:?}"
        );
        assert_eq!(sink.events(), [], "{code:?}");
        assert!(
            refusal.may_retry(),
            "{code:?}: a relayed failure reads back by the receiving decoder's rules until \
             story:relayed-failures-keep-their-retry-decision lands; then this case flips to \
             final: {refusal:?}"
        );
    }
}

fn started() -> Value {
    json!({"type": "message_start", "message": {"id": "msg_014a", "type": "message",
        "role": "assistant", "model": MODEL, "content": [],
        "usage": {"input_tokens": 11, "output_tokens": 1}}})
}

fn error_event(kind: &str) -> Value {
    json!({"type": "error", "error": {"type": kind, "message": PROSE}})
}

fn block(content_block: &Value) -> Value {
    json!({"type": "content_block_start", "index": 0, "content_block": content_block})
}

fn delta(delta: &Value) -> Value {
    json!({"type": "content_block_delta", "index": 0, "delta": delta})
}

/// The presence half: a keep-alive before and after the header leaves the attempt unanswered.
#[tokio::test]
async fn a_transient_error_after_keep_alives_around_the_header_is_retriable() {
    for (kind, class) in TRANSIENT {
        let ping = json!({"type": "ping"});
        let (decoded, sink) = decode(&[ping.clone(), started(), ping, error_event(kind)]).await;
        let refusal = decoded.expect_err("an error event");
        assert_eq!(refusal.code, class, "`{kind}`");
        assert!(refusal.may_retry(), "`{kind}`: {refusal:?}");
        assert_eq!(sink.events(), [], "`{kind}`");
    }
}

/// Output the caller may or may not be shown; after any of it a transient error is final.
fn outputs() -> Vec<(&'static str, Vec<Value>)> {
    let tool = block(&json!({"type": "tool_use", "id": "toolu_01", "name": "lookup", "input": {}}));
    let thinking = block(&json!({"type": "thinking", "thinking": "", "signature": ""}));
    let text = block(&json!({"type": "text", "text": ""}));
    vec![
        ("a tool-call announcement", vec![tool.clone()]),
        (
            "a tool argument delta",
            vec![
                tool,
                delta(&json!({"type": "input_json_delta", "partial_json": "{\"q\":"})),
            ],
        ),
        (
            "a thinking delta",
            vec![
                thinking.clone(),
                delta(&json!({"type": "thinking_delta", "thinking": "Checking the log"})),
            ],
        ),
        (
            "a signature delta",
            vec![
                thinking,
                delta(&json!({"type": "signature_delta", "signature": "EqQBCgIYAhIM"})),
            ],
        ),
        (
            "a redacted thinking block",
            vec![block(
                &json!({"type": "redacted_thinking", "data": "EmwKAhgBEgy3va3pzix"}),
            )],
        ),
        (
            "an empty text delta",
            vec![
                text.clone(),
                delta(&json!({"type": "text_delta", "text": ""})),
            ],
        ),
        (
            "a block that opened and stopped empty",
            vec![text, json!({"type": "content_block_stop", "index": 0})],
        ),
        (
            "a message delta",
            vec![json!({"type": "message_delta",
                "delta": {"stop_reason": "end_turn", "stop_sequence": null},
                "usage": {"output_tokens": 1}})],
        ),
        (
            "an event outside the pinned subset",
            vec![json!({"type": "content_block_preview", "index": 0})],
        ),
    ]
}

#[tokio::test]
async fn a_transient_error_after_any_silent_or_visible_output_is_final() {
    for (label, output) in outputs() {
        for (kind, class) in TRANSIENT {
            let mut payloads = vec![started()];
            payloads.extend(output.iter().cloned());
            payloads.push(error_event(kind));
            let (decoded, _) = decode(&payloads).await;
            let refusal = decoded.expect_err("an error event");
            assert_eq!(refusal.code, class, "{label}, `{kind}`: {refusal:?}");
            assert!(
                !refusal.retriable && !refusal.may_retry(),
                "{label}, `{kind}`: {refusal:?}"
            );
            assert!(!format!("{refusal:?}").contains(PROSE), "{label}");
        }
    }
}
