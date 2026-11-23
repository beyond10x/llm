//! The public usage reader (story:usage-reader): the model and the counters a Responses answer
//! reported, read from its bytes in pieces split anywhere, for a stream and for a body that is
//! not streamed. It must report what the turn decoder reports, and nothing it did not report.
//! No provider is contacted.

use llm_core::{Id, Protocol, Provenance, ReportedUsage, Usage};
use llm_responses::{Binding, UsageReader, decode_stream, read_usage};
use serde_json::Value;

/// The recorded live Codex text turn (`fixtures/live/README.md`).
const RECORDED: &[u8] = include_bytes!("fixtures/live/codex-text-turn.sse");

/// A Responses answer that was not streamed: the response object itself.
const BODY: &str = r#"{"id":"resp_1","object":"response","status":"completed","model":"gpt-5.1-2025-11-13",
"output":[{"type":"message","id":"msg_1","role":"assistant","content":[{"type":"output_text","text":"ok"}]}],
"usage":{"input_tokens":120,"input_tokens_details":{"cached_tokens":64},"output_tokens":9,
"output_tokens_details":{"reasoning_tokens":5},"total_tokens":129}}"#;

fn id(value: &str) -> Id {
    Id::new(value).expect("fixture identifier")
}

fn binding() -> Binding {
    Binding::new(
        Provenance {
            protocol: Protocol::Responses,
            provider: id("codex"),
            account: id("local"),
            endpoint: id("local-endpoint"),
            model: id("astra"),
            binding_revision: id("rev-1"),
        },
        id("gpt-6-astra"),
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

/// The `data:` payloads of a recorded stream, in order.
fn payloads(stream: &[u8]) -> Vec<Value> {
    std::str::from_utf8(stream)
        .expect("a UTF-8 recording")
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .map(|data| serde_json::from_str(data).expect("a recorded JSON payload"))
        .collect()
}

#[test]
fn the_recorded_codex_stream_reports_its_model_and_every_counter_the_decoder_reads() {
    let report = read_usage(RECORDED);
    assert_eq!(report.model, Some(id("gpt-6-astra")));
    assert_eq!(report.usage.input_tokens, Some(34));
    assert_eq!(report.usage.output_tokens, Some(80));
    assert_eq!(report.usage.cached_input_tokens, Some(0));
    assert_eq!(report.usage.reasoning_output_tokens, Some(50));
    assert!(report.final_usage);

    // One reading of the usage, not two: the reader reports exactly what the turn decoder does.
    let decoded = decode_stream(&binding(), &payloads(RECORDED))
        .result
        .expect("the recorded turn decodes");
    assert_eq!(report.model, decoded.observation.upstream_model);
    assert_eq!(Some(report.usage), decoded.observation.usage);
    assert_eq!(report.final_usage, decoded.observation.final_usage);
}

#[test]
fn every_split_of_the_recorded_codex_stream_reads_the_same() {
    assert_eq!(every_way(RECORDED), read_usage(RECORDED));
}

#[test]
fn a_response_body_that_is_not_streamed_reports_its_model_and_counters() {
    let report = every_way(BODY.as_bytes());
    assert_eq!(
        report,
        ReportedUsage {
            model: Some(id("gpt-5.1-2025-11-13")),
            usage: Usage {
                input_tokens: Some(120),
                output_tokens: Some(9),
                cached_input_tokens: Some(64),
                cache_creation_input_tokens: None,
                reasoning_output_tokens: Some(5),
            },
            final_usage: true,
        }
    );
}

#[test]
fn a_counter_the_answer_did_not_report_stays_unknown_and_is_never_zero() {
    let body = br#"{"id":"resp_2","object":"response","status":"completed","output":[],
        "usage":{"output_tokens":7}}"#;
    let report = every_way(body);
    assert_eq!(
        report,
        ReportedUsage {
            model: None,
            usage: Usage {
                output_tokens: Some(7),
                ..Usage::default()
            },
            final_usage: true,
        }
    );
    let stream = b"event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_3\",\"status\":\"completed\",\"output\":[]}}\n\n";
    assert_eq!(
        every_way(stream),
        ReportedUsage {
            final_usage: true,
            ..ReportedUsage::default()
        }
    );
}

#[test]
fn a_stream_cut_before_its_terminal_object_names_the_model_and_is_not_final() {
    let text = std::str::from_utf8(RECORDED).expect("a UTF-8 recording");
    let cut = text
        .find("event: response.completed")
        .expect("the recording completes");
    let report = every_way(&RECORDED[..cut]);
    assert_eq!(
        report,
        ReportedUsage {
            model: Some(id("gpt-6-astra")),
            usage: Usage::default(),
            final_usage: false,
        }
    );
}
