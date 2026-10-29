//! Adversary, wave 2026-10-29-w21, story:unpublished-tool-call-reaches-an-opted-in-caller.
//!
//! Routing re-validates every attempt's outcome against the selected request (`settle` in
//! `fallback.rs`). The selected request is a clone with the model renamed; these probes pin that
//! the control survives selection, that an opted-in turn succeeds on its first target, and that
//! without it the refusal stays final and never moves the turn to the next target.

use llm_core::{
    BoxFuture, CallId, Cancel, Error, ErrorCode, Id, Item, Model, StopReason, StreamSink, ToolCall,
    ToolName, ToolSpec, TurnObservation, TurnOutcome, TurnRequest,
};
use llm_routing::{Catalog, CatalogDocument, FallbackPolicy, FallbackRun, Halt, Models, Ports};
use serde_json::json;
use std::{
    collections::BTreeMap,
    future::Future,
    sync::Mutex,
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

/// Both example targets carry tools, and fallback is on.
fn catalog() -> Catalog {
    let mut doc = CatalogDocument::parse(EXAMPLE).unwrap();
    doc.routes[0].fallback_enabled = true;
    doc.serving_models[0].capabilities.tools = true;
    doc.validate().unwrap()
}

/// Answers every turn with a call to `find`, which no request here publishes.
struct CallsFind {
    provenance: llm_core::Provenance,
    capabilities: llm_core::Capabilities,
    seen: Mutex<Vec<bool>>,
}

impl Model for CallsFind {
    fn provenance(&self) -> &llm_core::Provenance {
        &self.provenance
    }
    fn capabilities(&self) -> &llm_core::Capabilities {
        &self.capabilities
    }
    fn turn<'a>(
        &'a self,
        request: &'a TurnRequest,
        _: &'a mut dyn StreamSink,
        _: &'a Cancel,
    ) -> BoxFuture<'a, Result<TurnOutcome, Error>> {
        Box::pin(async move {
            self.seen
                .lock()
                .unwrap()
                .push(request.controls.return_unpublished_calls);
            let mut observation = TurnObservation::new(self.provenance.clone());
            observation.final_usage = true;
            Ok(TurnOutcome {
                stop_reason: StopReason::ToolCalls,
                items: vec![Item::ToolCall(ToolCall {
                    call_id: CallId::new("call_1").unwrap(),
                    name: ToolName::new("find").unwrap(),
                    arguments: json!({}),
                })],
                observation,
            })
        })
    }
}

struct Fleet(BTreeMap<Id, CallsFind>);

impl Fleet {
    fn new(catalog: &Catalog) -> Self {
        Self(
            ["local-small", "remote-large"]
                .into_iter()
                .map(|serving| {
                    let binding = catalog.binding(&id(serving)).unwrap();
                    (
                        id(serving),
                        CallsFind {
                            provenance: binding.provenance().clone(),
                            capabilities: binding.capabilities().clone(),
                            seen: Mutex::new(Vec::new()),
                        },
                    )
                })
                .collect(),
        )
    }
    fn seen(&self, serving: &str) -> Vec<bool> {
        self.0[&id(serving)].seen.lock().unwrap().clone()
    }
}

impl Models for Fleet {
    fn model(&self, serving_model_id: &Id) -> Option<&dyn Model> {
        self.0
            .get(serving_model_id)
            .map(|model| model as &dyn Model)
    }
}

fn request(opted_in: bool) -> TurnRequest {
    let mut request = TurnRequest::new("code", vec![Item::user("hello")]);
    request.max_output_tokens = Some(128);
    request.tools = vec![ToolSpec {
        name: ToolName::new("discover").unwrap(),
        description: "d".to_owned(),
        input_schema: json!({"type": "object"}),
    }];
    request.controls.return_unpublished_calls = opted_in;
    request
}

fn run(catalog: &Catalog, turn: &TurnRequest, models: &Fleet) -> FallbackRun {
    let cancel = Cancel::new();
    let mut admit = |_: &llm_routing::Selection<'_>| Ok(());
    let mut sink = llm_core::VecSink::new(16, 4096);
    let ports = Ports {
        models,
        admit: &mut admit,
        sink: &mut sink,
        cancel: &cancel,
        pause: &|_| Box::pin(std::future::ready(())),
    };
    block_on(catalog.run_turn(turn, Some(100), FallbackPolicy::default(), ports)).unwrap()
}

#[test]
fn an_opted_in_turn_keeps_its_control_through_selection_and_succeeds_on_its_first_target() {
    let catalog = catalog();
    let models = Fleet::new(&catalog);
    let run = run(&catalog, &request(true), &models);
    assert_eq!(run.halt, Halt::Succeeded);
    assert_eq!(run.attempts.len(), 1);
    assert_eq!(models.seen("local-small"), [true]);
    assert_eq!(models.seen("remote-large"), Vec::<bool>::new());
    let outcome = run.result.unwrap();
    assert_eq!(outcome.tool_calls().next().unwrap().name.as_str(), "find");
}

#[test]
fn without_the_control_the_refusal_is_final_and_never_falls_back() {
    let catalog = catalog();
    let models = Fleet::new(&catalog);
    let run = run(&catalog, &request(false), &models);
    assert_eq!(run.halt, Halt::IneligibleFailure);
    assert_eq!(run.attempts.len(), 1);
    assert_eq!(models.seen("remote-large"), Vec::<bool>::new());
    let error = run.result.unwrap_err();
    assert_eq!(error.code, ErrorCode::Protocol);
    assert_eq!(
        error.message,
        "model output contains a duplicate call or unpublished tool"
    );
}
