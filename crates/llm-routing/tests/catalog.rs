use llm_core::{ErrorCode, Id, Item, ToolChoice, ToolName, ToolSpec, TurnRequest};
use llm_providers::BaseUrl;
use llm_routing::{Catalog, CatalogDocument, MAX_CONFIG_BYTES, Rejection};
use serde_json::json;

const EXAMPLE: &str = include_str!("../../../examples/catalog.toml");
fn document() -> CatalogDocument {
    CatalogDocument::parse(EXAMPLE).unwrap()
}
fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}
fn request() -> TurnRequest {
    let mut request = TurnRequest::new(
        "code",
        vec![Item::UserText {
            text: "hello".into(),
        }],
    );
    request.max_output_tokens = Some(128);
    request
}

#[test]
fn arbitrary_toml_models_resolve_without_secrets_network_or_runtime() {
    let catalog = Catalog::parse(EXAMPLE).unwrap();
    let original = request();
    let selected = catalog.resolve(&original, Some(512)).unwrap();
    assert_eq!(selected.target.id, id("coding-primary"));
    assert_eq!(
        selected.binding.request_url(),
        "http://127.0.0.1:8000/v1/chat/completions"
    );
    assert_eq!(selected.request.model, "small");
    let mut expected = original.clone();
    expected.model = "small".into();
    assert_eq!(selected.request, expected);
    assert_eq!(original.model, "code");
    let explanation = catalog.explain(&original, Some(512)).unwrap();
    assert_eq!(explanation.selected_target_id, Some(id("coding-primary")));
    assert_eq!(
        explanation.targets[1].rejections,
        [Rejection::FallbackDisabled]
    );
    // The unavailable authenticated alternative is inspected without attempting to resolve it.
    assert_eq!(catalog.routes().count(), 1);
    assert!(
        !serde_json::to_string(&explanation)
            .unwrap()
            .contains("lab-llm-token")
    );
}

#[test]
fn opt_in_selects_the_first_compatible_alternative_without_dropping_settings() {
    let mut doc = document();
    let mut turn = request();
    turn.sampling.temperature = Some(0.5);
    let refused = doc
        .clone()
        .validate()
        .unwrap()
        .explain(&turn, Some(100))
        .unwrap();
    assert_eq!(refused.selected_target_id, None);
    assert_eq!(refused.targets[0].rejections, [Rejection::Temperature]);
    assert_eq!(refused.targets[1].rejections, [Rejection::FallbackDisabled]);
    doc.routes[0].fallback_enabled = true;
    let catalog = doc.validate().unwrap();
    let selected = catalog.resolve(&turn, Some(100)).unwrap();
    assert_eq!(selected.target.id, id("coding-secondary"));
    assert_eq!(selected.request.sampling, turn.sampling);
    assert_eq!(selected.request.model, "large");
    assert_eq!(
        selected.binding.request_url(),
        "https://models.example.invalid/v1/responses"
    );
}

#[test]
fn capability_refusals_name_every_requested_dimension() {
    let mut turn = request();
    turn.tools.push(ToolSpec {
        name: ToolName::new("lookup").unwrap(),
        description: String::new(),
        input_schema: json!({"type":"object"}),
    });
    turn.tool_choice = ToolChoice::Required;
    turn.sampling.temperature = Some(0.5);
    turn.sampling.top_p = Some(0.8);
    turn.sampling.reasoning_effort = Some("high".into());
    turn.max_output_tokens = Some(2048);
    let catalog = document().validate().unwrap();
    let explanation = catalog.explain(&turn, Some(3072)).unwrap();
    assert_eq!(
        explanation.targets[0].rejections,
        [
            Rejection::Tools,
            Rejection::ToolChoice,
            Rejection::Temperature,
            Rejection::TopP,
            Rejection::ReasoningEffort,
            Rejection::OutputLimit,
            Rejection::ContextWindow
        ]
    );
    let error = catalog.resolve(&turn, Some(3072)).unwrap_err();
    for reason in &explanation.targets[0].rejections {
        assert!(error.message.contains(reason.label()));
    }
}

#[test]
fn unknown_or_overflowing_input_never_becomes_zero_and_limits_are_inclusive() {
    let catalog = document().validate().unwrap();
    let turn = request();
    let unknown = catalog.explain(&turn, None).unwrap();
    assert_eq!(unknown.input_tokens, None);
    assert!(
        unknown.targets[0]
            .rejections
            .contains(&Rejection::InputTokensUnknown)
    );
    assert!(catalog.resolve(&turn, None).is_err());
    assert!(catalog.resolve(&turn, Some(4096 - 128)).is_ok());
    assert!(catalog.resolve(&turn, Some(4096 - 127)).is_err());
    assert!(
        catalog.explain(&turn, Some(u64::MAX)).unwrap().targets[0]
            .rejections
            .contains(&Rejection::ContextWindow)
    );
    let mut implicit_limit = turn;
    implicit_limit.max_output_tokens = None;
    assert!(catalog.resolve(&implicit_limit, Some(4096 - 1024)).is_ok());
    assert!(catalog.resolve(&implicit_limit, Some(4096 - 1023)).is_err());
}

#[test]
fn opaque_state_never_crosses_bindings_even_with_explicit_fallback() {
    let mut doc = document();
    doc.routes[0].fallback_enabled = true;
    let catalog = doc.validate().unwrap();
    let origin = catalog
        .binding(&id("local-small"))
        .unwrap()
        .provenance()
        .clone();
    let mut turn = request();
    turn.items.push(Item::Opaque {
        provenance: origin,
        payload: json!({"private_state":"do-not-explain"}),
    });
    turn.sampling.temperature = Some(0.3); // primary cannot serve it; secondary cannot inherit state
    let explanation = catalog.explain(&turn, Some(100)).unwrap();
    assert!(explanation.selected_target_id.is_none());
    assert_eq!(explanation.targets[1].rejections, [Rejection::OpaqueState]);
    assert!(
        !serde_json::to_string(&explanation)
            .unwrap()
            .contains("do-not-explain")
    );
}

#[test]
fn repointing_an_endpoint_or_upstream_model_under_the_same_ids_invalidates_opaque_state() {
    let original = document().validate().unwrap();
    let provenance = original
        .binding(&id("local-small"))
        .unwrap()
        .provenance()
        .clone();
    let mut turn = request();
    turn.items.push(Item::Opaque {
        provenance: provenance.clone(),
        payload: json!({"state":1}),
    });
    assert!(original.resolve(&turn, Some(100)).is_ok());
    for change in 0..2 {
        let mut changed = document();
        if change == 0 {
            changed.endpoints[0].base_url = BaseUrl::new("http://127.0.0.1:9000/v1").unwrap();
        } else {
            changed.models[0].upstream_name = id("different/upstream-model");
        }
        let changed = changed.validate().unwrap();
        let target = changed.binding(&id("local-small")).unwrap().provenance();
        assert_eq!(target.endpoint, provenance.endpoint);
        assert_eq!(target.model, provenance.model);
        assert_ne!(target.binding_revision, provenance.binding_revision);
        assert_eq!(
            changed.explain(&turn, Some(100)).unwrap().targets[0].rejections,
            [Rejection::OpaqueState]
        );
    }
}

#[test]
fn declaration_order_and_toml_comments_do_not_change_identity_or_selection() {
    let mut original = document();
    original.routes[0].fallback_enabled = true;
    let canonical = original.clone().validate().unwrap();
    let mut reordered = original.clone();
    reordered.providers.reverse();
    reordered.accounts.reverse();
    reordered.endpoints.reverse();
    reordered.models.reverse();
    reordered.serving_models.reverse();
    reordered.routes.reverse();
    reordered.targets.reverse();
    let reordered = reordered.validate().unwrap();
    assert_eq!(canonical.digest(), reordered.digest());
    assert_eq!(canonical.digest().len(), 64);
    assert_eq!(
        canonical.resolve(&request(), Some(100)).unwrap().target.id,
        reordered.resolve(&request(), Some(100)).unwrap().target.id
    );
    assert_eq!(
        Catalog::parse(EXAMPLE).unwrap().digest(),
        Catalog::parse(&format!("# comment\n{EXAMPLE}\n"))
            .unwrap()
            .digest()
    );
    original.targets[0].position = 1;
    original.targets[1].position = 0;
    let reprioritized = original.validate().unwrap();
    assert_ne!(canonical.digest(), reprioritized.digest());
    assert_eq!(
        reprioritized
            .resolve(&request(), Some(100))
            .unwrap()
            .target
            .id,
        id("coding-secondary")
    );
}

#[test]
fn changed_endpoint_auth_capability_or_upstream_name_changes_configuration_identity() {
    let base = document().validate().unwrap();
    for change in 0..5 {
        let mut doc = document();
        match change {
            0 => doc.endpoints[0].base_url = BaseUrl::new("http://127.0.0.1:8001/v1").unwrap(),
            1 => doc.accounts[1] = account_with_rotated_reference(&doc),
            2 => doc.serving_models[0].capabilities.temperature = true,
            3 => doc.models[0].upstream_name = id("another/model"),
            _ => doc.routes[0].fallback_enabled = true,
        }
        assert_ne!(base.digest(), doc.validate().unwrap().digest());
    }
}

// Use the public serialized declaration to change a reference without adding credential I/O.
fn account_with_rotated_reference(doc: &CatalogDocument) -> llm_providers::Account {
    let mut value = serde_json::to_value(&doc.accounts[1]).unwrap();
    value["secret_reference_id"] = json!("rotated-reference-name");
    serde_json::from_value(value).unwrap()
}

#[test]
fn duplicates_gaps_empty_routes_and_missing_references_are_refused() {
    for change in 0..15 {
        let mut doc = document();
        match change {
            0 => doc.providers.push(doc.providers[0].clone()),
            1 => doc.accounts[0].provider_id = id("missing"),
            2 => doc.endpoints[0].account_id = id("missing"),
            3 => doc.serving_models[0].model_id = id("missing"),
            4 => doc.serving_models[0].endpoint_id = id("missing"),
            5 => doc.targets[0].route_id = id("missing"),
            6 => doc.targets[0].serving_model_id = id("missing"),
            7 => doc.targets[1].position = 0,
            8 => doc.targets[1].position = 2,
            9 => doc.targets[1].serving_model_id = doc.targets[0].serving_model_id.clone(),
            10 => doc.targets[1].id = doc.targets[0].id.clone(),
            11 => doc.targets.clear(),
            12 => doc.serving_models[0].capabilities.context_window = 0,
            13 => {
                let mut route = doc.routes[0].clone();
                route.id = id("other");
                doc.routes.push(route);
                let mut target = doc.targets[0].clone();
                target.id = id("other-target");
                target.route_id = id("other");
                doc.targets.push(target);
            }
            _ => {
                let mut unused = doc.accounts[1].clone();
                unused.id = id("unused");
                unused.provider_id = id("missing");
                doc.accounts.push(unused);
            }
        }
        assert!(doc.validate().is_err(), "accepted invalid case {change}");
    }
}

#[test]
fn version_typos_secrets_and_oversized_documents_fail_without_source_echo() {
    for source in [
        EXAMPLE.replace("llm.catalog/1", "llm.catalog/2"),
        EXAMPLE.replace("chat-completions", "chat-completion"),
        EXAMPLE.replace(
            "auth_kind = \"bearer\"",
            "auth_kind = \"bearer\"\nsecret = \"do-not-echo\"",
        ),
        EXAMPLE.replace(
            "http://127.0.0.1:8000/v1",
            "http://user:do-not-echo@127.0.0.1:8000/v1",
        ),
    ] {
        let error = Catalog::parse(&source).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert!(!format!("{error:?}").contains("do-not-echo"));
    }
    assert_eq!(
        Catalog::parse(&"a".repeat(MAX_CONFIG_BYTES + 1))
            .unwrap_err()
            .code,
        ErrorCode::TooLarge
    );
    let default_off = Catalog::parse(&EXAMPLE.replace("fallback_enabled = false", "")).unwrap();
    assert!(!default_off.routes().next().unwrap().fallback_enabled);
}

#[test]
fn invalid_requests_and_unknown_aliases_never_select_a_target() {
    let catalog = Catalog::parse(EXAMPLE).unwrap();
    let mut turn = request();
    turn.model = "unknown".into();
    assert!(catalog.explain(&turn, Some(0)).is_err());
    turn.model = "code".into();
    turn.sampling.temperature = Some(f64::NAN);
    assert!(catalog.explain(&turn, Some(0)).is_err());
}
