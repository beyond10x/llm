//! Adversary, wave 2026-10-20-w11, unit 2 (story:async-clients-without-a-tokio-runtime), pass 2.
//!
//! The Messages decoder is public and runs on any executor unless it was given a deadline
//! (`crates/llm-messages/src/lib.rs`: "`decode_stream`, which has no deadline, needs no runtime";
//! `crates/llm-messages/src/decode.rs` `emit`). Both halves, polled by a standard-library executor
//! on a thread that is in no Tokio runtime: without a deadline the decoder must not be refused,
//! and with one it must be refused where it would create its timer, not panic.
mod support;

use llm_core::{Cancel, Dispatch, Error, ErrorCode, Item, StreamEvent, TurnRequest, VecSink};
use llm_http::{Framing, SseDecoder};
use llm_messages::{StreamDecoder, decode_stream};
use serde_json::{Value, json};
use std::{
    any::Any,
    future::Future,
    panic::{AssertUnwindSafe, catch_unwind},
    pin::pin,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    thread::{self, Thread},
    time::{Duration, Instant},
};
use support::binding;

const REQUIREMENT: &str = "an llm client needs a Tokio runtime; run the call inside one, or use b10x-llm-blocking for a single model";

struct Unpark(Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// An executor that is not Tokio. Standard library only.
fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
        thread::park();
    }
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic without a message".to_owned())
}

fn text_turn() -> Vec<Value> {
    vec![
        json!({"type":"message_start","message":{"id":"msg_014a","type":"message","role":"assistant",
            "model":"example-model-20260201","content":[],"stop_reason":null,"stop_sequence":null,
            "usage":{"input_tokens":11,"output_tokens":1}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Guten "}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Tag"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},
            "usage":{"input_tokens":11,"output_tokens":8}}),
        json!({"type":"message_stop"}),
    ]
}

fn sse(events: &[Value]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for event in events {
        let name = event["type"].as_str().expect("event type");
        bytes.extend_from_slice(format!("event: {name}\ndata: {event}\n\n").as_bytes());
    }
    bytes
}

fn request() -> TurnRequest {
    TurnRequest::new("internal-model", vec![Item::user("Hallo")])
}

fn text(sink: &VecSink) -> String {
    sink.events()
        .iter()
        .filter_map(|event| match event {
            StreamEvent::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

/// `decode_stream` sets no deadline, so nothing in it needs Tokio: it decodes the whole turn on
/// an executor that is not Tokio, and its sink sees every delta.
#[test]
fn decode_stream_runs_to_its_outcome_on_an_executor_that_is_not_tokio() {
    let mut sink = VecSink::new(64, 64 * 1024);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        block_on(decode_stream(
            &sse(&text_turn()),
            &request(),
            binding().provenance(),
            &mut sink,
            &Cancel::new(),
        ))
    }))
    .map_err(|payload| panic_message(&*payload));
    match outcome {
        Ok(Ok(outcome)) => assert_eq!(outcome.stop_reason, llm_core::StopReason::EndTurn),
        other => panic!("decode_stream outside a Tokio runtime: {other:?}"),
    }
    assert_eq!(text(&sink), "Guten Tag");
}

/// The same events through a public `StreamDecoder` given a deadline: every event that hands
/// nothing to the sink is applied, and the first that does is refused, `unsupported` after
/// dispatch, where it would create the deadline's timer. No panic.
#[test]
fn a_decoder_given_a_deadline_is_refused_outside_a_runtime_where_it_would_start_its_timer() {
    let mut decoder = StreamDecoder::new(binding().provenance().clone())
        .with_deadline(Instant::now() + Duration::from_secs(30));
    let mut framing = SseDecoder::new(Framing::PayloadsOnly);
    let events: Vec<_> = framing
        .push(&sse(&text_turn()))
        .into_iter()
        .map(|event| event.expect("a framed event"))
        .collect();
    let mut sink = VecSink::new(64, 64 * 1024);
    let cancel = Cancel::new();
    let mut refusal: Option<Error> = None;
    for event in &events {
        let applied = catch_unwind(AssertUnwindSafe(|| {
            block_on(decoder.apply(event, &mut sink, &cancel))
        }))
        .unwrap_or_else(|payload| panic!("apply panicked: {}", panic_message(&*payload)));
        if let Err(error) = applied {
            refusal = Some(error);
            break;
        }
    }
    let refusal = refusal.expect("an event handed to the sink under a deadline is refused");
    assert_eq!(
        (refusal.code, refusal.dispatch, refusal.may_retry()),
        (ErrorCode::Unsupported, Dispatch::Accepted, false),
        "{refusal:?}"
    );
    assert_eq!(refusal.message, REQUIREMENT);
    assert!(
        sink.events().is_empty(),
        "an event reached the sink before the refusal"
    );
}
