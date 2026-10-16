//! The `llm.routes/1` document: strict parsing, its expansion into an `llm.catalog/1` catalog,
//! and the resolvers its secret references build. No test here sends a request.

use llm_core::{BillingKind, Error, ErrorCode, Id, Model, Protocol};
use llm_credentials::SecretResolver;
use llm_http::{HttpClient, Limits};
use llm_models::{
    CatalogModels, Resolvers,
    routes::{FORMAT, Routes, SecretKind, SecretSources, default_capabilities},
};
use std::{collections::BTreeMap, fmt::Write as _, path::PathBuf, sync::Arc};

/// No test environment sets this; every `env:` reference in the shared document names it.
const NEVER_SET: &str = "LLM_ROUTES_TEST_NEVER_SET";

const DOCUMENT: &str = "format: llm.routes/1
endpoints:
  anthropic: {base_url: https://api.anthropic.com/v1, protocol: messages,
              auth: {kind: api-key, header: x-api-key, secret: env:LLM_ROUTES_TEST_NEVER_SET}}
  openrouter: {base_url: https://openrouter.ai/api/v1, protocol: chat-completions,
               auth: {kind: bearer, secret: env:LLM_ROUTES_TEST_NEVER_SET}}
  gateway:
    base_url: https://gateway.example.invalid/v1
    protocol: responses
    auth: {kind: bearer, secret: file:~/.config/b10x/gateway-token}
  lab:
    base_url: http://127.0.0.1:8000/v1
    protocol: chat-completions
    auth: {kind: anonymous}
routes:
  sonnet: anthropic/claude-sonnet-4-5
  or-sonnet: openrouter/anthropic/claude-sonnet-4.5
  qwen:
    model: gateway/qwen
    capabilities: {tools: true, tool_choice: true, temperature: true, top_p: true,
                   reasoning_efforts: [], context_window: 32768, max_output_tokens: 8192}
  small: lab/small
";

/// One endpoint `p` and one route `r`; each refusal case replaces one placeholder.
const SINGLE: &str = "format: FORMAT
TOP
endpoints:
  p:
    base_url: https://p.example.invalid/v1
    protocol: PROTOCOL
    auth: AUTH
    ENDPOINT_EXTRA
routes:
  r: ROUTE
";

fn single(replacements: &[(&str, &str)]) -> String {
    let mut document = SINGLE.to_owned();
    for (placeholder, value) in [
        ("FORMAT", "llm.routes/1"),
        ("PROTOCOL", "messages"),
        (
            "AUTH",
            "{kind: bearer, secret: env:LLM_ROUTES_TEST_NEVER_SET}",
        ),
        ("ENDPOINT_EXTRA", ""),
        ("TOP", ""),
        ("ROUTE", "p/m"),
    ] {
        let value = replacements
            .iter()
            .find(|(name, _)| *name == placeholder)
            .map_or(value, |(_, replaced)| *replaced);
        document = document.replace(placeholder, value);
    }
    document
}

fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}

fn http() -> HttpClient {
    HttpClient::new(Limits::default()).unwrap()
}

/// An absolute home that need not exist: building resolvers reads nothing.
fn unread_home() -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("llm-routes-home-never-created")
}

fn refused(document: &str) -> Error {
    match Routes::parse(document) {
        Ok(_) => panic!("document was accepted"),
        Err(error) => error,
    }
}

/// The resolvers the document builds when every source it names is compiled in; otherwise a
/// stand-in keyed the same way, so the port rules are exercised in both builds.
fn resolvers_for(routes: &Routes) -> Box<dyn Resolvers> {
    #[cfg(all(feature = "environment", feature = "file"))]
    {
        Box::new(
            routes
                .resolvers(&SecretSources::new().with_home(unread_home()))
                .unwrap(),
        )
    }
    #[cfg(not(all(feature = "environment", feature = "file")))]
    {
        struct Refusing;
        impl SecretResolver for Refusing {
            fn resolve<'a>(
                &'a self,
                _reference: &'a llm_credentials::SecretRef,
            ) -> llm_core::BoxFuture<
                'a,
                Result<llm_credentials::ResolvedSecret, llm_credentials::SecretError>,
            > {
                Box::pin(async { Err(llm_credentials::SecretError::Unavailable) })
            }
        }
        let map: BTreeMap<Id, Arc<dyn SecretResolver>> = routes
            .endpoints()
            .filter(|endpoint| endpoint.secret_kind().is_some())
            .map(|endpoint| {
                (
                    endpoint.name().clone(),
                    Arc::new(Refusing) as Arc<dyn SecretResolver>,
                )
            })
            .collect();
        Box::new(map)
    }
}

#[test]
fn the_example_document_answers_each_route_and_direct_name() {
    assert_eq!(FORMAT, "llm.routes/1");
    let routes = Routes::parse(DOCUMENT).unwrap();
    for (name, endpoint, protocol, upstream) in [
        (
            "sonnet",
            "anthropic",
            Protocol::Messages,
            "claude-sonnet-4-5",
        ),
        (
            "or-sonnet",
            "openrouter",
            Protocol::ChatCompletions,
            "anthropic/claude-sonnet-4.5",
        ),
        ("qwen", "gateway", Protocol::Responses, "qwen"),
        ("small", "lab", Protocol::ChatCompletions, "small"),
        (
            "anthropic/claude-opus-4-1",
            "anthropic",
            Protocol::Messages,
            "claude-opus-4-1",
        ),
    ] {
        let target = routes.target(name).unwrap();
        assert_eq!(target.endpoint.as_str(), endpoint, "{name}");
        assert_eq!(target.protocol, protocol, "{name}");
        assert_eq!(target.upstream_model.as_str(), upstream, "{name}");
    }
    for name in ["nobody/model", "unknown", "anthropic/", "/model"] {
        assert_eq!(
            routes.target(name).unwrap_err().code,
            ErrorCode::InvalidRequest,
            "{name}"
        );
    }
    let names: Vec<&str> = routes.routes().map(|route| route.name().as_str()).collect();
    assert_eq!(names, ["or-sonnet", "qwen", "small", "sonnet"]);
    let serving: Vec<&str> = routes
        .catalog()
        .bindings()
        .map(|(id, _)| id.as_str())
        .collect();
    assert_eq!(serving, names);
    let qwen = routes.catalog().binding(&id("qwen")).unwrap();
    assert_eq!(qwen.capabilities().context_window, 32768);
    assert_eq!(qwen.capabilities().max_output_tokens, 8192);
    assert!(qwen.capabilities().reasoning_efforts.is_empty());
    let sonnet = routes.catalog().binding(&id("sonnet")).unwrap();
    assert_eq!(sonnet.capabilities(), &default_capabilities());
    assert_eq!(
        default_capabilities().reasoning_efforts,
        ["low", "medium", "high"]
    );
    assert_eq!(default_capabilities().context_window, 200_000);
    assert_eq!(default_capabilities().max_output_tokens, 16384);
    let endpoints: Vec<(&str, Option<&str>)> = routes
        .endpoints()
        .map(|endpoint| {
            (
                endpoint.name().as_str(),
                endpoint.secret_kind().map(SecretKind::as_str),
            )
        })
        .collect();
    assert_eq!(
        endpoints,
        [
            ("anthropic", Some("env")),
            ("gateway", Some("file")),
            ("lab", None),
            ("openrouter", Some("env")),
        ]
    );
}

#[test]
fn an_endpoint_declared_twice_is_refused() {
    let document = DOCUMENT.replace(
        "  lab:\n",
        "  anthropic: {base_url: https://second.example.invalid/v1, protocol: messages, auth: {kind: anonymous}}\n  lab:\n",
    );
    let error = refused(&document);
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert_eq!(
        error.message,
        "endpoint `anthropic`: the document declares it more than once"
    );
    let routes = "routes:\n  r: p/m\n  r: p/other\n";
    let error = refused(&single(&[]).replace("routes:\n  r: p/m\n", routes));
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert_eq!(
        error.message,
        "route `r`: the document declares it more than once"
    );
    // A repeated field of a declaration is refused by the parser, at its location.
    let error = refused(&single(&[("ENDPOINT_EXTRA", "protocol: responses")]));
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert!(
        error
            .message
            .starts_with("invalid llm.routes/1 YAML document at line "),
        "{}",
        error.message
    );
}

/// `providers:` was this document's name for its endpoints before the vocabulary settled on
/// `endpoints:`; it is now an unknown field like any other.
#[test]
fn the_retired_providers_field_is_refused_as_an_unknown_field() {
    let document = single(&[]).replace("\nendpoints:\n", "\nproviders:\n");
    assert!(document.contains("\nproviders:\n  p:\n"), "{document}");
    let error = refused(&document);
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert_eq!(
        error.message,
        "invalid llm.routes/1 YAML document at line 3 column 1"
    );
}

/// A refusal's kind comes from where it arose, never from searching the parser's message, which
/// quotes an unknown field's name: a field named after any refusal this module raises is an
/// unknown field, at every level, and nothing else.
#[test]
fn no_refusal_is_classified_by_the_text_of_the_field_it_names() {
    for marker in [
        "llm.routes expansion bound",
        "llm.routes duplicate mapping key",
        "llm.routes/1 document expands beyond its byte bound when its YAML aliases are resolved",
        "llm.routes/1 document expands beyond the catalog's bound",
        "routes document exceeds its byte bound",
        "the document declares it more than once",
        "a mapping key is repeated",
    ] {
        for document in [
            single(&[("TOP", &format!("\"{marker}\": 1"))]),
            single(&[("ENDPOINT_EXTRA", &format!("\"{marker}\": 1"))]),
            single(&[("AUTH", &format!("{{kind: anonymous, \"{marker}\": 1}}"))]),
            single(&[(
                "ROUTE",
                &format!("{{model: p/m, \"{marker}\": 1, capabilities: {{}}}}"),
            )]),
        ] {
            let error = refused(&document);
            assert_eq!(
                error.code,
                ErrorCode::InvalidRequest,
                "{marker}: {document}"
            );
            assert!(
                error
                    .message
                    .starts_with("invalid llm.routes/1 YAML document at line "),
                "{marker}: {}",
                error.message
            );
        }
    }
}

#[test]
fn strict_parsing_refuses_each_malformed_document_without_echoing_its_value() {
    for (case, document, value) in [
        (
            "unknown top-level field",
            single(&[("TOP", "extra: sk-unknown-top")]),
            "sk-unknown-top",
        ),
        (
            "unknown endpoint field",
            single(&[("ENDPOINT_EXTRA", "region: sk-unknown-endpoint")]),
            "sk-unknown-endpoint",
        ),
        (
            "unknown auth field",
            single(&[(
                "AUTH",
                "{kind: bearer, secret: env:LLM_ROUTES_TEST_NEVER_SET, token: sk-unknown-auth}",
            )]),
            "sk-unknown-auth",
        ),
        (
            "unknown format",
            single(&[("FORMAT", "llm.routes/2")]),
            "llm.routes/2",
        ),
        (
            "undeclared endpoint",
            single(&[("ROUTE", "nobody/sk-model")]),
            "nobody/sk-model",
        ),
        (
            "route without an endpoint",
            single(&[("ROUTE", "sk-bare-model")]),
            "sk-bare-model",
        ),
        ("unknown protocol", single(&[("PROTOCOL", "grpc")]), "grpc"),
        (
            "unknown auth kind",
            single(&[("AUTH", "{kind: password, secret: env:X}")]),
            "password",
        ),
        (
            "subscription login",
            single(&[("AUTH", "{kind: subscription-oauth, secret: env:X}")]),
            "subscription-oauth",
        ),
        (
            "literal secret",
            single(&[("AUTH", "{kind: bearer, secret: sk-literal}")]),
            "sk-literal",
        ),
        (
            "unknown secret source",
            single(&[("AUTH", "{kind: bearer, secret: \"vault:x\"}")]),
            "vault:x",
        ),
        (
            "api-key without header",
            single(&[("AUTH", "{kind: api-key, secret: env:SK_NO_HEADER}")]),
            "SK_NO_HEADER",
        ),
        (
            "anonymous with secret",
            single(&[("AUTH", "{kind: anonymous, secret: env:SK_ANONYMOUS}")]),
            "SK_ANONYMOUS",
        ),
        (
            "bearer without secret",
            single(&[("AUTH", "{kind: bearer}")]),
            "p.example.invalid",
        ),
        (
            "relative file path",
            single(&[("AUTH", "{kind: bearer, secret: file:relative/sk-path}")]),
            "relative/sk-path",
        ),
        (
            "unusable variable name",
            single(&[("AUTH", "{kind: bearer, secret: \"env:SK=X\"}")]),
            "SK=X",
        ),
        (
            "keychain without entry",
            single(&[("AUTH", "{kind: bearer, secret: \"keychain:sk-service\"}")]),
            "sk-service",
        ),
        (
            "invalid base url",
            single(&[]).replace("https://p.example.invalid/v1", "ftp://sk-host.invalid/"),
            "sk-host",
        ),
    ] {
        let error = refused(&document);
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{case}");
        assert!(
            !error.message.contains(value),
            "{case}: {} echoes `{value}`",
            error.message
        );
    }
}

#[test]
fn refusals_name_the_endpoint_or_route_they_concern() {
    let error = refused(&single(&[("PROTOCOL", "grpc")]));
    assert!(error.message.contains("`p`"), "{}", error.message);
    let error = refused(&single(&[("ROUTE", "nobody/model")]));
    assert!(error.message.contains("`r`"), "{}", error.message);
    let error = refused(&single(&[("AUTH", "{kind: bearer, secret: sk-literal}")]));
    assert!(error.message.contains("`p`"), "{}", error.message);
}

#[test]
fn an_oversized_document_is_refused_before_parsing() {
    let document = format!("{DOCUMENT}#{}\n", "x".repeat(llm_routing::MAX_CONFIG_BYTES));
    assert_eq!(refused(&document).code, ErrorCode::TooLarge);
}

const EXPANDED: &str =
    "llm.routes/1 document expands beyond its byte bound when its YAML aliases are resolved";

/// YAML aliases repeat what they anchor while the document is read. What the repetition may
/// cost is held to what the densest alias-free document of the same length costs, so a short
/// document cannot expand past it, whether it repeats long text or many small nodes; an alias
/// that stays within the bound is ordinary YAML and is read as written.
#[test]
fn a_document_whose_yaml_aliases_expand_past_the_byte_bound_is_refused() {
    let document = |copies: usize| {
        format!(
            "format: llm.routes/1
endpoints:
  p: {{base_url: https://p.example.invalid/v1, protocol: messages, auth: {{kind: anonymous}}}}
routes:
  r:
    model: p/m
    capabilities: {{tools: true, tool_choice: true, temperature: true, top_p: true,
                   context_window: 8192, max_output_tokens: 1024,
                   reasoning_efforts: [&e {}, {}]}}
",
            "a".repeat(64 * 1024),
            vec!["*e"; copies].join(", ")
        )
    };
    // 128 copies of 64 KiB is 8 MiB of text from a document of about 66 KB.
    let expanding = document(128);
    assert!(expanding.len() < llm_routing::MAX_CONFIG_BYTES / 16);
    let error = refused(&expanding);
    assert_eq!(
        (error.code, error.message.as_str()),
        (ErrorCode::TooLarge, EXPANDED)
    );
    let routes = Routes::parse(&document(2)).unwrap();
    let route = routes.routes().next().unwrap();
    assert_eq!(route.capabilities().reasoning_efforts.len(), 3);

    // Many small nodes cost memory too, each charged as a node: 100 aliases of a 200-node
    // sequence are 20000 nodes from a document of about 1 KB, within its budget at one byte a
    // node and far past it at the cost of a buffered node.
    let small = vec!["e"; 200].join(",");
    let aliases = vec!["*s"; 100].join(",");
    let nodes = format!(
        "format: llm.routes/1
endpoints:
  p: {{base_url: https://p.example.invalid/v1, protocol: messages, auth: {{kind: anonymous}}}}
routes:
  r:
    model: p/m
    capabilities: {{tools: true, tool_choice: true, temperature: true, top_p: true,
                   context_window: 8192, max_output_tokens: 1024,
                   reasoning_efforts: [&s [{small}], {aliases}]}}
"
    );
    assert!(nodes.len() < 2 * 1024, "{}", nodes.len());
    let error = refused(&nodes);
    assert_eq!(
        (error.code, error.message.as_str()),
        (ErrorCode::TooLarge, EXPANDED)
    );
}

/// The alias bound is relative to the document's length and never refuses a document without
/// aliases that is within its byte bound, however densely it packs its nodes: each shape below
/// is the densest found for its kind (two nodes per two bytes, one per byte, one and a half
/// bytes of text per byte), at the full byte bound. An ordinary document of 2000 routes loads.
#[test]
fn no_alias_free_document_within_the_byte_bound_is_refused_by_the_alias_bound() {
    let head = "format: llm.routes/1
endpoints:
  p: {base_url: https://p.example.invalid/v1, protocol: messages, auth: {kind: anonymous}}
routes:
  r:
    model: p/m
    capabilities: {tools: true, tool_choice: true, temperature: true, top_p: true,
                   context_window: 8192, max_output_tokens: 1024, reasoning_efforts: [a]}
";
    let fill = |open: &str, unit: &str, close: &str| {
        let room =
            (llm_routing::MAX_CONFIG_BYTES - head.len() - open.len() - close.len()) / unit.len();
        format!("{head}{open}{}{close}", unit.repeat(room))
    };
    for (shape, document) in [
        (
            "one-byte scalars in a flow sequence",
            fill("x: [", "a,", "a]\n"),
        ),
        (
            "keys with implicit null values in a flow mapping",
            fill("x: {", "a,", "a}\n"),
        ),
        (
            "explicit empty keys in a block mapping",
            fill("", "?\n", ""),
        ),
        ("escaped line separators", fill("x: \"", "\\L", "\"\n")),
    ] {
        assert!(
            document.len() <= llm_routing::MAX_CONFIG_BYTES
                && document.len() > llm_routing::MAX_CONFIG_BYTES - 8,
            "{shape}: {}",
            document.len()
        );
        if let Err(error) = Routes::parse(&document) {
            assert_ne!(error.message, EXPANDED, "{shape}");
        }
    }

    let mut ordinary = String::from(
        "format: llm.routes/1
endpoints:
  p: {base_url: https://p.example.invalid/v1, protocol: messages, auth: {kind: anonymous}}
routes:
",
    );
    for index in 0..2000 {
        writeln!(ordinary, "  route-{index:0>8}: p/model-{index:0>8}").unwrap();
    }
    let routes = Routes::parse(&ordinary).unwrap();
    assert_eq!(routes.routes().count(), 2000);
}

/// A credentialed endpoint bills as metered API use; an anonymous one as self-hosted.
#[test]
fn each_expanded_account_bills_by_how_its_endpoint_authenticates() {
    let routes = Routes::parse(DOCUMENT).unwrap();
    let billing: Vec<(&str, &str, BillingKind)> = routes
        .catalog()
        .bindings()
        .map(|(serving, binding)| {
            let account = &binding.declaration().account;
            (serving.as_str(), account.id.as_str(), account.billing_kind)
        })
        .collect();
    assert_eq!(
        billing,
        [
            ("or-sonnet", "openrouter", BillingKind::Metered),
            ("qwen", "gateway", BillingKind::Metered),
            ("small", "lab", BillingKind::SelfHosted),
            ("sonnet", "anthropic", BillingKind::Metered),
        ]
    );
    let direct = routes.binding("lab/other-model").unwrap();
    assert_eq!(
        direct.declaration().account.billing_kind,
        BillingKind::SelfHosted
    );
    let direct = routes.binding("anthropic/claude-opus-4-1").unwrap();
    assert_eq!(
        direct.declaration().account.billing_kind,
        BillingKind::Metered
    );
}

#[test]
fn a_relative_file_reference_is_refused() {
    for secret in [
        "file:relative/token",
        "file:./token",
        "file:~user/token",
        "file:/a/../token",
    ] {
        let document = single(&[("AUTH", &format!("{{kind: bearer, secret: \"{secret}\"}}"))]);
        let error = refused(&document);
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{secret}");
        assert!(error.message.contains("`p`"), "{secret}: {}", error.message);
    }
}

#[test]
fn building_resolvers_for_a_keychain_reference_without_an_injected_store_is_refused_naming_the_endpoint()
 {
    let document = single(&[(
        "AUTH",
        "{kind: bearer, secret: \"keychain:llm-routes-service/llm-routes-entry\"}",
    )]);
    let routes = Routes::parse(&document).unwrap();
    let error = routes
        .resolvers(&SecretSources::new().with_home(unread_home()))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert!(error.message.contains("`p`"), "{}", error.message);
    assert!(!error.message.contains("llm-routes-service"));
    assert!(!error.message.contains("llm-routes-entry"));
    #[cfg(feature = "keychain")]
    {
        assert!(
            error.message.contains("credential store"),
            "{}",
            error.message
        );
        let store: Arc<keyring_core::CredentialStore> = keyring_core::mock::Store::new().unwrap();
        let resolvers = routes
            .resolvers(&SecretSources::new().with_keychain(store))
            .unwrap();
        assert!(resolvers.resolver(&id("p")).is_some());
    }
}

#[cfg(not(feature = "environment"))]
#[test]
fn an_environment_reference_is_refused_by_name_when_its_feature_is_compiled_out() {
    let routes = Routes::parse(&single(&[])).unwrap();
    let error = routes
        .resolvers(&SecretSources::new().with_home(unread_home()))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert!(error.message.contains("`p`"), "{}", error.message);
    assert!(error.message.contains("environment"), "{}", error.message);
    assert!(!error.message.contains(NEVER_SET));
}

#[cfg(not(feature = "file"))]
#[test]
fn a_file_reference_is_refused_by_name_when_its_feature_is_compiled_out() {
    let document = single(&[("AUTH", "{kind: bearer, secret: \"file:~/token\"}")]);
    let routes = Routes::parse(&document).unwrap();
    let error = routes
        .resolvers(&SecretSources::new().with_home(unread_home()))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert!(error.message.contains("`p`"), "{}", error.message);
    assert!(error.message.contains("file"), "{}", error.message);
    assert!(!error.message.contains("token"));
}

#[cfg(not(feature = "keychain"))]
#[test]
fn a_keychain_reference_is_refused_by_name_when_its_feature_is_compiled_out() {
    let document = single(&[("AUTH", "{kind: bearer, secret: \"keychain:service/entry\"}")]);
    let routes = Routes::parse(&document).unwrap();
    let error = routes
        .resolvers(&SecretSources::new().with_home(unread_home()))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert!(error.message.contains("`p`"), "{}", error.message);
    assert!(error.message.contains("keychain"), "{}", error.message);
}

#[cfg(feature = "file")]
#[test]
fn a_home_file_reference_expands_against_the_callers_home_and_without_one_is_refused() {
    let document = single(&[(
        "AUTH",
        "{kind: bearer, secret: \"file:~/llm-routes-dir/llm-routes-token\"}",
    )]);
    let routes = Routes::parse(&document).unwrap();
    let error = routes.resolvers(&SecretSources::new()).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert!(error.message.contains("`p`"), "{}", error.message);
    assert!(!error.message.contains("llm-routes-dir"));
    let error = routes
        .resolvers(&SecretSources::new().with_home("relative-home"))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert!(error.message.contains("`p`"), "{}", error.message);

    #[cfg(target_os = "linux")]
    {
        use std::{fs, os::unix::fs::PermissionsExt};
        let fixture = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
        let home = fixture.path().canonicalize().unwrap();
        fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).unwrap();
        let directory = home.join("llm-routes-dir");
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        let token = directory.join("llm-routes-token");
        fs::write(&token, b"fixture-home-token\n").unwrap();
        fs::set_permissions(&token, fs::Permissions::from_mode(0o600)).unwrap();

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let binding = routes.catalog().binding(&id("r")).unwrap();
        let resolvers = routes
            .resolvers(&SecretSources::new().with_home(&home))
            .unwrap();
        let resolver = resolvers.resolver(&id("p")).unwrap();
        let prepared = runtime
            .block_on(binding.prepare_auth(resolver.as_ref(), &llm_core::Cancel::new()))
            .unwrap();
        let (headers, _) = prepared.into_parts();
        assert_eq!(headers["authorization"], "Bearer fixture-home-token");

        // Another home holds no such file: the reference is refused at turn time, by name.
        let other = home.join("llm-routes-dir");
        let resolvers = routes
            .resolvers(&SecretSources::new().with_home(&other))
            .unwrap();
        let resolver = resolvers.resolver(&id("p")).unwrap();
        let error = runtime
            .block_on(binding.prepare_auth(resolver.as_ref(), &llm_core::Cancel::new()))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Unauthorized);
        assert_eq!(
            error.message,
            "secret reference `p` refused: secret reference was not found"
        );
    }
}

#[test]
fn the_catalog_builder_answers_one_port_per_route_with_the_routes_protocol() {
    let routes = Routes::parse(DOCUMENT).unwrap();
    let resolvers = resolvers_for(&routes);
    let models = CatalogModels::build(routes.catalog(), &http(), &*resolvers).unwrap();
    assert_eq!(models.len(), routes.routes().count());
    for route in routes.routes() {
        let port = models.get(route.name()).unwrap();
        let target = routes.target(route.name().as_str()).unwrap();
        assert_eq!(port.protocol(), target.protocol, "{}", route.name());
        assert_eq!(port.provenance().provider, target.endpoint);
        assert_eq!(port.provenance().account, target.endpoint);
        assert_eq!(port.provenance().endpoint, target.endpoint);
        assert_eq!(
            port.resolver().is_some(),
            route.name().as_str() != "small",
            "{}",
            route.name()
        );
    }
}

/// No ambient fallback: the unset variable is refused by the endpoint's reference, before any
/// request, and the refusal names neither the variable nor a value.
#[cfg(feature = "environment")]
#[test]
fn an_unset_variable_is_refused_at_turn_time_by_the_endpoints_reference() {
    let routes = Routes::parse(&single(&[])).unwrap();
    let resolvers = routes.resolvers(&SecretSources::new()).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let binding = routes.binding("r").unwrap();
    let error = runtime
        .block_on(binding.prepare_auth(
            resolvers.resolver(&id("p")).unwrap().as_ref(),
            &llm_core::Cancel::new(),
        ))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unauthorized);
    assert_eq!(
        error.message,
        "secret reference `p` refused: secret reference was not found"
    );
    assert!(!error.message.contains(NEVER_SET));
}

#[test]
fn llm_models_port_for_a_direct_endpoint_model_name_builds_through_the_same_rules() {
    let routes = Routes::parse(DOCUMENT).unwrap();
    let resolvers = resolvers_for(&routes);
    let port = routes
        .port("anthropic/claude-opus-4-1", http(), &*resolvers)
        .unwrap();
    assert_eq!(port.protocol(), Protocol::Messages);
    assert_eq!(port.provenance().provider.as_str(), "anthropic");
    assert_eq!(port.provenance().account.as_str(), "anthropic");
    assert_eq!(
        port.provenance().model.as_str(),
        "anthropic/claude-opus-4-1"
    );
    assert!(port.resolver().is_some());
    assert_eq!(port.capabilities(), &default_capabilities());

    let route = routes.port("sonnet", http(), &*resolvers).unwrap();
    assert_eq!(route.protocol(), Protocol::Messages);
    assert_eq!(route.provenance().model.as_str(), "sonnet");

    // The same refusal `llm_models::port` gives a credentialed account without a resolver.
    let none: BTreeMap<Id, Arc<dyn SecretResolver>> = BTreeMap::new();
    for name in ["anthropic/claude-opus-4-1", "sonnet"] {
        let error = routes.port(name, http(), &none).err().unwrap();
        assert_eq!(error.code, ErrorCode::Unauthorized, "{name}");
        assert_eq!(
            error.message,
            "no credential resolver is supplied for account `anthropic`"
        );
    }
    let anonymous = routes.port("lab/other-model", http(), &none).unwrap();
    assert_eq!(anonymous.protocol(), Protocol::ChatCompletions);
    assert!(anonymous.resolver().is_none());
    for name in ["nobody/model", "unknown"] {
        let error = routes.port(name, http(), &none).err().unwrap();
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{name}");
    }
}

const MARKER: &str = "llm-routes-private-marker-value";
const MARKER_VARIABLE: &str = "LLM_ROUTES_TEST_MARKER_VARIABLE";
const MARKER_PATH: &str = "/llm-routes-absent-directory/llm-routes-secret-file";
const CHILD: &str = "LLM_ROUTES_TEST_SECRET_CHILD";
const CHILD_RAN: &str = "llm-routes-secret-child-ran";

fn leaks(text: &str) -> Option<&'static str> {
    [
        MARKER,
        MARKER_VARIABLE,
        MARKER_PATH,
        "llm-routes-absent-directory",
    ]
    .into_iter()
    .find(|forbidden| text.contains(forbidden))
}

/// The marker variable is set only in a child run of this test binary: no test changes its own
/// process environment.
#[test]
fn no_secret_material_leaves_the_document() {
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "no_secret_material_leaves_the_document",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD, "1")
            .env(MARKER_VARIABLE, MARKER)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "{stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(stdout.contains(CHILD_RAN), "{stdout}");
        return;
    }
    let document = format!(
        "format: llm.routes/1
endpoints:
  marked: {{base_url: https://marked.example.invalid/v1, protocol: responses,
           auth: {{kind: bearer, secret: \"env:{MARKER_VARIABLE}\"}}}}
  filed: {{base_url: https://filed.example.invalid/v1, protocol: messages,
          auth: {{kind: api-key, header: x-api-key, secret: \"file:{MARKER_PATH}\"}}}}
routes:
  m: marked/model
  f: filed/model
"
    );
    let routes = Routes::parse(&document).unwrap();
    for text in [format!("{routes:?}"), format!("{:?}", routes.catalog())] {
        assert_eq!(leaks(&text), None, "{text}");
    }
    for (_, binding) in routes.catalog().bindings() {
        let json = serde_json::to_string(binding.declaration()).unwrap();
        assert_eq!(leaks(&json), None, "{json}");
    }
    for bad in [
        document.replace(&format!("env:{MARKER_VARIABLE}"), MARKER),
        document.replace("protocol: responses", &format!("protocol: {MARKER}")),
        document.replace("kind: bearer", &format!("kind: {MARKER}")),
        document.replace("m: marked/model", &format!("m: nobody/{MARKER}")),
        document.replace("routes:", &format!("extra: {MARKER}\nroutes:")),
        document.replace(&format!("file:{MARKER_PATH}"), &format!("vault:{MARKER}")),
    ] {
        let error = refused(&bad);
        assert_eq!(leaks(&error.message), None, "{}", error.message);
    }
    for name in [format!("nobody/{MARKER}"), MARKER.to_owned()] {
        let error = routes.target(&name).unwrap_err();
        assert_eq!(leaks(&error.message), None, "{}", error.message);
    }

    #[cfg(all(feature = "environment", feature = "file"))]
    {
        let resolvers = routes.resolvers(&SecretSources::new()).unwrap();
        let text = format!("{resolvers:?}");
        assert_eq!(leaks(&text), None, "{text}");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let cancel = llm_core::Cancel::new();
        // The marker is read at turn time and travels only in the prepared header.
        let marked = routes.catalog().binding(&id("m")).unwrap();
        let prepared = runtime
            .block_on(
                marked.prepare_auth(resolvers.resolver(&id("marked")).unwrap().as_ref(), &cancel),
            )
            .unwrap();
        let text = format!("{prepared:?}");
        assert_eq!(leaks(&text), None, "{text}");
        let (headers, _) = prepared.into_parts();
        assert_eq!(
            headers["authorization"],
            format!("Bearer {MARKER}").as_str()
        );
        let filed = routes.catalog().binding(&id("f")).unwrap();
        let error = runtime
            .block_on(
                filed.prepare_auth(resolvers.resolver(&id("filed")).unwrap().as_ref(), &cancel),
            )
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Unauthorized);
        assert_eq!(
            error.message,
            "secret reference `filed` refused: secret reference was not found"
        );
        assert_eq!(leaks(&error.message), None);
    }
    println!("{CHILD_RAN}");
}
