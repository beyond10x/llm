//! Adversary cases of wave 28 against the decision-endpoint path: an independent reading of
//! the basis-point rule, and guards for mutants the suite of `endpoint.rs` does not catch.

use super::{
    AbstentionCause, Arc, AtomicUsize, BoxFuture, CallLog, CallRecord, Cancel, Chain,
    Classification, Duration, Error, ErrorCode, Fleet, INPUT, Id, Kind, Mutex, Ordering, Outcome,
    Ports, Recording, Scripted, Selection, Usage, block_on, catalog, chain, id,
};
use llm_calls::{CallFacts, CallIdentity, Stopwatch};
use llm_decide::{
    ChoiceAnswer, ChoiceCall, DecisionEndpoint, EndpointClassifier, LabelProbability, basis_points,
};

/// The basis points of `probability` read off its exact decimal expansion: a double from 0 to 1
/// has at most 1074 fraction digits, so 1080 of them are all of them. Times 10000 moves the
/// point four digits; the fifth decides the rounding, 5 and above up.
fn by_decimal_expansion(probability: f64) -> u16 {
    let text = format!("{probability:.1080}");
    let (whole, fraction) = text.split_once('.').expect("a fixed-point text");
    let digits: String = format!("{whole}{}", &fraction[..4]);
    let points: u16 = digits.parse().expect("at most 10000");
    points + u16::from(fraction.as_bytes()[4] >= b'5')
}

/// A fixed-seed sequence of 64-bit words (xorshift64).
struct Words(u64);

impl Words {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// "The whole number nearest to the probability times 10000, computed exactly on the double,
/// a value exactly halfway rounds up": checked against the decimal expansion for doubles over
/// the whole range, at every half basis point and at both neighbours of each.
#[test]
fn adversary_w28_basis_points_agree_with_the_exact_decimal_expansion_of_the_double() {
    let mut checked = 0_u32;
    let mut check = |probability: f64| {
        assert_eq!(
            basis_points(probability),
            Some(by_decimal_expansion(probability)),
            "{probability:e} ({:#018x})",
            probability.to_bits()
        );
        checked += 1;
    };
    // Every half basis point k/20000 as its nearest double, and the doubles next to it.
    for k in 0_u32..=20_000 {
        let near = f64::from(k) / 20_000.0;
        check(near);
        if near > 0.0 {
            check(f64::from_bits(near.to_bits() - 1));
        }
        if near < 1.0 {
            check(f64::from_bits(near.to_bits() + 1));
        }
    }
    // Exact ties: an odd multiple of 1/32 has 5 fraction bits and is k * 312.5 basis points.
    for k in (1_u32..32).step_by(2) {
        check(f64::from(k) / 32.0);
    }
    // Doubles of every binary exponent from the smallest subnormal up to 1, random fractions.
    let mut words = Words(0x9e37_79b9_7f4a_7c15);
    for exponent in 0_u64..1023 {
        for _ in 0..8 {
            let fraction = words.next() & ((1 << 52) - 1);
            check(f64::from_bits((exponent << 52) | fraction));
        }
    }
    // Random doubles where the rounding happens: 2^-15 up to 1.
    for _ in 0..20_000 {
        let word = words.next();
        let exponent = 1008 + (word >> 60) % 15;
        check(f64::from_bits((exponent << 52) | (word & ((1 << 52) - 1))));
    }
    assert!(checked > 80_000, "{checked}");
}

/// An endpoint that answers at once and writes its record under the id `writes_as`, or none.
struct Elsewhere {
    writes_as: Option<Id>,
    asked: AtomicUsize,
}

impl DecisionEndpoint for Elsewhere {
    fn choose<'a>(&'a self, call: ChoiceCall<'a>) -> BoxFuture<'a, Result<ChoiceAnswer, Error>> {
        Box::pin(async move {
            self.asked.fetch_add(1, Ordering::SeqCst);
            let stopwatch = Stopwatch::start(call.recording.clock);
            if let Some(other) = &self.writes_as {
                call.recording.sink.record(CallRecord::new(
                    CallIdentity::primary(other.clone()),
                    Kind::Turn,
                    call.selection.target.route_id.clone(),
                    stopwatch.stop(call.recording.clock),
                    CallFacts {
                        route_target_id: Some(call.selection.target.id.clone()),
                        upstream_model: None,
                        outcome: Outcome::Succeeded,
                        error_code: None,
                        dispatch: None,
                        attempts: 1,
                        usage: Usage::default(),
                        cost: None,
                    },
                ));
            }
            Ok(ChoiceAnswer::Chosen {
                choice: "positive".to_owned(),
                probabilities: vec![LabelProbability {
                    label: "positive".to_owned(),
                    probability: 0.9,
                }],
            })
        })
    }
}

struct Ran {
    attempts: Vec<Classification>,
    records: Vec<CallRecord>,
    admitted: usize,
}

fn run(chain: &Chain, cancel: &Cancel) -> Ran {
    let catalog = catalog();
    let clock = Scripted::at(1_000);
    let log = CallLog::new();
    let fleet = Fleet::of(&catalog, &clock, []);
    let admitted = Mutex::new(0_usize);
    let admit = |_: &Selection<'_>| -> Result<(), Error> {
        *admitted.lock().unwrap() += 1;
        Ok(())
    };
    let pause = |_: Duration| -> BoxFuture<'static, ()> { Box::pin(std::future::pending()) };
    let outcome = block_on(chain.classify(
        &id("req-1"),
        INPUT,
        Some(100),
        Ports {
            catalog: &catalog,
            models: &fleet,
            admit: &admit,
            cancel,
            pause: &pause,
            recording: Recording {
                sink: &log,
                clock: &*clock,
                prices: None,
            },
        },
    ));
    let admitted = *admitted.lock().unwrap();
    Ran {
        attempts: outcome.attempts,
        records: log.records(),
        admitted,
    }
}

/// The design: "a decision or abstention names it only when that record reached the sink". The
/// suite covers an endpoint that writes its record and one that writes none; a record under
/// another id is not the call's own.
#[test]
fn adversary_w28_a_record_under_another_id_is_not_named_as_the_calls_own() {
    let endpoint = Arc::new(Elsewhere {
        writes_as: Some(id("someone-elses-record")),
        asked: AtomicUsize::new(0),
    });
    let classifier = EndpointClassifier::new(id("endpoint"), "code", endpoint.clone());
    let ran = run(&chain(vec![Box::new(classifier)]), &Cancel::new());
    let [Classification::Decided(decision)] = ran.attempts.as_slice() else {
        panic!("a choice of the set decides: {:?}", ran.attempts);
    };
    assert_eq!(
        decision.call_record_id, None,
        "no record with the id req-1/endpoint reached the sink"
    );
    assert_eq!(ran.records.len(), 1);
    assert_eq!(ran.records[0].id, id("someone-elses-record"));
    assert_eq!(ran.records[0].kind, Kind::Classification);
}

/// A chain cancelled before the classifier's call makes none: the endpoint is not asked and the
/// limit decision is not consulted for a call that will not start.
#[test]
fn adversary_w28_a_chain_cancelled_before_the_call_asks_no_endpoint() {
    let endpoint = Arc::new(Elsewhere {
        writes_as: None,
        asked: AtomicUsize::new(0),
    });
    let classifier = EndpointClassifier::new(id("endpoint"), "code", endpoint.clone());
    let cancel = Cancel::new();
    cancel.cancel();
    let ran = run(&chain(vec![Box::new(classifier)]), &cancel);
    let [Classification::Abstained(abstention)] = ran.attempts.as_slice() else {
        panic!("a cancelled chain decides nothing: {:?}", ran.attempts);
    };
    assert_eq!(
        abstention.cause,
        AbstentionCause::ProviderError(ErrorCode::Cancelled)
    );
    assert_eq!(abstention.call_record_id, None);
    assert_eq!(endpoint.asked.load(Ordering::SeqCst), 0);
    assert_eq!(ran.admitted, 0);
    assert!(ran.records.is_empty());
}
