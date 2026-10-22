//! The event-stream codec against the smithy-rs test data and the recorded Bedrock bodies
//! (`tests/data/README.md` says where each comes from).

use llm_bedrock::eventstream::{
    HeaderValue, MAX_FRAME_BYTES, Message, MessageDecoder, decode_message, encode_message,
};
use llm_core::{Dispatch, Error, ErrorCode};
use llm_http::MAX_STREAM_BYTES;
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Every recorded event-stream body and its frame count. The counts were read off the bytes with
/// a decoder written apart from this crate, each frame's two checksums verified there.
const RECORDED: [(&str, usize); 9] = [
    ("ddtrace-anthropic_message_invoke_stream.bin", 23),
    ("ddtrace-bedrock_converse_stream.bin", 32),
    ("ddtrace-bedrock_converse_stream_prompt_caching-0.bin", 103),
    ("ddtrace-bedrock_converse_stream_prompt_caching-1.bin", 55),
    ("pydantic-test_bedrock_model_iter_stream-0.bin", 26),
    ("pydantic-test_bedrock_model_iter_stream-1.bin", 9),
    ("pydantic-test_bedrock_model_stream.bin", 33),
    (
        "pydantic-test_bedrock_model_stream_empty_text_delta.bin",
        10,
    ),
    ("pydantic-test_bedrock_model_thinking_part_stream.bin", 25),
];

fn data(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(relative)
}

fn read(relative: &str) -> Vec<u8> {
    fs::read(data(relative)).unwrap_or_else(|error| panic!("{relative}: {error}"))
}

fn header(name: &str, value: HeaderValue) -> (String, HeaderValue) {
    (name.to_owned(), value)
}

fn string(text: &str) -> HeaderValue {
    HeaderValue::String(text.to_owned())
}

/// Every result one decoder gives for `chunks`, then its `finish`.
fn decode_chunks<'a>(
    chunks: impl IntoIterator<Item = &'a [u8]>,
) -> (Vec<Result<Message, Error>>, Result<(), Error>) {
    let mut decoder = MessageDecoder::new();
    let mut results = Vec::new();
    for chunk in chunks {
        results.extend(decoder.push(chunk));
    }
    let finished = decoder.finish();
    (results, finished)
}

/// The messages of a body that must decode whole, with nothing left over.
fn decode_whole(name: &str, bytes: &[u8]) -> Vec<Message> {
    let (results, finished) = decode_chunks([bytes]);
    finished.unwrap_or_else(|error| panic!("{name}: {error}"));
    results
        .into_iter()
        .collect::<Result<_, _>>()
        .unwrap_or_else(|error| panic!("{name}: {error}"))
}

fn event_types(messages: &[Message]) -> Vec<&str> {
    messages
        .iter()
        .map(|message| message.string_header(":event-type").unwrap())
        .collect()
}

/// The first failure among `results`, which must hold no message.
fn only_failure(name: &str, results: Vec<Result<Message, Error>>) -> Error {
    assert!(
        results.iter().all(Result::is_err),
        "{name}: a message decoded"
    );
    results
        .into_iter()
        .find_map(Result::err)
        .unwrap_or_else(|| panic!("{name}: no failure"))
}

/// A bitwise CRC32, written apart from the crate's table so each checks the other.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// A prelude with a correct checksum, claiming `total` bytes and `headers` header bytes.
fn prelude(total: usize, headers: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&u32::try_from(total).unwrap().to_be_bytes());
    bytes.extend_from_slice(&headers.to_be_bytes());
    let checksum = crc32(&bytes);
    bytes.extend_from_slice(&checksum.to_be_bytes());
    bytes
}

#[test]
fn every_header_type_decodes_as_smithy_states_and_encodes_back_byte_for_byte() {
    let bytes = read("eventstream/valid_with_all_headers_and_payload");
    let expected = Message {
        headers: vec![
            header("true", HeaderValue::Bool(true)),
            header("false", HeaderValue::Bool(false)),
            header("byte", HeaderValue::Byte(50)),
            header("short", HeaderValue::Int16(20_000)),
            header("int", HeaderValue::Int32(500_000)),
            header("long", HeaderValue::Int64(50_000_000_000)),
            header("bytes", HeaderValue::ByteArray(b"some bytes".to_vec())),
            header("str", string("some str")),
            header("time", HeaderValue::Timestamp(5_000_000_000)),
            header(
                "uuid",
                HeaderValue::Uuid([
                    0xb7, 0x9b, 0xc9, 0x14, 0xde, 0x21, 0x4e, 0x13, 0xb8, 0xb2, 0xbc, 0x47, 0xe8,
                    0x5b, 0x7f, 0x0b,
                ]),
            ),
        ],
        payload: b"some payload".to_vec(),
    };
    let (message, consumed) = decode_message(&bytes).unwrap();
    assert_eq!((&message, consumed), (&expected, bytes.len()));
    assert_eq!(encode_message(&expected), bytes);
    assert_eq!(message.string_header("str"), Some("some str"));
    assert_eq!(message.string_header("bytes"), None);
    assert_eq!(message.header("int"), Some(&HeaderValue::Int32(500_000)));
    assert_eq!(message.header("absent"), None);
}

#[test]
fn a_frame_without_payload_and_one_without_headers_decode_and_encode_back() {
    let cases = [
        (
            "valid_empty_payload",
            Message {
                headers: vec![header("some-header", HeaderValue::Int16(500))],
                payload: Vec::new(),
            },
        ),
        (
            "valid_no_headers",
            Message {
                headers: Vec::new(),
                payload: b"another test payload".to_vec(),
            },
        ),
    ];
    for (name, expected) in cases {
        let bytes = read(&format!("eventstream/{name}"));
        assert_eq!(
            decode_message(&bytes).unwrap(),
            (expected.clone(), bytes.len()),
            "{name}"
        );
        assert_eq!(encode_message(&expected), bytes, "{name}");
    }
}

#[test]
fn the_test_checksum_agrees_with_a_recorded_prelude() {
    let bytes = read("eventstream/valid_with_all_headers_and_payload");
    assert_eq!(prelude(150, 122), bytes[..12]);
}

#[test]
fn each_invalid_smithy_frame_is_refused_by_the_check_it_breaks() {
    let cases = [
        (
            "invalid_prelude_checksum",
            "event-stream prelude checksum does not match",
        ),
        (
            "invalid_message_checksum",
            "event-stream message checksum does not match",
        ),
        (
            "invalid_header_value_type",
            "event-stream header value type is unknown",
        ),
        (
            "invalid_header_name_length",
            "event-stream header name length is zero or exceeds the headers",
        ),
        // smithy-rs reads this name past the end of the headers and refuses it as not UTF-8;
        // read inside the headers, its length is what fails first.
        (
            "invalid_header_name_length_too_long",
            "event-stream header name length is zero or exceeds the headers",
        ),
        (
            "invalid_headers_length",
            "event-stream headers length does not fit the frame",
        ),
        (
            "invalid_header_string_length_cut_off",
            "event-stream header string length exceeds the headers",
        ),
        (
            "invalid_header_string_value_length",
            "event-stream header string length exceeds the headers",
        ),
    ];
    for (name, refusal) in cases {
        let bytes = read(&format!("eventstream/{name}"));
        let error = decode_message(&bytes).unwrap_err();
        assert_eq!(
            (error.code, error.message.as_str(), error.retriable),
            (ErrorCode::Protocol, refusal, false),
            "{name}"
        );
        let (whole, finished) = decode_chunks([bytes.as_slice()]);
        assert_eq!(only_failure(name, whole).message, refusal, "{name}");
        assert!(finished.is_err(), "{name}");
        let (bytewise, finished) = decode_chunks(bytes.chunks(1));
        assert_eq!(only_failure(name, bytewise).message, refusal, "{name}");
        assert!(finished.is_err(), "{name}");
    }
}

#[test]
fn frames_before_a_malformed_one_come_out_and_the_decoder_then_stays_failed() {
    let first = read("eventstream/valid_no_headers");
    let mut bytes = first.clone();
    bytes.extend(read("eventstream/invalid_message_checksum"));
    bytes.extend(read("eventstream/valid_empty_payload"));
    for split in 0..=bytes.len() {
        let mut decoder = MessageDecoder::new();
        let mut results = decoder.push(&bytes[..split]);
        results.extend(decoder.push(&bytes[split..]));
        // A push after the failure repeats it; nothing after the malformed frame decodes.
        assert!((2..=3).contains(&results.len()), "split {split}");
        assert_eq!(
            results[0],
            Ok(decode_message(&first).unwrap().0),
            "split {split}"
        );
        assert_eq!(
            results[1].as_ref().unwrap_err().message,
            "event-stream message checksum does not match",
            "split {split}"
        );
        assert!(
            results[2..].iter().all(|result| result
                .as_ref()
                .is_err_and(|error| error.message == "event-stream decoder previously failed")),
            "split {split}"
        );
        let later = decoder.push(&first);
        assert_eq!(later.len(), 1);
        assert_eq!(
            later[0].as_ref().unwrap_err().message,
            "event-stream decoder previously failed"
        );
        assert_eq!(
            decoder.finish().unwrap_err().message,
            "event-stream decoder previously failed"
        );
    }
}

#[test]
fn the_recorded_bodies_are_exactly_the_files_counted_here() {
    let mut found: Vec<String> = fs::read_dir(data("frames"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "bin"))
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    found.sort();
    let counted: Vec<&str> = RECORDED.iter().map(|(name, _)| *name).collect();
    assert_eq!(found, counted);
}

#[test]
fn every_recorded_body_decodes_whole_and_each_frame_encodes_back_to_its_bytes() {
    for (name, frames) in RECORDED {
        let bytes = read(&format!("frames/{name}"));
        let messages = decode_whole(name, &bytes);
        assert_eq!(messages.len(), frames, "{name}");
        let mut at = 0;
        for message in &messages {
            let (decoded, consumed) = decode_message(&bytes[at..]).unwrap();
            assert_eq!(&decoded, message, "{name} at {at}");
            assert_eq!(encode_message(message), bytes[at..at + consumed], "{name}");
            assert_eq!(message.string_header(":message-type"), Some("event"));
            assert_eq!(
                message.string_header(":content-type"),
                Some("application/json")
            );
            assert!(
                serde_json::from_slice::<serde_json::Value>(&message.payload)
                    .unwrap()
                    .is_object(),
                "{name} at {at}"
            );
            at += consumed;
        }
        assert_eq!(at, bytes.len(), "{name}");
    }
}

#[test]
fn every_recorded_body_decodes_alike_at_every_split_into_two_chunks() {
    for (name, _) in RECORDED {
        let bytes = read(&format!("frames/{name}"));
        let whole = decode_whole(name, &bytes);
        for split in 0..=bytes.len() {
            let (head, tail) = bytes.split_at(split);
            let (results, finished) = decode_chunks([head, tail]);
            assert!(finished.is_ok(), "{name} split {split}");
            assert!(
                results.len() == whole.len()
                    && results
                        .iter()
                        .zip(&whole)
                        .all(|(got, want)| got.as_ref() == Ok(want)),
                "{name} split {split}"
            );
        }
    }
}

#[test]
fn every_recorded_body_decodes_alike_fed_one_byte_at_a_time() {
    for (name, _) in RECORDED {
        let bytes = read(&format!("frames/{name}"));
        let whole = decode_whole(name, &bytes);
        let (results, finished) = decode_chunks(bytes.chunks(1));
        assert!(finished.is_ok(), "{name}");
        let messages: Vec<Message> = results
            .into_iter()
            .collect::<Result<_, _>>()
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert!(messages == whole, "{name}");
    }
}

#[test]
fn a_converse_stream_and_an_invoke_stream_carry_their_recorded_event_sequences() {
    let name = "pydantic-test_bedrock_model_stream.bin";
    let converse = decode_whole(name, &read(&format!("frames/{name}")));
    let mut expected = vec!["messageStart"];
    expected.extend(["contentBlockDelta"; 29]);
    expected.extend(["contentBlockStop", "messageStop", "metadata"]);
    assert_eq!(event_types(&converse), expected);

    let name = "ddtrace-anthropic_message_invoke_stream.bin";
    let invoke = decode_whole(name, &read(&format!("frames/{name}")));
    assert_eq!(event_types(&invoke), ["chunk"; 23]);
}

#[test]
fn a_stream_cut_inside_a_frame_fails_finish_as_a_retriable_cut() {
    let bytes = read("eventstream/valid_with_all_headers_and_payload");
    for cut in 1..bytes.len() {
        let (results, finished) = decode_chunks([&bytes[..cut]]);
        assert!(results.is_empty(), "cut {cut}");
        let error = finished.unwrap_err();
        assert_eq!(
            (
                error.code,
                error.message.as_str(),
                error.dispatch,
                error.retriable
            ),
            (
                ErrorCode::Protocol,
                "event-stream ended inside a frame",
                Dispatch::Unknown,
                true
            ),
            "cut {cut}"
        );
        assert_eq!(
            decode_message(&bytes[..cut]).unwrap_err().message,
            "event-stream frame is truncated",
            "cut {cut}"
        );
    }
    let (results, finished) = decode_chunks([bytes.as_slice()]);
    assert_eq!(results.len(), 1);
    assert!(finished.is_ok());
    assert!(MessageDecoder::new().finish().is_ok());
}

#[test]
fn a_frame_length_outside_its_bounds_is_refused_from_the_prelude_alone() {
    let (results, finished) = decode_chunks([prelude(MAX_FRAME_BYTES, 0).as_slice()]);
    assert_eq!(results, [] as [Result<Message, Error>; 0]);
    assert_eq!(
        finished.unwrap_err().message,
        "event-stream ended inside a frame"
    );

    let past = prelude(MAX_FRAME_BYTES + 1, 0);
    let (results, finished) = decode_chunks([past.as_slice()]);
    let error = only_failure("past the frame bound", results);
    assert_eq!(
        (error.code, error.message.as_str()),
        (
            ErrorCode::TooLarge,
            "event-stream frame exceeds the 16 MiB bound"
        )
    );
    assert!(finished.is_err());
    assert_eq!(decode_message(&past).unwrap_err().code, ErrorCode::TooLarge);

    let short = prelude(15, 0);
    let error = decode_message(&short).unwrap_err();
    assert_eq!(
        (error.code, error.message.as_str()),
        (
            ErrorCode::Protocol,
            "event-stream frame length is below the 16-byte minimum"
        )
    );
    let (results, _) = decode_chunks([short.as_slice()]);
    assert_eq!(only_failure("short", results), error);
}

#[test]
fn a_stream_past_its_byte_bound_is_refused_and_the_decoder_stays_failed() {
    let frames = 8;
    let frame = encode_message(&Message {
        headers: Vec::new(),
        payload: vec![b'x'; MAX_STREAM_BYTES / frames - 16],
    });
    assert_eq!(frame.len() * frames, MAX_STREAM_BYTES);
    assert!(frame.len() <= MAX_FRAME_BYTES);
    let mut decoder = MessageDecoder::new();
    for index in 0..frames {
        let results = decoder.push(&frame);
        assert_eq!(results.len(), 1, "frame {index}");
        assert!(results[0].is_ok(), "frame {index}");
    }
    decoder.finish().unwrap();

    let results = decoder.push(b"x");
    let error = only_failure("past the stream bound", results);
    assert_eq!(
        (error.code, error.message.as_str()),
        (
            ErrorCode::TooLarge,
            "event-stream stream exceeds the byte bound"
        )
    );
    let later = decoder.push(&read("eventstream/valid_no_headers"));
    assert_eq!(
        only_failure("after the bound", later).message,
        "event-stream decoder previously failed"
    );
    assert_eq!(
        decoder.finish().unwrap_err().message,
        "event-stream decoder previously failed"
    );
}
