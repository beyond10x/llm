//! The retry class every refusal carries (Harness `WireError::retriable`,
//! `harness-wire/src/lib.rs`; assigned in `harness-http/src/status.rs:28`-`34` and
//! `harness-http/src/sse.rs:142`-`148`). It is stated on the error itself, so a caller decides
//! without a routing table; dispatch evidence is a separate fact and is never rewritten for it.

use llm_core::{Dispatch, Error, ErrorCode};
use serde_json::json;

const EVERY_CODE: [ErrorCode; 11] = [
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
fn a_new_error_is_final_until_its_producer_says_otherwise() {
    for code in EVERY_CODE {
        let error = Error::new(code, "fixture");
        assert!(!error.retriable, "{code:?}");
        assert!(!error.may_retry(), "{code:?}");
    }
}

/// 400, 401/403, 409 and credential refusals are never retried, whatever a producer claims.
/// Only the classes Harness retries may act on the flag: transport, rate-limited, unavailable,
/// and protocol (a stream cut inside an event).
#[test]
fn only_the_classes_harness_retries_may_act_on_the_retry_class() {
    for code in EVERY_CODE {
        let marked = Error::new(code, "fixture")
            .with_dispatch(Dispatch::Unknown)
            .with_retriable(true);
        assert!(marked.retriable, "{code:?}");
        let expected = matches!(
            code,
            ErrorCode::Transport
                | ErrorCode::RateLimited
                | ErrorCode::Unavailable
                | ErrorCode::Protocol
        );
        assert_eq!(marked.may_retry(), expected, "{code:?}");
    }
}

#[test]
fn marking_a_class_keeps_the_dispatch_evidence() {
    for dispatch in [
        Dispatch::NotSent,
        Dispatch::Rejected,
        Dispatch::Unknown,
        Dispatch::Accepted,
    ] {
        let error = Error::new(ErrorCode::Transport, "fixture")
            .with_dispatch(dispatch)
            .with_retriable(true);
        assert_eq!(error.dispatch, dispatch);
        let restamped = error.with_dispatch(Dispatch::Accepted);
        assert!(restamped.retriable, "restamping dispatch lost the class");
    }
}

#[test]
fn the_wire_form_states_a_retriable_class_and_omits_a_final_one() {
    let retriable = Error::new(ErrorCode::RateLimited, "fixture")
        .with_dispatch(Dispatch::Rejected)
        .with_retriable(true);
    let encoded = serde_json::to_value(&retriable).unwrap();
    assert_eq!(encoded["retriable"], json!(true));
    assert_eq!(serde_json::from_value::<Error>(encoded).unwrap(), retriable);

    let final_error = Error::new(ErrorCode::Refused, "fixture").with_dispatch(Dispatch::Rejected);
    let encoded = serde_json::to_value(&final_error).unwrap();
    assert!(
        encoded.get("retriable").is_none(),
        "a final error carries no class field: {encoded}"
    );
    // A peer that never heard of the class still decodes as final.
    let older = json!({"code": "transport", "message": "fixture", "dispatch": "unknown"});
    assert!(!serde_json::from_value::<Error>(older).unwrap().retriable);
}
