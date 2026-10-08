//! Port construction conformance observations.
//!
//! This module parses the authored catalog with the real `Catalog`, hands `b10x-llm-models` a
//! resolver lookup that answers a counting resolver for each listed account and nothing for any
//! other, and reports the port the crate built, or its refusal, and how often any resolver was
//! called. It reads no suite and branches on no scenario name.

use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use ess_conformance::target::TargetError;
use llm_core::{BoxFuture, Error, Id, Model};
use llm_credentials::{ResolvedSecret, SecretError, SecretRef, SecretResolver};
use llm_http::{HttpClient, Limits};
use llm_models::{CatalogModels, port};
use llm_routing::{Catalog, Models};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::target::Observed;

/// Every view name this domain answers through `query_view`.
pub const VIEWS: &[&str] = &[
    "llm.models.LastConstruction",
    "llm.models.LastCatalogConstruction",
];

/// Observe one command of this domain, or `None` when the command belongs to another.
pub fn observe(command: &str, input: &Value) -> Option<Result<Observed, TargetError>> {
    match command {
        "llm.models.Build" => Some(build(input)),
        "llm.models.BuildCatalog" => Some(build_catalog(input)),
        _ => None,
    }
}

fn unavailable(error: impl std::fmt::Display) -> TargetError {
    TargetError::unavailable("models observation", error.to_string())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BuildInput {
    catalog_toml: String,
    serving_model_id: String,
    resolver_accounts: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogInput {
    catalog_toml: String,
    resolver_accounts: Vec<String>,
    probe_ids: Vec<String>,
}

/// Answers nothing useful and counts every call; construction must never call it.
#[derive(Default)]
struct Counting(AtomicUsize);

impl SecretResolver for Counting {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Err(SecretError::Unavailable) })
    }
}

/// The caller's resolvers by account id.
type Lookup = BTreeMap<Id, Arc<dyn SecretResolver>>;

fn lookup(accounts: &[String]) -> Result<(Lookup, Arc<Counting>), TargetError> {
    let counting = Arc::new(Counting::default());
    let mut map = Lookup::new();
    for account in accounts {
        map.insert(
            Id::new(account.as_str()).map_err(unavailable)?,
            counting.clone(),
        );
    }
    Ok((map, counting))
}

fn http() -> Result<HttpClient, TargetError> {
    HttpClient::new(Limits::default()).map_err(unavailable)
}

fn protocol_label(model: &dyn Model) -> Value {
    json!(model.provenance().protocol)
}

fn refuse(facts: &mut Value, error: &Error) {
    facts["error_code"] = json!(error.code);
    facts["error_message"] = json!(error.message);
}

fn build(input: &Value) -> Result<Observed, TargetError> {
    let input: BuildInput = serde_json::from_value(input.clone()).map_err(unavailable)?;
    let (resolvers, counting) = lookup(&input.resolver_accounts)?;
    let mut facts = json!({
        "built": false, "error_code": null, "error_message": null, "port": null,
        "resolver_attached": null, "resolver_calls": 0
    });
    let http = http()?;
    let result = Catalog::parse(&input.catalog_toml).and_then(|catalog| {
        let id = Id::new(input.serving_model_id.as_str())
            .map_err(|_| Error::invalid("invalid serving model id"))?;
        port(&catalog, &id, http, &resolvers)
    });
    match result {
        Ok(built) => {
            facts["built"] = json!(true);
            facts["port"] = json!(built.protocol());
            facts["resolver_attached"] = json!(built.resolver().is_some());
        }
        Err(error) => refuse(&mut facts, &error),
    }
    facts["resolver_calls"] = json!(counting.0.load(Ordering::SeqCst));
    Ok(Observed {
        facts,
        view: "llm.models.LastConstruction",
        event: "llm.models.Built",
        field: "built",
    })
}

fn build_catalog(input: &Value) -> Result<Observed, TargetError> {
    let input: CatalogInput = serde_json::from_value(input.clone()).map_err(unavailable)?;
    let (resolvers, counting) = lookup(&input.resolver_accounts)?;
    let mut facts = json!({
        "built": false, "error_code": null, "error_message": null, "answers": [],
        "resolver_calls": 0
    });
    let http = http()?;
    match Catalog::parse(&input.catalog_toml)
        .and_then(|catalog| CatalogModels::build(&catalog, &http, &resolvers))
    {
        Ok(models) => {
            facts["built"] = json!(true);
            let mut answers = Vec::new();
            for probe in &input.probe_ids {
                let id = Id::new(probe.as_str()).map_err(unavailable)?;
                let answer = Models::model(&models, &id).map_or_else(
                    || "none".to_owned(),
                    |model| protocol_label(model).as_str().unwrap_or("?").to_owned(),
                );
                answers.push(format!("{probe}:{answer}"));
            }
            facts["answers"] = json!(answers);
        }
        Err(error) => refuse(&mut facts, &error),
    }
    facts["resolver_calls"] = json!(counting.0.load(Ordering::SeqCst));
    Ok(Observed {
        facts,
        view: "llm.models.LastCatalogConstruction",
        event: "llm.models.CatalogBuilt",
        field: "built",
    })
}
