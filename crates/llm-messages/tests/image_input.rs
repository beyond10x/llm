//! A user turn carries an image on the Messages wire (story:image-input): an `image` block with a
//! `base64` or `url` source, in the user message beside the user text, in item order. Ingress
//! reads such blocks back into image items; an image anywhere but a user message stays refused by
//! its path (`docs/messages.md`).
mod support;

use llm_core::{
    Dispatch, Error, ErrorCode, Id, ImageMediaType, ImageSource, Item, StopReason, TurnObservation,
    TurnOutcome, TurnRequest,
};
use llm_messages::{decode_request, encode_ingress_message, encode_request};
use serde_json::{Value, json};
use support::binding;

const RECORDED_BODY: &str = include_str!("fixtures/image-request.json");
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
const URL: &str = "https://images.example/a.png";

fn png() -> Item {
    Item::user_image(ImageSource::bytes(ImageMediaType::Png, PNG))
}

fn linked() -> Item {
    Item::user_image(ImageSource::url(URL))
}

fn encoded(request: &TurnRequest) -> Value {
    serde_json::from_slice(&encode_request(request, &binding()).expect("encodes"))
        .expect("JSON bytes")
}

fn decode(body: &Value) -> Result<TurnRequest, Error> {
    decode_request(body.to_string().as_bytes(), binding().provenance())
        .map(|ingress| ingress.request)
}

fn refused(body: &Value) -> Error {
    match decode(body) {
        Ok(request) => panic!("{body} was accepted as {request:?}"),
        Err(error) => {
            assert_eq!(error.dispatch, Dispatch::NotSent, "{body}");
            error
        }
    }
}

fn with(messages: &Value) -> Value {
    json!({"model": "caller-alias", "max_tokens": 512, "messages": messages})
}

fn user_blocks(blocks: &Value) -> Value {
    with(&json!([{"role": "user", "content": blocks}]))
}

/// The bytes `encode_request` returns, which are the bytes `MessagesClient` sends, for a text and
/// two images: recorded in `tests/fixtures/image-request.json`. Mutation: an image in a message
/// of its own, a source of another shape, blocks out of item order, the breakpoint moved.
#[test]
fn the_recorded_image_request_matches_its_fixture() {
    let mut request = TurnRequest::new(
        "internal-model",
        vec![Item::user("What is in these pictures?"), png(), linked()],
    );
    "describe images".clone_into(&mut request.instructions);
    assert_eq!(
        String::from_utf8(encode_request(&request, &binding()).expect("encodes")).unwrap(),
        RECORDED_BODY.strip_suffix('\n').expect("one final newline"),
        "the exact request bytes changed; regenerate the fixture only on purpose"
    );
}

#[test]
fn an_image_shares_the_user_message_of_the_text_beside_it_in_item_order() {
    let body = encoded(&TurnRequest::new(
        "internal-model",
        vec![
            Item::user("first"),
            png(),
            Item::assistant("A logo."),
            linked(),
            Item::user("second"),
        ],
    ));
    assert_eq!(
        body["messages"],
        json!([
            {"role": "user", "content": [
                {"type": "text", "text": "first"},
                {"type": "image", "source":
                    {"type": "base64", "media_type": "image/png", "data": "iVBORw0KGgo="}}]},
            {"role": "assistant", "content": [{"type": "text", "text": "A logo."}]},
            {"role": "user", "content": [
                {"type": "image", "source": {"type": "url", "url": URL}},
                {"type": "text", "text": "second", "cache_control": {"type": "ephemeral"}}]}
        ])
    );
}

/// The rolling breakpoint lands on the last block this projection built, an image included, and
/// a mark on an arriving image block is read into the prompt-cache hint as any other mark.
#[test]
fn an_image_that_ends_the_conversation_carries_the_rolling_breakpoint() {
    let body = encoded(&TurnRequest::new(
        "internal-model",
        vec![Item::user("Look"), png()],
    ));
    assert_eq!(
        body["messages"][0]["content"][1]["cache_control"],
        json!({"type": "ephemeral"})
    );
    let request = decode(&user_blocks(&json!([
        {"type": "text", "text": "Look"},
        {"type": "image", "source": {"type": "url", "url": URL},
         "cache_control": {"type": "ephemeral", "ttl": "1h"}}])))
    .expect("accepted");
    assert_eq!(request.items, vec![Item::user("Look"), linked()]);
    assert!(request.controls.hints.prompt_cache.is_some());
}

/// A message of text and image blocks is no longer refused: each block is an item, in order, and
/// the turn encodes back to the blocks it was read from.
#[test]
fn ingress_reads_a_message_of_text_and_image_blocks_in_order() {
    let blocks = json!([
        {"type": "text", "text": "Compare"},
        {"type": "image", "source":
            {"type": "base64", "media_type": "image/jpeg", "data": "iVBORw0KGgo="}},
        {"type": "text", "text": "with"},
        {"type": "image", "source": {"type": "url", "url": URL}}
    ]);
    let mut request = decode(&user_blocks(&blocks)).expect("accepted");
    assert_eq!(
        request.items,
        vec![
            Item::user("Compare"),
            Item::user_image(ImageSource::bytes(ImageMediaType::Jpeg, PNG)),
            Item::user("with"),
            linked(),
        ]
    );
    // The wire's model is the caller's routing name; routing would select the binding for it.
    "internal-model".clone_into(&mut request.model);
    let mut back = encoded(&request)["messages"][0]["content"].clone();
    back[3]
        .as_object_mut()
        .expect("a block")
        .remove("cache_control");
    assert_eq!(back, blocks);
}

#[test]
fn an_image_block_outside_a_user_message_is_refused_by_its_path() {
    let image = json!({"type": "image", "source": {"type": "url", "url": URL}});
    let error = refused(&with(&json!([
        {"role": "user", "content": "hi"},
        {"role": "assistant", "content": [image]}])));
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(
        error.message,
        "Messages image block is carried only in a user message: messages[].content[]"
    );
    // The system prompt and a system entry carry text only, as before.
    let error = refused(&json!({"model": "caller-alias", "max_tokens": 512,
        "system": [image], "messages": [{"role": "user", "content": "hi"}]}));
    assert_eq!(error.message, "Messages system block is not text");
    let error = refused(&with(&json!([
        {"role": "user", "content": "hi"},
        {"role": "system", "content": [image]}])));
    assert_eq!(
        error.message,
        "Messages field is outside the declared subset: messages[].content[].type"
    );
}

#[test]
fn an_image_source_this_version_does_not_carry_is_refused_by_its_path() {
    for (source, message) in [
        (
            json!({"type": "file", "file_id": "file_1"}),
            "Messages image source type is outside the declared subset",
        ),
        (
            json!({"type": "url", "url": URL, "detail": "high"}),
            "Messages field is outside the declared subset: messages[].content[].source.detail",
        ),
        (
            json!({"type": "base64", "media_type": "image/png", "data": "iVBORw0KGgo=", "url": URL}),
            "Messages field is outside the declared subset: messages[].content[].source.url",
        ),
        (
            json!({"type": "base64", "media_type": "image/bmp", "data": "iVBORw0KGgo="}),
            "Messages image media type is none of image/png, image/jpeg, image/gif and image/webp",
        ),
    ] {
        let error = refused(&user_blocks(&json!([{"type": "image", "source": source}])));
        assert_eq!(error.code, ErrorCode::Unsupported, "{source}");
        assert_eq!(error.message, message, "{source}");
    }
    let error = refused(&user_blocks(&json!([
        {"type": "image", "source": {"type": "url", "url": URL}, "title": "x"}])));
    assert_eq!(
        error.message,
        "Messages field is outside the declared subset: messages[].content[].title"
    );
}

#[test]
fn image_bytes_or_a_url_outside_the_contract_are_refused_without_quoting_them() {
    for source in [
        json!({"type": "base64", "media_type": "image/png", "data": "SECRET-SIGNATURE"}),
        json!({"type": "url", "url": "http://images.example/private.png?signature=SECRET-SIGNATURE"}),
        json!({"type": "url", "url": "https://images.example/private png?SECRET-SIGNATURE"}),
    ] {
        let error = refused(&user_blocks(&json!([{"type": "image", "source": source}])));
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{source}");
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
        items: vec![Item::assistant("A logo."), png()],
        observation: TurnObservation {
            upstream_model: Some(Id::new("example/Model-Revision").unwrap()),
            final_usage: true,
            ..TurnObservation::new(binding().provenance().clone())
        },
    };
    let error = encode_ingress_message(&outcome, "msg_1").expect_err("caller content");
    assert_eq!(error.code, ErrorCode::Protocol);
}
