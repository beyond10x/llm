//! Adversary cases for the rubric judge, second pass (wave 27, unit 1): the nesting an answer
//! may have, and two properties over numbers with a fixed seed: a bound and a literal order
//! every pair as their exact values do, and a number that is read and written comes back as
//! itself. The pass's case on the cost of reading a long answer counts the reader's work and
//! stands beside the reader, in `src/answer.rs`.
//!
//! The scripted model replays the authored answer; nothing is chosen here.

use llm_calls::{CallLog, Clock, Recording};
use llm_core::{
    BoxFuture, Cancel, Capabilities, Error, Id, Item, Model, Provenance, StopReason, StreamSink,
    TurnObservation, TurnOutcome, TurnRequest,
};
use llm_decide::{Ports, Rubric, judge};
use llm_routing::{Catalog, CatalogDocument, Models, Selection};
use serde_json::{Value, json};
use std::{
    cmp::Ordering,
    collections::BTreeMap,
    future::Future,
    task::{Context, Poll, Waker},
    time::Duration,
};

const EXAMPLE: &str = include_str!("../../../../examples/catalog.toml");

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

/// The example catalog with the route `code` cut to its one target that declares temperature.
fn pinned() -> Catalog {
    let mut doc = CatalogDocument::parse(EXAMPLE).unwrap();
    doc.targets
        .retain(|target| target.serving_model_id.as_str() == "remote-large");
    doc.targets[0].position = 0;
    doc.validate().unwrap()
}

struct Still;

impl Clock for Still {
    fn unix_ms(&self) -> u64 {
        1_762_430_400_000
    }
    fn monotonic(&self) -> Duration {
        Duration::ZERO
    }
}

struct Fake<'a> {
    provenance: Provenance,
    capabilities: Capabilities,
    answer: &'a str,
}

impl Model for Fake<'_> {
    fn provenance(&self) -> &Provenance {
        &self.provenance
    }
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }
    fn turn<'a>(
        &'a self,
        _: &'a TurnRequest,
        _: &'a mut dyn StreamSink,
        _: &'a Cancel,
    ) -> BoxFuture<'a, Result<TurnOutcome, Error>> {
        Box::pin(async move {
            let mut observation = TurnObservation::new(self.provenance.clone());
            observation.upstream_model = Some(id("example/Large-Model-2026-09"));
            observation.final_usage = true;
            Ok(TurnOutcome {
                stop_reason: StopReason::EndTurn,
                items: vec![Item::assistant(self.answer)],
                observation,
            })
        })
    }
}

struct Fleet<'a>(BTreeMap<Id, Fake<'a>>);

impl Models for Fleet<'_> {
    fn model(&self, serving_model_id: &Id) -> Option<&dyn Model> {
        self.0
            .get(serving_model_id)
            .map(|model| model as &dyn Model)
    }
}

/// The verdict JSON of `answer` judged against `schema`; `None` when the answer is refused.
fn judged(catalog: &Catalog, schema: &Value, answer: &str) -> Option<String> {
    let rubric = Rubric::new(id("r"), "v1", "Score the answer.", schema, "code").unwrap();
    let fleet = Fleet(
        catalog
            .bindings()
            .map(|(serving, binding)| {
                let model = Fake {
                    provenance: binding.provenance().clone(),
                    capabilities: binding.capabilities().clone(),
                    answer,
                };
                (serving.clone(), model)
            })
            .collect(),
    );
    let log = CallLog::new();
    let cancel = Cancel::new();
    let admit = |_: &Selection<'_>| Ok(());
    let pause = |_: Duration| -> BoxFuture<'static, ()> { Box::pin(async {}) };
    let verdict = block_on(judge(
        &rubric,
        "the subject",
        &id("req-1"),
        Some(100),
        Ports {
            catalog,
            models: &fleet,
            admit: &admit,
            cancel: &cancel,
            pause: &pause,
            recording: Recording {
                sink: &log,
                clock: &Still,
                prices: None,
            },
        },
    ))
    .unwrap();
    assert_eq!(verdict.verdict_json.is_none(), verdict.error_code.is_some());
    verdict.verdict_json
}

/// `docs/design.md`, Verdict: the text is one JSON value "nested no deeper than the JSON reader
/// goes (128 levels)". So an answer of 128 nested arrays is a verdict and one of 129 is not.
#[test]
fn an_answer_nests_128_levels_and_no_deeper() {
    let catalog = pinned();
    let around = |levels: usize| format!("{}{}", "[".repeat(levels), "]".repeat(levels));
    assert_eq!(judged(&catalog, &json!({}), &around(129)), None);
    assert_eq!(
        judged(&catalog, &json!({}), &around(128)),
        Some(around(128))
    );
}

/// A number as JSON holds it: an integer `u64` or `i64` holds, or a finite double.
#[derive(Clone, Copy, Debug)]
enum Num {
    U(u64),
    I(i64),
    F(f64),
}

impl Num {
    /// As an answer writes it: an integer with every digit, a double in exponent form, which is
    /// never read as an integer.
    fn text(self) -> String {
        match self {
            Self::U(value) => value.to_string(),
            Self::I(value) => value.to_string(),
            Self::F(value) => format!("{value:e}"),
        }
    }

    fn json(self) -> Value {
        match self {
            Self::U(value) => json!(value),
            Self::I(value) => json!(value),
            Self::F(value) => json!(value),
        }
    }

    fn integer(self) -> Option<i128> {
        match self {
            Self::U(value) => Some(i128::from(value)),
            Self::I(value) => Some(i128::from(value)),
            Self::F(_) => None,
        }
    }
}

/// The whole part of a double between -2^63 and 2^64, which `i128` holds exactly.
#[allow(
    clippy::cast_possible_truncation,
    reason = "the caller keeps the double inside what i128 holds"
)]
fn whole_part(value: f64) -> i128 {
    value.trunc() as i128
}

/// How an integer and a double order, by exact value and without converting the integer to a
/// double: outside the integers' range the double decides, inside it the whole parts are compared
/// and then the double's fraction.
fn integer_against_double(integer: i128, double: f64) -> Ordering {
    const TWO_TO_THE_64TH: f64 = 18_446_744_073_709_551_616.0;
    const MINUS_TWO_TO_THE_63RD: f64 = -9_223_372_036_854_775_808.0;
    if double >= TWO_TO_THE_64TH {
        return Ordering::Less;
    }
    if double < MINUS_TWO_TO_THE_63RD {
        return Ordering::Greater;
    }
    let fraction = double - double.trunc();
    integer.cmp(&whole_part(double)).then(if fraction > 0.0 {
        Ordering::Less
    } else if fraction < 0.0 {
        Ordering::Greater
    } else {
        Ordering::Equal
    })
}

/// The reference order of two numbers, written apart from the library's.
fn exact_order(left: Num, right: Num) -> Ordering {
    match (left, right) {
        (Num::F(left), Num::F(right)) => left.partial_cmp(&right).unwrap(),
        (Num::F(left), right) => integer_against_double(right.integer().unwrap(), left).reverse(),
        (left, Num::F(right)) => integer_against_double(left.integer().unwrap(), right),
        (left, right) => left.integer().unwrap().cmp(&right.integer().unwrap()),
    }
}

/// A fixed-seed generator (xorshift64*), so every run draws the same numbers.
struct Draws(u64);

impl Draws {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A finite double from any bit pattern.
    fn double(&mut self) -> f64 {
        loop {
            let value = f64::from_bits(self.next());
            if value.is_finite() {
                return value;
            }
        }
    }

    /// An integer or a double of any magnitude: integers are shifted down by a drawn count so
    /// small and large ones are as likely.
    fn number(&mut self) -> Num {
        let shift = self.next() % 64;
        match self.next() % 3 {
            0 => Num::U(self.next() >> shift),
            1 => Num::I(self.next().cast_signed() >> shift),
            _ => Num::F(self.double()),
        }
    }
}

/// The double nearest to an integer, which is where a rounding comparison goes wrong.
#[allow(
    clippy::cast_precision_loss,
    reason = "the rounding is the point: the pair differs by less than a double tells apart"
)]
fn nearest_double(value: Num) -> Num {
    match value {
        Num::U(value) => Num::F(value as f64),
        Num::I(value) => Num::F(value as f64),
        double @ Num::F(_) => double,
    }
}

/// `minimum`, `maximum` and `const` order `given` against `limit` as `exact_order` does.
fn holds_the_order(catalog: &Catalog, given: Num, limit: Num) {
    let order = exact_order(given, limit);
    let admits = |keyword: &str| {
        let schema = json!({keyword: limit.json()});
        judged(catalog, &schema, &given.text()).is_some()
    };
    assert_eq!(
        (admits("minimum"), admits("maximum"), admits("const")),
        (order.is_ge(), order.is_le(), order.is_eq()),
        "{given:?} against {limit:?} is {order:?}"
    );
}

/// Guard: `minimum`, `maximum` and `const` compare by exact value for every pair of the edges
/// (zero and minus zero, the least and the greatest subnormal, the least normal, the greatest
/// finite double, and both sides of 2^53, 2^63 and 2^64 as integers and as doubles) and for
/// drawn pairs, each also against the double nearest to it.
#[test]
fn a_bound_and_a_literal_order_every_pair_of_numbers_by_exact_value() {
    let catalog = pinned();
    let edges = [
        Num::U(0),
        Num::F(0.0),
        Num::F(-0.0),
        Num::U(1),
        Num::I(-1),
        Num::F(0.5),
        Num::F(-0.5),
        Num::F(1.5),
        Num::F(f64::from_bits(1)),
        Num::F(-f64::from_bits(1)),
        Num::F(f64::from_bits(0x000F_FFFF_FFFF_FFFF)),
        Num::F(f64::MIN_POSITIVE),
        Num::F(-f64::MIN_POSITIVE),
        Num::F(f64::MIN_POSITIVE * 2.0),
        Num::F(f64::MAX),
        Num::F(f64::MIN),
        Num::U(9_007_199_254_740_992),
        Num::U(9_007_199_254_740_993),
        Num::F(9_007_199_254_740_992.0),
        Num::F(9_007_199_254_740_994.0),
        Num::I(-9_007_199_254_740_993),
        Num::I(i64::MAX),
        Num::U(9_223_372_036_854_775_808),
        Num::U(9_223_372_036_854_775_809),
        Num::F(9_223_372_036_854_775_808.0),
        Num::F(9_223_372_036_854_774_784.0),
        Num::I(i64::MIN),
        Num::I(i64::MIN + 1),
        Num::F(-9_223_372_036_854_775_808.0),
        Num::F(-9_223_372_036_854_777_856.0),
        Num::U(u64::MAX),
        Num::U(u64::MAX - 1),
        Num::F(18_446_744_073_709_551_616.0),
        Num::F(18_446_744_073_709_549_568.0),
    ];
    for given in edges {
        for limit in edges {
            holds_the_order(&catalog, given, limit);
        }
    }
    let mut draws = Draws(0x9E37_79B9_7F4A_7C15);
    for _ in 0..1_000 {
        let (given, limit) = (draws.number(), draws.number());
        holds_the_order(&catalog, given, limit);
        holds_the_order(&catalog, given, nearest_double(given));
        holds_the_order(&catalog, nearest_double(limit), limit);
    }
}

/// Guard: a number that is read and written comes back as itself. An integer keeps every digit;
/// a double written in its shortest form by another writer than the library's reads as the same
/// double, and the verdict's own text, judged again, is written with the same bytes.
#[test]
fn a_number_that_is_read_and_written_comes_back_as_itself() {
    let catalog = pinned();
    let number = json!({"type": "number"});
    let mut draws = Draws(0xD1B5_4A32_D192_ED03);
    for _ in 0..2_000 {
        let shift = draws.next() % 64;
        for integer in [
            (draws.next() >> shift).to_string(),
            (draws.next().cast_signed() >> shift).to_string(),
        ] {
            assert_eq!(
                judged(&catalog, &number, &integer).as_deref(),
                Some(integer.as_str())
            );
        }
        let double = draws.double();
        let written = judged(&catalog, &number, &format!("{double:e}")).unwrap();
        let read: f64 = written.parse().unwrap();
        assert_eq!(
            read.to_bits(),
            double.to_bits(),
            "{double:e} is written {written}"
        );
        assert_eq!(
            judged(&catalog, &number, &written).as_deref(),
            Some(written.as_str()),
            "{double:e}"
        );
    }
}
