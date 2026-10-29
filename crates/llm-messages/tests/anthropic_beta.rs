//! The `anthropic-beta` names a client opts into: read from the request's header lines, carried
//! as one neutral hint, sent by this wire and reported as omitted where a wire cannot send them.
//!
//! Fixture documents only; no provider is contacted.
mod support;

use http::{HeaderMap, HeaderValue};
use llm_core::{Advisory, ErrorCode, MAX_ANTHROPIC_BETAS, Provenance, TurnRequest};
use llm_messages::{decode_request, decode_request_with_headers, omitted_advisories};
use serde_json::json;
use support::binding;

fn target() -> Provenance {
    binding().provenance().clone()
}

fn body() -> Vec<u8> {
    json!({
        "model": "caller-alias",
        "max_tokens": 512,
        "messages": [{"role": "user", "content": "Summarise the log"}],
    })
    .to_string()
    .into_bytes()
}

fn headers(lines: &[&str]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    for line in lines {
        headers.append("anthropic-beta", HeaderValue::from_str(line).unwrap());
    }
    headers
}

fn decoded(lines: &[&str]) -> TurnRequest {
    decode_request_with_headers(&body(), &headers(lines), &target())
        .expect("a Messages request")
        .request
}

#[test]
fn every_beta_header_line_is_carried_in_order_once_each() {
    let request = decoded(&[
        "interleaved-thinking-2025-05-14, context-1m-2025-08-07",
        "context-1m-2025-08-07,,fine-grained-tool-streaming-2025-05-14",
    ]);
    assert_eq!(
        request.controls.hints.anthropic_beta,
        [
            "interleaved-thinking-2025-05-14",
            "context-1m-2025-08-07",
            "fine-grained-tool-streaming-2025-05-14",
        ]
    );
    assert_eq!(
        request.controls.hints.advisories(),
        [Advisory::AnthropicBeta]
    );
    // This wire sends them, so a gateway forwarding to Messages reports nothing omitted.
    assert_eq!(omitted_advisories(&request), Vec::<Advisory>::new());
}

#[test]
fn without_a_beta_header_the_request_is_the_one_the_body_alone_decodes() {
    let with = decoded(&[]);
    let alone = decode_request(&body(), &target()).unwrap().request;
    assert_eq!(with, alone);
    assert_eq!(
        with.controls.hints.anthropic_beta,
        [] as [std::string::String; 0]
    );
}

#[test]
fn a_beta_name_outside_the_bound_is_refused_by_the_header_name() {
    for line in ["context 1m", "beta;q=1", "beta/2025", &"b".repeat(129)] {
        let error =
            decode_request_with_headers(&body(), &headers(&[line]), &target()).expect_err(line);
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{line}");
        assert!(
            error.message.contains("`anthropic-beta`"),
            "{line}: {error:?}"
        );
    }
}

#[test]
fn more_distinct_betas_than_a_turn_carries_are_refused_and_repeats_do_not_count() {
    let names: Vec<String> = (0..=MAX_ANTHROPIC_BETAS)
        .map(|n| format!("beta-{n}"))
        .collect();
    let error = decode_request_with_headers(&body(), &headers(&[&names.join(",")]), &target())
        .expect_err("one beta too many");
    assert!(error.message.contains("`anthropic-beta`"), "{error:?}");

    let repeated = vec!["beta-0"; MAX_ANTHROPIC_BETAS * 2].join(",");
    let request = decoded(&[&repeated, &names[..MAX_ANTHROPIC_BETAS].join(",")]);
    assert_eq!(
        request.controls.hints.anthropic_beta.len(),
        MAX_ANTHROPIC_BETAS
    );
}

#[test]
fn a_header_line_that_is_not_visible_ascii_is_refused() {
    let mut headers = headers(&[]);
    headers.append(
        "anthropic-beta",
        HeaderValue::from_bytes(b"beta-\xff").unwrap(),
    );
    let error = decode_request_with_headers(&body(), &headers, &target()).expect_err("opaque");
    assert!(error.message.contains("`anthropic-beta`"), "{error:?}");
}

#[test]
fn a_turn_whose_beta_hint_breaks_the_bound_is_refused_by_validation() {
    let mut request = decoded(&["context-1m-2025-08-07"]);
    request
        .controls
        .hints
        .anthropic_beta
        .push("context-1m-2025-08-07".to_owned());
    let error = request.validate().expect_err("a repeated name");
    assert!(error.message.contains("anthropic-beta"), "{error:?}");

    let mut request = decoded(&[]);
    request
        .controls
        .hints
        .anthropic_beta
        .push("two words".to_owned());
    assert!(request.validate().is_err());

    let mut request = decoded(&[]);
    request.controls.hints.anthropic_beta = (0..=MAX_ANTHROPIC_BETAS)
        .map(|n| format!("beta-{n}"))
        .collect();
    assert!(request.validate().is_err());
}
