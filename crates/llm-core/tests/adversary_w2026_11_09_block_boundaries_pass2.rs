//! Adversary cases, pass 2, for story:ingress-block-boundaries (wave 2026-11-09, llm unit 1).
//!
//! The claims under attack are the correction's (`95498057`, `crates/llm-core/src/port.rs`):
//! `VecSink` stores a `MessageBoundary` charged to neither bound, at most one per counted event
//! before it, and counts an `OpaqueItem` as one event without charging its payload to the byte
//! bound. Both variants round-trip through `llm-core`'s public serde forms.

use llm_core::{
    ErrorCode, Id, Item, Protocol, Provenance, StopReason, StreamEvent, StreamSink,
    TurnObservation, TurnOutcome, VecSink,
};
use serde_json::{Value, json};

fn id(value: &str) -> Id {
    Id::new(value).expect("fixture identifier")
}

fn provenance() -> Provenance {
    Provenance {
        protocol: Protocol::Messages,
        provider: id("p"),
        account: id("a"),
        endpoint: id("e"),
        model: id("m"),
        binding_revision: id("r"),
    }
}

fn text(delta: &str) -> StreamEvent {
    StreamEvent::TextDelta {
        text: delta.to_owned(),
    }
}

fn opaque(data: &str) -> StreamEvent {
    StreamEvent::OpaqueItem {
        provenance: provenance(),
        payload: json!({"type": "redacted_thinking", "data": data}),
    }
}

fn boundary() -> StreamEvent {
    StreamEvent::MessageBoundary {}
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
    }
}

fn emit(sink: &mut VecSink, event: StreamEvent) -> Result<(), ErrorCode> {
    block_on(sink.emit(event)).map_err(|error| error.code)
}

/// Every sequence a decoder of this unit emits fits a sink whose event bound is exactly the
/// number of non-boundary events in it, whatever boundaries sit between them: the Responses
/// interleaving decided in correction round 1 (A, |, B, |, A), the Messages text, redacted
/// thinking, text, and Bedrock Converse text, call, text.
#[test]
fn every_boundary_a_decoder_emits_fits_an_event_bound_of_the_counted_events() {
    let call = StreamEvent::ToolCallStarted {
        call_id: llm_core::CallId::new("call-1").expect("call id"),
        name: llm_core::ToolName::new("lookup").expect("tool name"),
    };
    let sequences: [Vec<StreamEvent>; 3] = [
        vec![
            text("Al"),
            boundary(),
            text("Beta."),
            boundary(),
            text("pha."),
        ],
        vec![text("A"), opaque(&"Z".repeat(8192)), boundary(), text("B")],
        vec![text("A"), call, boundary(), text("B")],
    ];
    for sequence in sequences {
        let counted = sequence
            .iter()
            .filter(|event| !matches!(event, StreamEvent::MessageBoundary {}))
            .count();
        let mut sink = VecSink::new(counted, 1024);
        for event in &sequence {
            assert_eq!(
                emit(&mut sink, event.clone()),
                Ok(()),
                "{event:?} in {sequence:?}"
            );
        }
        assert_eq!(sink.events(), sequence.as_slice());
        assert_eq!(
            emit(&mut sink, text("past")),
            Err(ErrorCode::TooLarge),
            "one counted event past the bound"
        );
    }
}

/// A sink at its event bound refuses every counted event after a boundary it still accepts, an
/// opaque item and a warning included, and keeps exactly the accepted prefix.
#[test]
fn a_sink_at_its_event_bound_takes_a_boundary_and_no_counted_event_after_it() {
    let mut sink = VecSink::new(1, 1024);
    assert_eq!(emit(&mut sink, text("A")), Ok(()));
    assert_eq!(emit(&mut sink, boundary()), Ok(()));
    for refused in [
        text("B"),
        opaque("Z"),
        StreamEvent::Warning {
            code: "w".to_owned(),
            message: "m".to_owned(),
        },
    ] {
        assert_eq!(emit(&mut sink, refused), Err(ErrorCode::TooLarge));
    }
    assert_eq!(emit(&mut sink, boundary()), Err(ErrorCode::TooLarge));
    assert_eq!(sink.events(), [text("A"), boundary()]);
}

/// A sink whose byte bound is spent still takes an opaque item (its payload is not charged) and
/// still refuses shown text: the opaque item did not reopen the byte bound.
#[test]
fn a_spent_byte_bound_takes_an_opaque_item_and_still_refuses_text() {
    let first = serde_json::to_vec(&text("A")).expect("encodable").len();
    let mut sink = VecSink::new(8, first);
    assert_eq!(emit(&mut sink, text("A")), Ok(()));
    assert_eq!(emit(&mut sink, opaque(&"Z".repeat(4096))), Ok(()));
    assert_eq!(emit(&mut sink, text("B")), Err(ErrorCode::TooLarge));
    assert_eq!(
        emit(&mut sink, boundary()),
        Ok(()),
        "two counted events before it"
    );
    assert_eq!(emit(&mut sink, text("")), Err(ErrorCode::TooLarge));
    assert_eq!(sink.text(), "A");
}

/// Both variants round-trip through their public serde form, and an opaque item carries exactly
/// the fields of the `Item::Opaque` it stands for, under its own kind.
#[test]
fn both_variants_round_trip_and_an_opaque_item_is_the_item_it_stands_for() {
    let events = vec![
        text("A"),
        boundary(),
        opaque("AAAA"),
        StreamEvent::OpaqueItem {
            provenance: provenance(),
            payload: json!({"kind": "not-a-tag", "nested": [null, 1, {"kind": "x"}]}),
        },
    ];
    let encoded = serde_json::to_value(&events).expect("encodable");
    let decoded: Vec<StreamEvent> = serde_json::from_value(encoded.clone()).expect("decodable");
    assert_eq!(decoded, events);
    assert_eq!(encoded[1], json!({"kind": "message-boundary"}));

    let item = Item::Opaque {
        provenance: provenance(),
        payload: json!({"type": "redacted_thinking", "data": "AAAA"}),
    };
    let mut as_item = serde_json::to_value(&item).expect("encodable");
    let mut as_event = encoded[2].clone();
    as_item.as_object_mut().expect("object").remove("kind");
    as_event.as_object_mut().expect("object").remove("kind");
    assert_eq!(as_item, as_event);

    let outcome = TurnOutcome {
        stop_reason: StopReason::EndTurn,
        items: vec![Item::assistant("A"), item, Item::assistant("B")],
        observation: TurnObservation::new(provenance()),
    };
    let round: TurnOutcome =
        serde_json::from_value(serde_json::to_value(&outcome).expect("encodable"))
            .expect("decodable");
    assert_eq!(round, outcome);

    for refused in [
        json!({"kind": "message-boundary", "payload": null}),
        json!({"kind": "opaque-item", "payload": {}}),
        json!({"kind": "opaque-item", "provenance": serde_json::to_value(provenance()).unwrap(),
            "payload": {}, "extra": 1}),
    ] {
        assert!(
            serde_json::from_value::<StreamEvent>(refused.clone()).is_err(),
            "{refused}"
        );
    }
    let _: Value = encoded;
}
