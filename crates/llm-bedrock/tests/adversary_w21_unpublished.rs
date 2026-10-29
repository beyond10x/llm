//! Adversary, wave 2026-10-29-w21, story:unpublished-tool-call-reaches-an-opted-in-caller.
//!
//! The story's point is that a caller who set `controls.return_unpublished_calls` can answer an
//! unpublished call "as a refusal the model learns from". The unit proved that answer is a valid
//! neutral turn (`llm-core/tests/unpublished_calls.rs::
//! a_returned_unpublished_call_answered_as_a_refusal_makes_a_valid_next_turn`), never that any
//! wire can send it. On Bedrock Converse the decoder hands back any `ToolName` the model wrote,
//! while the encoder refuses a name outside `[A-Za-z0-9_-]`: an unpublished name the model made
//! up (`repo_browser.open_file`, the namespaced form gpt-oss writes) is returned by one turn and
//! makes the very next turn unsendable, so the opted-in caller loses the conversation one turn
//! later instead of at once.
//!
//! Every answer body here is synthetic, built in this file; nothing is a recording.

use llm_bedrock::{
    Api, AwsCredentials, BedrockClient, BedrockTarget, CredentialSource, check_request,
    encode_request,
};
use llm_core::{
    BoxFuture, CallId, Cancel, Capabilities, Error, Id, Item, Model, Protocol, Provenance,
    ToolCall, ToolName, ToolSpec, TurnOutcome, TurnRequest, VecSink,
};
use llm_http::{HttpClient, Limits};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};
use url::Url;

const GPT_OSS: &str = "openai.gpt-oss-120b-1:0";
const SONNET_4: &str = "us.anthropic.claude-sonnet-4-20250514-v1:0";

struct Keys;

impl CredentialSource for Keys {
    fn credentials<'a>(&'a self, _: &'a Cancel) -> BoxFuture<'a, Result<AwsCredentials, Error>> {
        Box::pin(async {
            Ok(AwsCredentials::new(
                "AKIDFIXTURE",
                "fixture-secret-access-key",
                None,
                None,
            ))
        })
    }
}

/// Serves one whole JSON answer on 127.0.0.1 and closes; reads and discards the request.
async fn serve(body: Vec<u8>) -> (Url, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0; 4096];
        let end = loop {
            if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                break end;
            }
            let read = socket.read(&mut buffer).await.unwrap();
            assert!(read > 0);
            bytes.extend_from_slice(&buffer[..read]);
        };
        let head = String::from_utf8(bytes[..end].to_vec())
            .unwrap()
            .to_ascii_lowercase();
        let length: usize = head
            .lines()
            .find_map(|line| line.strip_prefix("content-length:"))
            .map_or(0, |value| value.trim().parse().unwrap());
        while bytes.len() < end + 4 + length {
            let read = socket.read(&mut buffer).await.unwrap();
            assert!(read > 0);
            bytes.extend_from_slice(&buffer[..read]);
        }
        let head = "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\n\r\n";
        socket.write_all(head.as_bytes()).await.unwrap();
        let _ = socket.write_all(&body).await;
        let _ = socket.flush().await;
    });
    (origin, server)
}

fn provenance(model: &str, protocol: Protocol) -> Provenance {
    Provenance {
        protocol,
        provider: Id::new("bedrock").unwrap(),
        account: Id::new("fixture-account").unwrap(),
        endpoint: Id::new("us-east-1").unwrap(),
        model: Id::new(model).unwrap(),
        binding_revision: Id::new("fixture-1").unwrap(),
    }
}

fn capabilities() -> Capabilities {
    Capabilities {
        tools: true,
        tool_choice: true,
        temperature: true,
        top_p: true,
        reasoning_efforts: Vec::new(),
        context_window: 200_000,
        max_output_tokens: 4_096,
        output_streaming: false,
        input_during_output: false,
    }
}

fn target(model: &str, api: Api, endpoint: Option<Url>) -> BedrockTarget {
    let protocol = match api {
        Api::Converse => Protocol::BedrockConverse,
        Api::AnthropicInvoke => Protocol::Messages,
    };
    BedrockTarget::new(
        provenance(model, protocol),
        capabilities(),
        "us-east-1",
        model,
        api,
        false,
        endpoint,
    )
    .expect("a valid fixture target")
}

fn http() -> HttpClient {
    HttpClient::new(Limits {
        response_headers: Duration::from_secs(10),
        idle: Duration::from_secs(10),
        total: Duration::from_secs(30),
    })
    .unwrap()
}

fn tool(name: &str) -> ToolSpec {
    ToolSpec {
        name: ToolName::new(name).unwrap(),
        description: format!("The {name} verb."),
        input_schema: json!({"type": "object"}),
    }
}

/// The verbs surface: `discover` and `call` published; the caller takes unpublished calls.
fn verbs_request(model: &str, opted_in: bool) -> TurnRequest {
    let mut request = TurnRequest::new(model, vec![Item::user("Summarise the repo")]);
    request.tools = vec![tool("discover"), tool("call")];
    request.controls.return_unpublished_calls = opted_in;
    request
}

/// A whole Converse answer: one `toolUse` block naming `name`.
fn tool_use_answer(name: &str) -> Vec<u8> {
    json!({
        "output": {"message": {"role": "assistant", "content": [
            {"toolUse": {"toolUseId": "tooluse_1", "name": name,
                         "input": {"path": "README.md"}}}]}},
        "stopReason": "tool_use",
        "usage": {"inputTokens": 40, "outputTokens": 9, "totalTokens": 49},
        "metrics": {"latencyMs": 10}
    })
    .to_string()
    .into_bytes()
}

async fn converse_turn(request: &TurnRequest, answer: Vec<u8>) -> Result<TurnOutcome, Error> {
    let (origin, server) = serve(answer).await;
    let client = BedrockClient::new(
        target(GPT_OSS, Api::Converse, Some(origin)),
        http(),
        Arc::new(Keys),
    )
    .unwrap();
    let mut sink = VecSink::new(10_000, 1 << 20);
    let result = client.turn(request, &mut sink, &Cancel::new()).await;
    server.await.unwrap();
    result
}

/// The caller's answer: the turn's items, then each call answered by a failed result.
fn answered(first: &TurnRequest, outcome: &TurnOutcome) -> TurnRequest {
    let mut next = first.clone();
    next.items.extend(outcome.items.iter().cloned());
    for call in outcome.tool_calls() {
        next.items.push(Item::ToolResult {
            call_id: call.call_id.clone(),
            output: json!({"error": "no tool by that name is published"}),
            failed: true,
        });
    }
    next
}

/// Was RED: Converse returned `repo_browser.open_file` to an opted-in caller, and the caller's
/// answer was refused before I/O, so the conversation ended one turn later. Decided in
/// story:unpublished-tool-call-reaches-an-opted-in-caller: Converse returns only a call it can be
/// answered under, and refuses the turn otherwise, as it did before the control existed. Every
/// call it does return can be answered on Converse.
#[tokio::test]
async fn an_unpublished_call_converse_returned_can_be_answered_on_converse() {
    let first = verbs_request(GPT_OSS, true);
    let error = converse_turn(&first, tool_use_answer("repo_browser.open_file"))
        .await
        .expect_err(
            "story:unpublished-tool-call-reaches-an-opted-in-caller: a name Converse cannot carry back is refused",
        );
    assert_eq!(error.code, llm_core::ErrorCode::Protocol, "{error}");

    let outcome = converse_turn(&first, tool_use_answer("open_file"))
        .await
        .expect("the opted-in caller is handed the unpublished call");
    let next = answered(&first, &outcome);
    assert_eq!(next.validate(), Ok(()), "the neutral next turn is valid");
    let target = target(GPT_OSS, Api::Converse, None);
    assert_eq!(
        check_request(&next, &target),
        Ok(()),
        "a call Converse handed to the caller must be answerable on Converse"
    );
}

/// GREEN probe: the same path with a name inside Converse's class answers fine, and the
/// failed result goes out as Converse's `status: error`.
#[tokio::test]
async fn a_plain_unpublished_name_is_answered_on_converse_as_an_error_result() {
    let first = verbs_request(GPT_OSS, true);
    let outcome = converse_turn(&first, tool_use_answer("file_read"))
        .await
        .expect("the opted-in caller is handed the unpublished call");
    let next = answered(&first, &outcome);
    let body: Value = serde_json::from_slice(
        &encode_request(&next, &target(GPT_OSS, Api::Converse, None)).unwrap(),
    )
    .unwrap();
    let messages = body["messages"].as_array().unwrap();
    let last = &messages[messages.len() - 1]["content"][0]["toolResult"];
    assert_eq!(last["toolUseId"], "tooluse_1");
    assert_eq!(last["status"], "error");
    assert_eq!(
        messages[messages.len() - 2]["content"][0]["toolUse"]["name"],
        "file_read"
    );
}

/// GREEN probe: without the control the same answer is refused, so the red case above is
/// reachable only through the opt-in.
#[tokio::test]
async fn without_the_control_converse_still_refuses_the_unpublished_call() {
    let error = converse_turn(&verbs_request(GPT_OSS, false), tool_use_answer("file_read"))
        .await
        .expect_err("refused");
    assert_eq!(
        error.message,
        "model output contains a duplicate call or unpublished tool"
    );
}

fn replayed_unpublished(model: &str) -> TurnRequest {
    let mut request = verbs_request(model, false);
    request.items.extend([
        Item::ToolCall(ToolCall {
            call_id: CallId::new("tooluse_1").unwrap(),
            name: ToolName::new("find").unwrap(),
            arguments: json!({"pattern": "*.md"}),
        }),
        Item::ToolResult {
            call_id: CallId::new("tooluse_1").unwrap(),
            output: json!("unpublished tool"),
            failed: true,
        },
    ]);
    request
}

/// GREEN probe: the control is local. Both Bedrock bodies are byte for byte what the same turn
/// without it sends.
#[test]
fn neither_bedrock_body_carries_the_control() {
    for (model, api) in [(GPT_OSS, Api::Converse), (SONNET_4, Api::AnthropicInvoke)] {
        let target = target(model, api, None);
        let off = replayed_unpublished(model);
        let mut on = off.clone();
        on.controls.return_unpublished_calls = true;
        let sent_off = encode_request(&off, &target).expect("encodes without the control");
        let sent_on = encode_request(&on, &target).expect("encodes with the control");
        assert_eq!(sent_on, sent_off, "{api:?}");
        assert!(
            !String::from_utf8_lossy(&sent_on).contains("unpublished_calls"),
            "{api:?}"
        );
    }
}
