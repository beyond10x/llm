//! The `llm.routes/1` document: strict parsing, its expansion into an `llm.catalog/1` catalog,
//! and the resolvers its secret references build. No test here sends a request.

use llm_core::{Error, ErrorCode, Id, Model, Protocol};
use llm_credentials::SecretResolver;
use llm_http::{HttpClient, Limits};
use llm_models::{
    CatalogModels, Resolvers,
    routes::{FORMAT, Routes, SecretKind, SecretSources, default_capabilities},
};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

/// No test environment sets this; every `env:` reference in the shared document names it.
const NEVER_SET: &str = "LLM_ROUTES_TEST_NEVER_SET";

const DOCUMENT: &str = "format: llm.routes/1
providers:
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

/// One provider `p` and one route `r`; each refusal case replaces one placeholder.
const SINGLE: &str = "format: FORMAT
TOP
providers:
  p:
    base_url: https://p.example.invalid/v1
    protocol: PROTOCOL
    auth: AUTH
    PROVIDER_EXTRA
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
        ("PROVIDER_EXTRA", ""),
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
            .providers()
            .filter(|provider| provider.secret_kind().is_some())
            .map(|provider| {
                (
                    provider.name().clone(),
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
    for (name, provider, protocol, upstream) in [
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
        assert_eq!(target.provider.as_str(), provider, "{name}");
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
    let providers: Vec<(&str, Option<&str>)> = routes
        .providers()
        .map(|provider| {
            (
                provider.name().as_str(),
                provider.secret_kind().map(SecretKind::as_str),
            )
        })
        .collect();
    assert_eq!(
        providers,
        [
            ("anthropic", Some("env")),
            ("gateway", Some("file")),
            ("lab", None),
            ("openrouter", Some("env")),
        ]
    );
}

#[test]
fn a_provider_declared_twice_is_refused() {
    let document = DOCUMENT.replace(
        "  lab:\n",
        "  anthropic: {base_url: https://second.example.invalid/v1, protocol: messages, auth: {kind: anonymous}}\n  lab:\n",
    );
    let error = refused(&document);
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert!(!error.message.contains("second.example.invalid"));
    let routes = "routes:\n  r: p/m\n  r: p/other\n";
    let error = refused(&single(&[]).replace("routes:\n  r: p/m\n", routes));
    assert_eq!(error.code, ErrorCode::InvalidRequest);
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
            "unknown provider field",
            single(&[("PROVIDER_EXTRA", "region: sk-unknown-provider")]),
            "sk-unknown-provider",
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
            "undeclared provider",
            single(&[("ROUTE", "nobody/sk-model")]),
            "nobody/sk-model",
        ),
        (
            "route without a provider",
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
fn refusals_name_the_provider_or_route_they_concern() {
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
fn building_resolvers_for_a_keychain_reference_without_an_injected_store_is_refused_naming_the_provider()
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
        assert_eq!(port.provenance().provider, target.provider);
        assert_eq!(port.provenance().account, target.provider);
        assert_eq!(port.provenance().endpoint, target.provider);
        assert_eq!(
            port.resolver().is_some(),
            route.name().as_str() != "small",
            "{}",
            route.name()
        );
    }
}

/// No ambient fallback: the unset variable is refused by the provider's reference, before any
/// request, and the refusal names neither the variable nor a value.
#[cfg(feature = "environment")]
#[test]
fn an_unset_variable_is_refused_at_turn_time_by_the_providers_reference() {
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
fn llm_models_port_for_a_direct_provider_model_name_builds_through_the_same_rules() {
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
providers:
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
