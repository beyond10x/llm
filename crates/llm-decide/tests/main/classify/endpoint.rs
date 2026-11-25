//! The decision-endpoint path: `EndpointClassifier` over a scripted `DecisionEndpoint`, the
//! example catalog, the scripted clock and the in-memory call log of the parent module.

use super::{
    AbstentionCause, Arc, AtomicBool, BoxFuture, CallLog, CallRecord, Cancel, Catalog, Chain,
    Classification, Classifier, Dispatch, Duration, Error, ErrorCode, Fleet, INPUT, Id, Kind,
    LabelSet, Mutex, Ordering, Outcome, Ports, Recording, Scripted, Selection, Usage, block_on,
    catalog, cause, chain, confidences, id, labels,
};
use llm_calls::{CallFacts, Stopwatch};
use llm_decide::{
    ChoiceAnswer, ChoiceCall, DecisionEndpoint, EndpointClassifier, LabelProbability, Path,
    basis_points,
};

/// What the scripted endpoint was asked, as the classifier put it.
#[derive(Debug, Clone, PartialEq)]
struct Asked {
    route: String,
    target: Id,
    instructions: String,
    choices: Vec<String>,
    input: String,
}

/// Replays one authored answer of a decision endpoint and keeps what it was asked. Like a real
/// client it writes the call's record, here of kind `Turn`, so the test sees the classifier
/// relabel it.
struct Endpoint {
    reply: Result<ChoiceAnswer, Error>,
    clock: Arc<Scripted>,
    takes_ms: u64,
    /// Waits until the call is cancelled, then reports the cancellation.
    hang: bool,
    writes_record: bool,
    asked: Mutex<Vec<Asked>>,
    cancelled: AtomicBool,
}

impl Endpoint {
    fn replying(clock: &Arc<Scripted>, reply: Result<ChoiceAnswer, Error>) -> Arc<Self> {
        Arc::new(Self {
            reply,
            clock: clock.clone(),
            takes_ms: 0,
            hang: false,
            writes_record: true,
            asked: Mutex::new(Vec::new()),
            cancelled: AtomicBool::new(false),
        })
    }
    fn with(
        clock: &Arc<Scripted>,
        reply: Result<ChoiceAnswer, Error>,
        change: impl FnOnce(&mut Self),
    ) -> Arc<Self> {
        let mut endpoint = Self {
            reply,
            clock: clock.clone(),
            takes_ms: 0,
            hang: false,
            writes_record: true,
            asked: Mutex::new(Vec::new()),
            cancelled: AtomicBool::new(false),
        };
        change(&mut endpoint);
        Arc::new(endpoint)
    }
    fn asked(&self) -> Vec<Asked> {
        self.asked.lock().unwrap().clone()
    }
}

impl DecisionEndpoint for Endpoint {
    fn choose<'a>(&'a self, call: ChoiceCall<'a>) -> BoxFuture<'a, Result<ChoiceAnswer, Error>> {
        Box::pin(async move {
            self.asked.lock().unwrap().push(Asked {
                route: call.route.to_owned(),
                target: call.selection.target.id.clone(),
                instructions: call.instructions.to_owned(),
                choices: call.choices.to_vec(),
                input: call.input.to_owned(),
            });
            let stopwatch = Stopwatch::start(call.recording.clock);
            self.clock.advance(self.takes_ms);
            let reply = if self.hang {
                call.cancel.cancelled().await;
                self.cancelled.store(true, Ordering::SeqCst);
                Err(Error::cancelled().with_dispatch(Dispatch::Unknown))
            } else {
                self.reply.clone()
            };
            if self.writes_record {
                let (outcome, error_code, dispatch) = match &reply {
                    Ok(_) => (Outcome::Succeeded, None, None),
                    Err(error) if error.code == ErrorCode::Cancelled => {
                        (Outcome::Cancelled, Some(error.code), Some(error.dispatch))
                    }
                    Err(error) => (Outcome::Failed, Some(error.code), Some(error.dispatch)),
                };
                call.recording.sink.record(CallRecord::new(
                    call.identity,
                    Kind::Turn,
                    call.selection.target.route_id.clone(),
                    stopwatch.stop(call.recording.clock),
                    CallFacts {
                        route_target_id: Some(call.selection.target.id.clone()),
                        upstream_model: None,
                        outcome,
                        error_code,
                        dispatch,
                        attempts: 1,
                        usage: Usage::default(),
                        cost: None,
                    },
                ));
            }
            reply
        })
    }
}

fn chosen(choice: &str, probabilities: &[(&str, f64)]) -> ChoiceAnswer {
    ChoiceAnswer::Chosen {
        choice: choice.to_owned(),
        probabilities: probabilities
            .iter()
            .map(|(label, probability)| LabelProbability {
                label: (*label).to_owned(),
                probability: *probability,
            })
            .collect(),
    }
}

struct Ran {
    attempts: Vec<Classification>,
    records: Vec<CallRecord>,
}

/// One chain run with the caller's admission and an estimate of 100 input tokens.
fn run_admitting(
    chain: &Chain,
    catalog: &Catalog,
    clock: &Arc<Scripted>,
    admit: &(dyn Fn(&Selection<'_>) -> Result<(), Error> + Sync),
) -> Ran {
    run_estimating(chain, catalog, clock, admit, Some(100))
}

/// One chain run with the caller's admission and its estimate of the input's tokens; a pause
/// moves the scripted clock by its wait.
fn run_estimating(
    chain: &Chain,
    catalog: &Catalog,
    clock: &Arc<Scripted>,
    admit: &(dyn Fn(&Selection<'_>) -> Result<(), Error> + Sync),
    input_tokens: Option<u64>,
) -> Ran {
    let log = CallLog::new();
    let cancel = Cancel::new();
    let fleet = Fleet::of(catalog, clock, []);
    let moved_by_waits = clock.clone();
    let pause = move |wait: Duration| -> BoxFuture<'static, ()> {
        let clock = moved_by_waits.clone();
        Box::pin(async move { clock.advance(u64::try_from(wait.as_millis()).unwrap()) })
    };
    let outcome = block_on(chain.classify(
        &id("req-1"),
        INPUT,
        input_tokens,
        Ports {
            catalog,
            models: &fleet,
            admit,
            cancel: &cancel,
            pause: &pause,
            recording: Recording {
                sink: &log,
                clock: &**clock,
                prices: None,
            },
        },
    ));
    Ran {
        attempts: outcome.attempts,
        records: log.records(),
    }
}

fn run(chain: &Chain, catalog: &Catalog, clock: &Arc<Scripted>) -> Ran {
    run_admitting(chain, catalog, clock, &|_: &Selection<'_>| Ok(()))
}

fn asking(endpoint: &Arc<Endpoint>) -> EndpointClassifier<Arc<Endpoint>> {
    EndpointClassifier::new(id("endpoint"), "code", endpoint.clone())
}

#[test]
fn a_decision_endpoint_classifier_asks_one_choice_question_and_decides() {
    let catalog = catalog();
    let clock = Scripted::at(1_000);
    let endpoint = Endpoint::with(
        &clock,
        Ok(chosen(
            "positive",
            &[("positive", 0.9), ("negative", 0.03125)],
        )),
        |endpoint| endpoint.takes_ms = 40,
    );
    let classifier = asking(&endpoint).with_instructions("How does the review feel?");
    assert_eq!(classifier.path(), Path::DecisionEndpoint);
    assert_eq!(classifier.min_confidence_bps(), None);
    let ran = run(&chain(vec![Box::new(classifier)]), &catalog, &clock);

    assert_eq!(
        endpoint.asked(),
        [Asked {
            route: "code".into(),
            target: id("coding-primary"),
            instructions: "How does the review feel?".into(),
            choices: vec!["positive".into(), "negative".into(), "neutral".into()],
            input: INPUT.into(),
        }],
        "one call, to the route's first compatible target, with the label set as the choices"
    );
    let [Classification::Decided(decision)] = ran.attempts.as_slice() else {
        panic!("a choice of the set decides: {:?}", ran.attempts);
    };
    assert_eq!(decision.label, "positive");
    assert_eq!(decision.label_set_version, "v1");
    // 0.9 is 9000; 0.03125 is 312.5 basis points, a tie, and rounds up; a label the answer gave
    // no probability stays unknown, never 0.
    assert_eq!(
        decision.confidences,
        confidences(&[
            ("positive", Some(9000)),
            ("negative", Some(313)),
            ("neutral", None)
        ])
    );
    assert_eq!(decision.latency_ms, 40);
    assert_eq!(decision.call_record_id, Some(id("req-1/endpoint")));
    assert_eq!(ran.records.len(), 1, "{:?}", ran.records);
    assert_eq!(ran.records[0].id, id("req-1/endpoint"));
    assert_eq!(
        ran.records[0].kind,
        Kind::Classification,
        "the endpoint's record reaches the caller's sink as a classification"
    );
    assert_eq!(ran.records[0].route_target_id, Some(id("coding-primary")));
}

#[test]
fn a_refusal_or_a_value_that_is_no_text_abstains_off_label() {
    let catalog = catalog();
    for reply in [ChoiceAnswer::Refused, ChoiceAnswer::NotText] {
        let clock = Scripted::at(1_000);
        let endpoint = Endpoint::replying(&clock, Ok(reply.clone()));
        let ran = run(&chain(vec![Box::new(asking(&endpoint))]), &catalog, &clock);
        let [Classification::Abstained(abstention)] = ran.attempts.as_slice() else {
            panic!("{reply:?} decides nothing: {:?}", ran.attempts);
        };
        assert_eq!(abstention.cause, AbstentionCause::OffLabel, "{reply:?}");
        assert!(abstention.is_final);
        assert_eq!(abstention.call_record_id, Some(id("req-1/endpoint")));
        assert_eq!(ran.records.len(), 1);
    }
}

#[test]
fn a_choice_answer_is_held_to_the_label_set() {
    let set = labels();
    let decoded = |answer: &ChoiceAnswer| set.decode_choice(answer).map(|a| a.confidences_bps);
    let bps = |pairs: &[(&str, u16)]| {
        Some(
            pairs
                .iter()
                .map(|(label, bps)| ((*label).to_owned(), *bps))
                .collect::<std::collections::BTreeMap<_, _>>(),
        )
    };

    // Every label weighed; the label named is the choice, not the largest probability.
    let answer = chosen(
        "neutral",
        &[("positive", 0.5), ("negative", 0.25), ("neutral", 0.25)],
    );
    assert_eq!(set.decode_choice(&answer).unwrap().label, "neutral");
    assert_eq!(
        decoded(&answer),
        bps(&[("positive", 5000), ("negative", 2500), ("neutral", 2500)])
    );
    // No probability at all: a decision whose confidences are all unknown.
    assert_eq!(decoded(&chosen("positive", &[])), bps(&[]));
    // A missing label stays unknown.
    assert_eq!(
        decoded(&chosen("positive", &[("positive", 1.0)])),
        bps(&[("positive", 10_000)])
    );
    // The sum over labels is neither checked nor normalized: above 1 and below 1 both stand.
    assert_eq!(
        decoded(&chosen("positive", &[("positive", 0.9), ("negative", 0.9)])),
        bps(&[("positive", 9000), ("negative", 9000)])
    );
    assert_eq!(
        decoded(&chosen("positive", &[("positive", 0.1), ("negative", 0.1)])),
        bps(&[("positive", 1000), ("negative", 1000)])
    );
    assert_eq!(
        decoded(&chosen("positive", &[("positive", 0.0), ("negative", 0.0)])),
        bps(&[("positive", 0), ("negative", 0)])
    );

    // A choice outside the set, with or without probabilities for the set.
    assert_eq!(decoded(&chosen("mixed", &[("positive", 0.9)])), None);
    assert_eq!(decoded(&chosen("", &[])), None);
    assert_eq!(decoded(&chosen("Positive", &[])), None, "labels are exact");
    // A probability for a value outside the set.
    assert_eq!(
        decoded(&chosen("positive", &[("positive", 0.9), ("mixed", 0.1)])),
        None
    );
    // One label twice, even with the same probability.
    assert_eq!(
        decoded(&chosen("positive", &[("positive", 0.5), ("positive", 0.5)])),
        None
    );
    // A probability outside 0 to 1, or no number at all.
    for outside in [
        -0.1,
        1.000_000_1,
        2.0,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ] {
        assert_eq!(
            decoded(&chosen("positive", &[("negative", outside)])),
            None,
            "{outside}"
        );
    }
    // A refusal and a value that is no text hold no label.
    assert_eq!(decoded(&ChoiceAnswer::Refused), None);
    assert_eq!(decoded(&ChoiceAnswer::NotText), None);
}

#[test]
fn a_choice_answer_far_longer_than_the_set_is_read_only_to_its_first_repeat() {
    let set = labels();
    // 200 000 probabilities for one label: refused at the second, whatever follows.
    let many: Vec<(&str, f64)> = std::iter::repeat_n(("positive", 0.5), 200_000).collect();
    assert_eq!(set.decode_choice(&chosen("positive", &many)), None);
}

#[test]
fn a_probability_becomes_basis_points_by_exact_rounding() {
    assert_eq!(basis_points(0.0), Some(0));
    assert_eq!(basis_points(-0.0), Some(0));
    assert_eq!(basis_points(1.0), Some(10_000));
    assert_eq!(basis_points(0.5), Some(5000));
    assert_eq!(basis_points(0.9), Some(9000));
    assert_eq!(basis_points(0.95), Some(9500));
    assert_eq!(basis_points(0.0001), Some(1));
    assert_eq!(basis_points(0.9999), Some(9999));
    // Exact ties round up: 1/32 is 312.5 and 3/32 is 937.5 basis points.
    assert_eq!(basis_points(0.031_25), Some(313));
    assert_eq!(basis_points(0.093_75), Some(938));
    // The doubles next to a tie are not ties.
    let below = f64::from_bits(0.031_25_f64.to_bits() - 1);
    let above = f64::from_bits(0.031_25_f64.to_bits() + 1);
    assert_eq!(basis_points(below), Some(312));
    assert_eq!(basis_points(above), Some(313));
    // The double nearest to 0.00005 is above it, so it is more than half a basis point. The
    // double nearest to 0.33335 is below it, so it is under 3333.5: a multiplication in doubles
    // would give 3333.5 and round it up.
    assert_eq!(basis_points(0.000_05), Some(1));
    assert_eq!(basis_points(0.000_049_999), Some(0));
    assert_eq!(basis_points(0.333_35), Some(3333));
    assert_eq!(basis_points(0.999_95), Some(10_000));
    // The largest double under 1 is within half a basis point of it.
    assert_eq!(
        basis_points(f64::from_bits(1.0_f64.to_bits() - 1)),
        Some(10_000)
    );
    assert_eq!(basis_points(0.999_949), Some(9999));
    // The smallest doubles above zero.
    assert_eq!(basis_points(f64::from_bits(1)), Some(0));
    assert_eq!(basis_points(f64::MIN_POSITIVE), Some(0));
    // Outside 0 to 1, or no number.
    assert_eq!(basis_points(f64::from_bits(1.0_f64.to_bits() + 1)), None);
    assert_eq!(basis_points(-f64::from_bits(1)), None);
    assert_eq!(basis_points(-0.5), None);
    assert_eq!(basis_points(1.5), None);
    assert_eq!(basis_points(f64::NAN), None);
    assert_eq!(basis_points(f64::INFINITY), None);
    assert_eq!(basis_points(f64::NEG_INFINITY), None);
}

#[test]
fn a_decision_endpoint_answer_under_the_floor_or_of_unknown_confidence_abstains() {
    let catalog = catalog();
    let floored = |answer: ChoiceAnswer| {
        let clock = Scripted::at(1_000);
        let endpoint = Endpoint::replying(&clock, Ok(answer));
        let classifier = asking(&endpoint).with_min_confidence_bps(9000).unwrap();
        let ran = run(&chain(vec![Box::new(classifier)]), &catalog, &clock);
        ran.attempts.last().and_then(cause)
    };
    assert_eq!(floored(chosen("positive", &[("positive", 0.9)])), None);
    assert_eq!(
        floored(chosen("positive", &[("positive", 0.899_94)])),
        Some(AbstentionCause::OffLabel),
        "8999 basis points are under a floor of 9000"
    );
    assert_eq!(
        floored(chosen("positive", &[("negative", 0.95)])),
        Some(AbstentionCause::OffLabel),
        "an unknown confidence meets no floor"
    );
    let clock = Scripted::at(1_000);
    let endpoint = Endpoint::replying(&clock, Ok(ChoiceAnswer::Refused));
    assert!(asking(&endpoint).with_min_confidence_bps(10_001).is_err());
    assert!(asking(&endpoint).with_min_confidence_bps(10_000).is_ok());
}

#[test]
fn a_decision_endpoint_failure_abstains_with_its_code() {
    let catalog = catalog();
    let failure = Error::new(ErrorCode::RateLimited, "scripted").with_dispatch(Dispatch::Accepted);

    // A failed call that wrote its record names it.
    let clock = Scripted::at(1_000);
    let endpoint = Endpoint::replying(&clock, Err(failure.clone()));
    let ran = run(&chain(vec![Box::new(asking(&endpoint))]), &catalog, &clock);
    let [Classification::Abstained(abstention)] = ran.attempts.as_slice() else {
        panic!("a failed call decides nothing: {:?}", ran.attempts);
    };
    assert_eq!(
        abstention.cause,
        AbstentionCause::ProviderError(ErrorCode::RateLimited)
    );
    assert_eq!(abstention.call_record_id, Some(id("req-1/endpoint")));
    assert_eq!(ran.records.len(), 1);
    assert_eq!(ran.records[0].outcome, Outcome::Failed);
    assert_eq!(endpoint.asked().len(), 1, "one call, no retry");

    // A request refused before any call wrote no record, so the abstention names none.
    let clock = Scripted::at(1_000);
    let refused = Endpoint::with(
        &clock,
        Err(Error::invalid("scripted refusal before the call")),
        |endpoint| endpoint.writes_record = false,
    );
    let ran = run(&chain(vec![Box::new(asking(&refused))]), &catalog, &clock);
    let [Classification::Abstained(abstention)] = ran.attempts.as_slice() else {
        panic!("a refused request decides nothing: {:?}", ran.attempts);
    };
    assert_eq!(
        abstention.cause,
        AbstentionCause::ProviderError(ErrorCode::InvalidRequest)
    );
    assert_eq!(abstention.call_record_id, None);
    assert!(ran.records.is_empty());
}

#[test]
fn a_decision_endpoint_call_open_at_the_deadline_is_cancelled_and_abstains_timeout() {
    let catalog = catalog();
    let deadline = Duration::from_millis(2_000);
    let set = || -> LabelSet { labels() };

    // The call hangs: it is cancelled at the deadline, its record is written, and the next
    // classifier, reached with no time left, abstains without a call.
    let clock = Scripted::at(1_000);
    let hanging = Endpoint::with(&clock, Ok(ChoiceAnswer::Refused), |endpoint| {
        endpoint.hang = true;
    });
    let never = Endpoint::replying(&clock, Ok(chosen("positive", &[])));
    let classifiers: Vec<Box<dyn Classifier>> = vec![
        Box::new(EndpointClassifier::new(
            id("first"),
            "code",
            hanging.clone(),
        )),
        Box::new(EndpointClassifier::new(id("second"), "code", never.clone())),
    ];
    let chain = Chain::new(id("chain"), set(), deadline, classifiers).unwrap();
    let ran = run(&chain, &catalog, &clock);
    let [
        Classification::Abstained(first),
        Classification::Abstained(second),
    ] = ran.attempts.as_slice()
    else {
        panic!("nothing decides past the deadline: {:?}", ran.attempts);
    };
    assert_eq!(first.cause, AbstentionCause::Timeout);
    assert_eq!(first.latency_ms, 2_000);
    assert_eq!(first.call_record_id, Some(id("req-1/first")));
    assert!(
        hanging.cancelled.load(Ordering::SeqCst),
        "the call is cancelled"
    );
    assert_eq!(second.cause, AbstentionCause::Timeout);
    assert!(second.is_final);
    assert_eq!(second.call_record_id, None);
    assert!(never.asked().is_empty(), "no call starts past the deadline");
    assert_eq!(ran.records.len(), 1);
    assert_eq!(ran.records[0].outcome, Outcome::Cancelled);

    // An answer that arrives exactly at the deadline is not used; one a millisecond earlier is.
    for (takes_ms, timed_out) in [(2_000, true), (1_999, false)] {
        let clock = Scripted::at(1_000);
        let endpoint = Endpoint::with(&clock, Ok(chosen("positive", &[])), |endpoint| {
            endpoint.takes_ms = takes_ms;
        });
        let chain = Chain::new(
            id("chain"),
            set(),
            deadline,
            vec![Box::new(asking(&endpoint))],
        )
        .unwrap();
        let ran = run(&chain, &catalog, &clock);
        assert_eq!(
            ran.attempts.last().and_then(cause),
            timed_out.then_some(AbstentionCause::Timeout),
            "{takes_ms} ms"
        );
        assert_eq!(ran.records.len(), 1);
    }
}

#[test]
fn an_unknown_route_or_a_refused_admission_abstains_without_a_call() {
    let catalog = catalog();
    let clock = Scripted::at(1_000);
    let endpoint = Endpoint::replying(&clock, Ok(chosen("positive", &[])));

    let unknown = EndpointClassifier::new(id("endpoint"), "no-such-route", endpoint.clone());
    let ran = run(&chain(vec![Box::new(unknown)]), &catalog, &clock);
    assert_eq!(
        ran.attempts.last().and_then(cause),
        Some(AbstentionCause::ProviderError(ErrorCode::InvalidRequest))
    );

    let over_budget = |_: &Selection<'_>| -> Result<(), Error> {
        Err(Error::new(ErrorCode::Refused, "scripted limit").with_dispatch(Dispatch::NotSent))
    };
    let ran = run_admitting(
        &chain(vec![Box::new(asking(&endpoint))]),
        &catalog,
        &clock,
        &over_budget,
    );
    let [Classification::Abstained(abstention)] = ran.attempts.as_slice() else {
        panic!("a refused admission decides nothing: {:?}", ran.attempts);
    };
    assert_eq!(
        abstention.cause,
        AbstentionCause::ProviderError(ErrorCode::Refused)
    );
    assert_eq!(abstention.call_record_id, None);
    assert!(ran.records.is_empty());
    assert!(
        endpoint.asked().is_empty(),
        "neither refusal reached the endpoint"
    );
}

/// Every other run estimates 100 input tokens, which every target of the route admits. The
/// target is selected against the estimate the caller gave: one no target has room for, or
/// none at all, selects nothing, and the endpoint is not asked.
#[test]
fn a_decision_endpoint_target_is_selected_against_the_callers_token_estimate() {
    let catalog = catalog();
    for estimate in [Some(u64::MAX), None] {
        let clock = Scripted::at(1_000);
        let endpoint = Endpoint::replying(&clock, Ok(chosen("positive", &[])));
        let ran = run_estimating(
            &chain(vec![Box::new(asking(&endpoint))]),
            &catalog,
            &clock,
            &|_: &Selection<'_>| Ok(()),
            estimate,
        );
        let [Classification::Abstained(abstention)] = ran.attempts.as_slice() else {
            panic!("no target is selected for {estimate:?}: {:?}", ran.attempts);
        };
        assert_eq!(
            abstention.cause,
            AbstentionCause::ProviderError(ErrorCode::Unsupported),
            "{estimate:?}"
        );
        assert_eq!(abstention.call_record_id, None);
        assert!(endpoint.asked().is_empty(), "{estimate:?}");
        assert!(ran.records.is_empty());
    }
}
