//! The header map the Messages client builds for a request, read through its private builder.
//!
//! A child file rather than a block of `client.rs`: `tests/falsification.rs` and
//! `tests/wire_names.rs` read every literal in the top level of `src/` as something the
//! projection says to a route or a caller, and these fixtures are neither.

use super::{MessagesClient, OPENCODE_SESSION_HEADER};
use http::HeaderMap;
use llm_core::{BoxFuture, Id};
use llm_credentials::{ResolvedSecret, SecretError, SecretRef, SecretResolver};
use llm_http::{HttpClient, Limits};
use llm_providers::{Binding, BindingDocument};
use std::{sync::Arc, time::Duration};

/// Never asked: the header map is built after authentication, from what it returned.
struct Unused;

impl SecretResolver for Unused {
    fn resolve<'a>(
        &'a self,
        _: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async { Err(SecretError::Unavailable) })
    }
}

fn binding(base_url: &str) -> Binding {
    serde_json::from_value::<BindingDocument>(serde_json::json!({
        "format": "llm.binding/1",
        "provider": {"id": "lab", "category": "hosted"},
        "account": {"id": "account", "provider_id": "lab", "auth_kind": "api-key",
            "billing_kind": "metered", "secret_reference_id": "messages-key",
            "api_key_header": "x-api-key"},
        "endpoint": {"id": "endpoint", "account_id": "account", "base_url": base_url},
        "model": {"id": "internal-model", "upstream_name": "example/Model-Revision"},
        "serving": {"id": "serving", "endpoint_id": "endpoint", "model_id": "internal-model",
            "protocol": "messages", "capabilities": {"tools": true, "tool_choice": true,
            "temperature": true, "top_p": true, "reasoning_efforts": [],
            "context_window": 32768, "max_output_tokens": 2048}}
    }))
    .unwrap()
    .bind()
    .unwrap()
}

fn client(base_url: &str) -> MessagesClient {
    let total = Duration::from_secs(10);
    MessagesClient::new(
        binding(base_url),
        HttpClient::new(Limits {
            response_headers: total,
            idle: total,
            total,
        })
        .unwrap(),
        Arc::new(Unused),
    )
    .unwrap()
}

fn sent(client: &MessagesClient) -> Vec<(String, String)> {
    let headers = client.wire_headers(HeaderMap::new(), None).unwrap();
    let mut sent: Vec<_> = headers
        .iter()
        .map(|(name, value)| (name.as_str().to_owned(), value.to_str().unwrap().to_owned()))
        .collect();
    sent.sort();
    sent
}

/// The header map `run` sends carries exactly the session the public accessor reports, so
/// the conformance observation, which reads the accessor, reads what goes on the wire.
#[test]
fn the_header_map_run_sends_carries_the_session_the_accessor_reports() {
    for client in [
        client("https://opencode.ai/inference/go/anthropic/v1"),
        client("https://opencode.ai/inference/go/anthropic/v1")
            .with_session(Id::new("conversation-42").unwrap()),
    ] {
        let (name, value) = client.session_header().expect("an OpenCode Go session");
        let sent = sent(&client);
        assert!(
            sent.contains(&(name.to_owned(), value.to_owned())),
            "{sent:?}"
        );
        assert_eq!(
            sent.iter()
                .filter(|(sent, _)| sent == OPENCODE_SESSION_HEADER)
                .count(),
            1
        );
        assert_eq!(
            sent,
            self::sent(&client),
            "every request sends the same one"
        );
    }
}

/// Any other endpoint keeps the exact list it sent before `OpenCode` Go was recognised.
#[test]
fn another_endpoint_sends_exactly_the_three_content_headers() {
    let client = client("https://api.anthropic.com/v1/");
    assert_eq!(client.session_header(), None);
    assert_eq!(
        sent(&client),
        [
            ("accept".to_owned(), "text/event-stream".to_owned()),
            ("anthropic-version".to_owned(), "2023-06-01".to_owned()),
            ("content-type".to_owned(), "application/json".to_owned()),
        ]
    );
}
