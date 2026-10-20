//! Adversary pass 2 on story:retry-classes-harness-retried (wave 2026-10-20-w09).
//!
//! Drives the encoder from what correction round 1 wrote on `IngressStream::fail`
//! (`crates/llm-messages/src/ingress.rs`) and in `docs/messages.md` ("Failure"): with nothing
//! streamed before it, `unavailable` (`overloaded_error`) and `transport`, `protocol` and
//! `cancelled` (`api_error`) read back retriable whatever this side decided, and every other code
//! reads back final. The code each reads back as is the encoder's own documented mapping. The
//! converse, a relayed failure after streamed output reads back final whatever its mark, is
//! asserted beside it.
mod support;

use llm_core::{
    CallId, Cancel, Error, ErrorCode, Item, Provenance, StreamEvent, ToolName, TurnOutcome,
    TurnRequest, VecSink,
};
use llm_messages::{IngressStream, decode_stream};
use serde_json::Value;
use support::binding;

const MODEL: &str = "example-model-20260201";

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

async fn decode(payloads: &[Value]) -> Result<TurnOutcome, Error> {
    let mut sink = VecSink::new(64, 64 * 1024);
    decode_stream(
        &sse(payloads),
        &request(),
        &target(),
        &mut sink,
        &Cancel::new(),
    )
    .await
}

/// The documented read-back: the code, and whether it is retriable with nothing streamed first.
fn documented(code: ErrorCode) -> (ErrorCode, bool) {
    match code {
        ErrorCode::Unavailable => (ErrorCode::Unavailable, true),
        ErrorCode::Transport | ErrorCode::Protocol | ErrorCode::Cancelled => {
            (ErrorCode::Transport, true)
        }
        ErrorCode::InvalidRequest | ErrorCode::Unsupported => (ErrorCode::InvalidRequest, false),
        ErrorCode::Unauthorized => (ErrorCode::Unauthorized, false),
        ErrorCode::RateLimited => (ErrorCode::RateLimited, false),
        ErrorCode::Refused => (ErrorCode::Refused, false),
        ErrorCode::TooLarge => (ErrorCode::TooLarge, false),
        ErrorCode::Deadline => (ErrorCode::Deadline, false),
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

#[tokio::test]
async fn every_code_reads_back_as_documented_when_nothing_was_streamed() {
    for code in EVERY_ERROR_CODE {
        for retriable in [false, true] {
            let (read_as, retried) = documented(code);
            let mut stream = IngressStream::new("msg_gateway_1", Some(MODEL)).expect("a stream");
            let payloads =
                stream.fail(&Error::new(code, "the upstream failed").with_retriable(retriable));
            let refusal = decode(&payloads).await.expect_err("an error event");
            assert_eq!(refusal.code, read_as, "{code:?}, marked {retriable}");
            assert_eq!(
                refusal.may_retry(),
                retried,
                "{code:?}, marked {retriable}: documented retriable is {retried}: {refusal:?}"
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
                call_id: CallId::new("toolu_01").expect("a call id"),
                name: ToolName::new("lookup").expect("a tool name"),
            },
        ),
    ]
}

#[tokio::test]
async fn a_relayed_failure_after_streamed_output_reads_back_final_whatever_its_mark() {
    for (label, event) in streamed() {
        for code in [ErrorCode::Unavailable, ErrorCode::Transport] {
            let mut stream = IngressStream::new("msg_gateway_1", Some(MODEL)).expect("a stream");
            let mut payloads = stream.chunk(&event);
            assert!(!payloads.is_empty(), "{label}: something was streamed");
            payloads
                .extend(stream.fail(&Error::new(code, "the upstream failed").with_retriable(true)));
            let refusal = decode(&payloads).await.expect_err("an error event");
            assert_eq!(refusal.code, code, "{label}, {code:?}");
            assert!(
                !refusal.retriable && !refusal.may_retry(),
                "{label}, {code:?}: {refusal:?}"
            );
        }
    }
}
