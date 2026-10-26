//! Adversary, wave 2026-10-26-w19, story:opencode-go-session, pass 1.
//!
//! Drives the trigger and the minted value against what the unit wrote about them: the doc
//! comment of `is_opencode_go` (the `url` crate's serialisation, then an exact prefix) and
//! `docs/messages.md` (`llm-<process>-<clock>-<sequence>`, hexadecimal, different for each client).
mod support;

use llm_core::Id;
use llm_http::{HttpClient, Limits};
use llm_messages::{MessagesClient, OPENCODE_SESSION_HEADER, is_opencode_go};
use llm_providers::BaseUrl;
use std::{sync::Arc, time::Duration};
use support::{StaticResolver, binding_with, capabilities};

const OPENCODE_GO: &str = "https://opencode.ai/inference/go/anthropic/v1";

fn client(url: &str) -> MessagesClient {
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
    .expect("a Messages binding")
}

fn minted(client: &MessagesClient) -> String {
    let (name, value) = client.session_header().expect("an OpenCode Go session");
    assert_eq!(name, OPENCODE_SESSION_HEADER);
    value.to_owned()
}

/// The three hexadecimal fields after `llm-`, or a panic naming the value.
fn fields(value: &str) -> [String; 3] {
    let rest = value
        .strip_prefix("llm-")
        .unwrap_or_else(|| panic!("`{value}` does not start with `llm-`"));
    let parts: Vec<&str> = rest.split('-').collect();
    assert_eq!(parts.len(), 3, "`{value}` is not llm-<process>-<clock>-<sequence>");
    for part in &parts {
        assert!(
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "`{part}` of `{value}` is not lowercase hexadecimal"
        );
    }
    [parts[0].to_owned(), parts[1].to_owned(), parts[2].to_owned()]
}

/// Forms the `url` crate rewrites before the prefix is compared: each lands on the side the
/// normalised URL belongs to, including a fullwidth (IDNA-mapped) host, backslashes, an
/// uppercase scheme and percent-encoded dot segments.
#[test]
fn the_trigger_compares_the_normalised_url() {
    for url in [
        "HTTPS://OPENCODE.AI/inference/go/anthropic/v1",
        "https://\u{ff4f}\u{ff50}\u{ff45}\u{ff4e}\u{ff43}\u{ff4f}\u{ff44}\u{ff45}.ai/inference/go/anthropic/v1",
        "https:\\\\opencode.ai\\inference\\go\\anthropic\\v1",
        "https://opencode.ai/inference/./go/anthropic/v1",
        "https://opencode.ai/inference/%2e/go/anthropic/v1",
        "https://opencode.ai/inference/zen/../go/anthropic/v1",
    ] {
        let base = BaseUrl::new(url).unwrap_or_else(|error| panic!("{url}: {error}"));
        assert!(is_opencode_go(&base), "{url} -> {}", base.as_str());
    }
    for url in [
        "https://opencode.ai/inference/go/%2e%2e/zen/v1",
        "https://opencode.ai/inference/go/%2E%2E/zen/v1",
        "https://opencode.ai/INFERENCE/GO/anthropic/v1",
        "https://opencode.ai/inference//go/anthropic/v1",
        "https://opencode.ai%2eexample/inference/go/anthropic/v1",
        "https://[::1]/inference/go/anthropic/v1",
        "https://127.0.0.1/inference/go/anthropic/v1",
        "https://opencode.ai:80/inference/go/anthropic/v1",
    ] {
        let base = BaseUrl::new(url).unwrap_or_else(|error| panic!("{url}: {error}"));
        assert!(!is_opencode_go(&base), "{url} -> {}", base.as_str());
    }
    // Userinfo and a query never form a base URL, so no binding can smuggle another host in.
    for url in [
        "https://opencode.ai@evil.example/inference/go/",
        "https://user@opencode.ai/inference/go/",
        "https://opencode.ai/inference/go/?x=1",
    ] {
        assert!(BaseUrl::new(url).is_err(), "{url} is refused");
    }
}

/// `docs/messages.md`: the minted value is `llm-<process>-<clock>-<sequence>`, hexadecimal. The
/// process field is this process's id.
#[test]
fn a_minted_session_has_the_documented_shape() {
    let value = minted(&client(OPENCODE_GO));
    let [process, _, _] = fields(&value);
    assert_eq!(process, format!("{:x}", std::process::id()), "{value}");
    assert!(value.len() <= 62, "`{value}` exceeds the 62 bytes its doc states");
    assert!(Id::new(value.as_str()).is_ok(), "{value}");
}

/// Two clients differ by their sequence, not only by the clock: `many_clients_in_one_process_
/// never_share_a_session` still passes when the sequence never advances, because consecutive
/// constructions differ in nanoseconds. Here the clock field is set aside.
#[test]
fn two_clients_differ_in_their_sequence_field() {
    let [_, _, first] = fields(&minted(&client(OPENCODE_GO)));
    let [_, _, second] = fields(&minted(&client(OPENCODE_GO)));
    assert_ne!(first, second, "the sequence did not advance between two clients");
}

/// The last name a caller gives wins, and a named session never falls back to the minted one.
#[test]
fn the_last_named_session_wins() {
    let client = client(OPENCODE_GO)
        .with_session(Id::new("conversation-1").unwrap())
        .with_session(Id::new("conversation-2").unwrap());
    assert_eq!(
        client.session_header(),
        Some((OPENCODE_SESSION_HEADER, "conversation-2"))
    );
}
