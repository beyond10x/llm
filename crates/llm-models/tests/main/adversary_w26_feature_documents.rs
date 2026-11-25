//! Adversary, wave 2026-11-05-w26, story:output-and-image-capabilities.
//!
//! Every place a document builds `Capabilities` reads a declaration as a boolean or as absent:
//! `null`, a string, a number and a misspelt sibling key are refused in an `llm.routes/1` YAML or
//! TOML document and in an `llm.binding/1` JSON document, never read as undeclared. A route that
//! declares a feature `false` refuses a turn needing it on every HTTP wire before any request is
//! made, and the same turn on an undeclared route does reach the server. Loopback only.

use llm_core::{
    Cancel, Dispatch, ErrorCode, Id, ImageMediaType, ImageSource, Item, Model, OutputFormat,
    TurnRequest, VecSink,
};
use llm_credentials::SecretResolver;
use llm_http::{HttpClient, Limits};
use llm_models::{Resolvers, routes::Routes};
use llm_providers::BindingDocument;
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{io::AsyncWriteExt, net::TcpListener};

const LIMITS: &str = "tools: false, tool_choice: false, temperature: false, top_p: false, \
reasoning_efforts: [], context_window: 32768, max_output_tokens: 8192";

fn yaml(declared: &str) -> String {
    format!(
        "format: llm.routes/1
endpoints:
  lab: {{base_url: http://127.0.0.1:9/v1, protocol: chat-completions, auth: {{kind: anonymous}}}}
routes:
  r:
    model: lab/m
    capabilities: {{{LIMITS}{declared}}}
"
    )
}

fn toml(declared: &str) -> String {
    format!(
        r#"format = "llm.routes/1"

[endpoints.lab]
base_url = "http://127.0.0.1:9/v1"
protocol = "chat-completions"
auth = {{ kind = "anonymous" }}

[routes.r]
model = "lab/m"
capabilities = {{ tools = false, tool_choice = false, temperature = false, top_p = false, reasoning_efforts = [], context_window = 32768, max_output_tokens = 8192{declared} }}
"#
    )
}

#[test]
fn a_routes_declaration_that_is_not_a_boolean_or_is_misspelt_is_refused_in_yaml_and_toml() {
    for declared in [
        ", image_input: ~",
        ", image_input: null",
        ", image_input: \"false\"",
        ", image_input: 'false'",
        ", image_input: no",
        ", image_input: off",
        ", image_input: 0",
        ", structured_output: \"true\"",
        ", structured_output: yes",
        ", structured_output: []",
        ", image_inputs: false",
        ", structuredOutput: false",
    ] {
        let refused = Routes::parse(&yaml(declared))
            .err()
            .unwrap_or_else(|| panic!("YAML `{declared}` was read"));
        assert_eq!(refused.code, ErrorCode::InvalidRequest, "YAML `{declared}`");
    }
    for declared in [
        ", image_input = \"false\"",
        ", image_input = 0",
        ", structured_output = \"true\"",
        ", image_inputs = false",
        ", structured_output = {}",
    ] {
        let refused = Routes::parse_toml(&toml(declared))
            .err()
            .unwrap_or_else(|| panic!("TOML `{declared}` was read"));
        assert_eq!(refused.code, ErrorCode::InvalidRequest, "TOML `{declared}`");
    }
    for (yaml_declared, toml_declared, expected) in [
        ("", "", (None, None)),
        (
            ", structured_output: true, image_input: false",
            ", structured_output = true, image_input = false",
            (Some(true), Some(false)),
        ),
        (
            ", structured_output: false, image_input: true",
            ", structured_output = false, image_input = true",
            (Some(false), Some(true)),
        ),
    ] {
        let from_yaml = Routes::parse(&yaml(yaml_declared)).unwrap();
        let from_toml = Routes::parse_toml(&toml(toml_declared)).unwrap();
        for routes in [&from_yaml, &from_toml] {
            let route = routes.route("r").unwrap().capabilities();
            assert_eq!(
                (route.structured_output, route.image_input),
                expected,
                "{yaml_declared}"
            );
            let (_, binding) = routes.catalog().bindings().next().unwrap();
            assert_eq!(binding.capabilities(), route, "{yaml_declared}");
        }
        assert_eq!(from_yaml.digest(), from_toml.digest(), "{yaml_declared}");
    }
}

const BINDING: &str = r#"{"format":"llm.binding/1",
"provider":{"id":"lab","category":"self-hosted"},
"account":{"id":"local","provider_id":"lab","auth_kind":"anonymous","billing_kind":"self-hosted"},
"endpoint":{"id":"local-vllm","account_id":"local","base_url":"http://127.0.0.1:9/v1"},
"model":{"id":"m","upstream_name":"example/m"},
"serving":{"id":"s","endpoint_id":"local-vllm","model_id":"m","protocol":"chat-completions",
"capabilities":{"tools":false,"tool_choice":false,"temperature":false,"top_p":false,
"reasoning_efforts":[],"context_window":32768,"max_output_tokens":8192DECLARED}}}"#;

#[test]
fn a_binding_declaration_that_is_not_a_boolean_or_is_misspelt_is_refused() {
    for declared in [
        r#","image_input":null"#,
        r#","structured_output":null"#,
        r#","structured_output":"true""#,
        r#","image_input":"false""#,
        r#","image_input":1"#,
        r#","image_inputs":false"#,
    ] {
        let document = BINDING.replace("DECLARED", declared);
        assert!(
            serde_json::from_str::<BindingDocument>(&document).is_err(),
            "`{declared}` was read"
        );
    }
    let undeclared: BindingDocument =
        serde_json::from_str(&BINDING.replace("DECLARED", "")).unwrap();
    let declared: BindingDocument = serde_json::from_str(&BINDING.replace(
        "DECLARED",
        r#","structured_output":false,"image_input":false"#,
    ))
    .unwrap();
    let undeclared = undeclared.bind().unwrap();
    let declared = declared.bind().unwrap();
    assert_eq!(undeclared.capabilities().image_input, None);
    assert_eq!(declared.capabilities().image_input, Some(false));
    assert_ne!(
        undeclared.provenance().binding_revision,
        declared.provenance().binding_revision
    );
}

struct Anonymous;

impl Resolvers for Anonymous {
    fn resolver(&self, _account_id: &Id) -> Option<Arc<dyn SecretResolver>> {
        None
    }
}

/// Counts every connection made to it and answers each with a final refusal.
async fn counting_server() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let accepted = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&accepted);
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            counted.fetch_add(1, Ordering::SeqCst);
            let _ = socket
                .write_all(
                    b"HTTP/1.1 502 Bad Gateway\r\nx-should-retry: false\r\ncontent-length: 0\r\n\
                      connection: close\r\n\r\n",
                )
                .await;
            let _ = socket.shutdown().await;
        }
    });
    (url, accepted)
}

fn http() -> HttpClient {
    HttpClient::new(Limits {
        response_headers: Duration::from_secs(10),
        idle: Duration::from_secs(10),
        total: Duration::from_secs(20),
    })
    .unwrap()
}

fn needing(model: &str, image: bool, format: bool) -> TurnRequest {
    let mut items = vec![Item::user("What is in this picture?")];
    if image {
        items.push(Item::user_image(ImageSource::bytes(
            ImageMediaType::Png,
            b"\x89PNG".to_vec(),
        )));
        items.push(Item::assistant("A lighthouse."));
        items.push(Item::user("And its colour?"));
    }
    let mut turn = TurnRequest::new(model, items);
    turn.max_output_tokens = Some(64);
    if format {
        turn.controls.output_format =
            Some(OutputFormat::json_schema(json!({"type": "object"})).with_name("place"));
    }
    turn
}

#[tokio::test]
async fn a_route_declaring_a_feature_false_refuses_the_turn_before_any_request_on_every_wire() {
    let (url, accepted) = counting_server().await;
    let document = format!(
        "format: llm.routes/1
endpoints:
  chat: {{base_url: '{url}', protocol: chat-completions, auth: {{kind: anonymous}}}}
  responses: {{base_url: '{url}', protocol: responses, auth: {{kind: anonymous}}}}
  messages: {{base_url: '{url}', protocol: messages, auth: {{kind: anonymous}}}}
routes:
  chat-off: {{model: chat/m, capabilities: {{{LIMITS}, structured_output: false, image_input: false}}}}
  responses-off: {{model: responses/m, capabilities: {{{LIMITS}, structured_output: false, image_input: false}}}}
  messages-off: {{model: messages/m, capabilities: {{{LIMITS}, structured_output: false, image_input: false}}}}
  chat-open: {{model: chat/m, capabilities: {{{LIMITS}}}}}
"
    );
    let routes = Routes::parse(&document).unwrap();
    for route in ["chat-off", "responses-off", "messages-off"] {
        let port = routes.port(route, http(), &Anonymous).unwrap();
        let model = port.provenance().model.as_str().to_owned();
        for (image, format, message) in [
            (true, false, "binding does not take image input"),
            (false, true, "binding does not take structured output"),
        ] {
            let mut sink = VecSink::new(16, 4096);
            let error = port
                .turn(&needing(&model, image, format), &mut sink, &Cancel::new())
                .await
                .expect_err("refused");
            assert_eq!(error.code, ErrorCode::Unsupported, "{route}: {error:?}");
            assert_eq!(error.message, message, "{route}");
            assert_eq!(error.dispatch, Dispatch::NotSent, "{route}");
        }
    }
    assert_eq!(
        accepted.load(Ordering::SeqCst),
        0,
        "a refused turn reached the server"
    );
    // The same image turn on a route that leaves both undeclared is sent.
    let port = routes.port("chat-open", http(), &Anonymous).unwrap();
    let model = port.provenance().model.as_str().to_owned();
    let mut sink = VecSink::new(16, 4096);
    let _ = port
        .turn(&needing(&model, true, true), &mut sink, &Cancel::new())
        .await;
    assert!(accepted.load(Ordering::SeqCst) >= 1);
}
