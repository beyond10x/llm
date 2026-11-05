//! A user turn carries an image (story:image-input): `Item::UserImage` holds inline bytes of one
//! of four media types or an `https` URL. Its bytes and its URL never reach a diagnostic or a
//! `Debug` form, inline bytes have a bound refused before I/O, and a turn without an image is the
//! turn it was before.

use llm_core::{
    ErrorCode, Id, ImageMediaType, ImageSource, Item, MAX_IMAGE_BYTES, MAX_IMAGE_URL_BYTES,
    Protocol, Provenance, StopReason, TurnDocument, TurnObservation, TurnOutcome, TurnRequest,
};
use serde_json::json;

/// The eight bytes of the PNG signature, standard base64 `iVBORw0KGgo=`.
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
const PNG_BASE64: &str = "iVBORw0KGgo=";
const URL: &str = "https://images.example/private/a.png?signature=SECRET-SIGNATURE";

fn png() -> Item {
    Item::user_image(ImageSource::bytes(ImageMediaType::Png, PNG))
}

fn linked() -> Item {
    Item::user_image(ImageSource::url(URL))
}

fn turn(items: Vec<Item>) -> TurnRequest {
    TurnRequest::new("test-model", items)
}

fn target() -> Provenance {
    Provenance {
        protocol: Protocol::Responses,
        provider: Id::new("local-provider").unwrap(),
        account: Id::new("anonymous").unwrap(),
        endpoint: Id::new("development").unwrap(),
        model: Id::new("test-model").unwrap(),
        binding_revision: Id::new("fake-model-v1").unwrap(),
    }
}

/// Fails when any form of the image's bytes or URL appears in `text`.
fn assert_redacted(text: &str) {
    for secret in [PNG_BASE64, "SECRET-SIGNATURE", "images.example", "PNG"] {
        assert!(!text.contains(secret), "{secret} reached: {text}");
    }
}

#[test]
fn a_user_image_is_written_as_its_kind_and_reads_back_from_a_turn_document() {
    assert_eq!(
        serde_json::to_value(png()).unwrap(),
        json!({"kind": "user-image", "source":
            {"kind": "bytes", "media_type": "image/png", "data": PNG_BASE64}})
    );
    assert_eq!(
        serde_json::to_value(linked()).unwrap(),
        json!({"kind": "user-image", "source": {"kind": "url", "url": URL}})
    );
    let request = turn(vec![Item::user("What is this?"), png(), linked()]);
    request.validate().expect("a valid turn");
    let document = serde_json::to_string(&TurnDocument::new(request.clone())).unwrap();
    assert!(
        document.starts_with(r#"{"format":"llm.turn/3","#),
        "{document}"
    );
    let read: TurnDocument = serde_json::from_str(&document).expect("reads back");
    assert_eq!(read.request, request);
}

/// The 0.20.0 turn exactly: an image item adds no key to a turn that carries none.
#[test]
fn a_turn_without_an_image_keeps_its_bytes() {
    let document = TurnDocument::new(turn(vec![Item::user("hi"), Item::assistant("hello")]));
    assert_eq!(
        serde_json::to_string(&document).unwrap(),
        r#"{"format":"llm.turn/3","request":{"model":"test-model","instructions":"","items":[{"kind":"user-text","text":"hi"},{"kind":"assistant-text","text":"hello"}],"tools":[]}}"#
    );
}

#[test]
fn every_media_type_is_written_as_its_name_and_no_other_name_reads() {
    for (media_type, name) in [
        (ImageMediaType::Png, "image/png"),
        (ImageMediaType::Jpeg, "image/jpeg"),
        (ImageMediaType::Gif, "image/gif"),
        (ImageMediaType::Webp, "image/webp"),
    ] {
        assert_eq!(media_type.name(), name);
        assert_eq!(ImageMediaType::from_name(name), Some(media_type));
        assert_eq!(serde_json::to_value(media_type).unwrap(), json!(name));
    }
    for other in ["image/bmp", "IMAGE/PNG", "image/svg+xml", "png", ""] {
        assert_eq!(ImageMediaType::from_name(other), None, "{other}");
    }
    let error = serde_json::from_value::<Item>(json!({"kind": "user-image", "source":
        {"kind": "bytes", "media_type": "image/bmp", "data": PNG_BASE64}}))
    .expect_err("an unknown media type is refused");
    assert!(!error.to_string().contains("image/bmp"), "{error}");
}

#[test]
fn inline_bytes_are_bounded_and_the_bound_is_refused_before_io() {
    let at_bound = turn(vec![Item::user_image(ImageSource::bytes(
        ImageMediaType::Jpeg,
        vec![7; MAX_IMAGE_BYTES],
    ))]);
    at_bound
        .validate()
        .expect("an image at the bound is carried");
    let past = turn(vec![Item::user_image(ImageSource::bytes(
        ImageMediaType::Jpeg,
        vec![7; MAX_IMAGE_BYTES + 1],
    ))]);
    let error = past.validate().expect_err("past the bound");
    assert_eq!(error.code, ErrorCode::TooLarge);
    assert_eq!(error.message, "an inline image exceeds its bound");
    let empty = turn(vec![Item::user_image(ImageSource::bytes(
        ImageMediaType::Gif,
        Vec::new(),
    ))]);
    assert_eq!(
        empty.validate().expect_err("no bytes").message,
        "an inline image carries no bytes"
    );
    // The base64 reader refuses past the bound before it decodes anything.
    let text = "A".repeat((MAX_IMAGE_BYTES / 3 + 2) * 4);
    let error = ImageSource::from_base64(ImageMediaType::Png, &text).expect_err("too large");
    assert_eq!(error.code, ErrorCode::TooLarge);
}

#[test]
fn an_image_url_is_https_with_a_host_of_visible_ascii_within_its_bound() {
    turn(vec![linked()]).validate().expect("an https URL");
    let long = format!("https://images.example/{}", "a".repeat(MAX_IMAGE_URL_BYTES));
    let error = turn(vec![Item::user_image(ImageSource::url(long))])
        .validate()
        .expect_err("past the bound");
    assert_eq!(error.code, ErrorCode::TooLarge);
    assert_eq!(error.message, "an image URL exceeds its bound");
    for refused in [
        "http://images.example/private/a.png?signature=SECRET-SIGNATURE",
        "HTTPS://images.example/private/a.png?signature=SECRET-SIGNATURE",
        "https:///private/a.png?signature=SECRET-SIGNATURE",
        "https://images.example/private a.png?signature=SECRET-SIGNATURE",
        "https://images.example/private/ä.png?signature=SECRET-SIGNATURE",
        "data:image/png;base64,iVBORw0KGgo=",
        "/private/a.png?signature=SECRET-SIGNATURE",
    ] {
        let error = turn(vec![Item::user_image(ImageSource::url(refused))])
            .validate()
            .expect_err(refused);
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{refused}");
        assert_eq!(
            error.message,
            "an image URL must be an https URL of visible ASCII naming a host"
        );
    }
}

/// Responses and Chat Completions carry an image as one URL: inline bytes as a base64 `data:`
/// URL, a link as itself.
#[test]
fn a_wire_url_is_a_base64_data_url_or_an_https_url_and_reads_back() {
    let bytes = ImageSource::bytes(ImageMediaType::Webp, PNG);
    assert_eq!(bytes.to_wire_url(), "data:image/webp;base64,iVBORw0KGgo=");
    assert_eq!(
        ImageSource::from_wire_url(&bytes.to_wire_url()).unwrap(),
        bytes
    );
    let link = ImageSource::url(URL);
    assert_eq!(link.to_wire_url(), URL);
    assert_eq!(ImageSource::from_wire_url(URL).unwrap(), link);
    assert_eq!(bytes.base64().as_deref(), Some(PNG_BASE64));
    assert_eq!(link.base64(), None);
    for refused in [
        "data:image/bmp;base64,iVBORw0KGgo=",
        "data:image/png,iVBORw0KGgo=",
        "data:image/png;base64,iVBORw0KGgo",
        "data:image/png;base64,iVBOR w0KGgo=",
        "http://images.example/private/a.png?signature=SECRET-SIGNATURE",
        "file:///private/a.png",
    ] {
        let error = ImageSource::from_wire_url(refused).expect_err(refused);
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{refused}");
        assert_redacted(&error.message);
    }
}

#[test]
fn no_image_byte_or_url_reaches_a_debug_form_or_a_diagnostic() {
    let request = turn(vec![png(), linked()]);
    assert_redacted(&format!("{request:?}"));
    assert_redacted(&format!("{:?}", png()));
    assert_redacted(&format!("{:#?}", linked()));
    let debug = format!("{:?}", png());
    assert!(debug.contains("Png") && debug.contains('8'), "{debug}");

    let bad_base64 = ImageSource::from_base64(ImageMediaType::Png, "iVBOR!SECRET-SIGNATURE=")
        .expect_err("not base64");
    assert_eq!(
        bad_base64.message,
        "inline image data is not standard base64"
    );
    assert_redacted(&bad_base64.message);

    for document in [
        json!({"kind": "user-image", "source": URL}),
        json!({"kind": "user-image", "source": {"kind": "url", "url": 7, "data": URL}}),
        json!({"kind": "user-image", "source": {"kind": "bytes", "media_type": URL, "data": PNG_BASE64}}),
        json!({"kind": "user-image", "source": {"kind": "bytes", "media_type": "image/png", "data": "SECRET-SIGNATURE"}}),
        json!({"kind": "user-image", "source": {"kind": "bytes", "media_type": "image/png", "data": PNG_BASE64, "url": URL}}),
        json!({"kind": "user-image", "source": {"kind": URL}}),
    ] {
        let error =
            serde_json::from_value::<Item>(document.clone()).expect_err(&document.to_string());
        assert_redacted(&error.to_string());
    }
    // A repeated key, which a JSON value cannot hold, read from text.
    let repeated =
        format!(r#"{{"kind":"user-image","source":{{"kind":"url","url":"{URL}","url":"{URL}"}}}}"#);
    let error = serde_json::from_str::<Item>(&repeated).expect_err("a repeated key");
    assert_redacted(&error.to_string());
}

#[test]
fn model_output_carrying_a_user_image_is_refused_as_caller_content() {
    let request = turn(vec![Item::user("What is this?")]);
    for image in [png(), linked()] {
        let outcome = TurnOutcome {
            stop_reason: StopReason::EndTurn,
            items: vec![Item::assistant("A logo."), image],
            observation: TurnObservation {
                final_usage: true,
                ..TurnObservation::new(target())
            },
        };
        let error = outcome
            .validate_for(&request, &target())
            .expect_err("caller-owned");
        assert_eq!(error.code, ErrorCode::Protocol);
        assert_eq!(error.message, "model output contains caller-owned content");
    }
}
