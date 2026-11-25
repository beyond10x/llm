//! Adversary cases for story:image-input in the neutral image (wave 2026-11-05-w26, unit 2):
//! the bounds of `crates/llm-core/src/image.rs` at exactly the limit and one past it, read
//! against what that file's own documentation promises.

use llm_core::{
    ErrorCode, ImageMediaType, ImageSource, Item, MAX_IMAGE_BYTES, MAX_IMAGE_URL_BYTES, TurnRequest,
};

fn base64_of(length: usize) -> String {
    ImageSource::bytes(ImageMediaType::Png, vec![7; length])
        .base64()
        .expect("inline bytes")
}

/// `ImageSource::from_base64`: "Refuses text that would decode past [`MAX_IMAGE_BYTES`], before
/// decoding it". The base64 of `MAX_IMAGE_BYTES` and of one byte more are the same length,
/// 6,990,508 characters, because 5 MiB is 2 modulo 3; a length check alone cannot tell them apart.
#[test]
fn from_base64_refuses_text_that_decodes_one_byte_past_the_bound() {
    let at = base64_of(MAX_IMAGE_BYTES);
    let past = base64_of(MAX_IMAGE_BYTES + 1);
    assert_eq!(
        at.len(),
        past.len(),
        "the premise: both encode to one length"
    );
    let read = ImageSource::from_base64(ImageMediaType::Png, &at).expect("at the bound");
    read.validate().expect("at the bound");
    match ImageSource::from_base64(ImageMediaType::Png, &past) {
        Ok(ImageSource::Bytes { data, .. }) => panic!(
            "from_base64 read {} bytes, {} past MAX_IMAGE_BYTES",
            data.len(),
            data.len() - MAX_IMAGE_BYTES
        ),
        Ok(_) => panic!("from_base64 read a URL"),
        Err(error) => {
            assert_eq!(error.code, ErrorCode::TooLarge);
            assert_eq!(error.message, "an inline image exceeds its bound");
        }
    }
}

/// `ImageSource::Url`: "`https://`, a host, visible ASCII only"; its refusal says "naming a
/// host". An authority of userinfo or a port alone names none.
#[test]
fn an_https_url_whose_authority_names_no_host_is_refused() {
    for url in [
        "https://@/a.png",
        "https://:443/a.png",
        "https://user:secret@/a.png",
        "https://user:secret@:443/a.png",
    ] {
        let error = ImageSource::url(url)
            .validate()
            .err()
            .unwrap_or_else(|| panic!("{url} names no host and was accepted"));
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{url}");
        assert_eq!(
            error.message,
            "an image URL must be an https URL of visible ASCII naming a host"
        );
    }
}

/// The URL bound at exactly `MAX_IMAGE_URL_BYTES` and one past it, and the inline bound on a turn.
#[test]
fn the_url_bound_holds_at_exactly_its_limit_and_refuses_one_past() {
    let prefix = "https://images.example/";
    let at = format!("{prefix}{}", "a".repeat(MAX_IMAGE_URL_BYTES - prefix.len()));
    assert_eq!(at.len(), MAX_IMAGE_URL_BYTES);
    ImageSource::url(at.clone())
        .validate()
        .expect("at the bound");
    let past = format!("{at}a");
    let error = TurnRequest::new("model", vec![Item::user_image(ImageSource::url(past))])
        .validate()
        .expect_err("one past the bound");
    assert_eq!(error.code, ErrorCode::TooLarge);
    assert_eq!(error.message, "an image URL exceeds its bound");
}

/// Standard base64 with its padding only: no URL-safe alphabet, no whitespace, no missing or
/// extra padding, no non-zero trailing bits. None of these is quoted in the refusal.
#[test]
fn inline_base64_is_canonical_standard_base64_only() {
    for text in [
        "iVBORw0KGgo",
        "iVBORw0KGgo==",
        "iVBORw0K\nGgo=",
        " iVBORw0KGgo=",
        "iVBORw0KGgo= ",
        "-_8=",
        "_w==",
        "/w==AA==",
        "AB==",
        "iVBORw0KGgo=\r\n",
    ] {
        let error = ImageSource::from_base64(ImageMediaType::Png, text)
            .err()
            .unwrap_or_else(|| panic!("{text:?} was read"));
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{text:?}");
        assert_eq!(error.message, "inline image data is not standard base64");
    }
    assert_eq!(
        ImageSource::from_base64(ImageMediaType::Png, "/w==").unwrap(),
        ImageSource::bytes(ImageMediaType::Png, [0xff])
    );
}
