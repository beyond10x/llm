//! A user turn carries an image on the Chat Completions wire (story:image-input): an `image_url`
//! part, inline bytes as a base64 `data:` URL, in the user message of the user text beside it,
//! in item order. Ingress reads such parts back into image items; an image anywhere but a user
//! message stays refused by its path (`docs/chat.md`).
mod common;

use common::{binding, binding_at, capable};
use llm_chat::{ChatClient, decode_ingress_request, encode_ingress_completion, project_request};
use llm_core::{
    Cancel, Dispatch, ErrorCode, Id, ImageMediaType, ImageSource, Item, Model, Protocol,
    StopReason, TurnObservation, TurnOutcome, TurnRequest, VecSink,
};
use llm_credentials::{ResolvedSecret, SecretError, SecretRef, SecretResolver};
use llm_http::{HttpClient, Limits};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const OPENAI_TEXT: &[u8] = include_bytes!("../fixtures/openai-text-and-usage.sse");
const RECORDED_BODY: &str = include_str!("../fixtures/image-request.json");
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
const URL: &str = "https://images.example/a.png";

fn png() -> Item {
    Item::user_image(ImageSource::bytes(ImageMediaType::Png, PNG))
}

fn linked() -> Item {
    Item::user_image(ImageSource::url(URL))
}

fn messages(items: Vec<Item>) -> Value {
    let request = TurnRequest::new("small", items);
    project_request(&request, &binding(), false).expect("projected")["messages"].clone()
}

fn refused(body: &Value) -> llm_core::Error {
    let error = decode_ingress_request(body).expect_err("refused");
    assert_eq!(error.dispatch, Dispatch::NotSent, "{body}");
    error
}

fn user(content: &Value) -> Value {
    json!({"model": "small", "messages": [{"role": "user", "content": content}]})
}

/// An anonymous binding never reaches a resolver.
struct NoResolver;
impl SecretResolver for NoResolver {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> llm_core::BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async { Err(SecretError::Missing) })
    }
}

/// Accepts one request, answers the pinned stream, and hands back the request body.
async fn serve_once(body: &'static [u8]) -> (String, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bound");
    let port = listener.local_addr().expect("address").port();
    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accepted");
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        let sent = loop {
            let read = socket.read(&mut buffer).await.expect("read");
            request.extend_from_slice(&buffer[..read]);
            let text = String::from_utf8_lossy(&request).into_owned();
            let body = text.split_once("\r\n\r\n").and_then(|(head, rest)| {
                head.to_ascii_lowercase()
                    .split_once("content-length:")
                    .and_then(|(_, tail)| tail.split(['\r', ';']).next())
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .filter(|length| rest.len() >= *length)
                    .map(|_| rest.to_owned())
            });
            if let Some(body) = body {
                break body;
            }
            assert!(read != 0, "the request ended before its body");
        };
        let mut response = Vec::from(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
        );
        response.extend_from_slice(body);
        socket.write_all(&response).await.expect("written");
        socket.shutdown().await.expect("closed");
        sent
    });
    (format!("http://127.0.0.1:{port}/v1"), handle)
}

/// The bytes the client sends for a text and two images, recorded in
/// `fixtures/image-request.json`. Mutation: an image sent in a message of its own, a `detail`
/// written, a data URL of another shape, parts out of item order.
#[tokio::test]
async fn the_recorded_image_request_matches_its_fixture() {
    let (base_url, server) = serve_once(OPENAI_TEXT).await;
    let client = ChatClient::new(
        binding_at(Protocol::ChatCompletions, capable(), &base_url),
        HttpClient::new(Limits::default()).expect("client"),
        Arc::new(NoResolver),
    );
    let mut request = TurnRequest::new(
        "small",
        vec![Item::user("What is in these pictures?"), png(), linked()],
    );
    "describe images".clone_into(&mut request.instructions);
    let mut sink = VecSink::new(64, 64 * 1024);
    let model: &dyn Model = &client;
    let outcome = model.turn(&request, &mut sink, &Cancel::new()).await;
    assert!(outcome.is_ok(), "{outcome:?}");
    let sent = server.await.expect("the fixture server");
    assert_eq!(
        sent,
        RECORDED_BODY.strip_suffix('\n').expect("one final newline"),
        "the exact request bytes changed; regenerate the fixture only on purpose"
    );
}

/// A run of user content holding an image is one user message of parts in item order; a run
/// without one is sent as before, one message of text per item.
#[test]
fn a_run_of_user_content_holding_an_image_is_one_message_in_item_order() {
    assert_eq!(
        messages(vec![
            Item::user("first"),
            png(),
            Item::user("second"),
            Item::assistant("A logo."),
            Item::user("third"),
            Item::user("fourth"),
            linked(),
        ]),
        json!([
            {"role": "user", "content": [
                {"type": "text", "text": "first"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,iVBORw0KGgo="}},
                {"type": "text", "text": "second"}]},
            {"role": "assistant", "content": "A logo."},
            {"role": "user", "content": [
                {"type": "text", "text": "third"},
                {"type": "text", "text": "fourth"},
                {"type": "image_url", "image_url": {"url": URL}}]}
        ])
    );
    assert_eq!(
        messages(vec![Item::user("a"), Item::user("b")]),
        json!([{"role": "user", "content": "a"}, {"role": "user", "content": "b"}])
    );
}

/// A message of text and image parts is no longer refused: each part is an item, in order, and
/// the turn projects back to the message it was read from.
#[test]
fn ingress_reads_a_message_of_text_and_image_parts_in_order() {
    let content = json!([
        {"type": "text", "text": "Compare"},
        {"type": "image_url", "image_url": {"url": "data:image/gif;base64,iVBORw0KGgo="}},
        {"type": "text", "text": "with"},
        {"type": "text", "text": "this"},
        {"type": "image_url", "image_url": {"url": URL, "detail": "auto"}}
    ]);
    let request = decode_ingress_request(&user(&content))
        .expect("accepted")
        .request;
    assert_eq!(
        request.items,
        vec![
            Item::user("Compare"),
            Item::user_image(ImageSource::bytes(ImageMediaType::Gif, PNG)),
            Item::user("with"),
            Item::user("this"),
            linked(),
        ]
    );
    let back = project_request(&request, &binding(), false).expect("projected");
    let mut sent = content;
    sent[4]["image_url"]
        .as_object_mut()
        .expect("an object")
        .remove("detail");
    assert_eq!(back["messages"], json!([{"role": "user", "content": sent}]));
    // Text parts without an image are joined into one text, as before.
    let joined = decode_ingress_request(&user(&json!([
        {"type": "text", "text": "a"}, {"type": "text", "text": "b"}])))
    .expect("accepted");
    assert_eq!(joined.request.items, vec![Item::user("ab")]);
}

#[test]
fn an_image_part_outside_a_user_message_is_refused_by_its_path() {
    let image = json!({"type": "image_url", "image_url": {"url": URL}});
    for message in [
        json!({"role": "assistant", "content": [image]}),
        json!({"role": "system", "content": [image]}),
        json!({"role": "developer", "content": [{"type": "text", "text": "x"}, image]}),
    ] {
        let mut body = json!({"model": "small", "messages": [message.clone()]});
        if message["role"] == "assistant" {
            body["messages"] = json!([{"role": "user", "content": "hi"}, message]);
        }
        let error = refused(&body);
        assert_eq!(error.code, ErrorCode::Unsupported, "{message}");
        assert_eq!(
            error.message,
            "chat image part is carried only in a user message: messages[].content[]",
            "{message}"
        );
    }
    // A tool message: its call is read first, then the same refusal.
    let error = refused(&json!({"model": "small", "messages": [
        {"role": "user", "content": "hi"},
        {"role": "assistant", "tool_calls": [{"id": "c1", "type": "function",
            "function": {"name": "look", "arguments": "{}"}}]},
        {"role": "tool", "tool_call_id": "c1", "content": [image]}]}));
    assert_eq!(
        error.message,
        "chat image part is carried only in a user message: messages[].content[]"
    );
}

#[test]
fn an_image_part_this_version_does_not_carry_is_refused_naming_its_field() {
    let error = refused(&user(&json!([
        {"type": "image_url", "image_url": {"url": URL, "detail": "low"}}])));
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(
        error.message,
        "chat image detail other than `auto` is outside the published subset"
    );
    let error = refused(&user(&json!([
        {"type": "image_url", "image_url": {"url": URL, "crop": 1}}])));
    assert_eq!(
        error.message,
        "chat request field is outside the published subset: messages[].content[].image_url.crop"
    );
    let error = refused(&user(&json!([
        {"type": "image_url", "image_url": {"url": URL}, "cache_control": {}}])));
    assert_eq!(
        error.message,
        "chat request field is outside the published subset: messages[].content[].cache_control"
    );
    let error = refused(&user(&json!([{"type": "image_url", "image_url": URL}])));
    assert_eq!(error.message, "chat image part carries no url");
}

#[test]
fn an_image_url_outside_the_contract_is_refused_without_quoting_it() {
    for url in [
        "http://images.example/private.png?signature=SECRET-SIGNATURE",
        "data:image/bmp;base64,SECRET-SIGNATURE",
        "data:image/png;base64,SECRET-SIGNATURE",
        "https://images.example/private png?signature=SECRET-SIGNATURE",
    ] {
        let error = refused(&user(&json!([
            {"type": "image_url", "image_url": {"url": url}}])));
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{url}");
        assert!(
            !error.message.contains("SECRET") && !format!("{error:?}").contains("SECRET"),
            "{error:?}"
        );
    }
}

/// A user image is the caller's: an outcome carrying one is not the model's answer.
#[test]
fn an_outcome_carrying_a_user_image_is_refused_as_caller_content() {
    let outcome = TurnOutcome {
        stop_reason: StopReason::EndTurn,
        items: vec![Item::assistant("A logo."), linked()],
        observation: TurnObservation {
            upstream_model: Some(Id::new("Qwen/Qwen3-8B").unwrap()),
            final_usage: true,
            ..TurnObservation::new(binding().provenance().clone())
        },
    };
    let error = encode_ingress_completion(&outcome, "chatcmpl-1", 0).expect_err("caller content");
    assert_eq!(error.code, ErrorCode::Protocol);
    assert_eq!(error.message, "model outcome contains caller-owned content");
}
