//! story:opencode-go-session-on-every-spelling.
//!
//! `OpenCode` Go refuses a request without `x-opencode-session`. Spellings of its base URL that
//! reach the same endpoint (a trailing dot on the host, a percent-encoded unreserved character in
//! the path, an upper-case host) either send the session or are refused before any I/O.
mod support;

use llm_http::{HttpClient, Limits};
use llm_messages::{MessagesClient, OPENCODE_SESSION_HEADER, is_opencode_go};
use llm_providers::BaseUrl;
use std::{sync::Arc, time::Duration};
use support::{StaticResolver, binding_with, capabilities};

fn build(url: &str) -> Result<MessagesClient, llm_core::Error> {
    let total = Duration::from_secs(10);
    MessagesClient::new(
        binding_with(url, capabilities()),
        HttpClient::new(Limits {
            response_headers: total,
            idle: total,
            total,
        })
        .unwrap(),
        Arc::new(StaticResolver::new("fixture-key")),
    )
}

const SPELLINGS: [&str; 7] = [
    "https://opencode.ai./inference/go/anthropic/v1",
    "https://opencode.ai/inference/%67o/anthropic/v1",
    "https://OPENCODE.AI/inference/go/anthropic/v1",
    "https://OpenCode.AI./inference/%47o/anthropic/v1",
    "https://opencode.ai/%69nference/go/anthropic/v1",
    "https://opencode.ai./inference/%67%6F/anthropic/v1/",
    "https://opencode.ai:443/inference/g%6f/anthropic/v1",
];

#[test]
fn every_spelling_of_opencode_go_sends_a_session_or_is_refused() {
    for url in SPELLINGS {
        // `%47o` is `Go`, a different path: not OpenCode Go at all.
        let go = !url.contains("%47o");
        match build(url) {
            Ok(client) => {
                let header = client.session_header();
                if go {
                    let (name, value) = header.unwrap_or_else(|| panic!("{url} sends no session"));
                    assert_eq!(name, OPENCODE_SESSION_HEADER);
                    assert!(!value.is_empty(), "{url}");
                } else {
                    assert_eq!(header, None, "{url}");
                }
            }
            Err(error) => assert!(go, "{url} is refused though it is not OpenCode Go: {error}"),
        }
        let base = BaseUrl::new(url).unwrap();
        assert_eq!(is_opencode_go(&base), go, "{url}");
    }
}

#[test]
fn the_named_spellings_send_a_session() {
    for url in [
        "https://opencode.ai./inference/go/anthropic/v1",
        "https://opencode.ai/inference/%67o/anthropic/v1",
        "https://OPENCODE.AI/inference/go/anthropic/v1",
    ] {
        let client = build(url).unwrap_or_else(|error| panic!("{url}: {error}"));
        assert_eq!(
            client.session_header().map(|(name, _)| name),
            Some(OPENCODE_SESSION_HEADER),
            "{url}"
        );
    }
}

#[test]
fn an_encoded_separator_under_opencode_go_is_refused_by_name_before_io() {
    for url in [
        "https://opencode.ai/inference/go%2Fanthropic/v1",
        "https://opencode.ai./inference/go/anthropic%2fv1",
        "https://opencode.ai/inference%2fgo/anthropic/v1",
        "https://opencode.ai/inference/go/anthropic%5Cv1",
        "https://opencode.ai./zen/%2f",
        "https://opencode.ai/zen/..%2Finference/go/anthropic/v1",
    ] {
        let error = build(url)
            .err()
            .unwrap_or_else(|| panic!("{url} is built, with no session guarantee"));
        assert!(
            error.to_string().contains("OpenCode Go"),
            "{url}: the refusal names OpenCode Go: {error}"
        );
    }
}

#[test]
fn look_alikes_stay_not_opencode_go() {
    for url in [
        "https://opencode.ai.example/inference/go/anthropic/v1",
        "https://opencode.ai../inference/go/anthropic/v1",
        "https://opencode.ai:8443/inference/go/anthropic/v1",
        "http://opencode.ai./inference/go/anthropic/v1",
        "https://opencode.ai/inference/%67ox/anthropic/v1",
    ] {
        let client = build(url).unwrap_or_else(|error| panic!("{url}: {error}"));
        assert_eq!(client.session_header(), None, "{url}");
    }
}
