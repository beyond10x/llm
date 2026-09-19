use llm_core::{BillingKind, Dispatch, Id, Protocol, Provenance, Usage};
use llm_cost::{
    AccountingInput, Amount, AttemptUsage, Basis, ChargeKind, ComputeUsage, CostError, Currency,
    Observation, PriceBook, Rate, RecordedCharge, UnknownReason,
};
use serde_json::json;

fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}
fn prices() -> PriceBook {
    PriceBook::parse_toml(include_str!("../../../examples/prices.toml")).unwrap()
}
fn attempt(name: &str) -> AttemptUsage {
    AttemptUsage {
        id: id(name),
        usage_record_id: id("usage"),
        request_id: id("request"),
        route_target_id: id("target"),
        serving_model_id: id("lab-serving"),
        price_id: id("model-price"),
        binding: Provenance {
            protocol: Protocol::Responses,
            provider: id("lab"),
            account: id("lab-account"),
            endpoint: id("endpoint"),
            model: id("model"),
            binding_revision: id("fixture-binding-r1"),
        },
        upstream_model: Some(id("lab/model-v1")),
        billing: BillingKind::Metered,
        dispatch: Dispatch::Accepted,
        failed: false,
        usage: Usage {
            input_tokens: Some(100),
            cached_input_tokens: Some(20),
            cache_creation_input_tokens: Some(10),
            output_tokens: Some(5),
            reasoning_output_tokens: Some(3),
        },
    }
}
fn charge() -> RecordedCharge {
    RecordedCharge {
        id: id("subscription"),
        source: "fixture invoice".into(),
        provider: id("lab"),
        account: id("lab-account"),
        subject: id("subscription-period"),
        period_start_unix_ms: 1,
        period_end_unix_ms: 2,
        currency: Currency::new("USD").unwrap(),
        amount: Some(Amount::parse("20").unwrap()),
        charge_kind: ChargeKind::Subscription,
    }
}

#[test]
fn exact_money_parsing_rounding_and_overflow() {
    for input in [
        "-1",
        "+1",
        "1e3",
        "1.0000000001",
        "01",
        ".1",
        "1.",
        "NaN",
        " 1",
        "18446744073.709551616",
    ] {
        assert_eq!(
            Amount::parse(input),
            Err(CostError::InvalidAmount),
            "{input}"
        );
    }
    assert_eq!(
        Amount::parse("18446744073.709551615").unwrap().nanos(),
        u64::MAX
    );
    assert_eq!(Amount::parse("1.250000000").unwrap().to_string(), "1.25");
    assert!(serde_json::from_value::<Amount>(json!(1.25)).is_err());
    assert_eq!(
        Rate {
            amount: Amount::from_nanos(1),
            per_units: 3
        }
        .price(1)
        .unwrap()
        .nanos(),
        1
    );
    assert_eq!(
        Rate {
            amount: Amount::from_nanos(u64::MAX),
            per_units: 1
        }
        .price(2),
        Err(CostError::Overflow)
    );
    assert_eq!(
        Rate {
            amount: Amount::ZERO,
            per_units: 0
        }
        .price(0),
        Err(CostError::InvalidDenominator)
    );
    for currency in ["usd", "US", "USDD", "1SD", "€"] {
        assert!(Currency::new(currency).is_err());
    }
}

#[test]
fn prices_disjoint_cache_and_counts_reasoning_once() {
    let observation = Observation::Attempt(attempt("one"));
    let report = prices()
        .quote(&AccountingInput::new(vec![observation.clone()]))
        .unwrap();
    assert_eq!(report.records[0].observation, observation);
    // 70*2 +20*0.5 +10*3 +5*8 = 220 currency-millionths.
    assert_eq!(
        report.records[0]
            .lines
            .iter()
            .map(|v| v.quantity)
            .collect::<Vec<_>>(),
        vec![Some(70), Some(20), Some(10), Some(5)]
    );
    assert_eq!(
        report.totals[&Basis::MeteredEstimate].complete_total,
        Some(Amount::parse("0.00022").unwrap())
    );
    assert_eq!(
        serde_json::to_value(report).unwrap()["format"],
        "llm.cost/1"
    );
}

#[test]
fn unknown_partitions_rates_and_actual_model_remain_unknown() {
    let mut a = attempt("one");
    a.usage.cached_input_tokens = None;
    let report = prices()
        .quote(&AccountingInput::new(vec![Observation::Attempt(a.clone())]))
        .unwrap();
    assert_eq!(report.totals[&Basis::MeteredEstimate].unknown_lines, 2);
    assert_eq!(report.totals[&Basis::MeteredEstimate].complete_total, None);
    assert_eq!(
        report.totals[&Basis::MeteredEstimate].known_subtotal,
        Amount::parse("0.00007").unwrap()
    );
    a.upstream_model = None;
    let report = prices()
        .quote(&AccountingInput::new(vec![Observation::Attempt(a)]))
        .unwrap();
    assert!(
        report.records[0]
            .lines
            .iter()
            .all(|v| v.amount.is_none() && v.unknown == Some(UnknownReason::ModelUnknown))
    );
    let mut doc = prices().document().clone();
    doc.models[0].cached_input = None;
    let mut a = attempt("one");
    a.usage.cached_input_tokens = Some(0);
    let report = PriceBook::from_document(doc)
        .unwrap()
        .quote(&AccountingInput::new(vec![Observation::Attempt(a)]))
        .unwrap();
    assert_eq!(report.records[0].lines[1].amount, Some(Amount::ZERO));
}

#[test]
fn refuses_invalid_partitions_duplicate_records_currency_and_periods() {
    let mut a = attempt("one");
    a.usage.input_tokens = Some(2);
    assert_eq!(
        prices()
            .quote(&AccountingInput::new(vec![Observation::Attempt(a)]))
            .unwrap_err(),
        CostError::InvalidUsage
    );
    let a = Observation::Attempt(attempt("one"));
    assert_eq!(
        prices()
            .quote(&AccountingInput::new(vec![a.clone(), a]))
            .unwrap_err(),
        CostError::DuplicateObservation
    );
    let mut c = charge();
    c.currency = Currency::new("EUR").unwrap();
    assert_eq!(
        prices()
            .quote(&AccountingInput::new(vec![Observation::Recorded(
                c.clone()
            )]))
            .unwrap_err(),
        CostError::CurrencyMismatch
    );
    c.currency = Currency::new("USD").unwrap();
    c.period_end_unix_ms = 1;
    assert_eq!(
        prices()
            .quote(&AccountingInput::new(vec![Observation::Recorded(c)]))
            .unwrap_err(),
        CostError::InvalidPeriod
    );
}

#[test]
fn failed_attempts_and_fixed_compute_subscription_charges_stay_attributed() {
    let mut failed = attempt("failed");
    failed.failed = true;
    failed.dispatch = Dispatch::Unknown;
    let mut subscription = attempt("included");
    subscription.billing = BillingKind::Subscription;
    let report = prices()
        .quote(&AccountingInput::new(vec![
            Observation::Attempt(failed.clone()),
            Observation::Attempt(attempt("successful")),
            Observation::Attempt(subscription),
            Observation::Recorded(charge()),
            Observation::Compute(ComputeUsage {
                id: id("pod-interval"),
                price_id: id("resource-price"),
                hosting_provider: id("lab-host"),
                account: id("lab-account"),
                resource_class: id("whole-pod"),
                resource_id: id("owned-pod"),
                duration_ms: Some(1000),
            }),
        ]))
        .unwrap();
    assert_eq!(report.records.len(), 5);
    assert_eq!(report.records[0].observation, Observation::Attempt(failed));
    assert_eq!(
        report.totals[&Basis::MeteredEstimate].complete_total,
        Some(Amount::parse("0.00044").unwrap())
    );
    assert_eq!(
        report.totals[&Basis::ReferenceUsage].complete_total,
        Some(Amount::parse("0.00022").unwrap())
    );
    assert_eq!(
        report.totals[&Basis::SubscriptionCharge].complete_total,
        Some(Amount::parse("20").unwrap())
    );
    assert_eq!(
        report.totals[&Basis::ComputeEstimate].complete_total,
        Some(Amount::parse("0.001").unwrap())
    );
}

#[test]
fn version_source_and_sorted_price_content_determine_identity() {
    let p = prices();
    let mut described = p.document().clone();
    described.source = "Operator rate card — 測定".into();
    let described = PriceBook::from_document(described).unwrap();
    assert_eq!(described.document().source, "Operator rate card — 測定");
    assert_ne!(p.digest(), described.digest());
    let mut doc = p.document().clone();
    let mut row = doc.models[0].clone();
    row.id = id("second");
    doc.models.push(row);
    let first = PriceBook::from_document(doc.clone()).unwrap();
    doc.models.reverse();
    assert_eq!(
        first.digest(),
        PriceBook::from_document(doc.clone()).unwrap().digest()
    );
    doc.revision = id("changed-revision");
    assert_ne!(
        first.digest(),
        PriceBook::from_document(doc.clone()).unwrap().digest()
    );
    doc.models[0].input.as_mut().unwrap().per_units = 0;
    assert_eq!(
        PriceBook::from_document(doc).unwrap_err(),
        CostError::InvalidDenominator
    );
    let mut raw = serde_json::to_value(p.document()).unwrap();
    raw["format"] = json!("llm.prices/2");
    assert_eq!(
        PriceBook::parse_json(&raw.to_string()).unwrap_err(),
        CostError::InvalidDocument
    );
    let mut doc = p.document().clone();
    doc.models.push(doc.models[0].clone());
    assert_eq!(
        PriceBook::from_document(doc).unwrap_err(),
        CostError::DuplicatePrice
    );
    assert_eq!(
        AccountingInput::parse_json(r#"{"format":"llm.usage/2","observations":[]}"#).unwrap_err(),
        CostError::InvalidDocument
    );
}
