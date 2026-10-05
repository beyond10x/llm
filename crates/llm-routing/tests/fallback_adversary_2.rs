//! Adversary pass 2 for ordered fallback: scripted `llm_core::Model` fakes, no network.
//!
//! `docs/verification/routing-fallback.md`: "Foreign evidence is never recorded." These cases
//! drive that sentence through the two paths the correction round did not exercise with foreign
//! evidence: a refused `Ok` outcome, and a contradictory failure after output became visible.

use llm_core::{
    BoxFuture, Cancel, Dispatch, Error, ErrorCode, Id, Item, Model, Provenance, StopReason,
    StreamEvent, StreamSink, TurnObservation, TurnOutcome, TurnRequest, Usage, VecSink,
};
use llm_routing::{AttemptResult, Catalog, CatalogDocument, FallbackPolicy, Halt, Models, Ports};
use std::{
    collections::BTreeMap,
    future::Future,
    sync::atomic::{AtomicUsize, Ordering},
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

fn catalog() -> Catalog {
    let mut doc = CatalogDocument::parse(EXAMPLE).unwrap();
    doc.routes[0].fallback_enabled = true;
    doc.validate().unwrap()
}

fn request() -> TurnRequest {
    let mut request = TurnRequest::new("code", vec![Item::user("hello")]);
    request.max_output_tokens = Some(128);
    request
}

#[derive(Clone, Copy)]
enum Script {
    Succeed,
    /// `Ok` with an outcome `validate_for` refuses because its observation is foreign.
    OkWithForeignObservation,
    /// Emits one event, then fails `rejected` carrying a foreign observation.
    EmitThenFailWithForeignObservation,
    /// Fails eligibly (`transport`, `not-sent`) with nothing emitted and no evidence.
    FailUnsent,
}

struct Fake {
    provenance: Provenance,
    capabilities: llm_core::Capabilities,
    foreign: Provenance,
    script: Script,
    calls: AtomicUsize,
}

impl Model for Fake {
    fn provenance(&self) -> &Provenance {
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
            self.calls.fetch_add(1, Ordering::SeqCst);
            let observed = |binding: &Provenance| {
                let mut observation = TurnObservation::new(binding.clone());
                observation.final_usage = true;
                observation.usage = Some(Usage {
                    input_tokens: Some(9),
                    ..Usage::default()
                });
                observation
            };
            match self.script {
                Script::Succeed => Ok(TurnOutcome {
                    stop_reason: StopReason::EndTurn,
                    items: vec![Item::assistant("done")],
                    observation: observed(&self.provenance),
                }),
                Script::OkWithForeignObservation => Ok(TurnOutcome {
                    stop_reason: StopReason::EndTurn,
                    items: vec![Item::assistant("done")],
                    observation: observed(&self.foreign),
                }),
                Script::EmitThenFailWithForeignObservation => {
                    sink.emit(StreamEvent::TextDelta {
                        text: "partial".to_owned(),
                    })
                    .await?;
                    Err(Error::new(ErrorCode::Transport, "scripted")
                        .with_dispatch(Dispatch::Rejected)
                        .with_observation(observed(&self.foreign)))
                }
                Script::FailUnsent => Err(Error::new(ErrorCode::Transport, "scripted")),
            }
        })
    }
}

struct Fleet(BTreeMap<Id, Fake>);

impl Fleet {
    fn new(catalog: &Catalog, primary: Script) -> Self {
        let foreign = catalog
            .binding(&id("remote-large"))
            .unwrap()
            .provenance()
            .clone();
        let make = |serving: &str, script| {
            let binding = catalog.binding(&id(serving)).unwrap();
            (
                id(serving),
                Fake {
                    provenance: binding.provenance().clone(),
                    capabilities: binding.capabilities().clone(),
                    foreign: foreign.clone(),
                    script,
                    calls: AtomicUsize::new(0),
                },
            )
        };
        Self(
            [
                make("local-small", primary),
                make("remote-large", Script::Succeed),
            ]
            .into_iter()
            .collect(),
        )
    }
    fn calls(&self, serving: &str) -> usize {
        self.0[&id(serving)].calls.load(Ordering::SeqCst)
    }
}

impl Models for Fleet {
    fn model(&self, serving_model_id: &Id) -> Option<&dyn Model> {
        self.0
            .get(serving_model_id)
            .map(|model| model as &dyn Model)
    }
}

fn recorded_observation(models: &Fleet, catalog: &Catalog) -> Option<TurnObservation> {
    let mut sink = VecSink::new(16, 4096);
    let mut admit = |_: &llm_routing::Selection<'_>| Ok(());
    let cancel = Cancel::new();
    let run = block_on(catalog.run_turn(
        &request(),
        Some(100),
        FallbackPolicy::default(),
        Ports {
            models,
            admit: &mut admit,
            sink: &mut sink,
            cancel: &cancel,
            pause: &|_| Box::pin(std::future::ready(())),
        },
    ))
    .unwrap();
    assert_eq!(models.calls("remote-large"), 0, "run fell back");
    assert_eq!(run.attempts.len(), 1);
    if let Err(error) = &run.result {
        assert_eq!(
            error.observation, None,
            "run result carries foreign evidence"
        );
    }
    match &run.attempts[0].result {
        AttemptResult::Failed { observation, .. } => observation.clone(),
        AttemptResult::Succeeded { .. } => panic!("the foreign outcome was accepted as success"),
    }
}

/// Mutant `fallback.rs:260` `if outcome.observation.validate_for(provenance).is_ok()` -> `if true`:
/// a refused `Ok` outcome whose observation belongs to another binding must not be recorded as
/// the attempt's own evidence.
#[test]
fn adversary_refused_outcome_never_records_foreign_evidence() {
    let catalog = catalog();
    let models = Fleet::new(&catalog, Script::OkWithForeignObservation);
    assert_eq!(
        recorded_observation(&models, &catalog),
        None,
        "foreign observation recorded as the refused attempt's own evidence"
    );
}

/// Mutant: move the `visible_events > 0` branch at `fallback.rs:279` ahead of the contradictory
/// evidence branch at `:267`. After visible output, foreign evidence must still be dropped.
#[test]
fn adversary_foreign_evidence_after_visible_output_is_never_recorded() {
    let catalog = catalog();
    let models = Fleet::new(&catalog, Script::EmitThenFailWithForeignObservation);
    assert_eq!(
        recorded_observation(&models, &catalog),
        None,
        "foreign observation recorded after visible output"
    );
}

/// Mutant: check the attempt bound at `fallback.rs:185` before the rejection skip at `:182`.
/// `Halt::AttemptBound` is "the caller's attempt bound was reached"; with the bound at one and
/// the only other target incompatible, nothing was cut at the bound, so the halt is `Exhausted`.
#[test]
fn adversary_an_incompatible_trailing_target_is_not_cut_by_the_bound() {
    let catalog = catalog();
    let models = Fleet::new(&catalog, Script::FailUnsent);
    let mut turn = request();
    // Opaque state bound to the primary makes the secondary incompatible.
    turn.items.push(Item::Opaque {
        provenance: catalog
            .binding(&id("local-small"))
            .unwrap()
            .provenance()
            .clone(),
        payload: serde_json::json!({"state": "fixture"}),
    });
    let mut sink = VecSink::new(16, 4096);
    let mut admit = |_: &llm_routing::Selection<'_>| Ok(());
    let cancel = Cancel::new();
    let run = block_on(catalog.run_turn(
        &turn,
        Some(100),
        FallbackPolicy::disabled(),
        Ports {
            models: &models,
            admit: &mut admit,
            sink: &mut sink,
            cancel: &cancel,
            pause: &|_| Box::pin(std::future::ready(())),
        },
    ))
    .unwrap();
    assert_eq!(run.attempts.len(), 1);
    assert_ne!(
        run.explanation.targets[1].rejections,
        [] as [llm_routing::Rejection; 0]
    );
    assert_eq!(
        run.halt,
        Halt::Exhausted,
        "halt claims a cut the bound never made"
    );
}
