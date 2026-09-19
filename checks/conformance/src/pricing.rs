//! Only real parser/quote results feed this view. Expected assertions are unavailable here.
use llm_cost::{AccountingInput, CostError, PriceBook};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    prices_toml: String,
    usage_json: String,
}

pub fn observe(input: &Input) -> Value {
    let mut facts = json!({"accepted":false,"error_code":null,"currency":null,
        "price_revision":null,"price_source":null,"price_as_of_unix_ms":null,
        "price_digest":null,"record_ids":[],"lines":[],"totals":[],"observations_preserved":false});
    if let Err(error) = quote(input, &mut facts) {
        facts["error_code"] = json!(error);
    }
    facts
}

fn quote(input: &Input, facts: &mut Value) -> Result<(), CostError> {
    let prices = PriceBook::parse_toml(&input.prices_toml)?;
    let usage = AccountingInput::parse_json(&input.usage_json)?;
    let report = prices.quote(&usage)?;
    facts["accepted"] = json!(true);
    facts["currency"] = json!(report.currency);
    facts["price_revision"] = json!(report.price_revision);
    facts["price_source"] = json!(report.price_source);
    facts["price_as_of_unix_ms"] = json!(report.price_as_of_unix_ms);
    facts["price_digest"] = json!(report.price_digest);
    facts["record_ids"] = json!(
        report
            .records
            .iter()
            .map(|r| r.observation.id())
            .collect::<Vec<_>>()
    );
    facts["observations_preserved"] = json!(
        report
            .records
            .iter()
            .map(|r| &r.observation)
            .eq(usage.observations.iter())
    );
    facts["lines"] = json!(
        report
            .records
            .iter()
            .flat_map(|r| {
                r.lines.iter().map(|line| json!({
        "record_id":r.observation.id(),"basis":r.basis,"unit":line.unit,"quantity":line.quantity.map(|n| n.to_string()),
        "amount":line.amount,"unknown":line.unknown
    }))
            })
            .collect::<Vec<_>>()
    );
    facts["totals"] = json!(
        report
            .totals
            .iter()
            .map(|(basis, total)| json!({"basis":basis,
        "known_subtotal":total.known_subtotal,"unknown_lines":total.unknown_lines,
        "complete_total":total.complete_total}))
            .collect::<Vec<_>>()
    );
    Ok(())
}
