//! Adversary cases for story:input-into-a-running-turn, pass 2: the corrected pattern of
//! `docs/design.md` "Input into a running turn", step 3, sent through the real Responses, Messages
//! and Chat Completions clients for every way the first turn can end.
//!
//! The next request is built as `crates/llm-docs/examples/interrupt_turn.rs` `converse` builds it:
//! the conversation so far; then the shown text (cancelled, when anything was shown) or the
//! outcome's items followed by a rejected result for each proposed call (finished); then the new
//! input. A local server records the body each client sends and answers 400, so the case checks
//! that the client sent the request and that the body keeps each provider's ordering rule: a
//! Messages `tool_result` first in the user message after its `tool_use`, then text; a Chat
//! Completions `tool` message for every call right after the assistant message; a Responses
//! `function_call_output` for every `function_call` before the next user message.
use llm_core::{
    CallId, Cancel, Id, Item, Model, Protocol, StopReason, ToolCall, ToolName, ToolSpec,
    TurnObservation, TurnOutcome, TurnRequest, VecSink,
};
use llm_credentials::SecretResolver;
use llm_http::{HttpClient, Limits};
use llm_routing::Catalog;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
};

const QUESTION: &str = "What will the weather be like tomorrow?";
const NEW_INPUT: &str = "Sorry, I meant on Sunday.";
const SHOWN: &str = "Tomorrow starts dry,";
const NOT_RUN: &str = "Not run: the user changed the question.";

/// How the first turn ended.
#[derive(Debug, Clone, Copy)]
enum Ending {
    CancelledPartWay,
    CancelledBeforeText,
    Finished,
    FinishedInACall,
    FinishedInTwoCalls,
    FinishedWithReasoningAndACall,
}

const ENDINGS: [Ending; 6] = [
    Ending::CancelledPartWay,
    Ending::CancelledBeforeText,
    Ending::Finished,
    Ending::FinishedInACall,
    Ending::FinishedInTwoCalls,
    Ending::FinishedWithReasoningAndACall,
];

fn catalog(protocol: &str, base_url: &str) -> Catalog {
    Catalog::parse(&format!(
        r#"format = "llm.catalog/1"

[[providers]]
id = "my-lab"
category = "self-hosted"

[[accounts]]
id = "local"
provider_id = "my-lab"
auth_kind = "anonymous"
billing_kind = "self-hosted"

[[endpoints]]
id = "local-server"
account_id = "local"
base_url = "{base_url}"

[[models]]
id = "small"
upstream_name = "example/Small-Model"

[[serving_models]]
id = "local-small"
endpoint_id = "local-server"
model_id = "small"
protocol = "{protocol}"
[serving_models.capabilities]
tools = true
tool_choice = false
temperature = false
top_p = false
reasoning_efforts = []
context_window = 8192
max_output_tokens = 1024

[[routes]]
id = "local"
alias = "code"

[[targets]]
id = "local-primary"
route_id = "local"
serving_model_id = "local-small"
position = 0
"#
    ))
    .expect("a valid catalog")
}

/// Reads one request and returns its body.
async fn read_body(socket: &mut TcpStream) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut buffer = vec![0; 1 << 16];
    loop {
        let length = socket.read(&mut buffer).await.expect("a readable socket");
        assert!(length > 0, "the client closed before sending a request");
        bytes.extend_from_slice(&buffer[..length]);
        let Some(split) = bytes.windows(4).position(|w| w == b"\r\n\r\n") else {
            continue;
        };
        let head = String::from_utf8_lossy(&bytes[..split]).to_string();
        let declared = head
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        if bytes.len() - split - 4 >= declared {
            return bytes[split + 4..split + 4 + declared].to_vec();
        }
    }
}

/// A server that sends each request body it reads to the returned channel and answers 400.
async fn recorder() -> (String, mpsc::UnboundedReceiver<Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
    let url = format!("http://{}/v1", listener.local_addr().expect("an address"));
    let (bodies, received) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let body = read_body(&mut socket).await;
            let _ = bodies.send(serde_json::from_slice(&body).expect("a JSON body"));
            let answer =
                "HTTP/1.1 400 Bad Request\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";
            let _ = socket.write_all(answer.as_bytes()).await;
            let _ = socket.shutdown().await;
        }
    });
    (url, received)
}

fn call(id: &str) -> ToolCall {
    ToolCall {
        call_id: CallId::new(id).unwrap(),
        name: ToolName::new("forecast").unwrap(),
        arguments: json!({"day": "tomorrow"}),
    }
}

/// The reasoning state each wire's decoder carries as an opaque item.
fn reasoning(protocol: Protocol) -> Value {
    match protocol {
        Protocol::Responses => {
            json!({"type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "gAAAAB"})
        }
        _ => json!({"type": "thinking", "thinking": "", "signature": "c2lnbmF0dXJl"}),
    }
}

/// What the first turn returned, and the text its sink accepted.
fn first_turn_ended(
    ending: Ending,
    first: &TurnRequest,
    model: &dyn Model,
) -> (Option<TurnOutcome>, &'static str) {
    let target = model.provenance().clone();
    let mut items = vec![Item::assistant(SHOWN)];
    match ending {
        Ending::CancelledPartWay => return (None, SHOWN),
        Ending::CancelledBeforeText => return (None, ""),
        Ending::Finished => {}
        Ending::FinishedInACall => items.push(Item::ToolCall(call("call-1"))),
        Ending::FinishedInTwoCalls => {
            items.push(Item::ToolCall(call("call-1")));
            items.push(Item::ToolCall(call("call-2")));
        }
        Ending::FinishedWithReasoningAndACall => {
            items.insert(
                0,
                Item::Opaque {
                    provenance: target.clone(),
                    payload: reasoning(target.protocol),
                },
            );
            items.push(Item::ToolCall(call("call-1")));
        }
    }
    let calls = items.iter().any(|item| item.as_tool_call().is_some());
    let outcome = TurnOutcome {
        stop_reason: if calls {
            StopReason::ToolCalls
        } else {
            StopReason::EndTurn
        },
        items,
        observation: TurnObservation {
            final_usage: true,
            ..TurnObservation::new(target.clone())
        },
    };
    outcome
        .validate_for(first, &target)
        .expect("an outcome a client returns for this request");
    (Some(outcome), SHOWN)
}

/// Where the new input goes: after the results, as documented, or, to show the order checks can
/// fail, between a finished turn's items and its results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Order {
    Documented,
    InputBeforeResults,
}

/// The next request, as `converse` builds it.
fn next_turn(
    first: &TurnRequest,
    returned: Option<TurnOutcome>,
    shown: &str,
    order: Order,
) -> TurnRequest {
    let mut items = first.items.clone();
    let mut input_placed = false;
    match returned {
        None if !shown.is_empty() => items.push(Item::assistant(shown)),
        None => {}
        Some(outcome) => {
            let open: Vec<CallId> = outcome
                .items
                .iter()
                .filter_map(Item::as_tool_call)
                .map(|call| call.call_id.clone())
                .collect();
            items.extend(outcome.items);
            if order == Order::InputBeforeResults {
                items.push(Item::user(NEW_INPUT));
                input_placed = true;
            }
            for call_id in open {
                items.push(Item::ToolResult {
                    call_id,
                    output: json!(NOT_RUN),
                    failed: true,
                });
            }
        }
    }
    if !input_placed {
        items.push(Item::user(NEW_INPUT));
    }
    let mut next = first.clone();
    next.items = items;
    next
}

fn ids(values: &[Value], key: &str) -> Vec<String> {
    values
        .iter()
        .filter_map(|value| value[key].as_str().map(str::to_owned))
        .collect()
}

/// Every `tool_use` is answered first thing in the next (user) message, text after the results.
fn messages_order(body: &Value) -> Result<(), String> {
    let messages = body["messages"].as_array().ok_or("no messages")?;
    for (index, message) in messages.iter().enumerate() {
        let blocks = message["content"].as_array().ok_or("no content")?;
        let uses: Vec<&Value> = blocks.iter().filter(|b| b["type"] == "tool_use").collect();
        if uses.is_empty() {
            continue;
        }
        let next = messages
            .get(index + 1)
            .ok_or("a tool_use ends the request")?;
        let answers = next["content"].as_array().ok_or("no content")?;
        let leading: Vec<Value> = answers
            .iter()
            .take_while(|b| b["type"] == "tool_result")
            .cloned()
            .collect();
        let requested: Vec<Value> = uses.into_iter().cloned().collect();
        if next["role"] != "user" || ids(&leading, "tool_use_id") != ids(&requested, "id") {
            return Err(format!(
                "message {index}'s tool_use is not answered first: {next}"
            ));
        }
    }
    Ok(())
}

/// Every assistant `tool_calls` entry has its `tool` message right after the assistant message.
fn chat_order(body: &Value) -> Result<(), String> {
    let messages = body["messages"].as_array().ok_or("no messages")?;
    for (index, message) in messages.iter().enumerate() {
        let Some(calls) = message["tool_calls"].as_array() else {
            continue;
        };
        let following: Vec<Value> = messages[index + 1..]
            .iter()
            .take_while(|m| m["role"] == "tool")
            .cloned()
            .collect();
        if ids(&following, "tool_call_id") != ids(calls, "id") {
            return Err(format!(
                "message {index}'s calls are not answered right after it"
            ));
        }
    }
    Ok(())
}

/// Every `function_call` has its `function_call_output` before the next user message.
fn responses_order(body: &Value) -> Result<(), String> {
    let input = body["input"].as_array().ok_or("no input")?;
    let mut open = Vec::new();
    for item in input {
        match item["type"].as_str() {
            Some("function_call") => open.push(item["call_id"].clone()),
            Some("function_call_output") => open.retain(|id| *id != item["call_id"]),
            _ if item["role"] == "user" && !open.is_empty() => {
                return Err(format!("user input before the outputs of {open:?}"));
            }
            _ => {}
        }
    }
    if open.is_empty() {
        Ok(())
    } else {
        Err(format!("calls without output: {open:?}"))
    }
}

async fn send_next_turn(protocol: &str, ending: Ending, order: Order) -> Result<(), String> {
    let (url, mut bodies) = recorder().await;
    let http = HttpClient::new(Limits {
        response_headers: Duration::from_secs(5),
        idle: Duration::from_secs(5),
        total: Duration::from_secs(10),
    })
    .expect("a bounded transport");
    let resolvers: BTreeMap<Id, Arc<dyn SecretResolver>> = BTreeMap::new();
    let model = llm_models::port(
        &catalog(protocol, &url),
        &Id::new("local-small").unwrap(),
        http,
        &resolvers,
    )
    .expect("a port");
    let mut first = TurnRequest::new(
        model.provenance().model.as_str(),
        vec![Item::user(QUESTION)],
    );
    first.tools = vec![ToolSpec {
        name: ToolName::new("forecast").unwrap(),
        description: "The forecast for one day.".to_owned(),
        input_schema: json!({"type": "object", "properties": {"day": {"type": "string"}}}),
    }];
    let (returned, shown) = first_turn_ended(ending, &first, &model);
    let next = next_turn(&first, returned, shown, order);
    let mut sink = VecSink::new(64, 64 * 1024);
    let answered = model.turn(&next, &mut sink, &Cancel::new()).await;
    let Ok(body) = bodies.try_recv() else {
        return Err(format!("the client did not send it: {answered:?}"));
    };
    match model.provenance().protocol {
        Protocol::Messages => messages_order(&body),
        Protocol::ChatCompletions => chat_order(&body),
        Protocol::Responses => responses_order(&body),
    }
    .map_err(|broken| format!("{broken}; body {body}"))
}

#[tokio::test]
async fn the_corrected_next_turn_is_sent_in_provider_order_for_every_ending_on_every_wire() {
    let mut failures = Vec::new();
    for protocol in ["responses", "messages", "chat-completions"] {
        for ending in ENDINGS {
            // Chat Completions decodes reasoning as deltas only; its outcome carries no opaque item.
            if protocol == "chat-completions"
                && matches!(ending, Ending::FinishedWithReasoningAndACall)
            {
                continue;
            }
            if let Err(broken) = send_next_turn(protocol, ending, Order::Documented).await {
                failures.push(format!("{protocol} {ending:?}: {broken}"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The control: the new input placed between a finished turn's call and its result passes every
/// client's own check, so it is sent, and each wire's order check refuses it.
#[tokio::test]
async fn the_order_checks_refuse_new_input_placed_before_the_results() {
    for protocol in ["responses", "messages", "chat-completions"] {
        let broken =
            send_next_turn(protocol, Ending::FinishedInACall, Order::InputBeforeResults).await;
        let message = broken.expect_err("the misordered request passed the order check");
        assert!(
            !message.starts_with("the client did not send it"),
            "{protocol}: {message}"
        );
    }
}
