use llm_cost::{AccountingInput, PriceBook};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let prices = PriceBook::parse_toml(include_str!("../../../examples/prices.toml"))?;
    let usage = AccountingInput::parse_json(include_str!("../../../examples/usage.json"))?;
    println!("{}", serde_json::to_string_pretty(&prices.quote(&usage)?)?);
    Ok(())
}
