//! Scripted failures through fake `llm_core::Model`s and a bounded sink: no projection, no network.

use llm_core::{
    BoxFuture, Cancel, Dispatch, Error, ErrorCode, Id, Item, Model, StopReason, StreamEvent,
    StreamSink, TurnObservation, TurnOutcome, TurnRequest, Usage, VecSink,
};
use llm_routing::{
    AttemptResult, Catalog, CatalogDocument, FallbackPolicy, FallbackRun, Halt, Models, Ports,
    Rejection, Selection,
};
use std::{
    collections::BTreeMap,
    future::Future,
    sync::atomic::{AtomicUsize, Ordering},
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
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

/// Three declared targets: the primary, an incompatible middle one, and a compatible last one.
fn catalog(fallback_enabled: bool) -> Catalog {
    let mut doc = CatalogDocument::parse(EXAMPLE).unwrap();
    doc.routes[0].fallback_enabled = fallback_enabled;
    let mut tiny = doc.serving_models[0].clone();
    tiny.id = id("local-tiny");
    tiny.capabilities.context_window = 128;
    tiny.capabilities.max_output_tokens = 128;
    doc.serving_models.push(tiny);
    doc.targets[1].position = 2;
    let mut middle = doc.targets[0].clone();
    middle.id = id("coding-middle");
    middle.serving_model_id = id("local-tiny");
    middle.position = 1;
    doc.targets.push(middle);
    doc.validate().unwrap()
}

fn request() -> TurnRequest {
    let mut request = TurnRequest::new("code", vec![Item::user("hello")]);
    request.max_output_tokens = Some(128);
    request
}

#[derive(Clone)]
enum Script {
    Succeed,
    Fail(ErrorCode, Dispatch),
    /// Fails and attaches its bound observation whatever the dispatch claim says.
    FailWithEvidence(ErrorCode, Dispatch),
}

struct Scripted {
    provenance: llm_core::Provenance,
    capabilities: llm_core::Capabilities,
    emits: usize,
    script: Script,
    usage: Option<u64>,
    /// Reports evidence under this binding instead of its own, when set.
    observed_as: Option<llm_core::Provenance>,
    calls: AtomicUsize,
}

impl Model for Scripted {
    fn provenance(&self) -> &llm_core::Provenance {
        &self.provenance
    }
    fn capabilities(&self) -> &llm_core::Capabilities {
        &self.capabilities
    }
    fn turn<'a>(
        &'a self,
        request: &'a TurnRequest,
        sink: &'a mut dyn StreamSink,
        _: &'a Cancel,
    ) -> BoxFuture<'a, Result<TurnOutcome, Error>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(request.model, self.provenance.model.as_str());
            for _ in 0..self.emits {
                sink.emit(StreamEvent::TextDelta {
                    text: format!("from {}", self.provenance.endpoint.as_str()),
                })
                .await?;
            }
            let mut observation = TurnObservation::new(
                self.observed_as
                    .clone()
                    .unwrap_or_else(|| self.provenance.clone()),
            );
            observation.usage = self.usage.map(|input| Usage {
                input_tokens: Some(input),
                ..Usage::default()
            });
            match &self.script {
                Script::Succeed => {
                    observation.final_usage = true;
                    Ok(TurnOutcome {
                        stop_reason: StopReason::EndTurn,
                        items: vec![Item::assistant("done")],
                        observation,
                    })
                }
                Script::Fail(code, dispatch) => {
                    let error = Error::new(*code, "scripted failure").with_dispatch(*dispatch);
                    Err(if *dispatch == Dispatch::NotSent {
                        error
                    } else {
                        error.with_observation(observation)
                    })
                }
                Script::FailWithEvidence(code, dispatch) => {
                    Err(Error::new(*code, "scripted failure")
                        .with_dispatch(*dispatch)
                        .with_observation(observation))
                }
            }
        })
    }
}

struct Fleet(BTreeMap<Id, Scripted>);

impl Fleet {
    fn new(catalog: &Catalog, scripts: &[(&str, usize, Script, Option<u64>)]) -> Self {
        Self(
            scripts
                .iter()
                .map(|(serving, emits, script, usage)| {
                    let binding = catalog.binding(&id(serving)).unwrap();
                    (
                        id(serving),
                        Scripted {
                            provenance: binding.provenance().clone(),
                            capabilities: binding.capabilities().clone(),
                            emits: *emits,
                            script: script.clone(),
                            usage: *usage,
                            observed_as: None,
                            calls: AtomicUsize::new(0),
                        },
                    )
                })
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

const TRANSPORT_UNSENT: Script = Script::Fail(ErrorCode::Transport, Dispatch::NotSent);

fn fleet(catalog: &Catalog, primary: Script, primary_emits: usize) -> Fleet {
    Fleet::new(
        catalog,
        &[
            ("local-small", primary_emits, primary, None),
            ("local-tiny", 0, Script::Succeed, None),
            ("remote-large", 1, Script::Succeed, Some(7)),
        ],
    )
}

fn run(
    catalog: &Catalog,
    turn: &TurnRequest,
    policy: FallbackPolicy,
    models: &Fleet,
    admit: &mut (dyn FnMut(&Selection<'_>) -> Result<(), Error> + Send),
    sink: &mut VecSink,
) -> FallbackRun {
    let cancel = Cancel::new();
    let ports = Ports {
        models,
        admit,
        sink,
        cancel: &cancel,
    };
    block_on(catalog.run_turn(turn, Some(100), policy, ports)).unwrap()
}

fn admit_all() -> impl FnMut(&Selection<'_>) -> Result<(), Error> + Send {
    |_: &Selection<'_>| Ok(())
}

fn attempted(run: &FallbackRun) -> Vec<&str> {
    run.attempts
        .iter()
        .map(|attempt| attempt.target_id.as_str())
        .collect()
}

#[test]
fn eligible_failure_before_output_attempts_only_the_declared_compatible_order() {
    let catalog = catalog(true);
    let models = fleet(
        &catalog,
        Script::Fail(ErrorCode::RateLimited, Dispatch::Rejected),
        0,
    );
    let mut sink = VecSink::new(16, 4096);
    let mut admitted = Vec::new();
    let mut admit = |selection: &Selection<'_>| {
        admitted.push(selection.target.id.clone());
        Ok(())
    };
    let run = run(
        &catalog,
        &request(),
        FallbackPolicy::default(),
        &models,
        &mut admit,
        &mut sink,
    );
    assert_eq!(run.halt, Halt::Succeeded);
    assert_eq!(attempted(&run), ["coding-primary", "coding-secondary"]);
    assert_eq!(admitted, [id("coding-primary"), id("coding-secondary")]);
    assert_eq!(models.calls("local-tiny"), 0);
    assert_eq!(
        run.explanation.targets[1].rejections,
        [Rejection::ContextWindow]
    );
    assert_eq!(
        run.result.as_ref().unwrap().items,
        [Item::assistant("done")]
    );
    assert_eq!(sink.text(), "from remote-models");
    // Every attempted route keeps its own evidence; unreported counters stay unknown.
    match &run.attempts[0].result {
        AttemptResult::Failed {
            code,
            dispatch,
            observation,
        } => {
            assert_eq!(*code, ErrorCode::RateLimited);
            assert_eq!(*dispatch, Dispatch::Rejected);
            let observation = observation.as_ref().unwrap();
            assert_eq!(observation.usage, None);
            assert!(!observation.final_usage);
        }
        AttemptResult::Succeeded { .. } => panic!("primary was scripted to fail"),
    }
    match &run.attempts[1].result {
        AttemptResult::Succeeded { observation } => {
            assert_eq!(
                &observation.binding,
                catalog.binding(&id("remote-large")).unwrap().provenance()
            );
            assert_eq!(observation.usage.as_ref().unwrap().input_tokens, Some(7));
            assert_eq!(observation.usage.as_ref().unwrap().output_tokens, None);
        }
        AttemptResult::Failed { .. } => panic!("alternative was scripted to succeed"),
    }
    assert_eq!(run.attempts[0].visible_events, 0);
    assert_eq!(run.attempts[1].visible_events, 1);
}

#[test]
fn refuses_fallback_after_partial_output_became_visible() {
    let catalog = catalog(true);
    let models = fleet(&catalog, TRANSPORT_UNSENT, 1);
    let mut sink = VecSink::new(16, 4096);
    let run = run(
        &catalog,
        &request(),
        FallbackPolicy::default(),
        &models,
        &mut admit_all(),
        &mut sink,
    );
    assert_eq!(run.halt, Halt::VisibleOutput);
    assert_eq!(attempted(&run), ["coding-primary"]);
    assert_eq!(models.calls("remote-large"), 0);
    assert_eq!(run.attempts[0].visible_events, 1);
    assert_eq!(run.result.unwrap_err().code, ErrorCode::Transport);
    assert_eq!(sink.text(), "from local-vllm");
}

#[test]
fn refuses_fallback_when_opaque_state_is_bound_to_the_failed_target() {
    let catalog = catalog(true);
    let models = fleet(&catalog, TRANSPORT_UNSENT, 0);
    let mut turn = request();
    turn.items.push(Item::Opaque {
        provenance: catalog
            .binding(&id("local-small"))
            .unwrap()
            .provenance()
            .clone(),
        payload: serde_json::json!({"state": "fixture"}),
    });
    let mut sink = VecSink::new(16, 4096);
    let run = run(
        &catalog,
        &turn,
        FallbackPolicy::default(),
        &models,
        &mut admit_all(),
        &mut sink,
    );
    assert_eq!(run.halt, Halt::Exhausted);
    assert_eq!(attempted(&run), ["coding-primary"]);
    assert_eq!(models.calls("remote-large"), 0);
    assert!(
        run.explanation.targets[2]
            .rejections
            .contains(&Rejection::OpaqueState)
    );
    assert_eq!(run.result.unwrap_err().code, ErrorCode::Transport);
}

#[test]
fn refuses_to_replay_an_ambiguously_dispatched_request() {
    let catalog = catalog(true);
    let models = fleet(
        &catalog,
        Script::Fail(ErrorCode::Transport, Dispatch::Unknown),
        0,
    );
    let mut sink = VecSink::new(16, 4096);
    let run = run(
        &catalog,
        &request(),
        FallbackPolicy::default(),
        &models,
        &mut admit_all(),
        &mut sink,
    );
    assert_eq!(run.halt, Halt::AmbiguousDispatch);
    assert_eq!(attempted(&run), ["coding-primary"]);
    assert_eq!(models.calls("remote-large"), 0);
    let error = run.result.unwrap_err();
    assert_eq!(error.dispatch, Dispatch::Unknown);
}

#[test]
fn refuses_an_alternative_whose_limits_are_exhausted() {
    let catalog = catalog(true);
    let models = Fleet::new(
        &catalog,
        &[
            (
                "local-small",
                0,
                Script::Fail(ErrorCode::Unavailable, Dispatch::Rejected),
                Some(5),
            ),
            ("local-tiny", 0, Script::Succeed, None),
            ("remote-large", 0, Script::Succeed, None),
        ],
    );
    let mut sink = VecSink::new(16, 4096);
    let mut admit = |selection: &Selection<'_>| {
        if selection.target.id.as_str() == "coding-secondary" {
            Err(Error::new(ErrorCode::Refused, "spending limit exhausted"))
        } else {
            Ok(())
        }
    };
    let run = run(
        &catalog,
        &request(),
        FallbackPolicy::default(),
        &models,
        &mut admit,
        &mut sink,
    );
    assert_eq!(run.halt, Halt::LimitRefused);
    assert_eq!(attempted(&run), ["coding-primary"]);
    assert_eq!(models.calls("remote-large"), 0);
    // The failed attempt's own evidence survives the refusal that ended the run.
    match &run.attempts[0].result {
        AttemptResult::Failed { observation, .. } => assert_eq!(
            observation
                .as_ref()
                .unwrap()
                .usage
                .as_ref()
                .unwrap()
                .input_tokens,
            Some(5)
        ),
        AttemptResult::Succeeded { .. } => panic!("primary was scripted to fail"),
    }
    assert_eq!(run.result.unwrap_err().code, ErrorCode::Refused);
}

#[test]
fn a_refused_primary_admission_makes_no_attempt_at_all() {
    let catalog = catalog(true);
    let models = fleet(&catalog, Script::Succeed, 0);
    let mut sink = VecSink::new(16, 4096);
    let mut admit = |_: &Selection<'_>| Err(Error::new(ErrorCode::Refused, "limit exhausted"));
    let run = run(
        &catalog,
        &request(),
        FallbackPolicy::default(),
        &models,
        &mut admit,
        &mut sink,
    );
    assert_eq!(run.halt, Halt::LimitRefused);
    assert!(run.attempts.is_empty());
    assert_eq!(models.calls("local-small"), 0);
}

#[test]
fn ineligible_failure_classes_never_fall_back() {
    for (code, dispatch) in [
        (ErrorCode::Unauthorized, Dispatch::Rejected),
        (ErrorCode::InvalidRequest, Dispatch::NotSent),
        (ErrorCode::Refused, Dispatch::Rejected),
        (ErrorCode::Protocol, Dispatch::Accepted),
        (ErrorCode::Transport, Dispatch::Accepted),
        (ErrorCode::Cancelled, Dispatch::NotSent),
        (ErrorCode::Deadline, Dispatch::NotSent),
    ] {
        let catalog = catalog(true);
        let models = fleet(&catalog, Script::Fail(code, dispatch), 0);
        let mut sink = VecSink::new(16, 4096);
        let run = run(
            &catalog,
            &request(),
            FallbackPolicy::default(),
            &models,
            &mut admit_all(),
            &mut sink,
        );
        assert_eq!(run.halt, Halt::IneligibleFailure, "{code:?} {dispatch:?}");
        assert_eq!(models.calls("remote-large"), 0, "{code:?} {dispatch:?}");
    }
}

#[test]
fn every_eligible_class_falls_back() {
    for (code, dispatch) in [
        (ErrorCode::Transport, Dispatch::NotSent),
        (ErrorCode::Unavailable, Dispatch::Rejected),
        (ErrorCode::RateLimited, Dispatch::NotSent),
    ] {
        let catalog = catalog(true);
        let models = fleet(&catalog, Script::Fail(code, dispatch), 0);
        let mut sink = VecSink::new(16, 4096);
        let run = run(
            &catalog,
            &request(),
            FallbackPolicy::default(),
            &models,
            &mut admit_all(),
            &mut sink,
        );
        assert_eq!(run.halt, Halt::Succeeded, "{code:?} {dispatch:?}");
        assert_eq!(attempted(&run), ["coding-primary", "coding-secondary"]);
    }
}

#[test]
fn the_route_and_the_caller_can_each_disable_fallback() {
    let disabled_route = catalog(false);
    let models = fleet(&disabled_route, TRANSPORT_UNSENT, 0);
    let mut sink = VecSink::new(16, 4096);
    let run_disabled = run(
        &disabled_route,
        &request(),
        FallbackPolicy::default(),
        &models,
        &mut admit_all(),
        &mut sink,
    );
    assert_eq!(run_disabled.halt, Halt::Exhausted);
    assert_eq!(attempted(&run_disabled), ["coding-primary"]);
    assert_eq!(
        run_disabled.explanation.targets[2].rejections,
        [Rejection::FallbackDisabled]
    );

    let enabled = catalog(true);
    let models = fleet(&enabled, TRANSPORT_UNSENT, 0);
    let run_bounded = run(
        &enabled,
        &request(),
        FallbackPolicy::disabled(),
        &models,
        &mut admit_all(),
        &mut sink,
    );
    assert_eq!(run_bounded.halt, Halt::AttemptBound);
    assert_eq!(attempted(&run_bounded), ["coding-primary"]);
    assert_eq!(models.calls("remote-large"), 0);
    assert!(
        FallbackPolicy {
            max_attempts: 0,
            deadline: None
        }
        .validate()
        .is_err()
    );
}

#[test]
fn a_passed_deadline_starts_no_attempt() {
    let catalog = catalog(true);
    let models = fleet(&catalog, Script::Succeed, 0);
    let mut sink = VecSink::new(16, 4096);
    let past = Instant::now()
        .checked_sub(Duration::from_millis(1))
        .unwrap();
    let run = run(
        &catalog,
        &request(),
        FallbackPolicy {
            deadline: Some(past),
            ..FallbackPolicy::default()
        },
        &models,
        &mut admit_all(),
        &mut sink,
    );
    assert_eq!(run.halt, Halt::Deadline);
    assert!(run.attempts.is_empty());
    assert_eq!(models.calls("local-small"), 0);
    assert_eq!(run.result.unwrap_err().code, ErrorCode::Deadline);
}

#[test]
fn a_model_that_does_not_serve_its_binding_is_refused_before_any_attempt() {
    let catalog = catalog(true);
    let mut models = fleet(&catalog, Script::Succeed, 0);
    let foreign = catalog
        .binding(&id("remote-large"))
        .unwrap()
        .provenance()
        .clone();
    models.0.get_mut(&id("local-small")).unwrap().provenance = foreign;
    let mut sink = VecSink::new(16, 4096);
    let cancel = Cancel::new();
    let error = block_on(catalog.run_turn(
        &request(),
        Some(100),
        FallbackPolicy::default(),
        Ports {
            models: &models,
            admit: &mut admit_all(),
            sink: &mut sink,
            cancel: &cancel,
        },
    ))
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert_eq!(models.calls("local-small"), 0);
}

#[test]
fn the_run_future_can_move_to_a_multithreaded_executor() {
    fn assert_send<T: Send>(_: &T) {}
    let catalog = catalog(true);
    let models = fleet(&catalog, Script::Succeed, 0);
    let mut sink = VecSink::new(16, 4096);
    let cancel = Cancel::new();
    let mut admit = admit_all();
    let turn = request();
    let future = catalog.run_turn(
        &turn,
        Some(100),
        FallbackPolicy::default(),
        Ports {
            models: &models,
            admit: &mut admit,
            sink: &mut sink,
            cancel: &cancel,
        },
    );
    assert_send(&future);
    assert_eq!(block_on(future).unwrap().halt, Halt::Succeeded);
}

fn without(mut models: Fleet, serving: &str) -> Fleet {
    models.0.remove(&id(serving));
    models
}

#[test]
fn a_target_the_run_can_never_attempt_needs_no_model() {
    // Caller-disabled fallback: the only attemptable target is the primary.
    let catalog = catalog(true);
    let models = without(fleet(&catalog, Script::Succeed, 0), "remote-large");
    let mut sink = VecSink::new(16, 4096);
    let succeeded = run(
        &catalog,
        &request(),
        FallbackPolicy::disabled(),
        &models,
        &mut admit_all(),
        &mut sink,
    );
    assert_eq!(succeeded.halt, Halt::Succeeded);
    assert_eq!(attempted(&succeeded), ["coding-primary"]);

    let models = without(fleet(&catalog, TRANSPORT_UNSENT, 0), "remote-large");
    let bounded = run(
        &catalog,
        &request(),
        FallbackPolicy::disabled(),
        &models,
        &mut admit_all(),
        &mut sink,
    );
    assert_eq!(bounded.halt, Halt::AttemptBound);
    assert_eq!(attempted(&bounded), ["coding-primary"]);
    assert_eq!(bounded.result.unwrap_err().code, ErrorCode::Transport);

    // Route-disabled fallback: alternatives are rejected, so they need no model either.
    let disabled_route = catalog_disabled();
    let models = without(fleet(&disabled_route, Script::Succeed, 0), "remote-large");
    let route_run = run(
        &disabled_route,
        &request(),
        FallbackPolicy::default(),
        &models,
        &mut admit_all(),
        &mut sink,
    );
    assert_eq!(route_run.halt, Halt::Succeeded);
}

#[test]
fn a_target_the_run_could_attempt_still_needs_its_model_before_any_attempt() {
    let catalog = catalog(true);
    let models = without(fleet(&catalog, Script::Succeed, 0), "remote-large");
    let mut sink = VecSink::new(16, 4096);
    let cancel = Cancel::new();
    let error = block_on(catalog.run_turn(
        &request(),
        Some(100),
        FallbackPolicy {
            max_attempts: 2,
            deadline: None,
        },
        Ports {
            models: &models,
            admit: &mut admit_all(),
            sink: &mut sink,
            cancel: &cancel,
        },
    ))
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert_eq!(models.calls("local-small"), 0);
}

fn catalog_disabled() -> Catalog {
    catalog(false)
}

#[test]
fn bound_evidence_on_a_contradictory_dispatch_claim_is_kept_and_never_replayed() {
    let catalog = catalog(true);
    let models = Fleet::new(
        &catalog,
        &[
            (
                "local-small",
                0,
                Script::FailWithEvidence(ErrorCode::Transport, Dispatch::NotSent),
                Some(5),
            ),
            ("local-tiny", 0, Script::Succeed, None),
            ("remote-large", 0, Script::Succeed, None),
        ],
    );
    let mut sink = VecSink::new(16, 4096);
    let run = run(
        &catalog,
        &request(),
        FallbackPolicy::default(),
        &models,
        &mut admit_all(),
        &mut sink,
    );
    assert_eq!(run.halt, Halt::AmbiguousDispatch);
    assert_eq!(models.calls("remote-large"), 0);
    match &run.attempts[0].result {
        AttemptResult::Failed {
            dispatch,
            observation,
            ..
        } => {
            assert_eq!(*dispatch, Dispatch::Unknown);
            let usage = observation.as_ref().unwrap().usage.as_ref().unwrap();
            assert_eq!(usage.input_tokens, Some(5));
        }
        AttemptResult::Succeeded { .. } => panic!("primary was scripted to fail"),
    }
    let error = run.result.unwrap_err();
    assert_eq!(error.dispatch, Dispatch::Unknown);
    assert!(error.observation.is_some());
}

#[test]
fn foreign_evidence_is_never_recorded_as_an_attempts_own() {
    let catalog = catalog(true);
    let mut models = Fleet::new(
        &catalog,
        &[
            (
                "local-small",
                0,
                Script::FailWithEvidence(ErrorCode::Transport, Dispatch::Rejected),
                Some(5),
            ),
            ("local-tiny", 0, Script::Succeed, None),
            ("remote-large", 0, Script::Succeed, None),
        ],
    );
    // The fake reports its observation under a different binding than the one it serves.
    let foreign = catalog
        .binding(&id("remote-large"))
        .unwrap()
        .provenance()
        .clone();
    let primary = models.0.get_mut(&id("local-small")).unwrap();
    primary.observed_as = Some(foreign);
    let mut sink = VecSink::new(16, 4096);
    let run = run(
        &catalog,
        &request(),
        FallbackPolicy::default(),
        &models,
        &mut admit_all(),
        &mut sink,
    );
    assert_eq!(run.halt, Halt::AmbiguousDispatch);
    assert_eq!(models.calls("remote-large"), 0);
    match &run.attempts[0].result {
        AttemptResult::Failed { observation, .. } => assert_eq!(*observation, None),
        AttemptResult::Succeeded { .. } => panic!("primary was scripted to fail"),
    }
}
