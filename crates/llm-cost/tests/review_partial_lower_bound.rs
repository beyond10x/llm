//! Conformance checks for the partial-usage lower-bound contract (docs/pricing.md, "For
//! `final_usage = false`, calculable amounts remain in `known_subtotal`" and "Protocol adapters
//! ... may only report partial quantities that are valid lower bounds").
//!
//! The property: if every quantity in a partial snapshot is <= the same quantity in the final
//! snapshot of that attempt, the partial `known_subtotal` must be <= the final `complete_total`.
use llm_core::{BillingKind, Dispatch, Id, Protocol, Provenance, Usage};
use llm_cost::{
    AccountingInput, Amount, AttemptUsage, Currency, Observation, PriceBook, budget::*,
};

fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}
fn prices() -> PriceBook {
    PriceBook::parse_toml(include_str!("../../../examples/prices.toml")).unwrap()
}
fn binding() -> Provenance {
    Provenance {
        protocol: Protocol::Messages,
        provider: id("lab"),
        account: id("lab-account"),
        endpoint: id("endpoint"),
        model: id("model"),
        binding_revision: id("fixture-binding-r1"),
    }
}
fn attempt(final_usage: bool, cached: u64) -> AttemptUsage {
    AttemptUsage {
        id: id("attempt"),
        usage_record_id: id("usage"),
        request_id: id("request"),
        route_target_id: id("target"),
        serving_model_id: id("lab-serving"),
        price_id: id("model-price"),
        binding: binding(),
        upstream_model: Some(id("lab/model-v1")),
        billing: BillingKind::Metered,
        dispatch: Dispatch::Accepted,
        failed: false,
        final_usage,
        usage: Usage {
            input_tokens: Some(1_000_000),
            cached_input_tokens: Some(cached),
            cache_creation_input_tokens: Some(0),
            output_tokens: Some(0),
            reasoning_output_tokens: None,
        },
    }
}
fn quote(a: AttemptUsage) -> llm_cost::Total {
    prices()
        .quote(&AccountingInput::new(vec![Observation::Attempt(a)]))
        .unwrap()
        .records[0]
        .total
        .clone()
}

/// Each partial quantity (input 1M, cached 0, creation 0, output 0) is a valid lower bound of the
/// final quantity (input 1M, cached 900k, creation 0, output 0). The derived uncached quantity
/// (input - cached - creation) is not monotone in those bounds, so the partial valuation exceeds
/// the final cost.
#[test]
fn partial_known_subtotal_never_exceeds_the_final_complete_total() {
    let partial = quote(attempt(false, 0));
    let complete = quote(attempt(true, 900_000)).complete_total.unwrap();
    assert!(
        partial.known_subtotal <= complete,
        "partial 'lower bound' {} exceeds final complete cost {}",
        partial.known_subtotal,
        complete
    );
}

/// The documented composition: settle an incomplete lower bound, then the final complete cost.
/// "Previously known cost cannot be reduced" turns an over-estimated partial into a refusal of the
/// correct final settlement, leaving the obligation uncertain and admission blocked.
#[test]
fn final_settlement_after_a_partial_settlement_is_accepted() {
    let mut e = BudgetEngine::new(BudgetPolicy {
        id: id("scope"),
        currency: Currency::new("USD").unwrap(),
        limit: Amount::parse("10").unwrap(),
        max_active: 4,
    })
    .unwrap();
    e.apply(
        1,
        BudgetCommand::Reserve {
            request: ReservationRequest {
                id: id("a"),
                operation: Operation::Inference {
                    binding: binding(),
                    billing: BillingKind::Metered,
                    request_id: id("request"),
                },
                reserved: Amount::parse("5").unwrap(),
                assumption: id("model-price"),
                expires_at_ms: 100,
            },
        },
    )
    .unwrap();
    e.apply(2, BudgetCommand::Begin { id: id("a") }).unwrap();
    let settle = |known: Amount, complete: bool| BudgetCommand::Settle {
        id: id("a"),
        settlement: Settlement {
            currency: Currency::new("USD").unwrap(),
            known,
            complete,
            evidence: id("observed"),
        },
    };
    e.apply(3, settle(quote(attempt(false, 0)).known_subtotal, false))
        .unwrap();
    let complete = quote(attempt(true, 900_000)).complete_total.unwrap();
    assert_eq!(
        e.apply(4, settle(complete, true)).err(),
        None,
        "final complete settlement was refused; obligation stays {:?}",
        e.reservation(&id("a")).unwrap().phase
    );
}
