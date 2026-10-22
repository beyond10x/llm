//! The TOML form of `llm.routes/1`, route aliases and prices, and Bedrock endpoints.

use llm_core::{ErrorCode, Id, Model, Protocol, TurnObservation, Usage};
use llm_credentials::SecretResolver;
use llm_http::{HttpClient, Limits};
use llm_models::routes::{AwsCredentialKind, Routes, SecretSources};
use std::{collections::BTreeMap, sync::Arc};

const YAML: &str = "format: llm.routes/1
endpoints:
  anthropic: {base_url: https://api.anthropic.com/v1, protocol: messages,
              auth: {kind: api-key, header: x-api-key, secret: env:LLM_MODELS_TEST_NEVER_SET}}
  bedrock-eu: {protocol: bedrock-converse, bedrock: {region: eu-central-1},
               auth: {kind: aws-container}}
  bedrock-keys: {protocol: messages, bedrock: {region: eu-central-1},
                 auth: {kind: aws-keys, access_key_id: env:LLM_MODELS_TEST_NEVER_SET_ID,
                        secret_access_key: env:LLM_MODELS_TEST_NEVER_SET_KEY}}
routes:
  haiku:
    model: anthropic/claude-haiku-5-5
    aliases: [fast]
    price: {currency: USD, input: '1', cached_input: '0.1', cache_creation_input: '1.25',
            output: '5'}
  opus: {model: bedrock-eu/eu.anthropic.claude-opus-5-5, aliases: [claude-opus]}
  sonnet: bedrock-keys/eu.anthropic.claude-sonnet-5-5
";

const TOML: &str = r#"format = "llm.routes/1"

[endpoints.anthropic]
base_url = "https://api.anthropic.com/v1"
protocol = "messages"
auth = { kind = "api-key", header = "x-api-key", secret = "env:LLM_MODELS_TEST_NEVER_SET" }

[endpoints.bedrock-eu]
protocol = "bedrock-converse"
bedrock = { region = "eu-central-1" }
auth = { kind = "aws-container" }

[endpoints.bedrock-keys]
protocol = "messages"
bedrock = { region = "eu-central-1" }
auth = { kind = "aws-keys", access_key_id = "env:LLM_MODELS_TEST_NEVER_SET_ID", secret_access_key = "env:LLM_MODELS_TEST_NEVER_SET_KEY" }

[routes.haiku]
model = "anthropic/claude-haiku-5-5"
aliases = ["fast"]
price = { currency = "USD", input = "1", cached_input = "0.1", cache_creation_input = "1.25", output = "5" }

[routes.opus]
model = "bedrock-eu/eu.anthropic.claude-opus-5-5"
aliases = ["claude-opus"]

[routes]
sonnet = "bedrock-keys/eu.anthropic.claude-sonnet-5-5"
"#;

fn http() -> HttpClient {
    HttpClient::new(Limits::default()).unwrap()
}

#[test]
fn a_toml_file_and_the_yaml_document_with_the_same_content_give_the_same_digest() {
    let yaml = Routes::parse(YAML).unwrap();
    let toml = Routes::parse_toml(TOML).unwrap();
    assert_eq!(yaml.digest(), toml.digest());
    assert_eq!(yaml.digest().len(), 64);
    let other = Routes::parse_toml(&TOML.replace("eu-central-1", "eu-west-1")).unwrap();
    assert_ne!(other.digest(), toml.digest());
    let names = |routes: &Routes| {
        routes
            .routes()
            .map(|route| route.name().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&yaml), names(&toml));
}

#[test]
fn a_toml_file_is_refused_at_the_line_of_an_unknown_key_without_echoing_its_value() {
    let error =
        Routes::parse_toml(&TOML.replace("aliases = [\"fast\"]", "alias = \"fixture-value\""))
            .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert!(
        error.message.contains("TOML document at line 20 column 1"),
        "{}",
        error.message
    );
    assert!(
        error.message.contains("unknown field `alias`"),
        "{}",
        error.message
    );
    assert!(
        !error.message.contains("fixture-value"),
        "{}",
        error.message
    );
    let error = Routes::parse_toml("format = ").unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
}

#[test]
fn an_alias_reaches_its_route_and_a_bedrock_route_its_wire() {
    let routes = Routes::parse_toml(TOML).unwrap();
    assert_eq!(routes.route("fast").unwrap().name().as_str(), "haiku");
    assert_eq!(routes.route("claude-opus").unwrap().name().as_str(), "opus");
    let opus = routes.target("claude-opus").unwrap();
    assert_eq!(opus.protocol, Protocol::BedrockConverse);
    assert_eq!(opus.upstream_model.as_str(), "eu.anthropic.claude-opus-5-5");
    let sonnet = routes.target("sonnet").unwrap();
    assert_eq!(sonnet.protocol, Protocol::Messages);
    let kinds: Vec<_> = routes
        .bedrock_endpoints()
        .map(|endpoint| (endpoint.name().to_string(), endpoint.credential_kind()))
        .collect();
    assert_eq!(
        kinds,
        [
            ("bedrock-eu".to_owned(), AwsCredentialKind::Container),
            ("bedrock-keys".to_owned(), AwsCredentialKind::Keys),
        ]
    );
    let catalog: Vec<_> = routes
        .catalog()
        .bindings()
        .map(|(id, _)| id.to_string())
        .collect();
    assert_eq!(catalog, ["haiku"]);
    let error = routes.binding("opus").unwrap_err();
    assert_eq!(error.code, ErrorCode::Unsupported);
}

#[test]
fn a_bedrock_port_is_built_for_its_route_and_refused_without_its_credentials() {
    let routes = Routes::parse_toml(TOML).unwrap();
    let none: BTreeMap<Id, Arc<dyn SecretResolver>> = BTreeMap::new();
    for name in ["opus", "sonnet"] {
        let error = routes.port(name, http(), &none).err().unwrap();
        assert_eq!(error.code, ErrorCode::Unauthorized, "{name}");
    }
}

#[cfg(feature = "environment")]
#[test]
fn without_a_container_token_file_only_the_container_route_is_refused() {
    let routes = Routes::parse_toml(TOML).unwrap();
    let resolvers = routes.resolvers(&SecretSources::new()).unwrap();
    let accounts: Vec<_> = resolvers.accounts().map(ToString::to_string).collect();
    assert_eq!(accounts, ["anthropic", "bedrock-keys"]);
    let port = routes.port("sonnet", http(), &resolvers).unwrap();
    assert_eq!(port.protocol(), Protocol::Messages);
    assert_eq!(port.provenance().endpoint.as_str(), "bedrock-keys");
    let port = routes.port("haiku", http(), &resolvers).unwrap();
    assert_eq!(port.protocol(), Protocol::Messages);
    let error = routes.port("opus", http(), &resolvers).err().unwrap();
    assert_eq!(error.code, ErrorCode::Unauthorized);
}

fn observed(routes: &Routes, usage: Usage, final_usage: bool) -> TurnObservation {
    let binding = routes.binding("haiku").unwrap();
    TurnObservation {
        usage: Some(usage),
        final_usage,
        ..TurnObservation::new(binding.provenance().clone())
    }
}

#[test]
fn a_priced_route_costs_a_turn_whose_usage_is_known_and_leaves_any_other_unknown() {
    let routes = Routes::parse_toml(TOML).unwrap();
    let complete = Usage {
        input_tokens: Some(1_000),
        output_tokens: Some(200),
        cached_input_tokens: Some(0),
        cache_creation_input_tokens: Some(0),
        reasoning_output_tokens: Some(0),
    };
    let cost = routes
        .cost("fast", &observed(&routes, complete.clone(), true))
        .unwrap()
        .unwrap();
    assert_eq!(cost.currency.as_str(), "USD");
    assert_eq!(cost.total.complete_total.unwrap().to_string(), "0.002");

    let partial = Usage {
        cached_input_tokens: None,
        ..complete.clone()
    };
    let cost = routes
        .cost("haiku", &observed(&routes, partial, true))
        .unwrap()
        .unwrap();
    assert_eq!(cost.total.complete_total, None);
    assert!(cost.total.unknown_lines > 0);

    let cost = routes
        .cost("haiku", &observed(&routes, complete.clone(), false))
        .unwrap()
        .unwrap();
    assert_eq!(cost.total.complete_total, None);

    assert!(
        routes
            .cost("sonnet", &observed(&routes, complete, true))
            .unwrap()
            .is_none()
    );
}
