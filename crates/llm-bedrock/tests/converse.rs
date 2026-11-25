//! The Converse wire end to end: recorded Bedrock Runtime answers replayed through
//! `BedrockClient` against a local fixture server, and the request body each turn sends.
//!
//! Every `pydantic-*` and `ddtrace-*` answer under `tests/data/frames/` is a recorded response
//! body, and every `.request.json` the body that produced it (`tests/data/README.md` names the
//! sources). The two `synthetic-*.json` answers there were written for the conformance
//! scenarios of a turn that stops without content, and no test here reads them.
//! The expected values below were read out of those files, not chosen: each text is the
//! concatenation of its block's text deltas, each count the number of non-empty deltas, each
//! usage the recorded `metadata` event's. Frames built here with `eventstream::encode_message`
//! are synthetic, and each test that builds one says so.

use llm_bedrock::{
    Api, AwsCredentials, BedrockClient, BedrockTarget, CredentialSource, check_request,
    encode_request,
    eventstream::{HeaderValue, Message, encode_message},
};
use llm_core::{
    ADVISORY_OMITTED, BoxFuture, CallId, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id,
    Item, MAX_ITEMS, MAX_REQUEST_BYTES, MAX_TOOLS, Model, Protocol, Provenance, StopReason,
    StreamEvent, Thinking, ToolCall, ToolChoice, ToolName, ToolSpec, TurnOutcome, TurnRequest,
    Usage, VecSink, encoded_len,
};
use llm_http::{HttpClient, Limits};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};
use url::Url;

const NOVA: &str = "us.amazon.nova-micro-v1:0";
const SONNET_3: &str = "anthropic.claude-3-sonnet-20240229-v1:0";
const SONNET_3_7: &str = "us.anthropic.claude-3-7-sonnet-20250219-v1:0";
const SONNET_4: &str = "us.anthropic.claude-sonnet-4-20250514-v1:0";
const GPT_OSS: &str = "openai.gpt-oss-120b-1:0";

const EVENT_STREAM: &str = "application/vnd.amazon.eventstream";

/// `pydantic-test_bedrock_model_stream.bin`, block 0.
const CAPITAL_TEXT: &str = "The capital of France is Paris. Paris is not only the capital city \
but also the most populous city in France, and it is a major center for culture, commerce, \
fashion, and international diplomacy. Known for its historical landmarks, such as the Eiffel \
Tower, the Louvre Museum, and Notre-Dame Cathedral, Paris is often referred to as \"The City of \
Light\" or \"The City of Love.\"";

/// `pydantic-test_bedrock_model_iter_stream-0.bin`, block 0.
const TEMPERATURE_TEXT: &str = "<thinking> To find the temperature of the capital of France, I \
need to first determine the capital of France and then get the current temperature in that \
city. The capital of France is Paris. I will use the \"get_temperature\" tool to find the current \
temperature in Paris.</thinking>\n";

/// `pydantic-test_bedrock_model_thinking_part_stream.bin`, block 0's reasoning text and
/// signature, and block 1.
const GREETING_REASONING: &str = "The user has greeted me with a simple \"Hello\". I should \
respond in a friendly and welcoming manner. This is a straightforward greeting, so I'll respond \
warmly and ask how I can help them today.";
const GREETING_SIGNATURE: &str = "Eu0CCkgIBxABGAIqQJDccbDQkr81n7QjZ0Fi43umSvw0YvnGkMPEpaGAa2btYHy\
Ww06KhwckvsnKzpKcxiRJT35meoG4/pdrTUiy2UISDPDaEWfOl3+HlRVsCxoMzfiqBp252RMvpmEyIjCbQ97Ac9Epkr5mg\
xeu1vGtJg+fDWIg0UnpMM8NYknhhvJmsXpYrfquwGL1ZnlBslUq0gHtbAAPwlWPmiQXU7gDQCDW9IdMVyw42b4f5MrAlpWk\
PWOJc9H+yYv0TpP/jY72SD1opqwkWnBgkzbi7A2jPmEFzIMQSO1KDXha5ADqQ3cLYMmVdNTSH9wlM7G7/JJ2/cqowqkwD6/\
q1AnYzcPte9iC67fY1LYN0NMCOSABFojP1rmkv9YBEulx5Y6eQpeVXBQiIqcGoCmWSumpGBskS1KxGerUmzUB0JmJnTENv4\
x3fSGSUSEPqMiz6Ebao8sVkb1wCWuZEXWJGtiQLMIm1o471iEYAQ==";
const GREETING_TEXT: &str = "Hello! It's nice to meet you. How can I help you today?";

/// `ddtrace-bedrock_converse_stream.bin` and `ddtrace-bedrock_converse.json`, block 0.
const TRACING_TEXT: &str = "Okay, let me use the tool to fetch an expert explanation for the \
concept of distributed tracing.";

/// `pydantic-test_bedrock_model_stream_empty_text_delta.bin`, blocks 1 and 2.
const HI_REASONING: &str = "The user just says \"Hi\". We need to respond appropriately, \
friendly greeting. No special instructions. Should be short.";
const HI_TEXT: &str = "Hello! How can I help you today?";

/// What the fixture server read: the request line, the header fields with lower-cased names,
/// and the body.
struct Request {
    line: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value.as_str())
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("the request body is JSON")
    }
}

async fn read_request(socket: &mut TcpStream) -> Request {
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    let end = loop {
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break end;
        }
        let read = socket.read(&mut buffer).await.unwrap();
        assert!(read > 0, "the client closed before sending a request");
        bytes.extend_from_slice(&buffer[..read]);
    };
    let head = String::from_utf8(bytes[..end].to_vec()).unwrap();
    let mut lines = head.split("\r\n");
    let line = lines.next().unwrap().to_owned();
    let headers: Vec<(String, String)> = lines
        .map(|field| {
            let (name, value) = field.split_once(':').unwrap();
            (name.trim().to_ascii_lowercase(), value.trim().to_owned())
        })
        .collect();
    let length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .map_or(0, |(_, value)| value.parse().unwrap());
    let mut body = bytes[end + 4..].to_vec();
    while body.len() < length {
        let read = socket.read(&mut buffer).await.unwrap();
        assert!(read > 0, "the client closed inside its body");
        body.extend_from_slice(&buffer[..read]);
    }
    Request {
        line,
        headers,
        body,
    }
}

/// Serves one connection on 127.0.0.1: reads the request, writes `head`, then `body` in small
/// parts, so frames arrive split across reads, then closes. Reports the request it read.
async fn serve(head: String, body: Vec<u8>) -> (Url, JoinHandle<Request>) {
    serve_in(head, body, 61).await
}

/// As [`serve`], the body written in parts of `part` bytes.
async fn serve_in(head: String, body: Vec<u8>, part: usize) -> (Url, JoinHandle<Request>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let request = read_request(&mut socket).await;
        socket.write_all(head.as_bytes()).await.unwrap();
        for part in body.chunks(part) {
            // The client may already have gone, after a failure it found.
            if socket.write_all(part).await.is_err() || socket.flush().await.is_err() {
                break;
            }
        }
        request
    });
    (origin, server)
}

/// A successful answer of `content_type` whose body ends when the connection closes.
fn success(content_type: &str) -> String {
    format!("HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\nconnection: close\r\n\r\n")
}

/// Fixed keys; nothing here reaches Bedrock. Counts how often it was asked.
#[derive(Default)]
struct Keys(AtomicUsize);

impl CredentialSource for Keys {
    fn credentials<'a>(&'a self, _: &'a Cancel) -> BoxFuture<'a, Result<AwsCredentials, Error>> {
        self.0.fetch_add(1, Ordering::SeqCst);
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

fn provenance(model: &str) -> Provenance {
    Provenance {
        protocol: Protocol::BedrockConverse,
        provider: Id::new("bedrock").unwrap(),
        account: Id::new("fixture-account").unwrap(),
        endpoint: Id::new("us-east-1").unwrap(),
        model: Id::new(model).unwrap(),
        binding_revision: Id::new("fixture-1").unwrap(),
    }
}

fn capabilities(stream: bool) -> Capabilities {
    Capabilities::text(200_000, 4_096)
        .with_tools(true)
        .with_tool_choice(true)
        .with_temperature(true)
        .with_top_p(true)
        .with_output_streaming(stream)
}

fn target(model: &str, stream: bool, endpoint: Url) -> BedrockTarget {
    BedrockTarget::new(
        provenance(model),
        capabilities(stream),
        "us-east-1",
        model,
        Api::Converse,
        stream,
        Some(endpoint),
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

/// Runs one turn of `request` on `target` and returns its result and what the sink was shown.
async fn turn(
    target: BedrockTarget,
    request: &TurnRequest,
) -> (Result<TurnOutcome, Error>, Vec<StreamEvent>) {
    let client = BedrockClient::new(target, http(), Arc::new(Keys::default())).unwrap();
    let mut sink = VecSink::new(10_000, 1 << 20);
    let result = client.turn(request, &mut sink, &Cancel::new()).await;
    (result, sink.into_events())
}

/// Replays one recorded answer to a turn of `request` on `model`, streamed or whole, and returns
/// the outcome, the events shown and the request the fixture server read.
async fn replay(
    model: &str,
    stream: bool,
    request: &TurnRequest,
    answer: &[u8],
) -> (TurnOutcome, Vec<StreamEvent>, Request) {
    let content_type = if stream {
        EVENT_STREAM
    } else {
        "application/json"
    };
    let (origin, server) = serve(success(content_type), answer.to_vec()).await;
    let (result, events) = turn(target(model, stream, origin), request).await;
    let outcome = result.expect("the recorded answer decodes");
    (outcome, events, server.await.unwrap())
}

/// Each event as a short tag, consecutive equal tags counted: the shape of what was shown.
fn shape(events: &[StreamEvent]) -> Vec<(&'static str, usize)> {
    let mut runs: Vec<(&'static str, usize)> = Vec::new();
    for event in events {
        let tag = match event {
            StreamEvent::TextDelta { .. } => "text",
            StreamEvent::ToolCallStarted { .. } => "tool-call",
            StreamEvent::ToolArgumentsDelta { .. } => "tool-arguments",
            StreamEvent::ReasoningDelta { .. } => "reasoning",
            StreamEvent::Warning { .. } => "warning",
            StreamEvent::RefusalDelta { .. } => "refusal",
            StreamEvent::MessageBoundary {} => "message-boundary",
            StreamEvent::OpaqueItem { .. } => "opaque-item",
        };
        match runs.last_mut() {
            Some((last, count)) if *last == tag => *count += 1,
            _ => runs.push((tag, 1)),
        }
    }
    runs
}

fn shown_text(events: &[StreamEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            StreamEvent::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn shown_reasoning(events: &[StreamEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            StreamEvent::ReasoningDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn shown_arguments(events: &[StreamEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            StreamEvent::ToolArgumentsDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect()
}

fn call(id: &str, name: &str, arguments: Value) -> Item {
    Item::ToolCall(ToolCall {
        call_id: CallId::new(id).unwrap(),
        name: ToolName::new(name).unwrap(),
        arguments,
    })
}

fn tool(name: &str, description: &str, input_schema: Value) -> ToolSpec {
    ToolSpec {
        name: ToolName::new(name).unwrap(),
        description: description.to_owned(),
        input_schema,
    }
}

/// The usage of an answer that reports `inputTokens` and `outputTokens` and no cache counter.
/// The body sent carries no `cachePoint`, so both cache counters are a known 0 and the whole
/// input is `inputTokens`.
fn uncached(input_tokens: u64, output_tokens: u64) -> Usage {
    Usage {
        input_tokens: Some(input_tokens),
        output_tokens: Some(output_tokens),
        cached_input_tokens: Some(0),
        cache_creation_input_tokens: Some(0),
        reasoning_output_tokens: None,
    }
}

/// A recorded request body as this encoder writes it. The recording clients sent an empty
/// `system`, an empty `inferenceConfig` and an empty `stopSequences`; each carries nothing, and
/// this encoder leaves them out.
fn without_empty_fields(mut body: Value) -> Value {
    let object = body.as_object_mut().unwrap();
    if let Some(config) = object
        .get_mut("inferenceConfig")
        .and_then(Value::as_object_mut)
        && config.get("stopSequences") == Some(&json!([]))
    {
        config.remove("stopSequences");
    }
    for name in ["system", "inferenceConfig"] {
        let empty = match object.get(name) {
            Some(Value::Array(items)) => items.is_empty(),
            Some(Value::Object(fields)) => fields.is_empty(),
            _ => false,
        };
        if empty {
            object.remove(name);
        }
    }
    body
}

fn recorded(bytes: &[u8]) -> Value {
    without_empty_fields(serde_json::from_slice(bytes).unwrap())
}

fn capital_request() -> TurnRequest {
    let mut request = TurnRequest::new(NOVA, vec![Item::user("What is the capital of France?")]);
    "You are a helpful chatbot.".clone_into(&mut request.instructions);
    request.sampling.temperature = Some(0.0);
    request
}

fn temperature_request() -> TurnRequest {
    let mut request = TurnRequest::new(
        NOVA,
        vec![Item::user(
            "What is the temperature of the capital of France?",
        )],
    );
    "You are a helpful chatbot.".clone_into(&mut request.instructions);
    request.sampling.top_p = Some(0.5);
    request.tools = vec![
        tool(
            "get_capital",
            "Get the capital of a country.",
            json!({"properties": {"country": {"description": "The country name.",
                "title": "Country", "type": "string"}}, "required": ["country"],
                "type": "object", "additionalProperties": false}),
        ),
        tool(
            "get_temperature",
            "Get the temperature in a city.",
            json!({"properties": {"city": {"description": "The city name.", "title": "City",
                "type": "string"}}, "required": ["city"], "type": "object",
                "additionalProperties": false}),
        ),
    ];
    request
}

fn greeting_request() -> TurnRequest {
    let mut request = TurnRequest::new(SONNET_4, vec![Item::user("Hello")]);
    request.controls.thinking = Some(Thinking::Enabled {
        budget_tokens: 1024,
    });
    request
}

/// The tracing turn of the dd-trace recordings, with the tool description each one sent.
fn tracing_request(description: &str) -> TurnRequest {
    let mut request = TurnRequest::new(
        SONNET_3,
        vec![Item::user(
            "Explain the concept of distributed tracing in a simple way",
        )],
    );
    "You are an expert swe that is to use the tool fetch_concept"
        .clone_into(&mut request.instructions);
    request.sampling.temperature = Some(0.7);
    request.sampling.top_p = Some(0.9);
    request.max_output_tokens = Some(1000);
    request.tools = vec![tool(
        "fetch_concept",
        description,
        json!({"type": "object", "properties": {"concept": {"type": "string",
            "description": "The concept to explain"}}, "required": ["concept"]}),
    )];
    request
}

fn greeting_reasoning(model: &str) -> Item {
    Item::Opaque {
        provenance: provenance(model),
        payload: json!({"reasoningContent": {"reasoningText": {
            "text": GREETING_REASONING, "signature": GREETING_SIGNATURE}}}),
    }
}

#[test]
fn the_protocol_is_named_bedrock_converse_on_the_wire() {
    assert_eq!(
        serde_json::to_value(Protocol::BedrockConverse).unwrap(),
        json!("bedrock-converse")
    );
    assert_eq!(
        serde_json::from_value::<Protocol>(json!("bedrock-converse")).unwrap(),
        Protocol::BedrockConverse
    );
}

#[tokio::test]
async fn a_recorded_text_stream_is_the_text_its_deltas_showed() {
    let request = capital_request();
    let (outcome, events, sent) = replay(
        NOVA,
        true,
        &request,
        include_bytes!("data/frames/pydantic-test_bedrock_model_stream.bin"),
    )
    .await;
    assert_eq!(outcome.stop_reason, StopReason::EndTurn);
    assert_eq!(outcome.items, vec![Item::assistant(CAPITAL_TEXT)]);
    assert_eq!(shape(&events), vec![("text", 29)]);
    assert_eq!(shown_text(&events), CAPITAL_TEXT);
    assert_eq!(outcome.observation.usage, Some(uncached(13, 82)));
    assert!(outcome.observation.final_usage);
    assert_eq!(outcome.observation.binding, provenance(NOVA));
    assert!(outcome.observation.upstream_model.is_none());
    assert!(outcome.observation.response_id.is_none());

    assert_eq!(
        sent.line,
        "POST /model/us.amazon.nova-micro-v1%3A0/converse-stream HTTP/1.1"
    );
    assert_eq!(sent.header("accept"), Some(EVENT_STREAM));
    assert_eq!(sent.header("content-type"), Some("application/json"));
    assert_eq!(
        sent.json(),
        recorded(include_bytes!(
            "data/frames/pydantic-test_bedrock_model_stream.request.json"
        ))
    );
}

#[tokio::test]
async fn a_recorded_tool_use_stream_carries_its_text_and_its_call() {
    let request = temperature_request();
    let (outcome, events, sent) = replay(
        NOVA,
        true,
        &request,
        include_bytes!("data/frames/pydantic-test_bedrock_model_iter_stream-0.bin"),
    )
    .await;
    assert_eq!(outcome.stop_reason, StopReason::ToolCalls);
    assert_eq!(
        outcome.items,
        vec![
            Item::assistant(TEMPERATURE_TEXT),
            call(
                "tooluse_lAG_zP8QRHmSYOwZzzaCqA",
                "get_temperature",
                json!({"city": "Paris"})
            ),
        ]
    );
    assert_eq!(
        shape(&events),
        vec![("text", 19), ("tool-call", 1), ("tool-arguments", 1)]
    );
    assert_eq!(shown_text(&events), TEMPERATURE_TEXT);
    assert!(events.contains(&StreamEvent::ToolCallStarted {
        call_id: CallId::new("tooluse_lAG_zP8QRHmSYOwZzzaCqA").unwrap(),
        name: ToolName::new("get_temperature").unwrap(),
    }));
    assert_eq!(shown_arguments(&events), r#"{"city":"Paris"}"#);
    assert_eq!(outcome.observation.usage, Some(uncached(471, 91)));
    assert_eq!(
        sent.json(),
        recorded(include_bytes!(
            "data/frames/pydantic-test_bedrock_model_iter_stream-0.request.json"
        ))
    );
}

#[tokio::test]
async fn a_tool_input_streamed_in_fragments_is_relayed_and_parsed_once() {
    let request = tracing_request("Fetch an expert explanation for a concept");
    let (outcome, events, sent) = replay(
        SONNET_3,
        true,
        &request,
        include_bytes!("data/frames/ddtrace-bedrock_converse_stream.bin"),
    )
    .await;
    assert_eq!(outcome.stop_reason, StopReason::ToolCalls);
    assert_eq!(
        outcome.items,
        vec![
            Item::assistant(TRACING_TEXT),
            call(
                "tooluse_HA9wsJ32RhOvn8XgIUDpGA",
                "fetch_concept",
                json!({"concept": "distributed tracing"})
            ),
        ]
    );
    // Six input fragments were recorded; the first is empty and shows nothing.
    assert_eq!(
        shape(&events),
        vec![("text", 20), ("tool-call", 1), ("tool-arguments", 5)]
    );
    assert_eq!(shown_text(&events), TRACING_TEXT);
    assert_eq!(
        shown_arguments(&events),
        r#"{"concept": "distributed tracing"}"#
    );
    assert_eq!(outcome.observation.usage, Some(uncached(259, 64)));
    assert_eq!(
        sent.line,
        "POST /model/anthropic.claude-3-sonnet-20240229-v1%3A0/converse-stream HTTP/1.1"
    );
    assert_eq!(
        sent.json(),
        recorded(include_bytes!(
            "data/frames/ddtrace-bedrock_converse_stream.request.json"
        ))
    );
}

#[tokio::test]
async fn recorded_reasoning_is_shown_and_kept_with_its_signature() {
    let request = greeting_request();
    let (outcome, events, sent) = replay(
        SONNET_4,
        true,
        &request,
        include_bytes!("data/frames/pydantic-test_bedrock_model_thinking_part_stream.bin"),
    )
    .await;
    assert_eq!(outcome.stop_reason, StopReason::EndTurn);
    assert_eq!(
        outcome.items,
        vec![greeting_reasoning(SONNET_4), Item::assistant(GREETING_TEXT)]
    );
    // The signature delta is not reasoning and shows nothing.
    assert_eq!(shape(&events), vec![("reasoning", 14), ("text", 5)]);
    assert_eq!(shown_reasoning(&events), GREETING_REASONING);
    assert_eq!(shown_text(&events), GREETING_TEXT);
    assert_eq!(outcome.observation.usage, Some(uncached(36, 73)));
    assert_eq!(
        sent.json(),
        recorded(include_bytes!(
            "data/frames/pydantic-test_bedrock_model_thinking_part_stream.request.json"
        ))
    );
}

#[tokio::test]
async fn kept_reasoning_goes_back_as_the_assistants_own_block() {
    let mut request = greeting_request();
    request.items.extend([
        greeting_reasoning(SONNET_4),
        Item::assistant(GREETING_TEXT),
        Item::user("Thank you"),
    ]);
    let (_, _, sent) = replay(
        SONNET_4,
        true,
        &request,
        include_bytes!("data/frames/pydantic-test_bedrock_model_thinking_part_stream.bin"),
    )
    .await;
    assert_eq!(
        sent.json()["messages"],
        json!([
            {"role": "user", "content": [{"text": "Hello"}]},
            {"role": "assistant", "content": [
                {"reasoningContent": {"reasoningText": {
                    "text": GREETING_REASONING, "signature": GREETING_SIGNATURE}}},
                {"text": GREETING_TEXT},
            ]},
            {"role": "user", "content": [{"text": "Thank you"}]},
        ])
    );
}

#[tokio::test]
async fn an_empty_text_block_leaves_no_item_and_unsigned_reasoning_is_kept() {
    let request = TurnRequest::new(GPT_OSS, vec![Item::user("Hi")]);
    let (outcome, events, sent) = replay(
        GPT_OSS,
        true,
        &request,
        include_bytes!("data/frames/pydantic-test_bedrock_model_stream_empty_text_delta.bin"),
    )
    .await;
    assert_eq!(outcome.stop_reason, StopReason::EndTurn);
    assert_eq!(
        outcome.items,
        vec![
            Item::Opaque {
                provenance: provenance(GPT_OSS),
                payload: json!({"reasoningContent": {"reasoningText": {"text": HI_REASONING}}}),
            },
            Item::assistant(HI_TEXT),
        ]
    );
    assert_eq!(shape(&events), vec![("reasoning", 1), ("text", 2)]);
    assert_eq!(shown_text(&events), HI_TEXT);
    // `serverToolUsage` is recorded empty: no server tool ran.
    assert_eq!(outcome.observation.usage, Some(uncached(70, 43)));
    assert_eq!(
        sent.json(),
        recorded(include_bytes!(
            "data/frames/pydantic-test_bedrock_model_stream_empty_text_delta.request.json"
        ))
    );
}

/// One recorded prompt-caching answer and what it carries. Its text is checked by length and
/// ends, not quoted whole.
struct Cached {
    answer: &'static [u8],
    length: usize,
    deltas: usize,
    start: &'static str,
    end: &'static str,
    usage: Usage,
}

/// The two prompt-caching answers report the input outside the cache and both cache counters;
/// the whole input is their sum (`totalTokens` minus the output confirms it: 1275 - 236 and
/// 1290 - 250).
#[tokio::test]
async fn cache_counters_add_up_to_the_whole_input() {
    let cases = [
        Cached {
            answer: include_bytes!(
                "data/frames/ddtrace-bedrock_converse_stream_prompt_caching-0.bin"
            ),
            length: 1134,
            deltas: 99,
            start: "# Service\n\nA service is a software",
            end: "in distributed systems and cloud computing environments.",
            usage: Usage {
                input_tokens: Some(1039),
                output_tokens: Some(236),
                cached_input_tokens: Some(0),
                cache_creation_input_tokens: Some(1028),
                reasoning_output_tokens: None,
            },
        },
        Cached {
            answer: include_bytes!(
                "data/frames/ddtrace-bedrock_converse_stream_prompt_caching-1.bin"
            ),
            length: 1240,
            deltas: 51,
            start: "# Machine Learning (ML) Application",
            end: "automate processes, gain insights, and enhance user experiences.",
            usage: Usage {
                input_tokens: Some(1040),
                output_tokens: Some(250),
                cached_input_tokens: Some(1028),
                cache_creation_input_tokens: Some(0),
                reasoning_output_tokens: None,
            },
        },
    ];
    for case in cases {
        let mut request = TurnRequest::new(SONNET_3_7, vec![Item::user("What is it")]);
        request.instructions = "Software architecture guidelines".to_owned();
        let (outcome, events, _) = replay(SONNET_3_7, true, &request, case.answer).await;
        assert_eq!(outcome.stop_reason, StopReason::EndTurn);
        let [Item::AssistantText { text }] = outcome.items.as_slice() else {
            panic!("one text item expected");
        };
        assert_eq!(text.len(), case.length);
        assert!(text.starts_with(case.start) && text.ends_with(case.end));
        assert_eq!(&shown_text(&events), text);
        assert_eq!(shape(&events), vec![("text", case.deltas)]);
        assert_eq!(outcome.observation.usage, Some(case.usage));
    }
}

#[tokio::test]
async fn a_recorded_whole_answer_is_shown_once_it_has_decoded() {
    let request = tracing_request(
        "Fetch an expert explanation for a concept, especially relevant for technical concepts \
         like distributed tracing",
    );
    let (outcome, events, sent) = replay(
        SONNET_3,
        false,
        &request,
        include_bytes!("data/frames/ddtrace-bedrock_converse.json"),
    )
    .await;
    assert_eq!(outcome.stop_reason, StopReason::ToolCalls);
    assert_eq!(
        outcome.items,
        vec![
            Item::assistant(TRACING_TEXT),
            call(
                "tooluse_iPA5tRLlQ5qSvfeYGeQVPg",
                "fetch_concept",
                json!({"concept": "distributed tracing"})
            ),
        ]
    );
    assert_eq!(
        shape(&events),
        vec![("text", 1), ("tool-call", 1), ("tool-arguments", 1)]
    );
    assert_eq!(shown_text(&events), TRACING_TEXT);
    assert_eq!(
        shown_arguments(&events),
        r#"{"concept":"distributed tracing"}"#
    );
    assert_eq!(outcome.observation.usage, Some(uncached(269, 77)));
    assert!(outcome.observation.final_usage);
    assert_eq!(
        sent.line,
        "POST /model/anthropic.claude-3-sonnet-20240229-v1%3A0/converse HTTP/1.1"
    );
    assert_eq!(sent.header("accept"), Some("application/json"));
    assert_eq!(
        sent.json(),
        recorded(include_bytes!(
            "data/frames/ddtrace-bedrock_converse.request.json"
        ))
    );
}

#[tokio::test]
async fn a_conversation_with_tool_results_groups_by_role() {
    let mut request = tracing_request("Fetch an expert explanation for a concept");
    request.tool_choice = ToolChoice::Required;
    request.items.extend([
        Item::assistant("Looking it up."),
        call(
            "tooluse_first",
            "fetch_concept",
            json!({"concept": "spans"}),
        ),
        Item::ToolResult {
            call_id: CallId::new("tooluse_first").unwrap(),
            output: json!("no expert available"),
            failed: true,
        },
    ]);
    let (_, _, sent) = replay(
        SONNET_3,
        true,
        &request,
        include_bytes!("data/frames/ddtrace-bedrock_converse_stream.bin"),
    )
    .await;
    let body = sent.json();
    assert_eq!(
        body["messages"],
        json!([
            {"role": "user", "content": [
                {"text": "Explain the concept of distributed tracing in a simple way"}]},
            {"role": "assistant", "content": [
                {"text": "Looking it up."},
                {"toolUse": {"toolUseId": "tooluse_first", "name": "fetch_concept",
                    "input": {"concept": "spans"}}},
            ]},
            {"role": "user", "content": [
                {"toolResult": {"toolUseId": "tooluse_first",
                    "content": [{"text": "no expert available"}], "status": "error"}},
            ]},
        ])
    );
    assert_eq!(body["toolConfig"]["toolChoice"], json!({"any": {}}));
}

#[tokio::test]
async fn an_omitted_hint_is_reported_after_the_turn() {
    let mut request = capital_request();
    request.controls.hints.prompt_cache_key = Some("fixture-cache-key".to_owned());
    let (_, events, sent) = replay(
        NOVA,
        true,
        &request,
        include_bytes!("data/frames/pydantic-test_bedrock_model_stream.bin"),
    )
    .await;
    assert_eq!(shape(&events), vec![("text", 29), ("warning", 1)]);
    assert!(matches!(
        events.last(),
        Some(StreamEvent::Warning { code, .. }) if code == ADVISORY_OMITTED
    ));
    assert!(!String::from_utf8_lossy(&sent.body).contains("fixture-cache-key"));
}

#[tokio::test]
async fn anthropic_beta_names_converse_cannot_send_are_reported_by_name_after_the_turn() {
    let mut request = capital_request();
    request.controls.hints.anthropic_beta = vec!["context-1m-2025-08-07".to_owned()];
    let (_, events, sent) = replay(
        NOVA,
        true,
        &request,
        include_bytes!("data/frames/pydantic-test_bedrock_model_stream.bin"),
    )
    .await;
    assert_eq!(shape(&events), vec![("text", 29), ("warning", 1)]);
    assert!(matches!(
        events.last(),
        Some(StreamEvent::Warning { code, message })
            if code == ADVISORY_OMITTED && message.contains("anthropic-beta")
    ));
    let body = String::from_utf8_lossy(&sent.body);
    assert!(!body.contains("context-1m"), "{body}");
    assert!(!body.contains("anthropic_beta"), "{body}");
}

/// A synthetic event frame, built here and not recorded.
fn event(kind: &str, payload: &Value) -> Vec<u8> {
    encode_message(&Message {
        headers: vec![
            (
                ":event-type".to_owned(),
                HeaderValue::String(kind.to_owned()),
            ),
            (
                ":content-type".to_owned(),
                HeaderValue::String("application/json".to_owned()),
            ),
            (
                ":message-type".to_owned(),
                HeaderValue::String("event".to_owned()),
            ),
        ],
        payload: serde_json::to_vec(payload).unwrap(),
    })
}

/// A synthetic answer: one text block, then `stop` and a usage report.
fn synthetic_answer(stop: &str, between: &[u8]) -> Vec<u8> {
    let mut body = event("messageStart", &json!({"role": "assistant"}));
    body.extend_from_slice(between);
    body.extend(event(
        "contentBlockDelta",
        &json!({"contentBlockIndex": 0, "delta": {"text": "Hi"}}),
    ));
    body.extend(event("contentBlockStop", &json!({"contentBlockIndex": 0})));
    body.extend(event("messageStop", &json!({"stopReason": stop})));
    body.extend(event(
        "metadata",
        &json!({"usage": {"inputTokens": 3, "outputTokens": 1, "totalTokens": 4}}),
    ));
    body
}

#[tokio::test]
async fn every_terminal_reason_is_carried_and_none_reads_as_completion() {
    for (reported, expected) in [
        ("max_tokens", StopReason::MaxOutputTokens),
        (
            "guardrail_intervened",
            StopReason::Incomplete {
                reason: "guardrail_intervened".to_owned(),
            },
        ),
        (
            "content_filtered",
            StopReason::Incomplete {
                reason: "content_filtered".to_owned(),
            },
        ),
    ] {
        let (origin, _server) = serve(success(EVENT_STREAM), synthetic_answer(reported, &[])).await;
        let request = TurnRequest::new(NOVA, vec![Item::user("Hello")]);
        let (result, _) = turn(target(NOVA, true, origin), &request).await;
        assert_eq!(result.unwrap().stop_reason, expected, "{reported}");
    }
}

#[tokio::test]
async fn an_event_outside_the_subset_is_kept_whole_and_reported() {
    // Synthetic: an event type this decoder does not know, before the first block.
    let unknown = event("futureEvent", &json!({"detail": 1}));
    let (origin, _server) = serve(
        success(EVENT_STREAM),
        synthetic_answer("end_turn", &unknown),
    )
    .await;
    let request = TurnRequest::new(NOVA, vec![Item::user("Hello")]);
    let (result, events) = turn(target(NOVA, true, origin), &request).await;
    let outcome = result.unwrap();
    assert_eq!(
        outcome.items,
        vec![
            Item::Opaque {
                provenance: provenance(NOVA),
                payload: json!({"detail": 1}),
            },
            Item::assistant("Hi"),
        ]
    );
    assert_eq!(shape(&events), vec![("warning", 1), ("text", 1)]);
    assert!(matches!(
        &events[0],
        StreamEvent::Warning { code, .. } if code == "unknown-stream-event"
    ));
}

/// story:ingress-block-boundaries. Text of a later content block is a new message: the caller is
/// shown a message boundary before it, and never before the first text.
#[tokio::test]
async fn text_of_a_later_block_follows_a_message_boundary() {
    // Synthetic: two text blocks, each opened by its first delta.
    let mut body = event("messageStart", &json!({"role": "assistant"}));
    for (index, text) in [(0, "One"), (1, "Two")] {
        body.extend(event(
            "contentBlockDelta",
            &json!({"contentBlockIndex": index, "delta": {"text": text}}),
        ));
        body.extend(event(
            "contentBlockStop",
            &json!({"contentBlockIndex": index}),
        ));
    }
    body.extend(event("messageStop", &json!({"stopReason": "end_turn"})));
    body.extend(event(
        "metadata",
        &json!({"usage": {"inputTokens": 3, "outputTokens": 2, "totalTokens": 5}}),
    ));
    let (origin, _server) = serve(success(EVENT_STREAM), body).await;
    let request = TurnRequest::new(NOVA, vec![Item::user("Hello")]);
    let (result, events) = turn(target(NOVA, true, origin), &request).await;
    assert_eq!(
        result.unwrap().items,
        vec![Item::assistant("One"), Item::assistant("Two")]
    );
    assert_eq!(
        shape(&events),
        vec![("text", 1), ("message-boundary", 1), ("text", 1)]
    );
}

#[tokio::test]
async fn a_stream_without_its_usage_report_is_refused() {
    // Synthetic: the recorded shape, cut after `messageStop`, its connection closed cleanly.
    let mut body = event("messageStart", &json!({"role": "assistant"}));
    body.extend(event(
        "contentBlockDelta",
        &json!({"contentBlockIndex": 0, "delta": {"text": "Hi"}}),
    ));
    body.extend(event("contentBlockStop", &json!({"contentBlockIndex": 0})));
    body.extend(event("messageStop", &json!({"stopReason": "end_turn"})));
    let (origin, _server) = serve(success(EVENT_STREAM), body).await;
    let request = TurnRequest::new(NOVA, vec![Item::user("Hello")]);
    let (result, _) = turn(target(NOVA, true, origin), &request).await;
    let error = result.unwrap_err();
    assert_eq!(error.code, ErrorCode::Protocol);
    assert_eq!(error.dispatch, Dispatch::Accepted);
    assert!(!error.retriable);
    let observation = error.observation.expect("the failure keeps its binding");
    assert!(!observation.final_usage);
}

/// A synthetic answer: one text block, then `end_turn` and a `metadata` event reporting `usage`.
fn reporting(usage: &Value) -> Vec<u8> {
    let mut body = event("messageStart", &json!({"role": "assistant"}));
    body.extend(event(
        "contentBlockDelta",
        &json!({"contentBlockIndex": 0, "delta": {"text": "Hi"}}),
    ));
    body.extend(event("contentBlockStop", &json!({"contentBlockIndex": 0})));
    body.extend(event("messageStop", &json!({"stopReason": "end_turn"})));
    body.extend(event("metadata", &json!({ "usage": usage })));
    body
}

/// No Converse body this crate sends carries a `cachePoint`, so a cache counter the answer
/// leaves out is a known 0, and the whole input is `inputTokens` plus the cache counters the
/// answer does report. An answer without `inputTokens` still leaves the whole input unknown.
#[tokio::test]
async fn an_absent_cache_counter_is_a_known_zero_on_converse() {
    let usage = |input: Option<u64>, read: u64, written: u64| Usage {
        input_tokens: input,
        output_tokens: Some(1),
        cached_input_tokens: Some(read),
        cache_creation_input_tokens: Some(written),
        reasoning_output_tokens: None,
    };
    let cases = [
        (
            json!({"inputTokens": 3, "outputTokens": 1, "totalTokens": 4}),
            usage(Some(3), 0, 0),
        ),
        (
            json!({"inputTokens": 3, "outputTokens": 1, "cacheReadInputTokens": 5}),
            usage(Some(8), 5, 0),
        ),
        (
            json!({"inputTokens": 3, "outputTokens": 1, "cacheWriteInputTokenCount": 7}),
            usage(Some(10), 0, 7),
        ),
        (json!({"outputTokens": 1}), usage(None, 0, 0)),
    ];
    let request = TurnRequest::new(NOVA, vec![Item::user("Hello")]);
    for (reported, expected) in cases {
        let (origin, _server) = serve(success(EVENT_STREAM), reporting(&reported)).await;
        let (result, _) = turn(target(NOVA, true, origin), &request).await;
        let outcome = result.unwrap();
        assert_eq!(outcome.observation.usage, Some(expected), "{reported}");
    }
}

/// Every object key at every depth of `value`.
fn every_key(value: &Value) -> Vec<&str> {
    match value {
        Value::Object(fields) => fields
            .iter()
            .flat_map(|(key, inner)| std::iter::once(key.as_str()).chain(every_key(inner)))
            .collect(),
        Value::Array(items) => items.iter().flat_map(every_key).collect(),
        _ => Vec::new(),
    }
}

/// The Converse body never carries a `cachePoint`, whatever the turn holds and with the
/// prompt-cache hint set. That is why an absent cache counter is a known 0 in the usage a
/// Converse turn reports. A change that sends cache points fails here: it has to revisit that
/// rule in the usage decoder, and this test, together.
#[tokio::test]
async fn the_converse_body_never_carries_a_cache_point() {
    let mut hinted = capital_request();
    hinted.controls.hints.prompt_cache_key = Some("fixture-cache-key".to_owned());
    let mut conversation = tracing_request("Fetch an expert explanation for a concept");
    conversation.items.extend([
        Item::assistant("Looking it up."),
        call(
            "tooluse_first",
            "fetch_concept",
            json!({"concept": "spans"}),
        ),
        Item::ToolResult {
            call_id: CallId::new("tooluse_first").unwrap(),
            output: json!("spans are timed operations"),
            failed: false,
        },
    ]);
    let mut kept = greeting_request();
    kept.items.extend([
        greeting_reasoning(SONNET_4),
        Item::assistant(GREETING_TEXT),
        Item::user("Thank you"),
    ]);
    let turns = [
        (NOVA, capital_request()),
        (NOVA, hinted),
        (NOVA, temperature_request()),
        (SONNET_3, conversation),
        (SONNET_4, kept),
    ];
    for (model, request) in turns {
        let (_, _, sent) = replay(
            model,
            true,
            &request,
            include_bytes!("data/frames/pydantic-test_bedrock_model_stream.bin"),
        )
        .await;
        let body = sent.json();
        assert!(!every_key(&body).contains(&"cachePoint"), "{model}: {body}");
    }
}

/// Synthetic: `MAX_ITEMS` one-letter text blocks, then `last`, then the terminal events.
fn at_the_item_bound(last: &[u8]) -> Vec<u8> {
    let mut body = event("messageStart", &json!({"role": "assistant"}));
    for index in 0..MAX_ITEMS {
        body.extend(event(
            "contentBlockDelta",
            &json!({"contentBlockIndex": index, "delta": {"text": "a"}}),
        ));
        body.extend(event(
            "contentBlockStop",
            &json!({"contentBlockIndex": index}),
        ));
    }
    body.extend_from_slice(last);
    body.extend(event("messageStop", &json!({"stopReason": "end_turn"})));
    body.extend(event(
        "metadata",
        &json!({"usage": {"inputTokens": 3, "outputTokens": 1, "totalTokens": 4}}),
    ));
    body
}

/// A block that stops without having opened adds no item, so it does not count against the
/// item bound: a turn holding as many items as a turn may hold still ends with all of them. A
/// block that opens past the bound is still refused.
#[tokio::test]
async fn a_block_that_stops_without_opening_is_not_counted_against_the_item_bound() {
    let request = TurnRequest::new(NOVA, vec![Item::user("Hello")]);
    let unopened = event("contentBlockStop", &json!({"contentBlockIndex": MAX_ITEMS}));
    let (origin, _server) = serve(success(EVENT_STREAM), at_the_item_bound(&unopened)).await;
    let (result, events) = turn(target(NOVA, true, origin), &request).await;
    let outcome = result.expect("a turn at the item bound");
    assert_eq!(outcome.items.len(), MAX_ITEMS);
    // One text delta per block, each after the first preceded by a message boundary.
    assert_eq!(
        shape(&events)
            .iter()
            .filter(|(tag, _)| *tag == "text")
            .map(|(_, count)| count)
            .sum::<usize>(),
        MAX_ITEMS
    );
    assert_eq!(events.len(), 2 * MAX_ITEMS - 1);

    let opened = event(
        "contentBlockDelta",
        &json!({"contentBlockIndex": MAX_ITEMS, "delta": {"text": "a"}}),
    );
    let (origin, _server) = serve(success(EVENT_STREAM), at_the_item_bound(&opened)).await;
    let (result, _) = turn(target(NOVA, true, origin), &request).await;
    assert_eq!(result.unwrap_err().code, ErrorCode::TooLarge);
}

/// Runs a turn that the encoder must refuse, against an endpoint nothing listens on, and
/// returns the refusal and how often credentials were asked for.
async fn refused(target: BedrockTarget, request: &TurnRequest) -> (Error, usize) {
    let keys = Arc::new(Keys::default());
    let client = BedrockClient::new(target, http(), keys.clone()).unwrap();
    let mut sink = VecSink::new(16, 1 << 16);
    let error = client
        .turn(request, &mut sink, &Cancel::new())
        .await
        .unwrap_err();
    assert_eq!(sink.events(), []);
    (error, keys.0.load(Ordering::SeqCst))
}

/// Every turn the Converse encoder refuses as unsupported: its name, the model and the turn.
fn unsupported_turns() -> Vec<(&'static str, &'static str, TurnRequest)> {
    let mut cases: Vec<(&str, &str, TurnRequest)> = Vec::new();

    let request = TurnRequest::new(NOVA, vec![Item::assistant("Hello")]);
    cases.push(("assistant first", NOVA, request));

    let mut request = temperature_request();
    request.controls.parallel_tool_calls = Some(true);
    cases.push(("parallel tool calls", NOVA, request));

    let mut request = capital_request();
    request.controls.thinking = Some(Thinking::Enabled { budget_tokens: 512 });
    cases.push(("thinking on a model that is not Anthropic's", NOVA, request));

    let mut request = capital_request();
    request.items.extend([
        call("tooluse_a", "get_capital", json!({"country": "France"})),
        Item::ToolResult {
            call_id: CallId::new("tooluse_a").unwrap(),
            output: json!("Paris"),
            failed: false,
        },
    ]);
    cases.push(("tool items without tools", NOVA, request));

    let mut request = temperature_request();
    request.items.extend([
        call("call:1", "get_capital", json!({"country": "France"})),
        Item::ToolResult {
            call_id: CallId::new("call:1").unwrap(),
            output: json!("Paris"),
            failed: false,
        },
    ]);
    cases.push(("a call id outside the wire's characters", NOVA, request));

    let mut request = greeting_request();
    let mut foreign = provenance(SONNET_4);
    foreign.binding_revision = Id::new("fixture-2").unwrap();
    request.items.extend([
        Item::Opaque {
            provenance: foreign,
            payload: json!({"reasoningContent": {"reasoningText": {"text": "x"}}}),
        },
        Item::user("Again"),
    ]);
    cases.push(("another binding's state", SONNET_4, request));

    let mut request = greeting_request();
    request.items.extend([
        Item::Opaque {
            provenance: provenance(SONNET_4),
            payload: json!({"type": "thinking", "thinking": "x"}),
        },
        Item::user("Again"),
    ]);
    cases.push(("state in another wire's shape", SONNET_4, request));

    let mut request = temperature_request();
    request.tools.push(tool(
        "workspace.read",
        "Read a file.",
        json!({"type": "object"}),
    ));
    cases.push(("a tool name outside the wire's characters", NOVA, request));

    let mut request = temperature_request();
    request.items.extend([
        call("tooluse_a", "get_capital", json!("France")),
        Item::ToolResult {
            call_id: CallId::new("tooluse_a").unwrap(),
            output: json!("Paris"),
            failed: false,
        },
    ]);
    cases.push(("a tool input that is not an object", NOVA, request));

    // story:chat-counts-every-output-delta: Converse has no block for refusal text, and sending
    // it as text would make it the assistant's answer.
    let mut request = capital_request();
    request.items.extend([
        Item::AssistantRefusal {
            text: "I can't help with that.".to_owned(),
        },
        Item::user("Again"),
    ]);
    cases.push(("assistant refusal text", NOVA, request));

    cases
}

#[tokio::test]
async fn what_converse_cannot_carry_is_refused_before_credentials_or_io() {
    let nowhere = Url::parse("http://127.0.0.1:9/").unwrap();
    for (name, model, request) in unsupported_turns() {
        let (error, asked) = refused(target(model, true, nowhere.clone()), &request).await;
        assert_eq!(error.code, ErrorCode::Unsupported, "{name}");
        assert_eq!(error.dispatch, Dispatch::NotSent, "{name}");
        assert!(!error.retriable, "{name}");
        assert_eq!(asked, 0, "{name}");
    }
}

/// A turn inside the neutral request bound whose Converse body is outside it: Converse wraps
/// each of `MAX_TOOLS` tools in `toolSpec` and `inputSchema.json`, and the text fills the
/// neutral bound.
fn request_whose_converse_body_exceeds_the_bound() -> TurnRequest {
    let mut request = TurnRequest::new(NOVA, vec![Item::user("")]);
    request.tools = (0..MAX_TOOLS)
        .map(|n| tool(&format!("t{n}"), "d", json!({"type": "object"})))
        .collect();
    let overhead = encoded_len(&request, usize::MAX).expect("encodable");
    request.items = vec![Item::user("a".repeat(MAX_REQUEST_BYTES - overhead))];
    request
        .validate()
        .expect("the neutral request is inside its bound");
    request
}

#[tokio::test]
async fn the_pre_send_check_answers_exactly_what_the_turn_refuses() {
    let nowhere = Url::parse("http://127.0.0.1:9/").unwrap();
    let mut cases = unsupported_turns();

    let mut request = capital_request();
    request.sampling.reasoning_effort = Some("high".to_owned());
    cases.push(("a reasoning effort", NOVA, request));

    cases.push((
        "a body outside the request bound",
        NOVA,
        request_whose_converse_body_exceeds_the_bound(),
    ));

    for (name, model, request) in cases {
        let target = target(model, true, nowhere.clone());
        let checked = check_request(&request, &target).expect_err(name);
        assert_eq!(
            encode_request(&request, &target),
            Err(checked.clone()),
            "{name}"
        );
        let (refused, asked) = refused(target, &request).await;
        assert_eq!(checked, refused, "{name}");
        assert_eq!(asked, 0, "{name}");
        if name == "a body outside the request bound" {
            assert_eq!(checked.code, ErrorCode::TooLarge);
        }
    }

    for (model, request) in [(NOVA, capital_request()), (NOVA, temperature_request())] {
        let (_, _, sent) = replay(
            model,
            true,
            &request,
            include_bytes!("data/frames/pydantic-test_bedrock_model_stream.bin"),
        )
        .await;
        let target = target(model, true, nowhere.clone());
        assert_eq!(check_request(&request, &target), Ok(()));
        let body: Value = serde_json::from_slice(&encode_request(&request, &target).unwrap())
            .expect("the encoded body is JSON");
        assert_eq!(body, sent.json());
    }
}

// ---------------------------------------------------------------------------------------------
// story:bedrock-converse-empty-turn-with-an-unmodelled-stop-reason. A Converse answer that stops
// for a reason this projection does not model (any but `end_turn`, `tool_use` and `max_tokens`)
// and holds no content block and no kept event is a classified failure with its final usage; with
// a content block, an empty text block included, it is the incomplete turn under the reason's own
// name, as the Messages decoder counts it. Every frame and every whole answer below is synthetic.
// ---------------------------------------------------------------------------------------------

const STOPPED_AT_THE_CONTEXT_WINDOW: &str =
    "the Bedrock Converse turn stopped at the model context window without content";
const REFUSED_WITHOUT_CONTENT: &str = "the Bedrock Converse turn was refused without content";
const STOPPED_FOR_NO_ROW: &str = "the Bedrock Converse turn stopped without content for a reason \
                                  this projection does not model";

/// One row of the rule (`docs/bedrock.md`, "Retry classes"): the stop reason, the code a turn
/// without a content block fails as, whether another attempt at the stream may answer, the fixed
/// diagnostic.
type Stop = (String, ErrorCode, bool, &'static str);

fn stops_without_content() -> Vec<Stop> {
    let row = |reason: &str, code, message| (reason.to_owned(), code, false, message);
    let no_row = |reason: &str| {
        (
            reason.to_owned(),
            ErrorCode::Unavailable,
            true,
            STOPPED_FOR_NO_ROW,
        )
    };
    vec![
        row(
            "model_context_window_exceeded",
            ErrorCode::TooLarge,
            STOPPED_AT_THE_CONTEXT_WINDOW,
        ),
        row(
            "guardrail_intervened",
            ErrorCode::Refused,
            REFUSED_WITHOUT_CONTENT,
        ),
        row(
            "content_filtered",
            ErrorCode::Refused,
            REFUSED_WITHOUT_CONTENT,
        ),
        no_row("stop_sequence"),
        no_row("a_reason_in_no_list"),
        // The Messages wire's name for a refusal is not a Converse reason.
        no_row("refusal"),
        // A row is its exact name: another case, a byte more and a byte less are in no list.
        no_row("MODEL_CONTEXT_WINDOW_EXCEEDED"),
        no_row("guardrail_intervened_"),
        no_row("content_filtere"),
        // One byte, and the 64 bytes a reason may have.
        no_row("Z"),
        no_row(&"r".repeat(64)),
    ]
}

/// Synthetic: a stream that opens, carries `content`, stops for `stop` and reports `metadata`.
fn stopped(stop: &str, content: &[u8], metadata: &Value) -> Vec<u8> {
    let mut body = event("messageStart", &json!({"role": "assistant"}));
    body.extend_from_slice(content);
    body.extend(event("messageStop", &json!({"stopReason": stop})));
    body.extend(event("metadata", metadata));
    body
}

/// Synthetic: a whole answer of `content` that stops for `stop` and reports `usage`.
fn whole(stop: &str, content: &Value, usage: &Value) -> Vec<u8> {
    json!({"metrics": {"latencyMs": 7},
        "output": {"message": {"role": "assistant", "content": content}},
        "stopReason": stop, "usage": usage})
    .to_string()
    .into_bytes()
}

/// The counters of a synthetic answer that read `input` tokens and wrote `output`.
fn counted(input: u64, output: u64) -> Value {
    json!({"inputTokens": input, "outputTokens": output, "totalTokens": input + output})
}

/// One turn on Nova that publishes the tool the synthetic answers call, answered with `body`:
/// streamed in parts of `part` bytes, or whole.
async fn answered(
    stream: bool,
    part: usize,
    body: Vec<u8>,
) -> (Result<TurnOutcome, Error>, Vec<StreamEvent>) {
    let content_type = if stream {
        EVENT_STREAM
    } else {
        "application/json"
    };
    let (origin, _server) = serve_in(success(content_type), body, part).await;
    let mut request = TurnRequest::new(NOVA, vec![Item::user("Hello")]);
    request.tools = vec![tool(
        "lookup",
        "Look up a record",
        json!({"type": "object"}),
    )];
    turn(target(NOVA, stream, origin), &request).await
}

/// The answer failed as `stop`'s row says, kept `usage` as final and showed the caller nothing.
fn assert_stopped_without_content(
    (result, shown): (Result<TurnOutcome, Error>, Vec<StreamEvent>),
    case: &str,
    (reason, code, retriable, message): &Stop,
    usage: Option<&Usage>,
) {
    let error = match result {
        Err(error) => error,
        Ok(outcome) => {
            panic!("{case} `{reason}`: an answer without content is no answer: {outcome:?}")
        }
    };
    assert_eq!(
        (error.code, error.dispatch, error.refusal),
        (*code, Dispatch::Accepted, None),
        "{case} `{reason}`: {error:?}"
    );
    assert_eq!(
        (error.retriable, error.may_retry()),
        (*retriable, *retriable),
        "{case} `{reason}`: {error:?}"
    );
    // Fixed text: the same for every reason of a row, and no byte of a long reason anywhere.
    assert_eq!(error.message, *message, "{case} `{reason}`");
    assert!(
        reason.len() < 13 || !format!("{error:?}").contains(reason.as_str()),
        "{case} `{reason}`: {error:?}"
    );
    let observation = error
        .observation
        .as_deref()
        .unwrap_or_else(|| panic!("{case} `{reason}`: the failure keeps what was reported"));
    assert_eq!(observation.binding, provenance(NOVA), "{case} `{reason}`");
    assert_eq!(observation.usage.as_ref(), usage, "{case} `{reason}`");
    assert!(observation.final_usage, "{case} `{reason}`");
    assert_eq!(shown, [] as [StreamEvent; 0], "{case} `{reason}`");
}

/// Each row by its own reason name, the stream written whole, in parts and one byte at a time.
#[tokio::test]
async fn every_unmodelled_reason_without_a_content_block_fails_the_stream_as_its_row_however_it_is_fed()
 {
    for (input, stop) in (10_u64..).zip(stops_without_content()) {
        let body = stopped(&stop.0, &[], &json!({"usage": counted(input, 0)}));
        for (fed, part) in [
            ("in one write", body.len()),
            ("in parts of 61 bytes", 61),
            ("one byte at a time", 1),
        ] {
            assert_stopped_without_content(
                answered(true, part, body.clone()).await,
                fed,
                &stop,
                Some(&uncached(input, 0)),
            );
        }
    }
}

/// A whole answer keeps each row's code, text and usage. `BedrockClient` makes every failure of
/// a whole answer final, so no row may be retried there (`tests/client.rs` holds the client's
/// side of it).
#[tokio::test]
async fn every_unmodelled_reason_without_a_content_block_fails_a_whole_answer_as_its_row_and_final()
{
    for (input, (reason, code, _, message)) in (40_u64..).zip(stops_without_content()) {
        let body = whole(&reason, &json!([]), &counted(input, 0));
        assert_stopped_without_content(
            answered(false, body.len(), body).await,
            "whole",
            &(reason, code, false, message),
            Some(&uncached(input, 0)),
        );
    }
}

/// A closed block: the frames that stream it with the item they leave, and the block a whole
/// answer holds with the item it leaves.
struct Closed {
    name: &'static str,
    streamed: (Vec<u8>, Item),
    whole: (Value, Item),
}

fn opaque(payload: Value) -> Item {
    Item::Opaque {
        provenance: provenance(NOVA),
        payload,
    }
}

/// Every kind of block but an empty text block: each leaves an item when it closes.
fn blocks_that_leave_an_item() -> Vec<Closed> {
    let delta = |delta: Value| {
        event(
            "contentBlockDelta",
            &json!({"contentBlockIndex": 0, "delta": delta}),
        )
    };
    let start = |start: Value| {
        event(
            "contentBlockStart",
            &json!({"contentBlockIndex": 0, "start": start}),
        )
    };
    let stop = || event("contentBlockStop", &json!({"contentBlockIndex": 0}));
    let reasoning =
        |text: &str| opaque(json!({"reasoningContent": {"reasoningText": {"text": text}}}));
    let reasoned = |name, text: &str| Closed {
        name,
        streamed: (
            [delta(json!({"reasoningContent": {"text": text}})), stop()].concat(),
            reasoning(text),
        ),
        whole: (
            json!({"reasoningContent": {"reasoningText": {"text": text}}}),
            reasoning(text),
        ),
    };
    let called = |name, id: &str, input: &str| {
        let arguments: Value = serde_json::from_str(input).unwrap();
        let begun = start(json!({"toolUse": {"toolUseId": id, "name": "lookup"}}));
        let frames = if input == "{}" {
            // A tool use that streams no input took none.
            [begun, stop()].concat()
        } else {
            [begun, delta(json!({"toolUse": {"input": input}})), stop()].concat()
        };
        Closed {
            name,
            streamed: (frames, call(id, "lookup", arguments.clone())),
            whole: (
                json!({"toolUse": {"toolUseId": id, "name": "lookup", "input": arguments}}),
                call(id, "lookup", arguments),
            ),
        }
    };
    let future = json!({"futureBlock": {"detail": 1}});
    vec![
        Closed {
            name: "a text block",
            streamed: (
                [delta(json!({"text": "Hi"})), stop()].concat(),
                Item::assistant("Hi"),
            ),
            whole: (json!({"text": "Hi"}), Item::assistant("Hi")),
        },
        reasoned("a reasoning block", "Weighing it."),
        // No event shows a reasoning block without text, and it still leaves its item.
        reasoned("a reasoning block without text", ""),
        called("a tool call", "tool_1", r#"{"id":7}"#),
        called("a tool call without input", "tool_2", "{}"),
        Closed {
            name: "a block outside the subset",
            streamed: (
                [start(future.clone()), stop()].concat(),
                opaque(json!({"contentBlockIndex": 0, "start": future.clone()})),
            ),
            whole: (future.clone(), opaque(future)),
        },
    ]
}

/// The turn is the incomplete turn under `reason`'s own name, holding exactly `item`.
fn assert_incomplete_with(
    (result, _): (Result<TurnOutcome, Error>, Vec<StreamEvent>),
    case: &str,
    reason: &str,
    item: &Item,
) {
    let outcome = result.unwrap_or_else(|error| panic!("{case} `{reason}`: {error:?}"));
    assert_eq!(
        outcome.stop_reason,
        StopReason::Incomplete {
            reason: reason.to_owned()
        },
        "{case}"
    );
    assert_eq!(
        outcome.items,
        std::slice::from_ref(item),
        "{case} `{reason}`"
    );
    assert_eq!(
        (outcome.observation.usage, outcome.observation.final_usage),
        (Some(uncached(3, 2)), true),
        "{case} `{reason}`"
    );
}

/// With an item nothing changed from the base: every reason of the table, after every kind of
/// closed block, streamed and whole, is `StopReason::Incomplete` under its own name and keeps
/// the item.
#[tokio::test]
async fn every_unmodelled_reason_with_an_item_is_still_the_incomplete_turn() {
    for (reason, ..) in stops_without_content() {
        for block in blocks_that_leave_an_item() {
            let (frames, item) = &block.streamed;
            let body = stopped(&reason, frames, &json!({"usage": counted(3, 2)}));
            let streamed = answered(true, 61, body).await;
            assert_incomplete_with(streamed, block.name, &reason, item);

            let (content, item) = &block.whole;
            let body = whole(&reason, &json!([content]), &counted(3, 2));
            let read = answered(false, body.len(), body).await;
            assert_incomplete_with(read, block.name, &reason, item);
        }
        // An event the stream decoder kept whole is an item too, and the caller was told.
        let kept = event("futureEvent", &json!({"detail": 2}));
        let body = stopped(&reason, &kept, &json!({"usage": counted(3, 2)}));
        let (result, shown) = answered(true, 61, body).await;
        assert_eq!(shape(&shown), vec![("warning", 1)], "`{reason}`");
        assert_incomplete_with(
            (result, shown),
            "a kept event",
            &reason,
            &opaque(json!({"detail": 2})),
        );
    }
}

/// The edge of the rule: a content block that left no item. A text block that carried no text
/// leaves none and shows the caller no event, yet the model opened a block, so the answer is
/// not "without content": it is the incomplete turn it was before the rule, with no item, on
/// every row. A `contentBlockStop` for a block that never opened is this wire's form of a text
/// block that took no delta, and counts the same.
///
/// The Messages decoder counts an empty text block as content, and Converse counts as it does:
/// a failure here would be marked for another attempt after the model had started answering.
#[tokio::test]
async fn after_an_empty_text_block_the_turn_is_incomplete_with_no_item_and_shows_nothing() {
    let delta = |index: u64| {
        event(
            "contentBlockDelta",
            &json!({"contentBlockIndex": index, "delta": {"text": ""}}),
        )
    };
    let stop = |index: u64| event("contentBlockStop", &json!({"contentBlockIndex": index}));
    let streams = [
        ("one empty text block", [delta(0), stop(0)].concat()),
        (
            "a text block of two empty deltas",
            [delta(0), delta(0), stop(0)].concat(),
        ),
        (
            "two empty text blocks",
            [delta(0), stop(0), delta(1), stop(1)].concat(),
        ),
        ("a block that stops without having opened", stop(0)),
    ];
    let wholes = [
        ("one empty text block, whole", json!([{"text": ""}])),
        (
            "two empty text blocks, whole",
            json!([{"text": ""}, {"text": ""}]),
        ),
    ];
    let incomplete = |(result, shown): (Result<TurnOutcome, Error>, Vec<StreamEvent>),
                      case: &str,
                      reason: &str,
                      input: u64| {
        let outcome = result.unwrap_or_else(|error| panic!("{case} `{reason}`: {error:?}"));
        assert_eq!(
            outcome.stop_reason,
            StopReason::Incomplete {
                reason: reason.to_owned()
            },
            "{case}"
        );
        assert_eq!(outcome.items, [] as [Item; 0], "{case} `{reason}`");
        assert_eq!(
            (outcome.observation.usage, outcome.observation.final_usage),
            (Some(uncached(input, 0)), true),
            "{case} `{reason}`"
        );
        assert_eq!(shown, [] as [StreamEvent; 0], "{case} `{reason}`");
    };
    for (reason, ..) in stops_without_content() {
        for (input, (case, content)) in (60_u64..).zip(&streams) {
            let body = stopped(&reason, content, &json!({"usage": counted(input, 0)}));
            incomplete(answered(true, 61, body).await, case, &reason, input);
        }
        for (input, (case, content)) in (70_u64..).zip(&wholes) {
            let body = whole(&reason, content, &counted(input, 0));
            let read = answered(false, body.len(), body).await;
            incomplete(read, case, &reason, input);
        }
    }
}

/// Usage on the failure is what `read_usage` makes of the answer, as on every Converse answer:
/// a `metadata` event or a whole answer without counters attaches no usage, one without
/// `inputTokens` leaves the input unknown, and a cache counter the answer left out beside
/// another counter is the known 0 it is on this wire (`docs/bedrock.md`, "Usage").
#[tokio::test]
async fn usage_on_the_failure_is_unknown_where_the_answer_left_it_out_and_a_cache_counter_a_known_zero()
 {
    let without_input = Usage {
        input_tokens: None,
        output_tokens: Some(0),
        cached_input_tokens: Some(0),
        cache_creation_input_tokens: Some(0),
        reasoning_output_tokens: None,
    };
    let only_a_cache_read = Usage {
        input_tokens: None,
        output_tokens: None,
        cached_input_tokens: Some(4),
        cache_creation_input_tokens: Some(0),
        reasoning_output_tokens: None,
    };
    let reports = [
        ("no usage", None, None),
        ("a null usage", Some(Value::Null), None),
        ("usage without a counter", Some(json!({})), None),
        (
            "usage without an input count",
            Some(json!({"outputTokens": 0})),
            Some(&without_input),
        ),
        (
            "usage of a cache read count only",
            Some(json!({"cacheReadInputTokens": 4})),
            Some(&only_a_cache_read),
        ),
    ];
    let rows = stops_without_content();
    for (case, usage, expected) in reports {
        for (reason, code, retriable, message) in [&rows[0], &rows[1], &rows[3]] {
            let mut metadata = json!({"metrics": {"latencyMs": 5}});
            let mut answer = json!({"output": {"message": {"role": "assistant", "content": []}},
                "stopReason": reason});
            if let Some(usage) = &usage {
                metadata["usage"] = usage.clone();
                answer["usage"] = usage.clone();
            }
            assert_stopped_without_content(
                answered(true, 61, stopped(reason, &[], &metadata)).await,
                case,
                &(reason.clone(), *code, *retriable, *message),
                expected,
            );
            let body = answer.to_string().into_bytes();
            assert_stopped_without_content(
                answered(false, body.len(), body).await,
                case,
                &(reason.clone(), *code, false, *message),
                expected,
            );
        }
    }
}

/// The decoder takes `metadata` before `messageStop` as well as after it, and the failure is
/// the same in both orders.
#[tokio::test]
async fn metadata_before_the_stop_gives_the_same_failure_as_metadata_after_it() {
    for (input, stop) in (80_u64..).zip(stops_without_content().into_iter().take(5)) {
        let mut body = event("messageStart", &json!({"role": "assistant"}));
        body.extend(event("metadata", &json!({"usage": counted(input, 0)})));
        body.extend(event("messageStop", &json!({"stopReason": &stop.0})));
        assert_stopped_without_content(
            answered(true, 61, body).await,
            "metadata first",
            &stop,
            Some(&uncached(input, 0)),
        );
    }
}

/// The three reasons this projection models are not in the rule: an answer without content that
/// stops for one of them is what it was, streamed and whole. `end_turn` and `max_tokens` are the
/// outcome with no item; `tool_use` without a call is the `protocol` refusal of a stop that
/// contradicts its tool obligations, final, and no row of the rule.
#[tokio::test]
async fn a_modelled_reason_without_an_item_is_what_it_was() {
    for (reported, expected) in [
        ("end_turn", Some(StopReason::EndTurn)),
        ("tool_use", None),
        ("max_tokens", Some(StopReason::MaxOutputTokens)),
    ] {
        let streamed = stopped(reported, &[], &json!({"usage": counted(5, 0)}));
        let read = whole(reported, &json!([]), &counted(5, 0));
        for (case, answer) in [
            ("stream", answered(true, 61, streamed).await),
            ("whole", answered(false, read.len(), read).await),
        ] {
            let (result, shown) = answer;
            assert_eq!(shown, [] as [StreamEvent; 0], "{case} `{reported}`");
            let Some(expected) = &expected else {
                let error = result.expect_err("a stop for tools that names no call");
                assert_eq!(
                    (error.code, error.dispatch, error.may_retry()),
                    (ErrorCode::Protocol, Dispatch::Accepted, false),
                    "{case} `{reported}`: {error:?}"
                );
                assert_eq!(
                    error.message, "model terminal reason contradicts its tool obligations",
                    "{case} `{reported}`"
                );
                continue;
            };
            let outcome = result.unwrap_or_else(|error| panic!("{case} `{reported}`: {error:?}"));
            assert_eq!(&outcome.stop_reason, expected, "{case}");
            assert_eq!(outcome.items, [] as [Item; 0], "{case} `{reported}`");
            assert_eq!(
                outcome.observation.usage,
                Some(uncached(5, 0)),
                "{case} `{reported}`"
            );
        }
    }
}

/// What the decoder refused before the rule it still refuses, as `protocol` and final: a block
/// left open at the stop, and a reason that is not a bounded name. None of them is the stop
/// without content.
#[tokio::test]
async fn a_block_left_open_or_a_reason_that_is_no_bounded_name_is_still_the_protocol_refusal() {
    let open_tool = event(
        "contentBlockStart",
        &json!({"contentBlockIndex": 0,
            "start": {"toolUse": {"toolUseId": "tool_1", "name": "lookup"}}}),
    );
    let open_text = event(
        "contentBlockDelta",
        &json!({"contentBlockIndex": 0, "delta": {"text": ""}}),
    );
    let metadata = json!({"usage": counted(3, 0)});
    let past_the_bound = "r".repeat(65);
    let mut refused = vec![
        (
            "a tool use block left open",
            answered(true, 61, stopped("stop_sequence", &open_tool, &metadata)).await,
        ),
        (
            "an empty text block left open",
            answered(
                true,
                61,
                stopped("guardrail_intervened", &open_text, &metadata),
            )
            .await,
        ),
    ];
    for reason in [past_the_bound.as_str(), "", "stop sequence"] {
        let read = whole(reason, &json!([]), &counted(3, 0));
        refused.push((
            "a streamed reason that is no bounded name",
            answered(true, 61, stopped(reason, &[], &metadata)).await,
        ));
        refused.push((
            "a whole answer's reason that is no bounded name",
            answered(false, read.len(), read).await,
        ));
    }
    for (case, (result, _)) in refused {
        let error = result.expect_err(case);
        assert_eq!(
            (error.code, error.dispatch),
            (ErrorCode::Protocol, Dispatch::Accepted),
            "{case}: {error:?}"
        );
        assert!(!error.retriable && !error.may_retry(), "{case}: {error:?}");
        assert!(
            error.message.starts_with("Bedrock Converse "),
            "{case}: {}",
            error.message
        );
    }
}
