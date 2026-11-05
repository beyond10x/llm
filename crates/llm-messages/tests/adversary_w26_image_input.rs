//! Adversary cases for story:image-input on the Messages wire (wave 2026-11-05-w26, unit 2).
//!
//! Each of the four ingress refusals that story added for an `image` block is aimed at by one
//! case here. A case plays the part of whoever serves the Messages wire: it decodes the arriving
//! request and, if the decoder accepts it, sends the decoded turn upstream through
//! `MessagesClient` to a loopback listener. It then asserts that the listener received no byte,
//! and that the refusal is the one named, by code, fixed text and `not-sent`. So a refusal that is
//! removed falls through to a request that goes out, and the case fails on the bytes; one that is
//! weakened into another refusal fails on the text.
//!
//! The shapes are Anthropic's published `ImageBlockParam`
//! (`https://platform.claude.com/docs/en/api/messages.md`, fetched 2026-11-05, lines 260-294):
//! `type: "image"`, `source` one of `Base64ImageSource` (`type: "base64"`, `data`, `media_type`
//! one of `image/jpeg`, `image/png`, `image/gif`, `image/webp`), `URLImageSource` (`type: "url"`,
//! `url`) or `FileImageSource` (`type: "file"`, `file_id`), which this version does not carry.
mod support;

use llm_core::{Cancel, Dispatch, Error, ErrorCode, Model, TurnRequest, VecSink};
use llm_http::{HttpClient, Limits};
use llm_messages::{MessagesClient, decode_request};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use support::{StaticResolver, binding, binding_with, capabilities};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const URL: &str = "https://images.example/a.png";
const PNG_BASE64: &str = "iVBORw0KGgo=";

fn with(messages: &Value) -> Value {
    json!({"model": "caller-alias", "max_tokens": 512, "messages": messages})
}

fn user_blocks(blocks: &Value) -> Value {
    with(&json!([{"role": "user", "content": blocks}]))
}

/// Every byte one accepted connection delivers until the request is whole, the peer closes, or
/// nothing arrives for a second.
async fn received(listener: TcpListener) -> Vec<u8> {
    let Ok(Ok((mut socket, _))) =
        tokio::time::timeout(Duration::from_secs(10), listener.accept()).await
    else {
        return Vec::new();
    };
    let mut bytes = Vec::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        match tokio::time::timeout(Duration::from_secs(1), socket.read(&mut buffer)).await {
            Ok(Ok(length)) if length > 0 => bytes.extend_from_slice(&buffer[..length]),
            _ => break,
        }
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let header = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
            let length = header
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if bytes.len() >= end + 4 + length {
                break;
            }
        }
    }
    let _ = socket
        .write_all(
            b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )
        .await;
    bytes
}

/// Decodes `body` as an arriving Messages request and, if it decodes, sends the turn upstream to
/// a loopback listener. Returns the decoder's answer and every byte the listener received.
async fn serve(body: &Value) -> (Result<TurnRequest, Error>, Vec<u8>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let decoded =
        decode_request(body.to_string().as_bytes(), binding().provenance()).map(|i| i.request);
    let Ok(request) = &decoded else {
        let connected = tokio::time::timeout(Duration::from_millis(50), listener.accept()).await;
        assert!(
            connected.is_err(),
            "a connection arrived for a refused request"
        );
        return (decoded, Vec::new());
    };
    let server = tokio::spawn(received(listener));
    let mut upstream = request.clone();
    // The wire's model is the caller's routing name; routing would select the binding for it.
    "internal-model".clone_into(&mut upstream.model);
    let total = Duration::from_secs(10);
    let client = MessagesClient::new(
        binding_with(&url, capabilities()),
        HttpClient::new(Limits {
            response_headers: total,
            idle: total,
            total,
        })
        .unwrap(),
        Arc::new(StaticResolver::new("fixture-key")),
    )
    .expect("a Messages binding");
    let mut sink = VecSink::new(16, 4096);
    let _ = client.turn(&upstream, &mut sink, &Cancel::new()).await;
    (decoded, server.await.unwrap())
}

/// The refusal `body` must meet: nothing sent upstream, then exactly `code` and `message`.
async fn refused_and_nothing_sent(body: &Value, code: ErrorCode, message: &str) {
    let (decoded, sent) = serve(body).await;
    assert!(
        sent.is_empty(),
        "{} bytes went upstream for {body}; the image reached the wire: {}",
        sent.len(),
        String::from_utf8_lossy(&sent).contains("\"image\"")
    );
    let error = decoded.expect_err("refused");
    assert_eq!(error.code, code, "{body}");
    assert_eq!(error.dispatch, Dispatch::NotSent, "{body}");
    assert_eq!(error.message, message, "{body}");
}

/// The fixture is live: a request the decoder accepts does reach the listener, so an empty
/// capture in the cases below means the refusal held, not that nothing could have been sent.
#[tokio::test]
async fn an_accepted_image_request_does_reach_the_listener() {
    let (decoded, sent) = serve(&user_blocks(&json!([
        {"type": "text", "text": "Look"},
        {"type": "image", "source": {"type": "url", "url": URL}}
    ])))
    .await;
    decoded.expect("accepted");
    let sent = String::from_utf8(sent).expect("UTF-8 request");
    let (_, body) = sent.split_once("\r\n\r\n").expect("a request body");
    let body: Value = serde_json::from_str(body).expect("a JSON body");
    assert_eq!(
        body["messages"][0]["content"][1],
        json!({"type": "image", "source": {"type": "url", "url": URL},
               "cache_control": {"type": "ephemeral"}})
    );
}

/// Aimed at `Messages image block carries no source` (`crates/llm-messages/src/codec.rs`,
/// `decode_block`). No case pinned this text before this file.
#[tokio::test]
async fn an_image_block_without_a_source_is_refused_and_nothing_is_sent() {
    for blocks in [
        json!([{"type": "image"}]),
        json!([{"type": "text", "text": "Look"}, {"type": "image"}]),
    ] {
        refused_and_nothing_sent(
            &user_blocks(&blocks),
            ErrorCode::InvalidRequest,
            "Messages image block carries no source",
        )
        .await;
    }
}

/// Aimed at `Messages image block is carried only in a user message: {at}`: an image in an
/// assistant message, opening it or between its other blocks, is never moved into a user message.
#[tokio::test]
async fn an_image_block_in_an_assistant_message_is_refused_and_nothing_is_sent() {
    let url_image = json!({"type": "image", "source": {"type": "url", "url": URL}});
    let inline = json!({"type": "image", "source":
        {"type": "base64", "media_type": "image/png", "data": PNG_BASE64}});
    for messages in [
        json!([{"role": "user", "content": "hi"}, {"role": "assistant", "content": [url_image]}]),
        json!([{"role": "user", "content": "hi"},
               {"role": "assistant", "content": [{"type": "text", "text": "A"}, inline,
                                                 {"type": "text", "text": "B"}]},
               {"role": "user", "content": "and?"}]),
    ] {
        refused_and_nothing_sent(
            &with(&messages),
            ErrorCode::Unsupported,
            "Messages image block is carried only in a user message: messages[].content[]",
        )
        .await;
    }
}

/// Aimed at `Messages image media type is none of image/png, image/jpeg, image/gif and
/// image/webp`: no other name, no other spelling of the four and no common alias is read as one.
#[tokio::test]
async fn an_image_of_a_media_type_outside_the_four_is_refused_and_nothing_is_sent() {
    for media_type in [
        "image/bmp",
        "image/svg+xml",
        "image/jpg",
        "IMAGE/PNG",
        "image/png ",
        "image/png; charset=binary",
        "",
    ] {
        refused_and_nothing_sent(
            &user_blocks(&json!([{"type": "image", "source":
                {"type": "base64", "media_type": media_type, "data": PNG_BASE64}}])),
            ErrorCode::Unsupported,
            "Messages image media type is none of image/png, image/jpeg, image/gif and image/webp",
        )
        .await;
    }
}

/// Aimed at `Messages image source type is outside the declared subset`: a `file` source, a source
/// with no type, and the two carried types in another spelling are each refused, never read as a
/// `base64` or `url` source.
#[tokio::test]
async fn an_image_source_of_another_type_is_refused_and_nothing_is_sent() {
    for source in [
        json!({"type": "file", "file_id": "file_1"}),
        json!({"media_type": "image/png", "data": PNG_BASE64}),
        json!({"url": URL}),
        json!({"type": "BASE64", "media_type": "image/png", "data": PNG_BASE64}),
        json!({"type": "URL", "url": URL}),
        json!({"type": 1, "url": URL}),
        json!({"type": null, "url": URL}),
    ] {
        refused_and_nothing_sent(
            &user_blocks(&json!([{"type": "image", "source": source}])),
            ErrorCode::Unsupported,
            "Messages image source type is outside the declared subset",
        )
        .await;
    }
}

/// The published `ImageBlockParam` also has `transformations`; this version does not carry it, so
/// it is refused by its path rather than dropped.
#[tokio::test]
async fn an_image_block_asking_for_transformations_is_refused_by_its_path_and_nothing_is_sent() {
    refused_and_nothing_sent(
        &user_blocks(
            &json!([{"type": "image", "source": {"type": "url", "url": URL},
            "transformations": {"oversized_image": "error"}}]),
        ),
        ErrorCode::Unsupported,
        "Messages field is outside the declared subset: messages[].content[].transformations",
    )
    .await;
}
