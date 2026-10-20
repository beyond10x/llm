//! Adversary pass 1 on story:retry-classes-harness-retried (wave 2026-10-20-w09).
//!
//! **Through a gateway.** [`IngressStream::fail`] writes a neutral failure under a wire code
//! chosen so that this crate's decoder reads it back (module documentation, "Failures"): a
//! retriable `unavailable` as `server_is_overloaded` and a final one as `server_error`. The split
//! exists only to carry the retry mark, and llm-gateway passes the upstream error's mark into it
//! (`crates/llm-gateway-cli/src/translate.rs:809` at llm-gateway `bc872f2`). Before this story
//! `server_error` and `rate_limit_exceeded` read back final, so a failure a gateway marked final
//! (after silent upstream output, or once its own retries were spent) stayed final one hop on.
//! The coordinator accepted that it no longer does (correction round 1); the gateway case below
//! records today's behaviour until story:relayed-failures-keep-their-retry-decision.
//!
//! **Silent output.** The story's rule is "retriable only before any output, visible or
//! silent". The unit's tests name an opening message item and a text delta; these name the rest:
//! reasoning, a tool-call announcement, an empty delta, a content part, an unknown event and a
//! payload with no type.

use llm_core::{Dispatch, Error, ErrorCode, Id, Protocol, Provenance};
use llm_responses::{Binding, IngressStream, decode_stream};
use serde_json::{Value, json};

const CREATED_AT: u64 = 1_772_000_000;

const PROSE: &str = "SECRET-UPSTREAM-TEXT";

/// The three codes marked retriable before output, and the class each reads as.
const TRANSIENT: [(&str, ErrorCode); 3] = [
    ("server_is_overloaded", ErrorCode::Unavailable),
    ("server_error", ErrorCode::Unavailable),
    ("rate_limit_exceeded", ErrorCode::RateLimited),
];

fn id(value: &str) -> Id {
    Id::new(value).expect("fixture identifier")
}

fn binding() -> Binding {
    Binding::new(
        Provenance {
            protocol: Protocol::Responses,
            provider: id("my-lab"),
            account: id("local"),
            endpoint: id("local-vllm"),
            model: id("small"),
            binding_revision: id("rev-1"),
        },
        id("example/Small-Model"),
    )
}

/// The refusal a caller reads from what a gateway wrote for `error`, nothing streamed first.
fn read_back(error: &Error) -> Error {
    let mut stream = IngressStream::new("resp_gateway_1", CREATED_AT);
    let payloads = stream.fail(error);
    decode_stream(&binding(), &payloads)
        .result
        .expect_err("a failed response is a refusal")
}

/// Today's behaviour, accepted by the coordinator: a relayed failure reads back by this decoder's
/// rules, so `server_error` and `rate_limit_exceeded` read back retriable when nothing was
/// streamed first, whatever the gateway decided. No wire field carries the gateway's decision;
/// carrying it across the hop is story:relayed-failures-keep-their-retry-decision, and this case
/// flips to final when that story lands.
#[test]
fn a_failure_a_gateway_marked_final_reads_back_retriable_today() {
    for code in [ErrorCode::Unavailable, ErrorCode::RateLimited] {
        let refusal = read_back(&Error::new(code, "the upstream failed").with_retriable(false));
        assert_eq!(
            (refusal.code, refusal.dispatch),
            (code, Dispatch::Accepted),
            "{code:?}"
        );
        assert!(
            refusal.may_retry(),
            "{code:?}: a relayed failure reads back by the receiving decoder's rules until \
             story:relayed-failures-keep-their-retry-decision lands; then this case flips to \
             final: {refusal:?}"
        );
    }
}

#[test]
fn a_failure_a_gateway_marked_retriable_reads_back_retriable() {
    let refusal =
        read_back(&Error::new(ErrorCode::Unavailable, "the upstream failed").with_retriable(true));
    assert_eq!(refusal.code, ErrorCode::Unavailable);
    assert!(refusal.may_retry(), "{refusal:?}");
}

fn failure(code: &str, nested: bool) -> Value {
    if nested {
        json!({"type": "response.failed", "response": {
            "id": "resp_1", "status": "failed", "error": {"code": code, "message": PROSE}
        }})
    } else {
        json!({"type": "error", "code": code, "message": PROSE})
    }
}

fn created() -> Vec<Value> {
    vec![
        json!({"type": "response.created", "response": {"id": "resp_1", "status": "in_progress"}}),
        json!({"type": "response.in_progress", "response": {"id": "resp_1", "status": "in_progress"}}),
    ]
}

/// The presence half: every lifecycle payload the design names leaves the attempt unanswered.
#[test]
fn a_transient_failure_after_every_lifecycle_payload_is_retriable() {
    for (code, class) in TRANSIENT {
        for nested in [false, true] {
            let mut payloads = vec![
                json!({"type": "keepalive"}),
                json!({"type": "response.queued", "response": {"id": "resp_1", "status": "queued"}}),
            ];
            payloads.extend(created());
            payloads.push(json!({"type": "keepalive"}));
            payloads.push(failure(code, nested));
            let decoding = decode_stream(&binding(), &payloads);
            let refusal = decoding.result.expect_err("a failed response");
            assert_eq!(refusal.code, class, "`{code}`, nested {nested}");
            assert!(
                refusal.may_retry(),
                "`{code}`, nested {nested}: {refusal:?}"
            );
            assert_eq!(decoding.events, [], "`{code}`");
        }
    }
}

/// Output the caller may or may not be shown; after any of it a transient failure is final.
fn outputs() -> Vec<(&'static str, Value)> {
    vec![
        (
            "a reasoning item opening",
            json!({"type": "response.output_item.added", "output_index": 0,
                "item": {"type": "reasoning", "id": "rs_1", "summary": []}}),
        ),
        (
            "a reasoning summary delta",
            json!({"type": "response.reasoning_summary_text.delta", "item_id": "rs_1",
                "output_index": 0, "summary_index": 0, "delta": "Checking the log"}),
        ),
        (
            "a reasoning text delta",
            json!({"type": "response.reasoning_text.delta", "item_id": "rs_1",
                "output_index": 0, "content_index": 0, "delta": "Checking the log"}),
        ),
        (
            "a tool-call announcement",
            json!({"type": "response.output_item.added", "output_index": 0,
                "item": {"type": "function_call", "id": "fc_1", "call_id": "call_1",
                    "name": "lookup", "arguments": ""}}),
        ),
        (
            "an empty text delta",
            json!({"type": "response.output_text.delta", "item_id": "msg_1",
                "output_index": 0, "content_index": 0, "delta": ""}),
        ),
        (
            "a content part opening",
            json!({"type": "response.content_part.added", "item_id": "msg_1",
                "output_index": 0, "content_index": 0,
                "part": {"type": "output_text", "text": "", "annotations": []}}),
        ),
        (
            "an event outside the pinned subset",
            json!({"type": "response.audio.delta", "delta": "AAAA"}),
        ),
        ("a payload with no type", json!({"sequence_number": 3})),
    ]
}

#[test]
fn a_transient_failure_after_any_silent_or_visible_output_is_final() {
    for (label, output) in outputs() {
        for (code, class) in TRANSIENT {
            for nested in [false, true] {
                let mut payloads = created();
                payloads.push(output.clone());
                payloads.push(failure(code, nested));
                let refusal = decode_stream(&binding(), &payloads)
                    .result
                    .expect_err("a failed response");
                assert_eq!(refusal.code, class, "{label}, `{code}`, nested {nested}");
                assert!(
                    !refusal.retriable && !refusal.may_retry(),
                    "{label}, `{code}`, nested {nested}: {refusal:?}"
                );
                assert!(!format!("{refusal:?}").contains(PROSE), "{label}");
            }
        }
    }
}
