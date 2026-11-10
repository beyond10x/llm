//! Adversary, story:opencode-go-session-on-every-spelling, pass 1.
//!
//! `classify_opencode` refuses an encoded `/` or `\` only where the decoded path so far is a
//! prefix of `/inference/go/` (or already under it). Its own rationale is that a server which
//! decodes `%2F` reaches `OpenCode` Go through such a spelling. On that same server model a
//! `..` segment behind an encoded separator resolves onto `/inference/go/` too, and those
//! spellings are neither refused nor given a session.
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

/// Decoding `%2F`/`%5C` and then resolving dot segments (the server model the unit's refusal is
/// written for) turns each of these into `/inference/go/anthropic/v1/`.
#[test]
fn a_dot_dot_behind_an_encoded_separator_is_refused_or_sends_a_session() {
    for url in [
        "https://opencode.ai/zen/..%2Finference/go/anthropic/v1",
        "https://opencode.ai/x%2F..%2Finference/go/anthropic/v1",
        "https://opencode.ai/zen%5C..%5Cinference/go/anthropic/v1",
    ] {
        match build(url) {
            Err(error) => assert!(error.to_string().contains("OpenCode Go"), "{url}: {error}"),
            Ok(client) => assert!(
                client.session_header().is_some(),
                "{url} is built with no session and no refusal"
            ),
        }
    }
}

/// Spellings the `url` crate folds onto the plain form before the classifier runs.
#[test]
fn url_crate_equivalents_send_a_session() {
    for url in [
        "https://opencode.ai:443/inference/go/anthropic/v1",
        "https://opencode.ai.:443/inference/go/anthropic/v1",
        "https://OPENCODE.AI.:443/inference/go/anthropic/v1",
        "https://opencode.ai/inference/./go/anthropic/v1",
        "https://opencode.ai/inference/%2E/go/anthropic/v1",
        "https://opencode.ai/zen/%2E%2E/inference/go/anthropic/v1",
        "https://opencode.ai/zen/.%2e/inference/go/anthropic/v1",
        "https://opencode%2Eai/inference/go/anthropic/v1",
        "https://opencode\u{3002}ai/inference/go/anthropic/v1",
        "https://opencode.ai\u{3002}/inference/go/anthropic/v1",
        "https://open\u{ad}code.ai/inference/go/anthropic/v1",
        "https://opencode.ai/inference/%67%6f/anthropic/v1",
        "https://opencode.ai/inference/go/anthropic/v1/",
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
fn look_alike_hosts_are_not_opencode_go() {
    for url in [
        "https://opencode.ai.evil.com/inference/go/anthropic/v1",
        "https://evilopencode.ai/inference/go/anthropic/v1",
        "https://opencode.ai.evil.com./inference/go/anthropic/v1",
        "https://xn--opencod-fya.ai/inference/go/anthropic/v1",
        "https://opencod\u{e9}.ai/inference/go/anthropic/v1",
        "https://opencode.aii/inference/go/anthropic/v1",
        "https://www.opencode.ai./inference/go/anthropic/v1",
    ] {
        let client = build(url).unwrap_or_else(|error| panic!("{url}: {error}"));
        assert_eq!(client.session_header(), None, "{url}");
        let named = client.with_session(llm_core::Id::new("conversation-1").unwrap());
        assert_eq!(named.session_header(), None, "{url}");
        assert!(!is_opencode_go(&BaseUrl::new(url).unwrap()), "{url}");
    }
}

#[test]
fn userinfo_never_forms_a_base_url() {
    for url in [
        "https://x@opencode.ai/inference/go/anthropic/v1",
        "https://x:y@opencode.ai./inference/go/anthropic/v1",
        "https://opencode.ai@evil.example/inference/go/anthropic/v1",
    ] {
        assert!(BaseUrl::new(url).is_err(), "{url}");
    }
}

/// `is_opencode_go`, the constructor and `with_session` agree on every spelling.
#[test]
fn is_opencode_go_agrees_with_the_constructor() {
    for url in [
        "https://opencode.ai./inference/go/anthropic/v1",
        "https://opencode.ai/inference/%67o/anthropic/v1",
        "https://opencode.ai/inference/%47o/anthropic/v1",
        "https://opencode.ai/inference//go/anthropic/v1",
        "https://opencode.ai/inference/go/anthropic%2Fv1",
        "https://opencode.ai./zen/%2f",
        "https://opencode.ai/inference/go/",
    ] {
        let go = is_opencode_go(&BaseUrl::new(url).unwrap());
        match build(url) {
            Ok(client) => {
                assert_eq!(client.session_header().is_some(), go, "{url}");
                let named = client.with_session(llm_core::Id::new("conversation-7").unwrap());
                let expected = go.then_some((OPENCODE_SESSION_HEADER, "conversation-7"));
                assert_eq!(named.session_header(), expected, "{url}");
            }
            Err(_) => assert!(!go, "{url} is refused yet is_opencode_go says true"),
        }
    }
}

#[test]
fn the_refusal_carries_no_url_or_secret() {
    let url = "https://opencode.ai./inference/go/secret-path%2Fv1";
    let error = build(url).err().expect("refused");
    let text = format!("{error} {error:?}");
    for leak in ["opencode.ai.", "secret-path", "%2F", "fixture-key", "https://"] {
        assert!(!text.contains(leak), "{leak} in {text}");
    }
}
