//! The TOML form of `llm.routes/1`, route aliases and prices, and Bedrock endpoints.

#[cfg(feature = "environment")]
use llm_core::Model;
use llm_core::{ErrorCode, Id, Protocol, TurnObservation, Usage};
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

/// The YAML cases in `routes.rs` hold the same for a route after the first and for a block.
#[test]
fn a_toml_file_is_refused_at_the_line_of_an_unknown_key_of_a_later_route() {
    let document = TOML.replace("aliases = [\"claude-opus\"]", "region = \"fixture-value\"");
    let line = 1 + document
        .lines()
        .position(|line| line.starts_with("region = "))
        .unwrap();
    let first = 1 + document
        .lines()
        .position(|line| line == "[routes.haiku]")
        .unwrap();
    assert!(line > first + 4, "{document}");
    let error = Routes::parse_toml(&document).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert!(
        error.message.starts_with(&format!(
            "invalid llm.routes/1 TOML document at line {line} column 1: unknown field `region`"
        )),
        "the key is on line {line}: {}",
        error.message
    );
    assert!(
        !error.message.contains("fixture-value") && !error.message.contains("NEVER_SET"),
        "{}",
        error.message
    );
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

/// A relative container token file and an aws-keys field on a keychain without a store each
/// refuse only their own endpoint's route, `unauthorized` and by the endpoint's name.
#[cfg(all(feature = "environment", feature = "file"))]
#[test]
fn a_bedrock_source_that_cannot_be_built_refuses_only_its_own_route() {
    let routes = Routes::parse_toml(&TOML.replace(
        "secret_access_key = \"env:LLM_MODELS_TEST_NEVER_SET_KEY\"",
        "secret_access_key = \"keychain:llm-routes-service/llm-routes-entry\"",
    ))
    .unwrap();
    let sources = SecretSources::new()
        .with_container_credentials("http://127.0.0.1:9/v1/credentials", "relative/token");
    let resolvers = routes.resolvers(&sources).unwrap();
    let accounts: Vec<_> = resolvers.accounts().map(ToString::to_string).collect();
    assert_eq!(accounts, ["anthropic"]);
    routes.port("haiku", http(), &resolvers).unwrap();
    for (name, endpoint) in [("opus", "`bedrock-eu`"), ("sonnet", "`bedrock-keys`")] {
        let error = routes.port(name, http(), &resolvers).err().unwrap();
        assert_eq!(error.code, ErrorCode::Unauthorized, "{name}");
        assert!(
            error.message.contains(endpoint),
            "{name}: {}",
            error.message
        );
        assert!(
            !error.message.contains("relative/token"),
            "{}",
            error.message
        );
        assert!(
            !error.message.contains("llm-routes-service"),
            "{}",
            error.message
        );
    }
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

const PROCESS_YAML: &str = "format: llm.routes/1
endpoints:
  bedrock-dev:
    protocol: bedrock-converse
    bedrock: {region: eu-central-1}
    auth: {kind: aws-process, command: [aws, configure, export-credentials, --profile, dev]}
routes:
  opus: bedrock-dev/eu.anthropic.claude-opus-5-5
";

const PROCESS_TOML: &str = r#"format = "llm.routes/1"

[endpoints.bedrock-dev]
protocol = "bedrock-converse"
bedrock = { region = "eu-central-1" }
auth = { kind = "aws-process", command = ["aws", "configure", "export-credentials", "--profile", "dev"] }

[routes]
opus = "bedrock-dev/eu.anthropic.claude-opus-5-5"
"#;

const EXPORT: [&str; 5] = ["aws", "configure", "export-credentials", "--profile", "dev"];

#[test]
fn an_aws_process_endpoint_loads_from_yaml_and_toml_with_its_command() {
    let yaml = Routes::parse(PROCESS_YAML).unwrap();
    let toml = Routes::parse_toml(PROCESS_TOML).unwrap();
    assert_eq!(yaml.digest(), toml.digest());
    for routes in [&yaml, &toml] {
        let endpoint = routes.bedrock_endpoints().next().unwrap();
        assert_eq!(endpoint.credential_kind(), AwsCredentialKind::Process);
        assert_eq!(endpoint.credential_kind().as_str(), "aws-process");
        assert_eq!(endpoint.command().unwrap(), EXPORT);
        assert!(endpoint.secret_kinds().is_empty());
        assert_eq!(
            routes.target("opus").unwrap().protocol,
            Protocol::BedrockConverse
        );
        assert_eq!(routes.catalog().bindings().count(), 0);
        assert_eq!(
            routes
                .resolvers(&SecretSources::new())
                .unwrap()
                .accounts()
                .count(),
            0
        );
    }
    // The command is part of the document's identity.
    let other = Routes::parse_toml(&PROCESS_TOML.replace("\"dev\"", "\"ops\"")).unwrap();
    assert_ne!(other.digest(), toml.digest());
    let container = Routes::parse_toml(&PROCESS_TOML.replace(
        r#"{ kind = "aws-process", command = ["aws", "configure", "export-credentials", "--profile", "dev"] }"#,
        r#"{ kind = "aws-container" }"#,
    ))
    .unwrap();
    assert_ne!(container.digest(), toml.digest());
    assert_eq!(
        container.bedrock_endpoints().next().unwrap().command(),
        None
    );
}

/// Each refusal names the endpoint and the field by its path, and echoes no value.
#[test]
fn an_unusable_aws_process_command_or_an_extra_auth_field_is_refused_by_its_path() {
    let auth = r#"{ kind = "aws-process", command = ["aws", "configure", "export-credentials", "--profile", "dev"] }"#;
    let cases = [
        (
            r#"{ kind = "aws-process", command = [] }"#,
            "`auth.command` is empty",
        ),
        (r#"{ kind = "aws-process" }"#, "requires `auth.command`"),
        (
            r#"{ kind = "aws-process", command = "aws-fixture-value" }"#,
            "`auth.command` is not a list of strings",
        ),
        (
            r#"{ kind = "aws-process", command = ["", "fixture-value"] }"#,
            "`auth.command[0]`, the program, is empty",
        ),
        (
            r#"{ kind = "aws-process", command = ["aws", 7] }"#,
            "`auth.command[1]` is not a string",
        ),
        (
            r#"{ kind = "aws-process", command = ["aws", "fixture-value", ["x"]] }"#,
            "`auth.command[2]` is not a string",
        ),
        (
            r#"{ kind = "aws-process", command = ["aws", { a = "fixture-value" }] }"#,
            "`auth.command[1]` is not a string",
        ),
        (
            r#"{ kind = "aws-process", command = ["aws"], access_key_id = "env:FIXTURE_VALUE" }"#,
            "`auth.access_key_id`",
        ),
        (
            r#"{ kind = "aws-process", command = ["aws"], secret_access_key = "env:FIXTURE_VALUE" }"#,
            "`auth.secret_access_key`",
        ),
        (
            r#"{ kind = "aws-process", command = ["aws"], session_token = "env:FIXTURE_VALUE" }"#,
            "`auth.session_token`",
        ),
        (
            r#"{ kind = "aws-process", command = ["aws"], secret = "env:FIXTURE_VALUE" }"#,
            "`auth.secret`",
        ),
        (
            r#"{ kind = "aws-process", command = ["aws"], header = "fixture-value" }"#,
            "`auth.header`",
        ),
        (
            r#"{ kind = "aws-container", command = ["aws"] }"#,
            "only an aws-process endpoint takes `auth.command`",
        ),
        (
            r#"{ kind = "aws-keys", access_key_id = "env:A", secret_access_key = "env:B", command = ["aws"] }"#,
            "only an aws-process endpoint takes `auth.command`",
        ),
    ];
    for (replacement, expected) in cases {
        let error = Routes::parse_toml(&PROCESS_TOML.replace(auth, replacement)).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{replacement}");
        assert!(
            error.message.starts_with("endpoint `bedrock-dev`: "),
            "{replacement}: {}",
            error.message
        );
        assert!(
            error.message.contains(expected),
            "{replacement}: {}",
            error.message
        );
        assert!(
            !error.message.contains("fixture-value") && !error.message.contains("FIXTURE"),
            "{}",
            error.message
        );
    }
}

/// The same refusals in YAML, and an unknown `auth` key in both forms, echo no value.
#[test]
fn an_unusable_aws_process_command_in_yaml_or_an_unknown_auth_key_is_refused() {
    let yaml_cases = [
        (
            "command: [aws, configure, export-credentials, --profile, dev]",
            "command: []",
            "`auth.command` is empty",
        ),
        (
            "command: [aws, configure, export-credentials, --profile, dev]",
            "command: [aws, null]",
            "`auth.command[1]` is not a string",
        ),
        (
            "command: [aws, configure, export-credentials, --profile, dev]",
            "command: ['', dev]",
            "`auth.command[0]`, the program, is empty",
        ),
        (
            "command: [aws, configure, export-credentials, --profile, dev]",
            "command: [aws], header: x-fixture",
            "`auth.header`",
        ),
    ];
    for (from, to, expected) in yaml_cases {
        let error = Routes::parse(&PROCESS_YAML.replace(from, to)).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{to}");
        assert!(error.message.contains(expected), "{to}: {}", error.message);
    }
    // An unknown key in `auth` is refused by the parser, by its name in TOML.
    let error = Routes::parse_toml(
        &PROCESS_TOML.replace("\"dev\"] }", "\"dev\"], profile = \"fixture-value\" }"),
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert!(
        error.message.contains("unknown field `profile`"),
        "{}",
        error.message
    );
    assert!(
        !error.message.contains("fixture-value"),
        "{}",
        error.message
    );
    let error =
        Routes::parse(&PROCESS_YAML.replace("dev]}", "dev], profile: fixture-value}")).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert!(
        !error.message.contains("fixture-value"),
        "{}",
        error.message
    );
}

#[test]
fn only_a_bedrock_endpoint_takes_aws_process_or_a_command() {
    let http_endpoint = "format = \"llm.routes/1\"

[endpoints.open]
base_url = \"https://open.example.invalid/v1\"
protocol = \"messages\"
auth = AUTH

[routes]
haiku = \"open/claude-haiku-5-5\"
";
    for (auth, expected) in [
        (
            r#"{ kind = "aws-process", command = ["aws"] }"#,
            "takes `auth.kind` aws-keys, aws-container or aws-process",
        ),
        (
            r#"{ kind = "anonymous", command = ["aws"] }"#,
            "only an aws-process endpoint takes `auth.command`",
        ),
    ] {
        let error = Routes::parse_toml(&http_endpoint.replace("AUTH", auth)).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{auth}");
        assert!(
            error.message.contains(expected),
            "{auth}: {}",
            error.message
        );
    }
}

/// A build without feature `process` refuses the route even when the caller allows it, and the
/// command (a program that does not exist) is never looked for.
#[cfg(not(feature = "process"))]
#[test]
fn without_feature_process_an_aws_process_route_is_refused_and_nothing_runs() {
    let directory = tempfile::tempdir().unwrap();
    let program = directory.path().join("no-such-program");
    let document = PROCESS_TOML.replace(
        r#"["aws", "configure", "export-credentials", "--profile", "dev"]"#,
        &serde_json::json!([program.display().to_string()]).to_string(),
    );
    let routes = Routes::parse_toml(&document).unwrap();
    let resolvers = routes
        .resolvers(&SecretSources::new())
        .unwrap()
        .allow_credential_process();
    let error = routes.port("opus", http(), &resolvers).err().unwrap();
    assert_eq!(error.code, ErrorCode::Unauthorized, "{error:?}");
    assert!(
        error.message.starts_with("endpoint `bedrock-dev`: "),
        "{}",
        error.message
    );
    assert!(
        error.message.contains("feature `process`"),
        "{}",
        error.message
    );
}
