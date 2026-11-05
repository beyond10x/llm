//! A user turn carries an image on the Responses wire (story:image-input): an `input_image`
//! part, inline bytes as a base64 `data:` URL, in the user message of the user text beside it,
//! in item order. Ingress reads such parts back into image items; an image anywhere but a user
//! message stays refused by its path (`docs/responses.md`).

mod wire_support;

use llm_core::{
    Cancel, Dispatch, ErrorCode, Id, ImageMediaType, ImageSource, Item, Model, StopReason,
    TurnObservation, TurnOutcome, TurnRequest, VecSink,
};
use llm_responses::{encode_ingress_response, ingest_request, project_request};
use serde_json::{Value, json};
use wire_support::{TEXT_STREAM, body_fixture, client, listener, projection, serve};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
const URL: &str = "https://images.example/a.png";
const SECRET_URL: &str = "http://images.example/private.png?signature=SECRET-SIGNATURE";

fn binding() -> llm_responses::Binding {
    projection("http://127.0.0.1:9/v1")
}

fn png() -> Item {
    Item::user_image(ImageSource::bytes(ImageMediaType::Png, PNG))
}

fn linked() -> Item {
    Item::user_image(ImageSource::url(URL))
}

fn input(request: &TurnRequest) -> Value {
    project_request(&binding(), request).expect("projects")["input"].clone()
}

fn body(input: &Value) -> Value {
    json!({
        "model": "example/Small-Model",
        "input": input,
        "tools": [],
        "stream": true, "store": false,
        "include": ["reasoning.encrypted_content"]
    })
}

fn refused(input: &Value) -> llm_core::Error {
    match ingest_request(&binding(), &body(input)) {
        Ok(request) => panic!("accepted, returning {:?}", request.items),
        Err(error) => {
            assert_eq!(error.dispatch, Dispatch::NotSent, "{input}");
            error
        }
    }
}

/// The bytes the client sends for a text and two images, recorded in
/// `tests/fixtures/image-request.json`. Mutation: an image sent in a message of its own, `detail`
/// left out, a data URL of another shape, parts out of item order.
#[tokio::test]
async fn the_recorded_image_request_matches_its_fixture() {
    let (listener, url) = listener().await;
    let server = serve(listener, vec![TEXT_STREAM.to_vec()]);
    let client = client(&url);
    let mut request = TurnRequest::new(
        "small",
        vec![Item::user("What is in these pictures?"), png(), linked()],
    );
    "describe images".clone_into(&mut request.instructions);
    let mut sink = VecSink::new(16, 4096);
    let outcome = client.turn(&request, &mut sink, &Cancel::new()).await;
    assert!(outcome.is_ok(), "{outcome:?}");
    let captured = server.await.expect("the fixture server").remove(0);
    assert_eq!(
        String::from_utf8_lossy(&captured.body),
        String::from_utf8_lossy(&body_fixture("image-request.json")),
        "the exact request bytes changed; regenerate the fixture only on purpose"
    );
}

/// A run of user content holding an image is one user message, its parts in item order; a run
/// without one is sent as before, one message per text.
#[test]
fn a_run_of_user_content_holding_an_image_is_one_message_in_item_order() {
    let request = TurnRequest::new(
        "small",
        vec![
            Item::user("first"),
            png(),
            Item::user("second"),
            Item::assistant("A logo."),
            Item::user("third"),
            Item::user("fourth"),
            linked(),
        ],
    );
    assert_eq!(
        input(&request),
        json!([
            {"type": "message", "role": "user", "content": [
                {"type": "input_text", "text": "first"},
                {"type": "input_image", "image_url": "data:image/png;base64,iVBORw0KGgo=",
                 "detail": "auto"},
                {"type": "input_text", "text": "second"}]},
            {"type": "message", "role": "assistant", "content": [
                {"type": "output_text", "text": "A logo."}]},
            {"type": "message", "role": "user", "content": [
                {"type": "input_text", "text": "third"},
                {"type": "input_text", "text": "fourth"},
                {"type": "input_image", "image_url": URL, "detail": "auto"}]}
        ])
    );
    let text_only = TurnRequest::new("small", vec![Item::user("a"), Item::user("b")]);
    assert_eq!(
        input(&text_only),
        json!([
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "a"}]},
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "b"}]}
        ])
    );
}

/// A message of text and image parts is no longer refused: each part is an item, in order, and
/// the turn projects back to the message it was read from.
#[test]
fn ingress_reads_a_message_of_text_and_image_parts_in_order() {
    let message = json!([{"type": "message", "role": "user", "content": [
        {"type": "input_text", "text": "Compare"},
        {"type": "input_image", "image_url": "data:image/webp;base64,iVBORw0KGgo="},
        {"type": "input_text", "text": "with"},
        {"type": "input_text", "text": "this"},
        {"type": "input_image", "image_url": URL, "detail": "auto", "file_id": null}]}]);
    let request = ingest_request(&binding(), &body(&message)).expect("accepted");
    assert_eq!(
        request.items,
        vec![
            Item::user("Compare"),
            Item::user_image(ImageSource::bytes(ImageMediaType::Webp, PNG)),
            Item::user("with"),
            Item::user("this"),
            linked(),
        ]
    );
    let back = input(&request);
    assert_eq!(back[0]["content"].as_array().map(Vec::len), Some(5));
    assert_eq!(
        back[0]["content"][1]["image_url"],
        message[0]["content"][1]["image_url"]
    );
    assert_eq!(back[0]["content"][4]["image_url"], URL);
    // An image alone is a message of its own.
    let alone = json!([{"type": "message", "role": "user", "content": [
        {"type": "input_image", "image_url": URL}]}]);
    let request = ingest_request(&binding(), &body(&alone)).expect("accepted");
    assert_eq!(request.items, vec![linked()]);
}

#[test]
fn an_image_part_outside_a_user_message_is_refused_by_its_path() {
    let image = json!({"type": "input_image", "image_url": URL});
    let text = |kind: &str| json!({"type": kind, "text": "Hi"});
    for input in [
        json!([
            {"type": "message", "role": "user", "content": [text("input_text")]},
            {"type": "message", "role": "assistant", "content": [image]}]),
        json!([
            {"type": "message", "role": "user", "content": [text("input_text")]},
            {"type": "message", "role": "assistant", "content": [text("output_text"), image]}]),
        json!([
            {"type": "message", "role": "developer", "content": [text("input_text"), image]},
            {"type": "message", "role": "user", "content": [text("input_text")]}]),
    ] {
        let error = refused(&input);
        assert_eq!(error.code, ErrorCode::Unsupported, "{input}");
        assert_eq!(
            error.message, "an image part is carried only in a user message: input[].content[]",
            "{input}"
        );
    }
    // A tool result is one string on this wire; an image in it is refused as before.
    let error = refused(&json!([
        {"type": "function_call", "call_id": "c1", "name": "look", "arguments": "{}"},
        {"type": "function_call_output", "call_id": "c1", "output": [
            {"type": "input_image", "image_url": URL}]}]));
    assert_eq!(error.message, "a tool result's `output` is not a string");
}

#[test]
fn an_image_part_this_version_does_not_carry_is_refused_naming_its_field() {
    let part = |part: Value| json!([{"type": "message", "role": "user", "content": [part]}]);
    let error = refused(&part(
        json!({"type": "input_image", "image_url": URL, "detail": "high"}),
    ));
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(
        error.message,
        "`input[].content[].detail` other than `auto` is outside the pinned subset"
    );
    let error = refused(&part(json!({"type": "input_image", "file_id": "file-1"})));
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(
        error.message,
        "an image part refers to a file the provider stored, which this projection never keeps"
    );
    let error = refused(&part(
        json!({"type": "input_image", "image_url": URL, "crop": 1}),
    ));
    assert_eq!(
        error.message,
        "request body carries a field outside the pinned subset: input[].content[].crop"
    );
    let error = refused(&part(json!({"type": "input_image"})));
    assert_eq!(error.message, "an image part has no `image_url`");
}

#[test]
fn an_image_url_outside_the_contract_is_refused_without_quoting_it() {
    for url in [
        SECRET_URL,
        "data:image/bmp;base64,SECRET-SIGNATURE",
        "data:image/png;base64,SECRET-SIGNATURE",
        "https://images.example/private png?signature=SECRET-SIGNATURE",
    ] {
        let error = refused(&json!([{"type": "message", "role": "user", "content": [
            {"type": "input_image", "image_url": url}]}]));
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
    let binding = binding();
    let outcome = TurnOutcome {
        stop_reason: StopReason::EndTurn,
        items: vec![Item::assistant("A logo."), png()],
        observation: TurnObservation {
            upstream_model: Some(Id::new("example/Small-Model").unwrap()),
            final_usage: true,
            ..TurnObservation::new(binding.provenance().clone())
        },
    };
    let error = encode_ingress_response(&outcome, "resp_1", 0).expect_err("caller content");
    assert_eq!(error.code, ErrorCode::Protocol);
    assert_eq!(error.message, "model outcome contains caller-owned content");
}
