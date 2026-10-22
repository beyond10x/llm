//! The class of each failure Bedrock Runtime reports: an exception frame inside a stream, and an
//! HTTP refusal before one.
//!
//! Every diagnostic is a fixed message. An exception's own `message` and an error body are never
//! read into one; an exception type is named only through the fixed text of the arm that knows
//! it.

use crate::eventstream::Message;
use llm_core::{Dispatch, Error, ErrorCode};
use llm_http::{HeaderMap, status_error};
use serde_json::Value;
use std::time::SystemTime;

/// The header Bedrock Runtime names a refusal's exception type in, before any frame was sent.
const ERROR_TYPE: &str = "x-amzn-errortype";

/// The class of one `:message-type: exception` (or `error`) frame.
///
/// The type is read from `:exception-type`, or from `:error-code` on a generic `error` frame.
/// Five types are Bedrock's own temporary state and retriable while the turn has not
/// `answered`: `throttlingException` (`rate-limited`), and `serviceUnavailableException`,
/// `internalServerException`, `modelNotReadyException` and `modelTimeoutException`
/// (`transport`). Bedrock's API reference tells a caller to retry the first four; the AWS SDKs
/// retry them as throttling or transient errors. A model timeout is classed as the HTTP 408
/// Bedrock answers it with before a stream opens, which [`http_error`] makes `transport` and
/// retriable. Once the turn has answered every type is final: another attempt would answer, and
/// bill, again. `modelStreamErrorException` and `validationException` are final whenever they
/// arrive, and so is every type this table does not name. Dispatch is `accepted`: the frame is
/// the answer.
pub(crate) fn exception(message: &Message, answered: bool) -> Error {
    let kind = message
        .string_header(":exception-type")
        .or_else(|| message.string_header(":error-code"))
        .unwrap_or_default();
    let (code, transient, text) = match kind {
        "throttlingException" => (
            ErrorCode::RateLimited,
            true,
            "Bedrock reported throttling inside the stream",
        ),
        "serviceUnavailableException" => (
            ErrorCode::Transport,
            true,
            "Bedrock reported the service unavailable inside the stream",
        ),
        "internalServerException" => (
            ErrorCode::Transport,
            true,
            "Bedrock reported an internal failure inside the stream",
        ),
        "modelNotReadyException" => (
            ErrorCode::Transport,
            true,
            "Bedrock reported the model not ready inside the stream",
        ),
        "modelStreamErrorException" => (
            ErrorCode::Protocol,
            false,
            "Bedrock reported a model stream error",
        ),
        "validationException" => (
            ErrorCode::Refused,
            false,
            "Bedrock refused the request as invalid inside the stream",
        ),
        "modelTimeoutException" => (
            ErrorCode::Transport,
            true,
            "Bedrock reported that the model timed out",
        ),
        "serviceQuotaExceededException" => (
            ErrorCode::Refused,
            false,
            "Bedrock reported an exhausted service quota inside the stream",
        ),
        "accessDeniedException" => (
            ErrorCode::Unauthorized,
            false,
            "Bedrock refused this account's access inside the stream",
        ),
        _ => (
            ErrorCode::Refused,
            false,
            "Bedrock reported a failure inside the stream",
        ),
    };
    Error::new(code, text)
        .with_dispatch(Dispatch::Accepted)
        .with_retriable(transient && !answered)
}

/// The class of an HTTP refusal: [`status_error`] for `status`, refined by the exception type
/// Bedrock names in `x-amzn-errortype` where that type changes the class.
///
/// `ServiceQuotaExceededException` is final whatever its status: the quota refuses every
/// identical request until it is raised. `ValidationException` is final whatever its status.
/// `ThrottlingException` and `ModelNotReadyException`, both 429, keep the retriable
/// `rate-limited` class a 429 has. The header value may carry a `:`-separated suffix, which is
/// not read.
pub(crate) fn http_error(status: u16, headers: &HeaderMap) -> Error {
    let error = status_error(status, headers, SystemTime::now());
    match error_type(headers) {
        Some("ServiceQuotaExceededException") => {
            let mut refused = Error::new(
                ErrorCode::Refused,
                format!("HTTP endpoint returned status {status} for an exhausted Bedrock quota"),
            )
            .with_dispatch(Dispatch::Rejected)
            .with_retriable(false);
            refused.retry_after_ms = error.retry_after_ms;
            refused
        }
        Some("ValidationException") => error.with_retriable(false),
        _ => error,
    }
}

/// This wire's [`llm_http::QuotaRule`]: a 429 [`http_error`] reads as an exhausted quota, by its
/// `x-amzn-errortype` alone. The body is not read.
pub(crate) fn quota(headers: &HeaderMap, _: &Value) -> bool {
    http_error(429, headers).code == ErrorCode::Refused
}

/// The exception type `x-amzn-errortype` names, without its suffix, when it is one header field
/// of the shape an AWS type name has.
fn error_type(headers: &HeaderMap) -> Option<&str> {
    let mut values = headers.get_all(ERROR_TYPE).iter();
    let (Some(value), None) = (values.next(), values.next()) else {
        return None;
    };
    let name = value.to_str().ok()?.split(':').next()?.trim();
    (!name.is_empty() && name.bytes().all(|byte| byte.is_ascii_alphanumeric())).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::{exception, http_error, quota};
    use crate::eventstream::{HeaderValue, Message};
    use llm_core::{Dispatch, ErrorCode};
    use llm_http::{HeaderMap, HeaderValue as HttpValue};
    use serde_json::json;

    fn frame(kind: &str) -> Message {
        Message {
            headers: vec![
                (
                    ":exception-type".to_owned(),
                    HeaderValue::String(kind.to_owned()),
                ),
                (
                    ":message-type".to_owned(),
                    HeaderValue::String("exception".to_owned()),
                ),
            ],
            payload: br#"{"message":"fixture text that must not be copied"}"#.to_vec(),
        }
    }

    fn named(value: &'static str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-amzn-errortype", HttpValue::from_static(value));
        headers
    }

    #[test]
    fn each_transient_exception_is_retriable_only_before_the_turn_answered() {
        for (kind, code) in [
            ("throttlingException", ErrorCode::RateLimited),
            ("serviceUnavailableException", ErrorCode::Transport),
            ("internalServerException", ErrorCode::Transport),
            ("modelNotReadyException", ErrorCode::Transport),
            ("modelTimeoutException", ErrorCode::Transport),
        ] {
            let before = exception(&frame(kind), false);
            assert_eq!(before.code, code, "{kind}");
            assert!(before.retriable && before.may_retry(), "{kind}");
            assert_eq!(before.dispatch, Dispatch::Accepted);
            let after = exception(&frame(kind), true);
            assert_eq!(after.code, code, "{kind}");
            assert!(!after.retriable, "{kind}");
        }
    }

    /// A model timeout inside the stream is the class of the 408 Bedrock answers it with before
    /// a stream opens.
    #[test]
    fn a_model_timeout_is_classed_as_a_408_before_the_stream() {
        let before_stream = http_error(408, &named("ModelTimeoutException"));
        let in_stream = exception(&frame("modelTimeoutException"), false);
        assert_eq!(
            (in_stream.code, in_stream.retriable, in_stream.may_retry()),
            (
                before_stream.code,
                before_stream.retriable,
                before_stream.may_retry()
            )
        );
        assert!(in_stream.may_retry());
        assert!(!exception(&frame("modelTimeoutException"), true).retriable);
    }

    #[test]
    fn every_other_exception_is_final_and_quotes_nothing() {
        for kind in [
            "modelStreamErrorException",
            "validationException",
            "serviceQuotaExceededException",
            "accessDeniedException",
            "somethingNewException",
            "",
        ] {
            let error = exception(&frame(kind), false);
            assert!(!error.retriable, "{kind}");
            assert_eq!(error.dispatch, Dispatch::Accepted);
            assert!(!error.message.contains("fixture"));
            assert!(!error.message.contains("somethingNew"));
        }
    }

    #[test]
    fn a_generic_error_frame_is_read_by_its_error_code() {
        let message = Message {
            headers: vec![
                (
                    ":error-code".to_owned(),
                    HeaderValue::String("throttlingException".to_owned()),
                ),
                (
                    ":message-type".to_owned(),
                    HeaderValue::String("error".to_owned()),
                ),
            ],
            payload: Vec::new(),
        };
        assert_eq!(exception(&message, false).code, ErrorCode::RateLimited);
    }

    #[test]
    fn the_error_type_header_refines_the_status_class() {
        let throttled = http_error(429, &named("ThrottlingException:http://example.invalid/"));
        assert_eq!(throttled.code, ErrorCode::RateLimited);
        assert!(throttled.retriable);
        let not_ready = http_error(429, &named("ModelNotReadyException"));
        assert!(not_ready.retriable);
        let quota_429 = http_error(429, &named("ServiceQuotaExceededException"));
        assert_eq!(quota_429.code, ErrorCode::Refused);
        assert_eq!(quota_429.dispatch, Dispatch::Rejected);
        assert!(!quota_429.retriable);
        let invalid = http_error(400, &named("ValidationException"));
        assert_eq!(invalid.code, ErrorCode::Refused);
        assert!(!invalid.retriable);
        let unavailable = http_error(503, &named("ServiceUnavailableException"));
        assert!(unavailable.retriable);
        assert_eq!(
            http_error(429, &HeaderMap::new()).code,
            ErrorCode::RateLimited
        );
    }

    #[test]
    fn the_quota_rule_reads_only_the_error_type() {
        assert!(quota(&named("ServiceQuotaExceededException"), &json!({})));
        assert!(!quota(&named("ThrottlingException"), &json!({})));
        assert!(!quota(
            &HeaderMap::new(),
            &json!({"message": "ServiceQuotaExceededException"})
        ));
        let mut twice = named("ServiceQuotaExceededException");
        twice.append(
            "x-amzn-errortype",
            HttpValue::from_static("ServiceQuotaExceededException"),
        );
        assert!(!quota(&twice, &json!({})));
    }
}
