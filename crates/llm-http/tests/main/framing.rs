use llm_core::ErrorCode;
use llm_http::{Framing, MAX_EVENT_BYTES, MAX_STREAM_BYTES, SseDecoder, SseEvent};
use serde_json::json;

#[test]
fn every_chunk_boundary_preserves_utf8_crlf_multiline_data_and_event_names() {
    let bytes = "\u{feff}:comment\r\nevent: delta\r\ndata: {\"text\":\r\ndata: \"🦀\"}\r\n\r\ndata: [DONE]\r\n\r\n".as_bytes();
    let expected = vec![
        Ok(SseEvent::Payload {
            event: Some("delta".into()),
            data: json!({"text":"🦀"}),
        }),
        Ok(SseEvent::Done),
    ];
    for split in 0..=bytes.len() {
        let mut decoder = SseDecoder::new(Framing::DoneSentinel);
        let mut events = decoder.push(&bytes[..split]);
        events.extend(decoder.push(&bytes[split..]));
        assert_eq!(events, expected, "split {split}");
        decoder.finish().unwrap();
    }
}

#[test]
fn malformed_frame_cannot_erase_an_already_received_prefix() {
    let bytes = b"data: {\"text\":\"visible\"}\n\ndata: not-json\n\n";
    for split in 0..bytes.len() {
        let mut decoder = SseDecoder::new(Framing::PayloadsOnly);
        let mut events = decoder.push(&bytes[..split]);
        events.extend(decoder.push(&bytes[split..]));
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0],
            Ok(SseEvent::Payload {
                event: None,
                data: json!({"text":"visible"})
            })
        );
        assert_eq!(events[1].as_ref().unwrap_err().code, ErrorCode::Protocol);
        assert!(decoder.finish().is_err());
    }
}

#[test]
fn sentinel_is_explicit_and_eof_never_invents_a_terminal_event() {
    let mut decoder = SseDecoder::new(Framing::PayloadsOnly);
    assert_eq!(
        decoder.push(b"data: [DONE]\n\n")[0]
            .as_ref()
            .unwrap_err()
            .code,
        ErrorCode::Protocol
    );
    let mut decoder = SseDecoder::new(Framing::DoneSentinel);
    assert_eq!(
        decoder.push(b""),
        [] as [Result<SseEvent, llm_core::Error>; 0]
    );
    decoder.finish().unwrap();
    assert_eq!(
        decoder.push(b"data: {}"),
        [] as [Result<SseEvent, llm_core::Error>; 0]
    );
    assert!(decoder.finish().is_err());
    let mut decoder = SseDecoder::new(Framing::DoneSentinel);
    let events = decoder.push(b"data: [DONE]\n\ndata: {}\n\n");
    assert_eq!(events[0], Ok(SseEvent::Done));
    assert!(events[1].is_err());
}

#[test]
fn line_event_and_stream_limits_hold_across_chunks() {
    let mut decoder = SseDecoder::new(Framing::PayloadsOnly);
    let long_line = vec![b'x'; MAX_EVENT_BYTES];
    assert_eq!(
        decoder.push(&long_line),
        [] as [Result<SseEvent, llm_core::Error>; 0]
    );
    assert_eq!(
        decoder.push(b"x")[0].as_ref().unwrap_err().code,
        ErrorCode::TooLarge
    );

    let mut decoder = SseDecoder::new(Framing::PayloadsOnly);
    let line = format!("data: {}\n", " ".repeat(MAX_EVENT_BYTES / 2));
    assert_eq!(
        decoder.push(line.as_bytes()),
        [] as [Result<SseEvent, llm_core::Error>; 0]
    );
    assert_eq!(
        decoder.push(line.as_bytes())[0].as_ref().unwrap_err().code,
        ErrorCode::TooLarge
    );

    let mut decoder = SseDecoder::new(Framing::PayloadsOnly);
    let comment = format!(":{}\n", "x".repeat(1022));
    for _ in 0..MAX_STREAM_BYTES / 1024 {
        assert_eq!(
            decoder.push(comment.as_bytes()),
            [] as [Result<SseEvent, llm_core::Error>; 0]
        );
    }
    assert_eq!(
        decoder.push(b"x")[0].as_ref().unwrap_err().code,
        ErrorCode::TooLarge
    );
}
