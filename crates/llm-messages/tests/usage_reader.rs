//! The public usage reader (story:usage-reader): the model and the counters a Messages answer
//! reported, read from its bytes in pieces split anywhere, for a stream and for a message that is
//! not streamed. It must report what the turn decoder reports, and nothing it did not report.
//! No provider is contacted.
mod support;

use llm_core::{Cancel, Id, Item, ReportedUsage, TurnOutcome, TurnRequest, Usage, VecSink};
use llm_messages::{UsageReader, decode_message, decode_stream, read_usage};
use serde_json::Value;
use support::binding;

/// The two turns recorded from `api.anthropic.com` (`fixtures/live/README.md`): the first wrote
/// the prompt prefix to the cache, the second read it back.
const WRITE_EVENTS: &[u8] = include_bytes!("fixtures/live/messages-cache-write.sse");
const READ_EVENTS: &[u8] = include_bytes!("fixtures/live/messages-cache-read.sse");
const RECORDED_MODEL: &str = "claude-haiku-4-5-20251001";
const CACHED: u64 = 13_027;

/// A Messages answer that was not streamed.
const MESSAGE: &str = r#"{"id":"msg_01","type":"message","role":"assistant","model":"claude-sonnet-4-5-20250929",
"content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn","stop_sequence":null,
"usage":{"input_tokens":12,"cache_creation_input_tokens":100,"cache_read_input_tokens":2000,
"output_tokens":7,"output_tokens_details":{"thinking_tokens":3}}}"#;

fn id(value: &str) -> Id {
    Id::new(value).expect("fixture identifier")
}

fn request() -> TurnRequest {
    TurnRequest::new(
        "internal-model",
        vec![Item::user("Reply with the single word: ok")],
    )
}

fn read_in(pieces: &[&[u8]]) -> ReportedUsage {
    let mut reader = UsageReader::new();
    for piece in pieces {
        reader.feed(piece);
    }
    reader.finish()
}

/// What the reader reports for `body` fed whole, after asserting that every split into two
/// pieces and one byte at a time report exactly the same.
fn every_way(body: &[u8]) -> ReportedUsage {
    let whole = read_usage(body);
    for cut in 0..=body.len() {
        let (left, right) = body.split_at(cut);
        assert_eq!(read_in(&[left, right]), whole, "split at byte {cut}");
    }
    let bytes: Vec<&[u8]> = body.chunks(1).collect();
    assert_eq!(read_in(&bytes), whole, "fed one byte at a time");
    whole
}

async fn decoded(stream: &[u8]) -> TurnOutcome {
    let mut sink = VecSink::new(64, 64 * 1024);
    decode_stream(
        stream,
        &request(),
        binding().provenance(),
        &mut sink,
        &Cancel::new(),
    )
    .await
    .expect("the recorded turn decodes")
}

/// The reader reports exactly the model and counters the turn decoder reports for `stream`.
async fn agrees_with_the_decoder(stream: &[u8], report: &ReportedUsage) {
    let outcome = decoded(stream).await;
    assert_eq!(report.model, outcome.observation.upstream_model);
    assert_eq!(Some(&report.usage), outcome.observation.usage.as_ref());
    assert_eq!(report.final_usage, outcome.observation.final_usage);
}

#[tokio::test]
async fn the_recorded_cache_write_turn_reports_its_model_and_every_counter_the_decoder_reads() {
    let report = every_way(WRITE_EVENTS);
    assert_eq!(
        report,
        ReportedUsage {
            model: Some(id(RECORDED_MODEL)),
            usage: Usage {
                input_tokens: Some(3 + CACHED),
                output_tokens: Some(4),
                cached_input_tokens: Some(0),
                cache_creation_input_tokens: Some(CACHED),
                reasoning_output_tokens: None,
            },
            final_usage: true,
        }
    );
    agrees_with_the_decoder(WRITE_EVENTS, &report).await;
}

#[tokio::test]
async fn the_recorded_cache_read_turn_reports_its_model_and_every_counter_the_decoder_reads() {
    let report = every_way(READ_EVENTS);
    assert_eq!(
        report,
        ReportedUsage {
            model: Some(id(RECORDED_MODEL)),
            usage: Usage {
                input_tokens: Some(3 + CACHED),
                output_tokens: Some(4),
                cached_input_tokens: Some(CACHED),
                cache_creation_input_tokens: Some(0),
                reasoning_output_tokens: None,
            },
            final_usage: true,
        }
    );
    agrees_with_the_decoder(READ_EVENTS, &report).await;
}

#[test]
fn a_message_that_is_not_streamed_reports_its_model_and_counters_as_the_decoder_does() {
    let report = every_way(MESSAGE.as_bytes());
    assert_eq!(
        report,
        ReportedUsage {
            model: Some(id("claude-sonnet-4-5-20250929")),
            usage: Usage {
                input_tokens: Some(12 + 100 + 2000),
                output_tokens: Some(7),
                cached_input_tokens: Some(2000),
                cache_creation_input_tokens: Some(100),
                reasoning_output_tokens: Some(3),
            },
            final_usage: true,
        }
    );
    let message: Value = serde_json::from_str(MESSAGE).expect("a JSON message");
    let outcome =
        decode_message(&message, &request(), binding().provenance()).expect("the message decodes");
    assert_eq!(report.model, outcome.observation.upstream_model);
    assert_eq!(Some(report.usage), outcome.observation.usage);
}

#[test]
fn a_counter_the_route_never_reported_stays_unknown_and_is_never_zero() {
    // No cache counter and no input count: the input total cannot be known, and no counter the
    // route left out is reported as zero.
    let stream = b"event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_02\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"example-model\",\"content\":[],\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"output_tokens\":1}}}\n\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":4}}\n\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\n";
    assert_eq!(
        every_way(stream),
        ReportedUsage {
            model: Some(id("example-model")),
            usage: Usage {
                output_tokens: Some(4),
                ..Usage::default()
            },
            final_usage: true,
        }
    );
}

#[test]
fn a_stream_cut_before_message_stop_keeps_what_it_reported_and_is_not_final() {
    let text = std::str::from_utf8(WRITE_EVENTS).expect("a UTF-8 recording");
    let cut = text
        .find("event: message_stop")
        .expect("the recording stops");
    let report = every_way(&WRITE_EVENTS[..cut]);
    assert_eq!(report.model, Some(id(RECORDED_MODEL)));
    assert_eq!(report.usage.output_tokens, Some(4));
    assert!(!report.final_usage);
}
