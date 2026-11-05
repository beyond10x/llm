//! Structured output and image input are declarations no document made before them. Every
//! checked-in document that carries capabilities (the example catalog, and every catalog,
//! routes, binding and capabilities document an authored scenario holds) reads with both
//! undeclared, and its capabilities write exactly the bytes the 0.21.0 struct wrote, so no
//! configuration digest or binding revision moves. A routes document that declares them carries
//! them into the catalog it expands into, where selection reads them.

use llm_core::{Capabilities, ImageMediaType, ImageSource, Item, TurnRequest};
use llm_models::routes::Routes;
use llm_providers::BindingDocument;
use llm_routing::{Catalog, Rejection};
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::{fs, path::Path};

/// `llm_core::Capabilities` as 0.21.0 declared it, frozen here: the shape every document from
/// before the two declarations was hashed in.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools)]
struct Capabilities0210 {
    tools: bool,
    tool_choice: bool,
    temperature: bool,
    top_p: bool,
    reasoning_efforts: Vec<String>,
    context_window: u64,
    max_output_tokens: u64,
    #[serde(default = "streams", skip_serializing_if = "is_true")]
    output_streaming: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    input_during_output: bool,
}

const fn streams() -> bool {
    true
}

#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_true(value: &bool) -> bool {
    *value
}

const EXAMPLE: &str = include_str!("../../../examples/catalog.toml");

/// The example catalog's configuration digest and binding revisions, as
/// `website/docs/guides/explain-a-route.md` prints them.
const EXAMPLE_DIGEST: &str = "1365b05fe63542f8211e21a9a2566c71f2a6d44ec9bdc564ba46c592a861825f";
const EXAMPLE_REVISIONS: [(&str, &str); 2] = [
    (
        "local-small",
        "1a6ea40f98c8eef26ff66e7c753dda816f45a1aa3488bb31f62ef25a89c328a1",
    ),
    (
        "remote-large",
        "5dd662a7d1b9e5280a10e6b691ff04c79326d1562d18d3206e9d1bd141ed9a13",
    ),
];

fn undeclared_and_unchanged(place: &str, capabilities: &Capabilities) {
    assert_eq!(capabilities.structured_output, None, "{place}");
    assert_eq!(capabilities.image_input, None, "{place}");
    let written = serde_json::to_string(capabilities).unwrap();
    let frozen: Capabilities0210 = serde_json::from_str(&written)
        .unwrap_or_else(|error| panic!("{place} writes a member 0.21.0 did not: {error}"));
    assert_eq!(serde_json::to_string(&frozen).unwrap(), written, "{place}");
}

fn catalog(place: &str, catalog: &Catalog) {
    for (id, binding) in catalog.bindings() {
        undeclared_and_unchanged(&format!("{place} {id}"), binding.capabilities());
    }
}

#[test]
fn the_example_catalog_declares_neither_feature_and_keeps_its_digest_and_revisions() {
    let example = Catalog::parse(EXAMPLE).unwrap();
    catalog("examples/catalog.toml", &example);
    assert_eq!(example.digest(), EXAMPLE_DIGEST);
    for (serving, revision) in EXAMPLE_REVISIONS {
        let binding = example
            .binding(&llm_core::Id::new(serving).unwrap())
            .unwrap();
        assert_eq!(binding.provenance().binding_revision.as_str(), revision);
    }
}

const DECLARING: &str = "format: llm.routes/1
endpoints:
  lab:
    base_url: http://127.0.0.1:8000/v1
    protocol: chat-completions
    auth: {kind: anonymous}
routes:
  plain: lab/small
  sighted:
    model: lab/small
    capabilities: {tools: false, tool_choice: false, temperature: false, top_p: false,
                   reasoning_efforts: [], context_window: 32768, max_output_tokens: 8192,
                   structured_output: true, image_input: false}
";

#[test]
fn a_routes_mapping_declares_both_features_and_selection_reads_them_from_its_catalog() {
    let routes = Routes::parse(DECLARING).unwrap();
    let sighted = routes.route("sighted").unwrap().capabilities();
    assert_eq!(sighted.structured_output, Some(true));
    assert_eq!(sighted.image_input, Some(false));
    undeclared_and_unchanged("plain", routes.route("plain").unwrap().capabilities());

    let image = |model: &str| {
        let mut turn = TurnRequest::new(model, vec![Item::user("What is this?")]);
        turn.items.push(Item::user_image(ImageSource::bytes(
            ImageMediaType::Png,
            b"\x89PNG".to_vec(),
        )));
        turn.max_output_tokens = Some(128);
        turn
    };
    let refused = routes
        .catalog()
        .explain(&image("sighted"), Some(100))
        .unwrap();
    assert_eq!(refused.targets[0].rejections, [Rejection::ImageInput]);
    assert_eq!(refused.selected_target_id, None);
    let taken = routes
        .catalog()
        .explain(&image("plain"), Some(100))
        .unwrap();
    assert_eq!(taken.targets[0].rejections, []);
}

#[derive(Default)]
struct Checked {
    catalogs: usize,
    routes: usize,
    bindings: usize,
    capabilities: usize,
}

#[test]
fn every_scenario_document_from_before_the_declarations_reads_them_undeclared_and_keeps_its_bytes()
{
    let contracts = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts");
    let mut checked = Checked::default();
    let mut domains: Vec<_> = fs::read_dir(&contracts)
        .unwrap()
        .map(|entry| entry.unwrap().path().join("scenarios"))
        .filter(|path| path.is_dir())
        .collect();
    domains.sort();
    for scenarios in domains {
        let mut files: Vec<_> = fs::read_dir(&scenarios)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        files.sort();
        for file in files {
            let place = file.strip_prefix(&contracts).unwrap().display().to_string();
            let scenario: Value = serde_yaml::from_str(&fs::read_to_string(&file).unwrap())
                .unwrap_or_else(|error| panic!("{place}: {error}"));
            let Some(Value::Sequence(timeline)) = scenario.get("timeline") else {
                continue;
            };
            for step in timeline {
                let Some(Value::Mapping(input)) = step.get("input") else {
                    continue;
                };
                for (key, value) in input {
                    let (Some(key), Some(text)) = (key.as_str(), value.as_str()) else {
                        continue;
                    };
                    // A document naming either declaration is not one from before them.
                    if text.contains("structured_output") || text.contains("image_input") {
                        continue;
                    }
                    let place = format!("{place} {key}");
                    match key {
                        "catalog_toml" => {
                            if let Ok(parsed) = Catalog::parse(text) {
                                catalog(&place, &parsed);
                                checked.catalogs += 1;
                            }
                        }
                        "routes_yaml" => {
                            if let Ok(parsed) = Routes::parse(text) {
                                for route in parsed.routes() {
                                    undeclared_and_unchanged(
                                        &format!("{place} {}", route.name()),
                                        route.capabilities(),
                                    );
                                }
                                catalog(&place, parsed.catalog());
                                checked.routes += 1;
                            }
                        }
                        "binding_json" => {
                            if let Ok(binding) = serde_json::from_str::<BindingDocument>(text)
                                .map_err(|_| ())
                                .and_then(|document| document.bind().map_err(|_| ()))
                            {
                                undeclared_and_unchanged(&place, binding.capabilities());
                                checked.bindings += 1;
                            }
                        }
                        "capabilities_json" => {
                            if let Ok(parsed) = serde_json::from_str::<Capabilities>(text) {
                                undeclared_and_unchanged(&place, &parsed);
                                checked.capabilities += 1;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    // The walk reads the documents it claims to: a floor under each kind's count today. A
    // scenario whose document is refused (most `llm.routes` ones) holds no declaration to check,
    // and a `binding_json` that is a provenance (every `llm.responses` one) holds none either.
    let counts = format!(
        "catalogs {}, routes {}, bindings {}, capabilities {}",
        checked.catalogs, checked.routes, checked.bindings, checked.capabilities
    );
    eprintln!("checked {counts}");
    assert!(
        checked.catalogs >= 60
            && checked.routes >= 10
            && checked.bindings >= 20
            && checked.capabilities >= 10,
        "{counts}"
    );
}
