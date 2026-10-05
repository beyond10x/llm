//! What the wire-parity tests share: a bound client over a local socket, a server that records
//! the exact request it received, the canonical turn the recorded fixtures pin, and the fixtures.
//!
//! No provider is contacted and no credential is read from this machine: every endpoint is
//! `127.0.0.1` and the credential comes from an injected resolver.

#![allow(dead_code)]

use llm_core::{
    AuthKind, BillingKind, BoxFuture, CallId, Capabilities, Id, Item, Protocol, Provenance,
    Sampling, ToolCall, ToolChoice, ToolName, ToolSpec, TurnRequest,
};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_http::{HttpClient, Limits};
use llm_providers::{
    Account, BaseUrl, Binding, BindingDocument, Endpoint, Provider, ServedModel, ServingModel,
};
use llm_responses::ResponsesClient;
use serde_json::json;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};

pub const SSE_HEAD: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";

/// A one-message answer that ends with its terminal object and nothing after it.
pub const TEXT_STREAM: &[u8] = b"event: response.created\n\
data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"status\":\"in_progress\"}}\n\n\
event: response.output_text.delta\n\
data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"delta\":\"Done.\"}\n\n\
event: response.completed\n\
data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"model\":\"example/Small-Model\",\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"id\":\"msg_1\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Done.\"}]}],\"usage\":{\"input_tokens\":12,\"output_tokens\":2}}}\n\n";

pub fn id(value: &str) -> Id {
    Id::new(value).expect("fixture identifier")
}

fn capabilities() -> Capabilities {
    Capabilities {
        tools: true,
        tool_choice: true,
        temperature: true,
        top_p: true,
        reasoning_efforts: vec!["medium".to_owned()],
        context_window: 32_768,
        max_output_tokens: 2_048,
    }
}

/// A bearer-authenticated Responses binding at `base_url`.
pub fn binding(base_url: &str) -> Binding {
    BindingDocument::new(
        Provider {
            id: id("my-lab"),
            category: id("hosted"),
        },
        Account {
            id: id("local"),
            provider_id: id("my-lab"),
            auth_kind: AuthKind::Bearer,
            billing_kind: BillingKind::Metered,
            secret_reference_id: Some(SecretRef::new("responses-key").expect("reference")),
            api_key_header: None,
        },
        Endpoint {
            id: id("local-endpoint"),
            account_id: id("local"),
            base_url: BaseUrl::new(base_url).expect("fixture URL"),
        },
        ServedModel {
            id: id("small"),
            upstream_name: id("example/Small-Model"),
        },
        ServingModel {
            id: id("serving"),
            endpoint_id: id("local-endpoint"),
            model_id: id("small"),
            protocol: Protocol::Responses,
            capabilities: capabilities(),
        },
    )
    .bind()
    .expect("a valid fixture binding")
}

/// The projection's view of [`binding`]: the same provenance and upstream model name.
pub fn projection(base_url: &str) -> llm_responses::Binding {
    let bound = binding(base_url);
    llm_responses::Binding::new(bound.provenance().clone(), id(bound.upstream_model()))
}

struct StaticResolver;

impl SecretResolver for StaticResolver {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async move {
            Ok(ResolvedSecret {
                secret: Secret::new(b"fixture-token".to_vec())?,
                version: SecretVersion::new("generation-1".to_owned())?,
            })
        })
    }
}

pub fn client(url: &str) -> ResponsesClient {
    let total = Duration::from_secs(10);
    ResponsesClient::new(
        binding(url),
        HttpClient::new(Limits {
            response_headers: total,
            idle: total,
            total,
        })
        .expect("bounded transport"),
        Arc::new(StaticResolver),
    )
    .expect("a Responses binding")
}

/// The turn the recorded fixtures pin. Every body field the projection can send is set, so the
/// fixture pins each one: a standing instruction, a person's input, replayed reasoning, one call
/// and its result, one published tool, an output bound, all three sampling fields and a forced
/// tool. `provenance` is the client's own, so the replayed reasoning belongs to it.
pub fn canonical_request(provenance: &Provenance) -> TurnRequest {
    let file_read = ToolName::new("file_read").expect("tool name");
    TurnRequest {
        model: "small".to_owned(),
        instructions: "be useful".to_owned(),
        items: vec![
            Item::user("read the readme"),
            Item::Opaque {
                provenance: provenance.clone(),
                payload: json!({
                    "id": "rs_1",
                    "type": "reasoning",
                    "summary": [],
                    "encrypted_content": "OPAQUE",
                }),
            },
            Item::ToolCall(ToolCall {
                call_id: CallId::new("call_1").expect("call id"),
                name: file_read.clone(),
                arguments: json!({"path": "README.md"}),
            }),
            Item::ToolResult {
                call_id: CallId::new("call_1").expect("call id"),
                output: json!({"text": "hello"}),
                failed: false,
            },
        ],
        tools: vec![ToolSpec {
            name: file_read.clone(),
            description: "Read one file".to_owned(),
            input_schema: json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"],
            }),
        }],
        max_output_tokens: Some(1024),
        sampling: Sampling {
            temperature: Some(0.2),
            top_p: Some(0.95),
            reasoning_effort: Some("medium".to_owned()),
        },
        tool_choice: ToolChoice::Named(file_read),
    }
}

pub async fn listener() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local port");
    let url = format!("http://{}/v1", listener.local_addr().expect("an address"));
    (listener, url)
}

/// One request exactly as it arrived: the head as text and the body as bytes.
pub struct Captured {
    pub head: String,
    pub body: Vec<u8>,
}

impl Captured {
    pub fn request_line(&self) -> &str {
        self.head.lines().next().expect("a request line")
    }

    /// Every header as `(lowercase name, value)`, in arrival order.
    pub fn headers(&self) -> Vec<(String, String)> {
        self.head
            .lines()
            .skip(1)
            .filter_map(|line| line.split_once(':'))
            .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
            .collect()
    }

    pub fn header(&self, name: &str) -> Option<String> {
        self.headers()
            .into_iter()
            .find(|(found, _)| found == name)
            .map(|(_, value)| value)
    }

    /// The head as a recorded fixture holds it: the request line, then every header as
    /// `name: value` sorted by name, with the credential and the local address replaced. Every
    /// other name and value is kept as it arrived.
    pub fn recorded_head(&self) -> String {
        let mut lines: Vec<String> = self
            .headers()
            .into_iter()
            .map(|(name, value)| {
                let value = match name.as_str() {
                    "authorization" => "<credential omitted>".to_owned(),
                    "host" => "<local address>".to_owned(),
                    _ => value,
                };
                format!("{name}: {value}")
            })
            .collect();
        lines.sort();
        let mut recorded = String::new();
        recorded.push_str(self.request_line());
        recorded.push('\n');
        for line in lines {
            recorded.push_str(&line);
            recorded.push('\n');
        }
        recorded
    }
}

async fn read_request(socket: &mut TcpStream) -> Captured {
    let mut bytes = Vec::new();
    let mut buffer = [0; 1024];
    loop {
        let length = socket.read(&mut buffer).await.expect("a readable socket");
        assert!(length > 0, "the client closed before sending a request");
        bytes.extend_from_slice(&buffer[..length]);
        assert!(bytes.len() < 256 * 1024);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8(bytes[..end].to_vec()).expect("an ASCII head");
            let length: usize = head
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|value| value.trim().to_owned())
                })
                .expect("a bounded body")
                .parse()
                .expect("a numeric content-length");
            if bytes.len() >= end + 4 + length {
                return Captured {
                    head,
                    body: bytes[end + 4..end + 4 + length].to_vec(),
                };
            }
        }
    }
}

/// Answers one connection per stream, in order, and returns each request as it arrived.
pub fn serve(listener: TcpListener, streams: Vec<Vec<u8>>) -> JoinHandle<Vec<Captured>> {
    tokio::spawn(async move {
        let mut captured = Vec::new();
        for stream in streams {
            let mut socket = tokio::time::timeout(Duration::from_secs(10), listener.accept())
                .await
                .expect("the client never opened a connection")
                .expect("an accepted connection")
                .0;
            captured.push(read_request(&mut socket).await);
            socket.write_all(SSE_HEAD).await.expect("head written");
            socket.write_all(&stream).await.expect("stream written");
            socket.shutdown().await.expect("closed");
        }
        captured
    })
}

/// A file under `tests/fixtures`, found at run time from this package's manifest directory.
pub fn fixture(name: &str) -> Vec<u8> {
    let root = std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets the manifest directory");
    let path = PathBuf::from(root)
        .join("tests")
        .join("fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|error| panic!("reading `{}`: {error}", path.display()))
}

/// A recorded body fixture: the exact bytes, stored with one trailing newline so the file is a
/// well-formed text file. The newline is the file's, not the request's.
pub fn body_fixture(name: &str) -> Vec<u8> {
    let mut bytes = fixture(name);
    assert_eq!(bytes.pop(), Some(b'\n'), "`{name}` ends with one newline");
    bytes
}
