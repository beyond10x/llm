//! Adversary pass 2 on story:retry-classes-harness-retried (wave 2026-10-20-w09).
//!
//! Drives the encoder from the table correction round 1 wrote into the module documentation of
//! `crates/llm-responses/src/ingress.rs` ("Failures"): for every neutral code, the wire code it is
//! written as, the code it reads back as, and whether it reads back retriable when nothing was
//! streamed first. Every row is asserted from the table's own text, for both retry marks the
//! relaying side could have set. The converse the design rests on, that a relayed failure after
//! streamed output reads back final whatever its mark, is asserted beside it.

use llm_core::{CallId, Error, ErrorCode, Id, Protocol, Provenance, StreamEvent, ToolName};
use llm_responses::{Binding, IngressStream, decode_stream};

const CREATED_AT: u64 = 1_772_000_000;

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

/// The module table, row by row: neutral code and its retry mark, wire `error.code`, read back
/// as, and "retriable when nothing was streamed first". The mark matters only for `unavailable`.
fn documented(code: ErrorCode, retriable: bool) -> (&'static str, ErrorCode, bool) {
    match code {
        ErrorCode::Unavailable if retriable => {
            ("server_is_overloaded", ErrorCode::Unavailable, true)
        }
        ErrorCode::Unavailable => ("server_error", ErrorCode::Unavailable, true),
        ErrorCode::RateLimited => ("rate_limit_exceeded", ErrorCode::RateLimited, true),
        ErrorCode::Refused => ("refused", ErrorCode::Refused, false),
        ErrorCode::InvalidRequest => ("invalid_request", ErrorCode::Refused, false),
        ErrorCode::Transport => ("transport", ErrorCode::Refused, false),
        ErrorCode::Protocol => ("protocol", ErrorCode::Refused, false),
        ErrorCode::Unauthorized => ("unauthorized", ErrorCode::Refused, false),
        ErrorCode::TooLarge => ("too_large", ErrorCode::Refused, false),
        ErrorCode::Unsupported => ("unsupported", ErrorCode::Refused, false),
        ErrorCode::Cancelled => ("cancelled", ErrorCode::Refused, false),
        ErrorCode::Deadline => ("deadline", ErrorCode::Refused, false),
    }
}

const EVERY_ERROR_CODE: [ErrorCode; 11] = [
    ErrorCode::InvalidRequest,
    ErrorCode::Transport,
    ErrorCode::Protocol,
    ErrorCode::Unauthorized,
    ErrorCode::RateLimited,
    ErrorCode::Refused,
    ErrorCode::TooLarge,
    ErrorCode::Unsupported,
    ErrorCode::Cancelled,
    ErrorCode::Deadline,
    ErrorCode::Unavailable,
];

#[test]
fn every_row_of_the_failures_table_reads_back_as_documented_when_nothing_was_streamed() {
    for code in EVERY_ERROR_CODE {
        for retriable in [false, true] {
            let (wire, read_as, retried) = documented(code, retriable);
            let mut stream = IngressStream::new("resp_gateway_1", CREATED_AT);
            let payloads =
                stream.fail(&Error::new(code, "the upstream failed").with_retriable(retriable));
            let failed = payloads.last().expect("a terminal payload");
            assert_eq!(
                failed["response"]["error"]["code"], wire,
                "{code:?}, marked {retriable}"
            );
            let refusal = decode_stream(&binding(), &payloads)
                .result
                .expect_err("a failed response");
            assert_eq!(refusal.code, read_as, "{code:?}, marked {retriable}");
            assert_eq!(
                refusal.may_retry(),
                retried,
                "{code:?}, marked {retriable}: the table says retriable is {retried}: {refusal:?}"
            );
        }
    }
}

fn streamed() -> Vec<(&'static str, StreamEvent)> {
    vec![
        (
            "a text delta",
            StreamEvent::TextDelta {
                text: "Hel".to_owned(),
            },
        ),
        (
            "a reasoning delta",
            StreamEvent::ReasoningDelta {
                text: "Checking".to_owned(),
            },
        ),
        (
            "a tool-call announcement",
            StreamEvent::ToolCallStarted {
                call_id: CallId::new("call_1").expect("a call id"),
                name: ToolName::new("lookup").expect("a tool name"),
            },
        ),
    ]
}

#[test]
fn a_relayed_failure_after_streamed_output_reads_back_final_whatever_its_mark() {
    for (label, event) in streamed() {
        for code in [ErrorCode::Unavailable, ErrorCode::RateLimited] {
            let mut stream = IngressStream::new("resp_gateway_1", CREATED_AT);
            let mut payloads = stream.chunk(&event);
            assert!(!payloads.is_empty(), "{label}: something was streamed");
            payloads
                .extend(stream.fail(&Error::new(code, "the upstream failed").with_retriable(true)));
            let refusal = decode_stream(&binding(), &payloads)
                .result
                .expect_err("a failed response");
            assert_eq!(refusal.code, code, "{label}, {code:?}");
            assert!(
                !refusal.retriable && !refusal.may_retry(),
                "{label}, {code:?}: {refusal:?}"
            );
        }
    }
}
