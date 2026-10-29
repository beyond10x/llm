//! Adversary pass 1 on story:saved-codex-metadata-item-on-resume (wave 2026-10-29-w22, D208).
//!
//! The decision: a saved `response.metadata` item of the selected binding is not sent; no other
//! item kind is dropped; another binding's state is still refused first. These cases attack the
//! match (shapes and near-miss names), positions and bodies that end up empty, every binding
//! coordinate, the `encode_request` bytes with a `Conversation`, the read-back through
//! `ingest_request`, the ingress encoder and the client over a loopback socket. No provider is
//! contacted.

use llm_core::{
    AuthKind, BillingKind, BoxFuture, CallId, Cancel, Capabilities, ErrorCode, Id, Item, Model,
    Protocol, Provenance, StopReason, ToolCall, ToolName, ToolSpec, TurnObservation, TurnOutcome,
    TurnRequest, VecSink,
};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_http::{HttpClient, Limits};
use llm_providers::{
    Account, BaseUrl, BindingDocument, Endpoint, Provider, ServedModel, ServingModel,
};
use llm_responses::{
    Binding, Conversation, IngressStream, ResponsesClient, encode_ingress_response, encode_request,
    ingest_request, project_request,
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const UPSTREAM_MODEL: &str = "example/Small-Model";
const TOOL: &str = "pick";

fn id(value: &str) -> Id {
    Id::new(value).expect("fixture identifier")
}

fn provenance() -> Provenance {
    Provenance {
        protocol: Protocol::Responses,
        provider: id("codex"),
        account: id("local"),
        endpoint: id("local-endpoint"),
        model: id("small"),
        binding_revision: id("rev-1"),
    }
}

fn binding() -> Binding {
    Binding::new(provenance(), id(UPSTREAM_MODEL))
}

fn opaque(payload: Value) -> Item {
    Item::Opaque {
        provenance: provenance(),
        payload,
    }
}

/// The event as the live Codex stream carried it, trimmed to its envelope.
fn metadata_event() -> Value {
    json!({"type": "response.metadata", "sequence_number": 30,
           "response_id": "resp_1", "metadata": {"fixture": "synthetic"}})
}

fn is_own_metadata(item: &Item) -> bool {
    matches!(item, Item::Opaque { provenance: kept, payload }
        if kept == &provenance() && payload["type"] == "response.metadata")
}

fn tool() -> ToolSpec {
    ToolSpec {
        name: ToolName::new(TOOL).expect("tool name"),
        description: "Pick.".to_owned(),
        input_schema: json!({"type": "object"}),
    }
}

fn call(n: usize) -> ToolCall {
    ToolCall {
        call_id: CallId::new(format!("call_{n}")).expect("call id"),
        name: ToolName::new(TOOL).expect("tool name"),
        arguments: json!({"n": n}),
    }
}

fn result(n: usize) -> Item {
    Item::ToolResult {
        call_id: CallId::new(format!("call_{n}")).expect("call id"),
        output: json!({"n": n}),
        failed: false,
    }
}

fn request(items: Vec<Item>, instructions: &str) -> TurnRequest {
    let mut request = TurnRequest::new("small", items);
    instructions.clone_into(&mut request.instructions);
    request.tools = vec![tool()];
    request
}

fn without_metadata(items: &[Item]) -> Vec<Item> {
    items
        .iter()
        .filter(|item| !is_own_metadata(item))
        .cloned()
        .collect()
}

fn input(body: &Value) -> &Vec<Value> {
    body["input"].as_array().expect("an input list")
}

/// Mutant guard: a match by suffix, by trimmed value or by the word `metadata` alone would leave
/// these out. None of them is the event the decision names, so each is sent verbatim, in place.
#[test]
fn a_type_that_only_ends_with_or_resembles_the_event_name_is_still_sent_verbatim() {
    for kind in [
        "x.response.metadata",
        "codex.response.metadata",
        " response.metadata",
        "response.metadata ",
        "response.metadata\n",
        "metadata",
        "response_metadata",
        "response.Metadata",
        "RESPONSE.METADATA",
    ] {
        let payload = json!({"type": kind, "metadata": {}});
        let items = vec![
            Item::user("first question"),
            opaque(payload.clone()),
            Item::assistant("first answer"),
            Item::user("second question"),
        ];
        let body = project_request(&binding(), &request(items, "")).expect("projects");
        assert_eq!(input(&body).len(), 4, "{kind:?}");
        assert_eq!(input(&body)[1], payload, "{kind:?}");
    }
}

/// Mutant guard: a match that also required the fields the recorded event had (`metadata`,
/// `response_id`, `sequence_number`) would send these. Each is the named event, so each is left.
#[test]
fn the_event_is_left_out_whatever_else_its_payload_holds() {
    for payload in [
        json!({"type": "response.metadata"}),
        json!({"type": "response.metadata", "metadata": null}),
        json!({"type": "response.metadata", "metadata": "a string"}),
        json!({"type": "response.metadata", "item": {"type": "message"}}),
        json!({"type": "response.metadata", "output_index": 0, "unknown": [1, 2, 3]}),
        metadata_event(),
    ] {
        let items = vec![
            Item::user("first question"),
            opaque(payload.clone()),
            Item::assistant("first answer"),
            Item::user("second question"),
        ];
        let body = project_request(&binding(), &request(items, "")).expect("projects");
        let types: Vec<&str> = input(&body)
            .iter()
            .filter_map(|entry| entry["type"].as_str())
            .collect();
        assert_eq!(types, ["message", "message", "message"], "{payload}");
    }
}

/// A conversation that holds nothing but the item, or ends with it, projects exactly as the
/// same conversation without it, with and without instructions.
#[test]
fn a_conversation_that_empties_or_ends_with_the_item_projects_as_without_it() {
    let conversations = [
        vec![opaque(metadata_event())],
        vec![opaque(metadata_event()), opaque(metadata_event())],
        vec![
            Item::user("q"),
            Item::assistant("a"),
            opaque(metadata_event()),
        ],
        vec![
            Item::user("q"),
            Item::ToolCall(call(1)),
            opaque(metadata_event()),
        ],
    ];
    for instructions in ["", "Be brief."] {
        for items in &conversations {
            let with = project_request(&binding(), &request(items.clone(), instructions));
            let without =
                project_request(&binding(), &request(without_metadata(items), instructions));
            match (with, without) {
                (Ok(with), Ok(without)) => assert_eq!(with, without, "{items:?}"),
                (Err(with), Err(without)) => assert_eq!(with.code, without.code, "{items:?}"),
                (with, without) => panic!("{items:?}: {with:?} against {without:?}"),
            }
        }
    }
}

/// Every coordinate of the binding is checked before anything is left out, at every position.
#[test]
fn another_bindings_metadata_item_is_refused_at_every_coordinate_and_position() {
    let mut others = Vec::new();
    for coordinate in 0..6 {
        let mut other = provenance();
        match coordinate {
            0 => other.protocol = Protocol::Messages,
            1 => other.provider = id("other-provider"),
            2 => other.account = id("other-account"),
            3 => other.endpoint = id("other-endpoint"),
            4 => other.model = id("other-model"),
            _ => other.binding_revision = id("rev-2"),
        }
        others.push(other);
    }
    let base = vec![
        Item::user("first question"),
        Item::assistant("first answer"),
        Item::user("second question"),
    ];
    for other in others {
        for at in 0..=base.len() {
            let mut items = base.clone();
            items.insert(
                at,
                Item::Opaque {
                    provenance: other.clone(),
                    payload: metadata_event(),
                },
            );
            let error = project_request(&binding(), &request(items, ""))
                .expect_err("another binding's state is refused");
            assert_eq!(error.code, ErrorCode::Unsupported, "{other:?} at {at}");
        }
    }
}

/// Unbound state holding the event is refused as all unbound state is: it is not left out.
#[test]
fn an_unattributed_metadata_entry_is_still_refused_by_name() {
    let items = vec![
        Item::user("first question"),
        Item::UnattributedOpaque {
            protocol: Protocol::Responses,
            payload: metadata_event(),
        },
        Item::user("second question"),
    ];
    let error = project_request(&binding(), &request(items, "")).expect_err("unbound state");
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(error.message, Item::UNATTRIBUTED_REFUSAL);
}

/// A tiny deterministic generator, so the property below has a fixed seed and no dependency.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, bound: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        usize::try_from((self.0 >> 33) % u64::try_from(bound).expect("bound")).expect("index")
    }
}

/// Property, seed 0xD208: over 400 generated conversations (text, reasoning, another opaque
/// event, tool call and result pairs, saved metadata items anywhere, including between a call
/// and its result), with and without instructions and a `Conversation`, the bytes
/// `encode_request` returns equal those of the same conversation without the metadata items,
/// and every other item is an entry, in order.
#[test]
fn the_encoded_bytes_equal_the_conversation_without_the_items_for_generated_conversations() {
    let mut random = Lcg(0xD208);
    let shapes = [
        metadata_event(),
        json!({"type": "response.metadata"}),
        json!({"type": "response.metadata", "metadata": {"nested": {"type": "reasoning"}}}),
    ];
    for case in 0..400 {
        let mut items = vec![Item::user("opening question")];
        let mut calls = 0;
        for _ in 0..random.next(12) {
            match random.next(7) {
                0 => items.push(Item::user(format!("user {case}"))),
                1 => items.push(Item::assistant(format!("assistant {case}"))),
                2 => items.push(opaque(
                    json!({"type": "reasoning", "id": format!("rs_{case}"),
                                              "summary": [], "encrypted_content": "e"}),
                )),
                3 => items.push(opaque(
                    json!({"type": "response.not_in_the_pin", "n": case}),
                )),
                4 => {
                    calls += 1;
                    items.push(Item::ToolCall(call(calls)));
                    if random.next(2) == 0 {
                        items.push(opaque(shapes[random.next(shapes.len())].clone()));
                    }
                    items.push(result(calls));
                }
                _ => items.push(opaque(shapes[random.next(shapes.len())].clone())),
            }
        }
        let instructions = if random.next(2) == 0 { "" } else { "Be brief." };
        let conversation =
            (random.next(2) == 0).then(|| Conversation::new(id(&format!("conv-{case}"))));
        let with = encode_request(
            &binding(),
            &request(items.clone(), instructions),
            conversation.as_ref(),
        )
        .unwrap_or_else(|error| panic!("case {case}: {error:?}"));
        let kept = without_metadata(&items);
        let without = encode_request(
            &binding(),
            &request(kept.clone(), instructions),
            conversation.as_ref(),
        )
        .expect("the conversation without the items encodes");
        assert_eq!(
            String::from_utf8_lossy(&with),
            String::from_utf8_lossy(&without),
            "case {case}"
        );
        let body: Value = serde_json::from_slice(&with).expect("JSON");
        let offset = usize::from(!instructions.is_empty());
        assert_eq!(input(&body).len(), kept.len() + offset, "case {case}");
        for (index, item) in kept.iter().enumerate() {
            if let Item::Opaque { payload, .. } = item {
                assert_eq!(&input(&body)[index + offset], payload, "case {case}");
            }
        }
    }
}

/// The body reads back, through the same contract, as the conversation without the item: the
/// reasoning item is bound back to the binding and nothing else changed.
#[test]
fn the_body_reads_back_as_the_conversation_without_the_item() {
    let items = vec![
        Item::user("first question"),
        opaque(json!({"type": "reasoning", "id": "rs_1", "summary": [],
                      "encrypted_content": "e"})),
        Item::ToolCall(call(1)),
        opaque(metadata_event()),
        result(1),
        Item::assistant("first answer"),
        opaque(metadata_event()),
        Item::user("second question"),
    ];
    let sent = request(items.clone(), "");
    let body = project_request(&binding(), &sent).expect("projects");
    let mut read = ingest_request(&binding(), &body).expect("the body reads back");
    read.bind_unattributed(&provenance())
        .expect("the reasoning item binds");
    assert_eq!(read, request(without_metadata(&items), ""));
}

fn observation() -> TurnObservation {
    TurnObservation {
        binding: provenance(),
        upstream_model: Some(id(UPSTREAM_MODEL)),
        response_id: Some(id("resp_gateway_1")),
        usage: None,
        final_usage: true,
    }
}

/// The gateway direction: an answer carrying the item is not re-emitted to a client, which would
/// replay it. Both encoders refuse it before writing anything.
#[test]
fn the_ingress_encoders_refuse_an_answer_carrying_the_item() {
    let outcome = TurnOutcome {
        stop_reason: StopReason::EndTurn,
        items: vec![Item::assistant("answer"), opaque(metadata_event())],
        observation: observation(),
    };
    let error = encode_ingress_response(&outcome, "resp_gateway_1", 1_772_000_000)
        .expect_err("not re-emitted");
    assert_eq!(error.code, ErrorCode::Unsupported);
    let error = IngressStream::new("resp_gateway_1", 1_772_000_000)
        .close(&outcome)
        .expect_err("not re-emitted streamed");
    assert_eq!(error.code, ErrorCode::Unsupported);
}

// The client, end to end over a loopback socket.

fn capabilities() -> Capabilities {
    Capabilities {
        tools: true,
        tool_choice: true,
        temperature: true,
        top_p: true,
        reasoning_efforts: vec!["medium".to_owned()],
        context_window: 32_768,
        max_output_tokens: 2_048,
        output_streaming: true,
        input_during_output: false,
    }
}

fn client_binding(base_url: &str) -> llm_providers::Binding {
    BindingDocument::new(
        Provider {
            id: id("codex"),
            category: id("hosted"),
        },
        Account {
            id: id("local"),
            provider_id: id("codex"),
            auth_kind: AuthKind::Bearer,
            billing_kind: BillingKind::Subscription,
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
            upstream_name: id(UPSTREAM_MODEL),
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

async fn read_request(socket: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0; 1024];
    loop {
        let length = socket.read(&mut buffer).await.expect("a readable socket");
        assert!(length > 0, "the client closed before sending a request");
        bytes.extend_from_slice(&buffer[..length]);
        assert!(bytes.len() < 256 * 1024);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
            let length: usize = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .expect("a bounded body")
                .trim()
                .parse()
                .expect("a numeric content-length");
            if bytes.len() >= end + 4 + length {
                return String::from_utf8(bytes).expect("UTF-8 request");
            }
        }
    }
}

async fn answer(socket: &mut TcpStream) {
    let events = [
        json!({"type": "response.created", "sequence_number": 0,
               "response": {"id": "resp_2", "status": "in_progress", "output": []}}),
        json!({"type": "response.completed", "sequence_number": 1, "response": {
            "id": "resp_2", "object": "response", "status": "completed", "model": UPSTREAM_MODEL,
            "output": [{"id": "msg_2", "type": "message", "role": "assistant",
                        "status": "completed",
                        "content": [{"type": "output_text", "text": "done", "annotations": []}]}],
            "usage": {"input_tokens": 9, "output_tokens": 1}}}),
    ];
    socket
        .write_all(b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n")
        .await
        .expect("head");
    for event in events {
        let kind = event["type"].as_str().expect("type");
        let frame = format!("event: {kind}\ndata: {event}\n\n");
        let chunk = format!("{:x}\r\n{frame}\r\n", frame.len());
        socket.write_all(chunk.as_bytes()).await.expect("chunk");
    }
    socket.write_all(b"0\r\n\r\n").await.expect("last chunk");
    socket.shutdown().await.expect("closed");
}

/// The shape a 0.16.0 Codex tool turn saved: the event streamed after the finished call, so the
/// item sits between the call and the result the caller appended. What reaches the socket is the
/// call followed by its result, with no metadata entry.
#[tokio::test]
async fn a_resumed_tool_conversation_sends_no_metadata_entry_over_the_wire() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
    let url = format!(
        "http://{}/backend-api/codex",
        listener.local_addr().expect("an address")
    );
    let bound = client_binding(&url);
    let kept = Item::Opaque {
        provenance: bound.provenance().clone(),
        payload: metadata_event(),
    };
    let server = tokio::spawn(async move {
        let mut socket = tokio::time::timeout(Duration::from_secs(10), listener.accept())
            .await
            .expect("the client connects")
            .expect("accepted")
            .0;
        let captured = read_request(&mut socket).await;
        answer(&mut socket).await;
        captured
    });
    let total = Duration::from_secs(10);
    let client = ResponsesClient::new(
        bound,
        HttpClient::new(Limits {
            response_headers: total,
            idle: total,
            total,
        })
        .expect("bounded transport"),
        Arc::new(StaticResolver),
    )
    .expect("a Responses binding");
    let resumed = request(
        vec![
            Item::user("Pick one"),
            Item::ToolCall(call(1)),
            kept,
            result(1),
        ],
        "",
    );
    let mut sink = VecSink::new(32, 4096);
    let outcome = client
        .turn(&resumed, &mut sink, &Cancel::new())
        .await
        .expect("the resumed turn is answered");
    assert_eq!(outcome.items, vec![Item::assistant("done")]);
    let captured = server.await.expect("the fixture server");
    let body: Value = serde_json::from_str(
        captured
            .split_once("\r\n\r\n")
            .expect("a head and a body")
            .1,
    )
    .expect("a JSON body");
    let types: Vec<&str> = input(&body)
        .iter()
        .map(|entry| entry["type"].as_str().unwrap_or("message"))
        .collect();
    assert_eq!(
        types,
        ["message", "function_call", "function_call_output"],
        "{body}"
    );
    assert_eq!(input(&body)[1]["call_id"], "call_1");
    assert_eq!(input(&body)[2]["call_id"], "call_1");
}
