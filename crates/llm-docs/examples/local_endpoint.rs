//! Send one turn to the anonymous Chat Completions server `examples/catalog.toml` declares.
//!
//! Start a vLLM-compatible server on `127.0.0.1:8000`, then run
//! `cargo run --locked -p llm-docs --example local_endpoint` from the repository root.
use llm_core::{Cancel, Id, Item, Model, TurnRequest, VecSink};
use llm_credentials::SecretResolver;
use llm_http::{HttpClient, Limits};
use llm_routing::Catalog;
use std::{collections::BTreeMap, sync::Arc};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let catalog = Catalog::parse(&std::fs::read_to_string("examples/catalog.toml")?)?;
    // The local account is anonymous, so the caller supplies no resolver for it.
    let resolvers: BTreeMap<Id, Arc<dyn SecretResolver>> = BTreeMap::new();
    let client = llm_models::port(
        &catalog,
        &Id::new("local-small")?,
        HttpClient::new(Limits::default())?,
        &resolvers,
    )?;

    let request = TurnRequest::new("small", vec![Item::user("Hallo")]);
    let mut sink = VecSink::new(64, 64 * 1024);
    let outcome = client.turn(&request, &mut sink, &Cancel::new()).await?;

    println!("text:           {}", sink.text());
    println!("stop reason:    {:?}", outcome.stop_reason);
    println!(
        "upstream model: {:?}",
        outcome.observation.upstream_model.map(|m| m.to_string())
    );
    println!("usage:          {:?}", outcome.observation.usage);
    println!("final usage:    {}", outcome.observation.final_usage);
    Ok(())
}
