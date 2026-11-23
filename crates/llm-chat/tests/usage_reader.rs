//! The public usage reader (story:usage-reader): the model and the counters a Chat Completions
//! answer reported, read from its bytes in pieces split anywhere, for a stream and for a body that
//! is not streamed. It must report what the turn decoder reports, and nothing it did not report.
//! No provider is contacted.
mod common;

use common::{binding, id};
use llm_chat::{UsageReader, decode_completion, project_response_bytes, read_usage};
use llm_core::{ReportedUsage, Usage};
use serde_json::Value;

const OPENAI_TEXT: &[u8] = include_bytes!("../fixtures/openai-text-and-usage.sse");
const PARTIAL_USAGE: &[u8] = include_bytes!("../fixtures/openai-partial-usage.sse");

/// Every recorded or pinned stream of this crate, by name.
const STREAMS: [(&str, &[u8]); 11] = [
    (
        "llama-server-text-and-usage",
        include_bytes!("../fixtures/llama-server-text-and-usage.sse"),
    ),
    (
        "llama-server-text-timings-only",
        include_bytes!("../fixtures/llama-server-text-timings-only.sse"),
    ),
    (
        "llama-server-timings-before-a-usage-chunk",
        include_bytes!("../fixtures/llama-server-timings-before-a-usage-chunk.sse"),
    ),
    (
        "llama-server-timings-before-the-last-chunk",
        include_bytes!("../fixtures/llama-server-timings-before-the-last-chunk.sse"),
    ),
    (
        "llama-server-tool-call-and-usage",
        include_bytes!("../fixtures/llama-server-tool-call-and-usage.sse"),
    ),
    ("openai-partial-usage", PARTIAL_USAGE),
    (
        "openai-refusal-and-reasoning",
        include_bytes!("../fixtures/openai-refusal-and-reasoning.sse"),
    ),
    ("openai-text-and-usage", OPENAI_TEXT),
    (
        "openai-tool-calls",
        include_bytes!("../fixtures/openai-tool-calls.sse"),
    ),
    (
        "vllm-text-no-usage",
        include_bytes!("../fixtures/vllm-text-no-usage.sse"),
    ),
    (
        "vllm-tool-call-and-usage",
        include_bytes!("../fixtures/vllm-tool-call-and-usage.sse"),
    ),
];

/// A Chat Completions answer that was not streamed.
const COMPLETION: &str = r#"{"id":"chatcmpl-1","object":"chat.completion","created":1772000000,
"model":"gpt-4o-mini-2024-07-18","choices":[{"index":0,"message":{"role":"assistant","content":"ok"},
"finish_reason":"stop"}],"usage":{"prompt_tokens":31,"completion_tokens":9,"total_tokens":40,
"prompt_tokens_details":{"cached_tokens":16},"completion_tokens_details":{"reasoning_tokens":4}}}"#;

fn read_in(pieces: &[&[u8]]) -> ReportedUsage {
    let mut reader = UsageReader::new();
    for piece in pieces {
        reader.feed(piece);
    }
    reader.finish()
}

/// What the reader reports for `body` fed whole, after asserting that it reports exactly the same
/// fed one byte at a time and split into two pieces at each of `cuts`.
fn alike(body: &[u8], cuts: impl IntoIterator<Item = usize>) -> ReportedUsage {
    let whole = read_usage(body);
    for cut in cuts {
        let (left, right) = body.split_at(cut);
        assert_eq!(read_in(&[left, right]), whole, "split at byte {cut}");
    }
    let bytes: Vec<&[u8]> = body.chunks(1).collect();
    assert_eq!(read_in(&bytes), whole, "fed one byte at a time");
    whole
}

/// [`alike`] at every split point.
fn every_way(body: &[u8]) -> ReportedUsage {
    alike(body, 0..=body.len())
}

#[test]
fn a_streamed_answer_reports_its_model_and_every_counter_reasoning_included() {
    assert_eq!(
        every_way(OPENAI_TEXT),
        ReportedUsage {
            model: Some(id("gpt-4o-mini-2024-07-18")),
            usage: Usage {
                input_tokens: Some(31),
                output_tokens: Some(9),
                cached_input_tokens: Some(16),
                cache_creation_input_tokens: None,
                reasoning_output_tokens: Some(4),
            },
            final_usage: true,
        }
    );
}

#[test]
fn every_pinned_stream_reads_alike_split_anywhere_and_as_the_decoder_reports_it() {
    // Every 17th split of each: all of them, 21 KB of readings of up to 4 KB, take half a minute
    // in a debug build. `a_streamed_answer_reports_its_model_and_every_counter_reasoning_included`
    // reads one of them at every split.
    let binding = binding();
    for (name, stream) in STREAMS {
        let report = alike(stream, (0..=stream.len()).step_by(17));
        let (_, outcome) = project_response_bytes(stream, binding.provenance());
        let outcome = outcome.unwrap_or_else(|error| panic!("{name} decodes: {error:?}"));
        assert_eq!(
            report.model, outcome.observation.upstream_model,
            "{name}: model"
        );
        assert_eq!(
            report.usage,
            outcome.observation.usage.unwrap_or_default(),
            "{name}: counters"
        );
        assert_eq!(
            report.final_usage, outcome.observation.final_usage,
            "{name}: final"
        );
    }
}

#[test]
fn a_completion_that_is_not_streamed_reports_its_model_and_counters_as_the_decoder_does() {
    let report = every_way(COMPLETION.as_bytes());
    assert_eq!(
        report,
        ReportedUsage {
            model: Some(id("gpt-4o-mini-2024-07-18")),
            usage: Usage {
                input_tokens: Some(31),
                output_tokens: Some(9),
                cached_input_tokens: Some(16),
                cache_creation_input_tokens: None,
                reasoning_output_tokens: Some(4),
            },
            final_usage: true,
        }
    );
    let body: Value = serde_json::from_str(COMPLETION).expect("a JSON completion");
    let outcome = decode_completion(&body, binding().provenance()).expect("the body decodes");
    assert_eq!(report.model, outcome.observation.upstream_model);
    assert_eq!(Some(report.usage), outcome.observation.usage);
}

#[test]
fn a_counter_a_usage_report_omits_stays_unknown_and_is_never_zero() {
    assert_eq!(
        every_way(PARTIAL_USAGE),
        ReportedUsage {
            model: Some(id("gpt-4o-mini-2024-07-18")),
            usage: Usage {
                input_tokens: Some(5),
                ..Usage::default()
            },
            final_usage: true,
        }
    );
}

#[test]
fn a_stream_cut_before_its_sentinel_keeps_what_it_reported_and_is_not_final() {
    let text = std::str::from_utf8(OPENAI_TEXT).expect("a UTF-8 fixture");
    let cut = text.find("data: [DONE]").expect("the fixture ends");
    let report = alike(&OPENAI_TEXT[..cut], (0..=cut).step_by(17));
    assert_eq!(report.usage.reasoning_output_tokens, Some(4));
    assert_eq!(report.model, Some(id("gpt-4o-mini-2024-07-18")));
    assert!(!report.final_usage);
}
