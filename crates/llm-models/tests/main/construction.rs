//! Port construction from a catalog: protocol picks the client, account picks the resolver.
//! No test here sends a request or resolves a secret.

use llm_core::{BoxFuture, ErrorCode, Id, Model, Protocol};
use llm_credentials::{ResolvedSecret, SecretError, SecretRef, SecretResolver};
use llm_http::{HttpClient, Limits};
use llm_models::{CatalogModels, port};
use llm_routing::{Catalog, Models};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

const CATALOG: &str = r#"
format = "llm.catalog/1"

[[providers]]
id = "my-lab"
category = "self-hosted"

[[accounts]]
id = "local"
provider_id = "my-lab"
auth_kind = "anonymous"
billing_kind = "self-hosted"

[[accounts]]
id = "remote"
provider_id = "my-lab"
auth_kind = "bearer"
billing_kind = "metered"
secret_reference_id = "lab-llm-token"

[[accounts]]
id = "keyed"
provider_id = "my-lab"
auth_kind = "api-key"
billing_kind = "metered"
secret_reference_id = "lab-llm-key"
api_key_header = "x-api-key"

[[endpoints]]
id = "local-vllm"
account_id = "local"
base_url = "http://127.0.0.1:8000/v1"

[[endpoints]]
id = "remote-models"
account_id = "remote"
base_url = "https://models.example.invalid/v1"

[[endpoints]]
id = "keyed-models"
account_id = "keyed"
base_url = "https://keyed.example.invalid/v1"

[[models]]
id = "small"
upstream_name = "example/Small-Model"

[[models]]
id = "large"
upstream_name = "example/Large-Model"

[[serving_models]]
id = "local-small"
endpoint_id = "local-vllm"
model_id = "small"
protocol = "chat-completions"
[serving_models.capabilities]
tools = false
tool_choice = false
temperature = false
top_p = false
reasoning_efforts = []
context_window = 4096
max_output_tokens = 1024

[[serving_models]]
id = "remote-large"
endpoint_id = "remote-models"
model_id = "large"
protocol = "responses"
[serving_models.capabilities]
tools = true
tool_choice = true
temperature = true
top_p = true
reasoning_efforts = ["medium", "high"]
context_window = 32768
max_output_tokens = 8192

[[serving_models]]
id = "keyed-large"
endpoint_id = "keyed-models"
model_id = "large"
protocol = "messages"
[serving_models.capabilities]
tools = true
tool_choice = true
temperature = true
top_p = false
reasoning_efforts = []
context_window = 32768
max_output_tokens = 8192

[[routes]]
id = "coding"
alias = "code"
fallback_enabled = false

[[targets]]
id = "coding-primary"
route_id = "coding"
serving_model_id = "local-small"
position = 0
"#;

fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}

fn http() -> HttpClient {
    HttpClient::new(Limits::default()).unwrap()
}

/// Counts every call; a construction that resolves anything is wrong.
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

fn resolvers(accounts: &[&str]) -> (BTreeMap<Id, Arc<dyn SecretResolver>>, Arc<Counting>) {
    let counting = Arc::new(Counting::default());
    let map = accounts
        .iter()
        .map(|account| (id(account), counting.clone() as Arc<dyn SecretResolver>))
        .collect();
    (map, counting)
}

#[test]
fn the_bindings_protocol_selects_the_client() {
    let catalog = Catalog::parse(CATALOG).unwrap();
    let (lookup, counting) = resolvers(&["remote", "keyed"]);
    for (serving, protocol) in [
        ("local-small", Protocol::ChatCompletions),
        ("remote-large", Protocol::Responses),
        ("keyed-large", Protocol::Messages),
    ] {
        let built = port(&catalog, &id(serving), http(), &lookup).unwrap();
        assert_eq!(built.protocol(), protocol, "{serving}");
        assert_eq!(built.provenance().protocol, protocol, "{serving}");
        assert_eq!(
            built.provenance(),
            catalog.binding(&id(serving)).unwrap().provenance(),
            "{serving}"
        );
    }
    assert_eq!(counting.0.load(Ordering::SeqCst), 0);
}

#[test]
fn a_credentialed_account_carries_the_callers_resolver_for_it() {
    let catalog = Catalog::parse(CATALOG).unwrap();
    let (lookup, _) = resolvers(&["remote", "keyed"]);
    let built = port(&catalog, &id("remote-large"), http(), &lookup).unwrap();
    assert!(Arc::ptr_eq(
        built.resolver().unwrap(),
        &lookup[&id("remote")]
    ));
}

#[test]
fn an_anonymous_account_builds_without_any_resolver() {
    let catalog = Catalog::parse(CATALOG).unwrap();
    let none: BTreeMap<Id, Arc<dyn SecretResolver>> = BTreeMap::new();
    let built = port(&catalog, &id("local-small"), http(), &none).unwrap();
    assert_eq!(built.protocol(), Protocol::ChatCompletions);
    assert!(built.resolver().is_none());
    // A resolver offered for an anonymous account is not attached either.
    let (offered, counting) = resolvers(&["local"]);
    let built = port(&catalog, &id("local-small"), http(), &offered).unwrap();
    assert!(built.resolver().is_none());
    assert_eq!(counting.0.load(Ordering::SeqCst), 0);
}

#[test]
fn a_credentialed_account_without_a_resolver_is_refused_unauthorized() {
    let catalog = Catalog::parse(CATALOG).unwrap();
    let (lookup, counting) = resolvers(&["keyed"]);
    let error = port(&catalog, &id("remote-large"), http(), &lookup)
        .err()
        .unwrap();
    assert_eq!(error.code, ErrorCode::Unauthorized);
    assert_eq!(
        error.message,
        "no credential resolver is supplied for account `remote`"
    );
    assert!(!error.may_retry());
    assert_eq!(counting.0.load(Ordering::SeqCst), 0);
}

#[test]
fn an_unknown_serving_model_id_is_refused() {
    let catalog = Catalog::parse(CATALOG).unwrap();
    let (lookup, _) = resolvers(&["remote", "keyed"]);
    let error = port(&catalog, &id("absent-model"), http(), &lookup)
        .err()
        .unwrap();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert_eq!(error.message, "no such serving model in the catalog");
}

#[test]
fn a_closure_is_a_resolver_lookup() {
    let catalog = Catalog::parse(CATALOG).unwrap();
    let counting: Arc<dyn SecretResolver> = Arc::new(Counting::default());
    let lookup = |account: &Id| (account.as_str() == "keyed").then(|| counting.clone());
    let built = port(&catalog, &id("keyed-large"), http(), &lookup).unwrap();
    assert_eq!(built.protocol(), Protocol::Messages);
}

#[test]
fn the_catalog_builder_answers_every_declared_serving_model_and_no_other() {
    let catalog = Catalog::parse(CATALOG).unwrap();
    let (lookup, counting) = resolvers(&["remote", "keyed"]);
    let models = CatalogModels::build(&catalog, &http(), &lookup).unwrap();
    for (serving, protocol) in [
        ("local-small", Protocol::ChatCompletions),
        ("remote-large", Protocol::Responses),
        ("keyed-large", Protocol::Messages),
    ] {
        let model = Models::model(&models, &id(serving)).unwrap();
        assert_eq!(model.provenance().protocol, protocol, "{serving}");
        assert_eq!(models.get(&id(serving)).unwrap().protocol(), protocol);
    }
    assert!(Models::model(&models, &id("absent-model")).is_none());
    assert_eq!(models.len(), 3);
    assert_eq!(counting.0.load(Ordering::SeqCst), 0);
}

#[test]
fn the_catalog_builder_refuses_a_credentialed_account_without_a_resolver() {
    let catalog = Catalog::parse(CATALOG).unwrap();
    let (lookup, _) = resolvers(&["remote"]);
    let error = CatalogModels::build(&catalog, &http(), &lookup)
        .err()
        .unwrap();
    assert_eq!(error.code, ErrorCode::Unauthorized);
    assert_eq!(
        error.message,
        "no credential resolver is supplied for account `keyed`"
    );
}
