use llm_core::{Item, TurnRequest};
use llm_routing::Catalog;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let catalog = Catalog::parse(include_str!("../../../examples/catalog.toml"))?;
    let mut request = TurnRequest::new(
        "code",
        vec![Item::UserText {
            text: "Explain this function.".into(),
        }],
    );
    request.max_output_tokens = Some(256);
    // For this example the caller supplies a conservative bound. Production callers must derive
    // one for all candidate models, including instructions, history, tool schemas and overhead.
    let explanation = catalog.explain(&request, Some(512))?;
    println!("{}", serde_json::to_string_pretty(&explanation)?);
    Ok(())
}
