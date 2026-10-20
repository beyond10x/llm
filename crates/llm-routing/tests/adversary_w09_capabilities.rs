//! Adversary pass 1, wave 2026-10-20-w09: the two streaming declarations in `llm.binding/1` JSON
//! and `llm.catalog/1` TOML, every combination of absent, true, false and ill-typed values; what an
//! accepted document writes is what 0.7.0 wrote; route explanation output is unchanged.

use llm_core::{ErrorCode, Item, TurnRequest};
use llm_providers::BindingDocument;
use llm_routing::Catalog;
use serde_json::Value;

const EXAMPLE: &str = include_str!("../../../examples/catalog.toml");
const MARKER: &str = "zq-marker-7f3";
const BEFORE_DIGEST: &str = "1365b05fe63542f8211e21a9a2566c71f2a6d44ec9bdc564ba46c592a861825f";
const BEFORE_REVISION: &str = "1a6ea40f98c8eef26ff66e7c753dda816f45a1aa3488bb31f62ef25a89c328a1";
/// The `local-small` binding of `examples/catalog.toml` as 0.7.0 serialized it.
const BEFORE: &str = r#"{"format":"llm.binding/1","provider":{"id":"my-lab","category":"self-hosted"},"account":{"id":"local","provider_id":"my-lab","auth_kind":"anonymous","billing_kind":"self-hosted"},"endpoint":{"id":"local-vllm","account_id":"local","base_url":"http://127.0.0.1:8000/v1"},"model":{"id":"small","upstream_name":"example/Small-Model"},"serving":{"id":"local-small","endpoint_id":"local-vllm","model_id":"small","protocol":"chat-completions","capabilities":{"tools":false,"tool_choice":false,"temperature":false,"top_p":false,"reasoning_efforts":[],"context_window":4096,"max_output_tokens":1024}}}"#;
/// The keys a 0.7.0 `Capabilities` (`deny_unknown_fields`) reads, and no others.
const V070_KEYS: [&str; 7] = [
    "tools",
    "tool_choice",
    "temperature",
    "top_p",
    "reasoning_efforts",
    "context_window",
    "max_output_tokens",
];

fn v070_readable(capabilities: &Value) -> bool {
    capabilities
        .as_object()
        .is_some_and(|object| object.keys().all(|key| V070_KEYS.contains(&key.as_str())))
}

#[test]
fn adversary_w09_every_binding_combination_is_refused_or_written_as_0_7_0_wrote_it() {
    let values = [
        None,
        Some("true"),
        Some("false"),
        Some("null"),
        Some("\"true\""),
        Some("1"),
    ];
    for streaming in values {
        for input in values {
            let tail = [
                streaming.map(|value| format!(",\"output_streaming\":{value}")),
                input.map(|value| format!(",\"input_during_output\":{value}")),
            ]
            .into_iter()
            .flatten()
            .collect::<String>();
            let source = BEFORE.replace(
                "\"max_output_tokens\":1024}",
                &format!("\"max_output_tokens\":1024{tail}}}"),
            );
            let expect_accepted =
                matches!(streaming, None | Some("true")) && matches!(input, None | Some("false"));
            let bound = serde_json::from_str::<BindingDocument>(&source)
                .map_err(|_| ())
                .and_then(|document| document.bind().map_err(|_| ()));
            assert_eq!(bound.is_ok(), expect_accepted, "{tail}");
            if let Ok(binding) = bound {
                assert_eq!(
                    binding.provenance().binding_revision.as_str(),
                    BEFORE_REVISION,
                    "{tail}"
                );
                // `BaseUrl` writes its normalized form, with a trailing `/`, as 0.7.0 did.
                assert_eq!(
                    serde_json::to_string(binding.declaration()).unwrap(),
                    BEFORE.replace("8000/v1\"", "8000/v1/\""),
                    "{tail}"
                );
            }
        }
    }
}

#[test]
fn adversary_w09_a_binding_repeating_a_declaration_is_refused() {
    for tail in [
        ",\"input_during_output\":false,\"input_during_output\":true",
        ",\"input_during_output\":true,\"input_during_output\":false",
        ",\"output_streaming\":true,\"output_streaming\":false",
        ",\"output_streaming\":false,\"output_streaming\":true",
    ] {
        let source = BEFORE.replace(
            "\"max_output_tokens\":1024}",
            &format!("\"max_output_tokens\":1024{tail}}}"),
        );
        assert!(
            serde_json::from_str::<BindingDocument>(&source).is_err(),
            "{tail}"
        );
    }
}

#[test]
fn adversary_w09_every_catalog_spelling_of_a_claim_is_refused_and_echoes_no_value() {
    for lines in [
        "input_during_output = true\n",
        "output_streaming = false\n",
        "input_during_output = false\ninput_during_output = true\n",
        "output_streaming = true\noutput_streaming = false\n",
        "input_during_output = \"zq-marker-7f3\"\n",
        "output_streaming = 'zq-marker-7f3'\n",
        "input_during_output = 1\n",
        "inputDuringOutput = true\n",
        "input-during-output = true\n",
        "output_streaming = true\ninput_during_output = true\n",
        "output_streaming = false\ninput_during_output = false\n",
    ] {
        let declared = EXAMPLE.replace(
            "[serving_models.capabilities]\n",
            &format!("[serving_models.capabilities]\n{lines}"),
        );
        assert_ne!(declared, EXAMPLE);
        let error = Catalog::parse(&declared).map(|_| ()).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{lines}");
        assert!(
            !error.message.contains(MARKER),
            "{lines}: {}",
            error.message
        );
    }
}

#[test]
fn adversary_w09_route_explanation_writes_only_what_0_7_0_reads() {
    for source in [
        EXAMPLE.to_owned(),
        EXAMPLE.replace(
            "[serving_models.capabilities]\n",
            "[serving_models.capabilities]\noutput_streaming = true\ninput_during_output = false\n",
        ),
    ] {
        let catalog = Catalog::parse(&source).unwrap();
        assert_eq!(catalog.digest(), BEFORE_DIGEST);
        let mut request = TurnRequest::new(
            "code",
            vec![Item::UserText {
                text: "Explain this function.".into(),
            }],
        );
        request.max_output_tokens = Some(256);
        let explanation =
            serde_json::to_value(catalog.explain(&request, Some(512)).unwrap()).unwrap();
        let targets = explanation["targets"].as_array().unwrap();
        assert_eq!(targets.len(), 2);
        for target in targets {
            assert!(v070_readable(&target["capabilities"]), "{target}");
        }
    }
}
