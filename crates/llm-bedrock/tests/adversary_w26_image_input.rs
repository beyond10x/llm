//! Adversary cases for story:image-input on Bedrock `InvokeModel` (wave 2026-11-05-w26, unit 2).
//!
//! Anthropic's published vision guide (`https://platform.claude.com/docs/en/build-with-claude/vision.md`,
//! fetched 2026-11-05) states for this operation, in its front matter, lines 32-34:
//!
//! > Amazon Bedrock (InvokeModel):
//! >   availability: stable
//! >   note: Only base64 image sources; per-image limit 5 MB (base64-encoded) and a lower total
//! >   request size.
//!
//! and, at line 60: "On Claude Models on Google Cloud (Google managed) and Amazon Bedrock, only
//! base64-encoded sources are available." The story's acceptance: "A wire that cannot carry the
//! source (Converse and a URL) refuses by name before I/O", and "Inline bytes have a size bound
//! refused before I/O". `check_request` and `encode_request` are what `BedrockClient` asks before
//! it signs or sends anything, so a turn either of them accepts is a turn that goes out.

use llm_bedrock::{Api, BedrockTarget, check_request, encode_request};
use llm_core::{
    Capabilities, Dispatch, ErrorCode, Id, ImageMediaType, ImageSource, Item, MAX_IMAGE_BYTES,
    Protocol, Provenance, TurnRequest,
};
use url::Url;

const MODEL: &str = "fixture-model";
const URL: &str = "https://images.example/private.png?signature=SECRET-SIGNATURE";

fn invoke() -> BedrockTarget {
    BedrockTarget::new(
        Provenance {
            protocol: Protocol::Messages,
            provider: Id::new("bedrock").unwrap(),
            account: Id::new("fixture-account").unwrap(),
            endpoint: Id::new("us-east-1").unwrap(),
            model: Id::new(MODEL).unwrap(),
            binding_revision: Id::new("fixture-1").unwrap(),
        },
        Capabilities::text(200_000, 4_096),
        "us-east-1",
        MODEL,
        Api::AnthropicInvoke,
        true,
        Some(Url::parse("http://127.0.0.1:9/").unwrap()),
    )
    .expect("a valid fixture target")
}

/// An image by URL is a source this operation does not take, so it is refused by name before
/// I/O, as Converse refuses it, and the URL reaches neither the message nor the debug form.
#[test]
fn invoke_model_refuses_an_image_by_url_before_io() {
    let request = TurnRequest::new(
        MODEL,
        vec![Item::user("Look"), Item::user_image(ImageSource::url(URL))],
    );
    let sent =
        encode_request(&request, &invoke()).map(|body| String::from_utf8_lossy(&body).into_owned());
    let error = match sent {
        Ok(body) => panic!(
            "InvokeModel sent an image by URL, which the operation does not take: {}",
            &body[body.find(r#"{"type":"image""#).unwrap_or(0)..]
        ),
        Err(error) => error,
    };
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(error.dispatch, Dispatch::NotSent);
    assert!(!error.message.contains("SECRET"), "{error:?}");
    assert!(!format!("{error:?}").contains("SECRET"), "{error:?}");
    assert_eq!(
        check_request(&request, &invoke()).expect_err("the same answer"),
        error
    );
}

/// Inline bytes inside the neutral bound whose base64 passes the 5 MB this operation documents
/// for one image are refused before I/O rather than sent for the far side to refuse.
#[test]
fn invoke_model_refuses_an_inline_image_past_its_documented_base64_limit_before_io() {
    // 4,000,000 bytes are 5,333,336 base64 characters: past 5 MB read as 5,000,000 bytes and as
    // 5,242,880 bytes alike, and still inside MAX_IMAGE_BYTES.
    let raw = 4_000_000;
    assert!(raw <= MAX_IMAGE_BYTES);
    let request = TurnRequest::new(
        MODEL,
        vec![
            Item::user("Look"),
            Item::user_image(ImageSource::bytes(ImageMediaType::Png, vec![7; raw])),
        ],
    );
    request.validate().expect("inside the neutral bound");
    let error = match encode_request(&request, &invoke()) {
        Ok(body) => panic!(
            "InvokeModel sent a {}-byte body holding an image of {} base64 characters",
            body.len(),
            raw.div_ceil(3) * 4
        ),
        Err(error) => error,
    };
    assert_eq!(error.code, ErrorCode::TooLarge);
    assert_eq!(error.dispatch, Dispatch::NotSent);
}
