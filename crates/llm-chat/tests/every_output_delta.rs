//! story:chat-counts-every-output-delta. A Chat Completions stream counts every delta that carries
//! text as output, carries each such field as its own kind, and returns a complete turn once
//! `[DONE]` arrives.
//!
//! - `delta.refusal` text reaches the caller as `StreamEvent::RefusalDelta` and the turn as
//!   `Item::AssistantRefusal`, never in the assistant text.
//! - `delta.reasoning`, the name some servers give `reasoning_content`, is a
//!   `StreamEvent::ReasoningDelta`. Both keys non-empty with different text in one delta is
//!   refused by the field's path; the same text is carried once.
//! - A cut after either is final; a cut after a chunk that carries no text stays retriable.
//! - A body dropped after `[DONE]` returns the turn, as Messages does after `message_stop`.
//!
//! Local sockets only.
mod common;

use common::{binding, binding_at};
use llm_chat::{ChatClient, IngressStream, encode_ingress_completion, project_response_bytes};
use llm_core::{
    Cancel, Capabilities, Dispatch, Error, ErrorCode, Item, Model, Protocol, StopReason,
    StreamEvent, TurnOutcome, TurnRequest, VecSink,
};
use llm_credentials::{ResolvedSecret, SecretError, SecretRef, SecretResolver};
use llm_http::{HttpClient, Limits};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const REFUSAL_AND_REASONING: &[u8] = include_bytes!("../fixtures/openai-refusal-and-reasoning.sse");

const SSE_HEAD: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";

/// A chunked stream head: a chunk the server stops writing is a body that failed in transit.
const CHUNKED_HEAD: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n";

/// The start of the next event, cut before its payload ends.
const PARTIAL: &str = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"lo";

/// One streamed chunk whose only choice carries `delta` and `finish_reason`.
fn chunk(delta: &str, finish_reason: &str) -> String {
    format!(
        "data: {{\"id\":\"chatcmpl-every\",\"object\":\"chat.completion.chunk\",\"model\":\"Qwen/Qwen3-8B\",\"choices\":[{{\"index\":0,\"delta\":{delta},\"finish_reason\":{finish_reason}}}]}}\n\n"
    )
}

fn refusal(text: &str) -> StreamEvent {
    StreamEvent::RefusalDelta {
        text: text.to_owned(),
    }
}

fn reasoning(text: &str) -> StreamEvent {
    StreamEvent::ReasoningDelta {
        text: text.to_owned(),
    }
}

/// An anonymous binding never reaches a resolver; this one proves it by refusing.
struct NoResolver;
impl SecretResolver for NoResolver {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> llm_core::BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async { Err(SecretError::Missing) })
    }
}

/// Answers one request with `head` and `body`, closes, and returns the turn and what the caller
/// was shown.
async fn turn(head: &'static [u8], body: String) -> (Result<TurnOutcome, Error>, VecSink) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local port");
    let url = format!("http://{}/v1", listener.local_addr().expect("an address"));
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("an accepted connection");
        let mut seen = Vec::new();
        let mut buffer = vec![0; 1 << 16];
        while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
            let length = socket.read(&mut buffer).await.expect("a readable socket");
            assert!(length > 0, "the client closed before sending a request");
            seen.extend_from_slice(&buffer[..length]);
        }
        socket.write_all(head).await.expect("head written");
        // The client may stop reading once it has its answer, so a refused write is not a
        // failure of the fixture.
        let _ = socket.write_all(body.as_bytes()).await;
        let _ = socket.shutdown().await;
    });
    let client = ChatClient::new(
        binding_at(
            Protocol::ChatCompletions,
            Capabilities::text(32_768, 4_096),
            &url,
        ),
        HttpClient::new(Limits::default()).expect("bounded transport"),
        Arc::new(NoResolver),
    );
    let mut sink = VecSink::new(64, 64 * 1024);
    let outcome = client
        .turn(
            &TurnRequest::new("small", vec![Item::user("Hallo")]),
            &mut sink,
            &Cancel::new(),
        )
        .await;
    server.await.expect("the fixture server");
    (outcome, sink)
}

/// The two ways a connection is cut mid-turn, each read by the transport as retriable.
#[derive(Clone, Copy, Debug)]
enum Cut {
    /// The body ends inside an event: `protocol`.
    InsideAnEvent,
    /// A chunked body ends inside a chunk: `transport`.
    InsideAChunk,
}

impl Cut {
    const ALL: [Self; 2] = [Self::InsideAnEvent, Self::InsideAChunk];

    const fn code(self) -> ErrorCode {
        match self {
            Self::InsideAnEvent => ErrorCode::Protocol,
            Self::InsideAChunk => ErrorCode::Transport,
        }
    }

    /// Serves `before` whole, then cuts the connection inside the next event.
    async fn turn(self, before: &str) -> (Error, VecSink) {
        let (outcome, sink) = match self {
            Self::InsideAnEvent => turn(SSE_HEAD, format!("{before}{PARTIAL}")).await,
            Self::InsideAChunk => turn(CHUNKED_HEAD, chunked_then_cut(before)).await,
        };
        (outcome.expect_err("a cut turn is a refusal"), sink)
    }
}

/// `before` as one whole chunk, then a chunk that promises 1024 bytes and carries a few.
fn chunked_then_cut(before: &str) -> String {
    // A zero-length chunk would end the body cleanly, so an empty prefix sends none.
    let whole = if before.is_empty() {
        String::new()
    } else {
        format!("{:x}\r\n{before}\r\n", before.len())
    };
    format!("{whole}400\r\n{PARTIAL}")
}

/// The opening chunk, as `fixtures/openai-text-and-usage.sse` pins it.
fn opening() -> String {
    chunk(
        r#"{"role":"assistant","content":"","refusal":null}"#,
        "null",
    )
}

/// Chunks that carry no text: after any of them, nothing was answered yet.
fn silent() -> Vec<(&'static str, String)> {
    vec![
        ("only role", String::new()),
        ("an empty tool_calls array", chunk(r#"{"tool_calls":[]}"#, "null")),
        (
            "empty content with a null finish reason",
            chunk(r#"{"content":""}"#, "null"),
        ),
        (
            "a null reasoning_content",
            chunk(r#"{"reasoning_content":null}"#, "null"),
        ),
        ("a null refusal", chunk(r#"{"refusal":null}"#, "null")),
        ("an empty refusal", chunk(r#"{"refusal":""}"#, "null")),
        ("a null reasoning", chunk(r#"{"reasoning":null}"#, "null")),
        ("an empty reasoning", chunk(r#"{"reasoning":""}"#, "null")),
        (
            "a usage-only chunk",
            "data: {\"id\":\"chatcmpl-every\",\"object\":\"chat.completion.chunk\",\"choices\":[],\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":0}}\n\n".to_owned(),
        ),
    ]
}

/// A cut after nothing but chunks that carry no text keeps the transport's retry class.
#[tokio::test]
async fn a_cut_after_chunks_that_carry_no_text_stays_retriable() {
    for cut in Cut::ALL {
        for (label, chunk) in silent() {
            let (error, sink) = cut.turn(&format!("{}{chunk}", opening())).await;
            let case = format!("{cut:?} after {label}");
            assert_eq!(
                (error.code, error.dispatch),
                (cut.code(), Dispatch::Accepted),
                "{case}"
            );
            assert!(error.may_retry(), "{case}: nothing was answered: {error:?}");
            assert_eq!(sink.events(), [], "{case}");
        }
    }
}

/// A cut after a non-empty `refusal` or `reasoning` delta is final: the model answered, and the
/// caller was shown it under its own kind.
#[tokio::test]
async fn a_cut_after_a_refusal_or_reasoning_delta_is_final() {
    for cut in Cut::ALL {
        for (label, chunk, shown) in [
            (
                "a refusal delta",
                chunk(r#"{"refusal":"I can't"}"#, "null"),
                refusal("I can't"),
            ),
            (
                "a reasoning delta",
                chunk(r#"{"reasoning":"Weighing it"}"#, "null"),
                reasoning("Weighing it"),
            ),
        ] {
            let (error, sink) = cut.turn(&format!("{}{chunk}", opening())).await;
            let case = format!("{cut:?} after {label}");
            assert_eq!(
                (error.code, error.dispatch),
                (cut.code(), Dispatch::Accepted),
                "{case}"
            );
            assert!(
                !error.retriable && !error.may_retry(),
                "{case}: an answered attempt cannot replay: {error:?}"
            );
            assert_eq!(sink.events(), [shown], "{case}");
            assert!(error.observation.is_some(), "{case}");
        }
    }
}

/// Refusal text is its own kind on the stream and in the turn, and never assistant text;
/// `reasoning` is a reasoning delta.
#[test]
fn refusal_and_reasoning_text_reach_the_caller_as_their_own_kinds() {
    let binding = binding();
    let (events, outcome) = project_response_bytes(REFUSAL_AND_REASONING, binding.provenance());
    let outcome = outcome.expect("projected");
    assert_eq!(
        events,
        vec![
            reasoning("The request asks for"),
            reasoning(" something I decline."),
            refusal("I can't"),
            refusal(" help with that."),
        ]
    );
    assert_eq!(
        outcome.items,
        vec![Item::AssistantRefusal {
            text: "I can't help with that.".to_owned()
        }]
    );
    assert!(
        !outcome
            .items
            .iter()
            .any(|item| matches!(item, Item::AssistantText { .. })),
        "refusal text never joins the assistant text"
    );
    assert_eq!(outcome.stop_reason, StopReason::EndTurn);
}

/// Text and refusal in one turn stay apart, each in its own item.
#[test]
fn text_and_refusal_in_one_turn_stay_apart() {
    let binding = binding();
    let body = format!(
        "{}{}{}{}data: [DONE]\n\n",
        chunk(r#"{"content":"Partly: "}"#, "null"),
        chunk(r#"{"refusal":"no more."}"#, "null"),
        chunk(r#"{"content":"done"}"#, "null"),
        chunk("{}", "\"stop\""),
    );
    let (events, outcome) = project_response_bytes(body.as_bytes(), binding.provenance());
    let outcome = outcome.expect("projected");
    assert_eq!(
        events,
        vec![
            StreamEvent::TextDelta {
                text: "Partly: ".to_owned()
            },
            refusal("no more."),
            StreamEvent::TextDelta {
                text: "done".to_owned()
            },
        ]
    );
    assert_eq!(
        outcome.items,
        vec![
            Item::assistant("Partly: done"),
            Item::AssistantRefusal {
                text: "no more.".to_owned()
            },
        ]
    );
}

/// `reasoning` and `reasoning_content` with the same text are one delta; with different text the
/// wire gives no way to tell which is the reasoning, and the delta is refused by its path.
#[test]
fn reasoning_under_both_names_is_carried_once_or_refused_by_its_path() {
    let binding = binding();
    let same = format!(
        "{}{}data: [DONE]\n\n",
        chunk(
            r#"{"reasoning":"Checking","reasoning_content":"Checking"}"#,
            "null"
        ),
        chunk("{}", "\"stop\""),
    );
    let (events, outcome) = project_response_bytes(same.as_bytes(), binding.provenance());
    assert_eq!(events, vec![reasoning("Checking")]);
    assert_eq!(outcome.expect("projected").items, vec![]);

    // One name empty or null is the other name alone.
    for delta in [
        r#"{"reasoning":"","reasoning_content":"Checking"}"#,
        r#"{"reasoning":"Checking","reasoning_content":null}"#,
    ] {
        let body = format!(
            "{}{}data: [DONE]\n\n",
            chunk(delta, "null"),
            chunk("{}", "\"stop\"")
        );
        let (events, outcome) = project_response_bytes(body.as_bytes(), binding.provenance());
        assert_eq!(events, vec![reasoning("Checking")], "{delta}");
        outcome.expect(delta);
    }

    let different = format!(
        "{}data: [DONE]\n\n",
        chunk(
            r#"{"reasoning":"SECRET-ONE","reasoning_content":"SECRET-TWO"}"#,
            "null"
        ),
    );
    let (events, outcome) = project_response_bytes(different.as_bytes(), binding.provenance());
    let error = outcome.expect_err("two different reasonings in one delta are refused");
    assert_eq!(events, vec![]);
    assert_eq!(
        (error.code, error.dispatch),
        (ErrorCode::Protocol, Dispatch::Accepted)
    );
    assert!(
        error.message.contains("choices[].delta.reasoning"),
        "the refusal names the field: {error:?}"
    );
    assert!(!format!("{error:?}").contains("SECRET-"), "{error:?}");
}

/// A body dropped after `[DONE]` returns the complete turn: the stream had already said it was
/// whole, as Messages returns after its terminal event.
#[tokio::test]
async fn a_body_dropped_after_done_returns_the_complete_turn() {
    let whole = format!(
        "{}{}{}data: [DONE]\n\n",
        opening(),
        chunk(r#"{"content":"Hello"}"#, "null"),
        chunk("{}", "\"stop\""),
    );
    let (outcome, sink) = turn(CHUNKED_HEAD, chunked_then_cut(&whole)).await;
    let outcome = outcome.expect("a turn whose [DONE] arrived is complete");
    assert_eq!(outcome.items, vec![Item::assistant("Hello")]);
    assert_eq!(outcome.stop_reason, StopReason::EndTurn);
    assert!(outcome.observation.final_usage);
    assert_eq!(sink.text(), "Hello");
}

/// The gateway answers on this wire with the refusal under the wire's own `refusal` key, streamed
/// and not, and the projection reads each back as the refusal it was.
#[test]
fn the_ingress_encoders_carry_refusal_text_under_the_refusal_key() {
    let binding = binding();
    let (_, outcome) = project_response_bytes(REFUSAL_AND_REASONING, binding.provenance());
    let outcome = outcome.expect("projected");

    let body = encode_ingress_completion(&outcome, "chatcmpl-x", 1).expect("encoded");
    let message = &body["choices"][0]["message"];
    assert_eq!(message["refusal"], json!("I can't help with that."));
    assert_eq!(message["content"], Value::Null);

    let mut stream = IngressStream::new("chatcmpl-x", 1);
    let mut chunks = Vec::new();
    for text in ["I can't", " help with that."] {
        let chunk = stream.chunk(&refusal(text)).expect("a refusal chunk");
        assert_eq!(chunk["choices"][0]["delta"], json!({"refusal": text}));
        chunks.push(chunk);
    }
    chunks.extend(stream.close(&outcome, true).expect("closed"));
    let mut frames: Vec<String> = chunks
        .iter()
        .map(|chunk| format!("data: {chunk}"))
        .collect();
    frames.push("data: [DONE]".to_owned());
    let bytes = frames.join("\n\n") + "\n\n";
    let (events, read_back) = project_response_bytes(bytes.as_bytes(), binding.provenance());
    assert_eq!(
        events,
        vec![refusal("I can't"), refusal(" help with that.")]
    );
    assert_eq!(read_back.expect("read back").items, outcome.items);
}

/// A turn whose history carries a refusal sends it back in the assistant message's own `refusal`
/// field, beside its content and never inside it; a non-streamed body is read the same way.
#[test]
fn refusal_text_in_the_history_is_sent_under_the_refusal_key_and_read_back_from_a_completion() {
    let request = TurnRequest::new(
        "small",
        vec![
            Item::user("Help?"),
            Item::assistant("Partly."),
            Item::AssistantRefusal {
                text: "Not the rest.".to_owned(),
            },
            Item::user("Again"),
        ],
    );
    let body = llm_chat::project_request(&request, &binding(), true).expect("projected");
    assert_eq!(
        body["messages"][1],
        json!({"role": "assistant", "content": "Partly.", "refusal": "Not the rest."})
    );
    // Gateway ingress reads back what was sent: the refusal is the refusal it was.
    let decoded = llm_chat::decode_ingress_request(&body).unwrap_or_else(|error| {
        panic!(
            "story:chat-counts-every-output-delta: gateway ingress reads an assistant refusal: {}",
            error.message
        )
    });
    assert_eq!(
        decoded.request.items, request.items,
        "story:chat-counts-every-output-delta: the refusal reads back as Item::AssistantRefusal"
    );

    let completion = json!({
        "id": "chatcmpl-x", "object": "chat.completion", "model": "Qwen/Qwen3-8B",
        "choices": [{"index": 0, "finish_reason": "stop",
            "message": {"role": "assistant", "content": null, "refusal": "Not the rest."}}],
    });
    let outcome = llm_chat::decode_completion(&completion, binding().provenance()).expect("read");
    assert_eq!(
        outcome.items,
        vec![Item::AssistantRefusal {
            text: "Not the rest.".to_owned()
        }]
    );
}

/// A refused message without text goes out as `"content": null` beside `refusal`, the shape the
/// wire returns for it, and gateway ingress reads it back; a `refusal` there that is not a string
/// is refused by its path.
#[test]
fn a_refusal_only_message_reads_back_and_a_refusal_that_is_not_a_string_is_refused_by_its_path() {
    let request = TurnRequest::new(
        "small",
        vec![
            Item::user("Help?"),
            Item::AssistantRefusal {
                text: "No.".to_owned(),
            },
            Item::user("Why not?"),
        ],
    );
    let body = llm_chat::project_request(&request, &binding(), false).expect("projected");
    assert_eq!(
        body["messages"][1],
        json!({"role": "assistant", "content": null, "refusal": "No."})
    );
    let decoded = llm_chat::decode_ingress_request(&body).expect("read back");
    assert_eq!(decoded.request.items, request.items);

    for refusal in [json!(7), json!({"text": "No."}), json!(["No."])] {
        let mut body = body.clone();
        body["messages"][1]["refusal"] = refusal.clone();
        let error = llm_chat::decode_ingress_request(&body).expect_err("not a string");
        assert_eq!(
            (error.code, error.message.as_str()),
            (
                ErrorCode::InvalidRequest,
                "chat request field `messages[].refusal` is not a string"
            ),
            "{refusal}"
        );
    }
    let mut body = body;
    body["messages"][1]["refusal"] = Value::Null;
    let decoded = llm_chat::decode_ingress_request(&body).expect("null is absent");
    assert_eq!(
        decoded.request.items,
        vec![Item::user("Help?"), Item::user("Why not?")]
    );
}

/// A `refusal`, `reasoning` or `reasoning_content` that is present, not `null` and not a string
/// is refused naming the field by its path, and never its value.
#[test]
fn a_text_field_that_is_not_a_string_is_refused_by_its_path() {
    let binding = binding();
    for (delta, path) in [
        (r#"{"refusal":7}"#, "choices[].delta.refusal"),
        (
            r#"{"reasoning":{"t":"SECRET-X"}}"#,
            "choices[].delta.reasoning",
        ),
        (
            r#"{"reasoning_content":["SECRET-X"]}"#,
            "choices[].delta.reasoning_content",
        ),
    ] {
        let body = format!("{}data: [DONE]\n\n", chunk(delta, "null"));
        let (events, outcome) = project_response_bytes(body.as_bytes(), binding.provenance());
        let error = outcome.expect_err(delta);
        assert_eq!(events, vec![], "{delta}");
        assert_eq!(
            (error.code, error.message.clone()),
            (
                ErrorCode::Protocol,
                format!("chat completion field is not a string: {path}")
            ),
            "{delta}"
        );
        assert!(!format!("{error:?}").contains("SECRET-"), "{delta}");
    }
    let completion = json!({
        "id": "chatcmpl-x", "object": "chat.completion", "model": "Qwen/Qwen3-8B",
        "choices": [{"index": 0, "finish_reason": "stop",
            "message": {"role": "assistant", "content": null, "refusal": 7}}],
    });
    let error =
        llm_chat::decode_completion(&completion, binding.provenance()).expect_err("refused");
    assert_eq!(
        error.message,
        "chat completion field is not a string: choices[].message.refusal"
    );
}
