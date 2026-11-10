//! Adversary cases, pass 2, for story:ingress-block-boundaries (wave 2026-11-09, llm unit 1).
//!
//! The correction stopped charging an `OpaqueItem`'s payload to `VecSink`'s byte bound. Under
//! fallback that must not turn the item back into "nothing shown": an attempt whose only output
//! is a payload far past the caller's byte bound has still shown output, so an eligible failure
//! after it falls back to nothing, and the caller's sink keeps the item.

use llm_core::{
    BoxFuture, Cancel, Dispatch, Error, ErrorCode, Id, Item, Model, StreamEvent, StreamSink,
    TurnObservation, TurnOutcome, TurnRequest, VecSink,
};
use llm_routing::{Catalog, CatalogDocument, FallbackPolicy, Halt, Models, Ports, Selection};
use std::{
    collections::BTreeMap,
    future::Future,
    task::{Context, Poll, Waker},
};

const EXAMPLE: &str = include_str!("../../../examples/catalog.toml");

fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
    }
}

struct Shown {
    provenance: llm_core::Provenance,
    capabilities: llm_core::Capabilities,
    /// Emits one opaque item of this many bytes, then fails eligible; or answers when `None`.
    opaque_then_fail: Option<usize>,
}

impl Model for Shown {
    fn provenance(&self) -> &llm_core::Provenance {
        &self.provenance
    }
    fn capabilities(&self) -> &llm_core::Capabilities {
        &self.capabilities
    }
    fn turn<'a>(
        &'a self,
        _: &'a TurnRequest,
        sink: &'a mut dyn StreamSink,
        _: &'a Cancel,
    ) -> BoxFuture<'a, Result<TurnOutcome, Error>> {
        Box::pin(async move {
            if let Some(bytes) = self.opaque_then_fail {
                sink.emit(StreamEvent::OpaqueItem {
                    provenance: self.provenance.clone(),
                    payload: serde_json::json!({"type": "redacted_thinking",
                        "data": "Z".repeat(bytes)}),
                })
                .await?;
                return Err(
                    Error::new(ErrorCode::Transport, "scripted").with_dispatch(Dispatch::NotSent)
                );
            }
            sink.emit(StreamEvent::TextDelta {
                text: "fallback".to_owned(),
            })
            .await?;
            let mut observation = TurnObservation::new(self.provenance.clone());
            observation.final_usage = true;
            Ok(TurnOutcome {
                stop_reason: llm_core::StopReason::EndTurn,
                items: vec![Item::assistant("fallback")],
                observation,
            })
        })
    }
}

struct Fleet(BTreeMap<Id, Shown>);

impl Models for Fleet {
    fn model(&self, serving_model_id: &Id) -> Option<&dyn Model> {
        self.0.get(serving_model_id).map(|model| model as &dyn Model)
    }
}

#[test]
fn an_opaque_item_past_the_byte_bound_is_output_and_ends_fallback() {
    let mut doc = CatalogDocument::parse(EXAMPLE).unwrap();
    doc.routes[0].fallback_enabled = true;
    let catalog: Catalog = doc.validate().unwrap();
    let models = Fleet(
        [("local-small", Some(64 * 1024)), ("remote-large", None)]
            .into_iter()
            .map(|(serving, opaque_then_fail)| {
                let binding = catalog.binding(&id(serving)).unwrap();
                (
                    id(serving),
                    Shown {
                        provenance: binding.provenance().clone(),
                        capabilities: binding.capabilities().clone(),
                        opaque_then_fail,
                    },
                )
            })
            .collect(),
    );
    let mut request = TurnRequest::new("code", vec![Item::user("hello")]);
    request.max_output_tokens = Some(128);
    let mut sink = VecSink::new(16, 64);
    let cancel = Cancel::new();
    let mut admit = |_: &Selection<'_>| Ok(());
    let run = block_on(catalog.run_turn(
        &request,
        Some(100),
        FallbackPolicy::default(),
        Ports {
            models: &models,
            admit: &mut admit,
            sink: &mut sink,
            cancel: &cancel,
            pause: &|_| Box::pin(std::future::ready(())),
        },
    ))
    .unwrap();
    assert_eq!(run.halt, Halt::VisibleOutput);
    assert_eq!(run.attempts.len(), 1);
    assert_eq!(run.attempts[0].visible_events, 1);
    assert_eq!(run.result.expect_err("no fallback").code, ErrorCode::Transport);
    assert!(matches!(sink.events(), [StreamEvent::OpaqueItem { .. }]));
    assert_eq!(sink.text(), "");
}
