//! A user turn carries an image on both Bedrock wires (story:image-input).
//!
//! Converse sends inline bytes as an `image` block, `{format, source: {bytes}}`, in the user
//! message beside the user text, in item order, and refuses an image by URL before any I/O: its
//! image source has no URL (Bedrock Runtime API reference, `ImageBlock`, `ImageSource`, as
//! story:image-input cites it). `InvokeModel` sends the Messages body, so an image is a Messages
//! `image` block (`docs/bedrock.md`).

use llm_bedrock::{Api, BedrockTarget, check_request, encode_request};
use llm_core::{
    Capabilities, Dispatch, ErrorCode, Id, ImageMediaType, ImageSource, Item, Protocol, Provenance,
    TurnRequest,
};
use serde_json::{Value, json};
use url::Url;

const MODEL: &str = "fixture-model";
const RECORDED_CONVERSE_BODY: &str = include_str!("fixtures/image-converse-request.json");
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
const URL: &str = "https://images.example/private.png?signature=SECRET-SIGNATURE";

fn target(api: Api) -> BedrockTarget {
    let protocol = match api {
        Api::Converse => Protocol::BedrockConverse,
        Api::AnthropicInvoke => Protocol::Messages,
    };
    BedrockTarget::new(
        Provenance {
            protocol,
            provider: Id::new("bedrock").unwrap(),
            account: Id::new("fixture-account").unwrap(),
            endpoint: Id::new("us-east-1").unwrap(),
            model: Id::new(MODEL).unwrap(),
            binding_revision: Id::new("fixture-1").unwrap(),
        },
        Capabilities::text(200_000, 4_096),
        "us-east-1",
        MODEL,
        api,
        true,
        Some(Url::parse("http://127.0.0.1:9/").unwrap()),
    )
    .expect("a valid fixture target")
}

fn image(media_type: ImageMediaType) -> Item {
    Item::user_image(ImageSource::bytes(media_type, PNG))
}

fn encoded(items: Vec<Item>, api: Api) -> Value {
    let request = TurnRequest::new(MODEL, items);
    serde_json::from_slice(&encode_request(&request, &target(api)).expect("encodes"))
        .expect("JSON bytes")
}

/// The bytes `encode_request` returns, which are the bytes `BedrockClient` sends, for a text and
/// an inline image on Converse: recorded in `tests/fixtures/image-converse-request.json`.
/// Mutation: the image in a message of its own, the format written as a MIME name, the bytes
/// not base64, blocks out of item order.
#[test]
fn the_recorded_converse_image_request_matches_its_fixture() {
    let mut request = TurnRequest::new(
        MODEL,
        vec![
            Item::user("What is in this picture?"),
            image(ImageMediaType::Png),
        ],
    );
    "describe images".clone_into(&mut request.instructions);
    assert_eq!(
        String::from_utf8(encode_request(&request, &target(Api::Converse)).expect("encodes"))
            .unwrap(),
        RECORDED_CONVERSE_BODY
            .strip_suffix('\n')
            .expect("one final newline"),
        "the exact request bytes changed; regenerate the fixture only on purpose"
    );
}

#[test]
fn converse_names_each_media_type_by_its_format_in_the_user_message_beside_its_text() {
    let body = encoded(
        vec![
            image(ImageMediaType::Jpeg),
            Item::user("between"),
            image(ImageMediaType::Gif),
            Item::assistant("Two pictures."),
            image(ImageMediaType::Webp),
        ],
        Api::Converse,
    );
    let block =
        |format: &str| json!({"image": {"format": format, "source": {"bytes": "iVBORw0KGgo="}}});
    assert_eq!(
        body["messages"],
        json!([
            {"role": "user", "content": [block("jpeg"), {"text": "between"}, block("gif")]},
            {"role": "assistant", "content": [{"text": "Two pictures."}]},
            {"role": "user", "content": [block("webp")]}
        ])
    );
}

/// An image is user content: a conversation may open with one.
#[test]
fn a_conversation_may_open_with_an_image() {
    let body = encoded(vec![image(ImageMediaType::Png)], Api::Converse);
    assert_eq!(body["messages"][0]["role"], "user");
}

/// Converse's image source carries bytes or an S3 location, never a URL, so an image by URL is
/// refused by name before any I/O, and the URL reaches neither the message nor the debug form.
#[test]
fn converse_refuses_an_image_by_url_before_io_without_quoting_it() {
    let request = TurnRequest::new(
        MODEL,
        vec![Item::user("Look"), Item::user_image(ImageSource::url(URL))],
    );
    let error = encode_request(&request, &target(Api::Converse)).expect_err("refused");
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(error.dispatch, Dispatch::NotSent);
    assert_eq!(
        error.message,
        "Bedrock Converse carries an image only as inline bytes, not by URL"
    );
    assert_eq!(
        check_request(&request, &target(Api::Converse)).expect_err("the same answer"),
        error
    );
    assert!(!format!("{error:?}").contains("SECRET"), "{error:?}");
}

/// `InvokeModel` carries the Messages body, so the image is the Messages `image` block in the
/// user message of the text beside it.
#[test]
fn invoke_sends_the_image_as_the_messages_image_block() {
    let body = encoded(
        vec![Item::user("What is this?"), image(ImageMediaType::Png)],
        Api::AnthropicInvoke,
    );
    assert_eq!(
        body["messages"][0]["content"][1]["source"],
        json!({"type": "base64", "media_type": "image/png", "data": "iVBORw0KGgo="})
    );
    assert_eq!(body["messages"].as_array().map(Vec::len), Some(1));
}
