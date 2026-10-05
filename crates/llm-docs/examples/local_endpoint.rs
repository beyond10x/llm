//! Send one turn to the anonymous Chat Completions server `examples/catalog.toml` declares.
//!
//! Start a vLLM-compatible server on `127.0.0.1:8000`, then run
//! `cargo run --locked -p llm-docs --example local_endpoint` from the repository root.
use llm_chat::ChatClient;
use llm_core::{BoxFuture, Cancel, Id, Item, Model, TurnRequest, VecSink};
use llm_credentials::{ResolvedSecret, SecretError, SecretRef, SecretResolver};
use llm_http::{HttpClient, Limits};
use llm_routing::Catalog;
use std::sync::Arc;

/// The local target is anonymous, so nothing is ever resolved.
struct NoSecrets;
impl SecretResolver for NoSecrets {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async { Err(SecretError::Missing) })
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let catalog = Catalog::parse(&std::fs::read_to_string("examples/catalog.toml")?)?;
    let binding = catalog
        .binding(&Id::new("local-small")?)
        .ok_or("no such serving model")?
        .clone();
    let client = ChatClient::new(
        binding,
        HttpClient::new(Limits::default())?,
        Arc::new(NoSecrets),
    );

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
