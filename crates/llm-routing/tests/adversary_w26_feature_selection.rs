//! Adversary, wave 2026-11-05-w26, story:output-and-image-capabilities.
//!
//! The semantics the lead fixed, driven against selection and fallback:
//!
//! - A turn needs structured output when `controls.output_format` is set, and image input when
//!   any item is an `Item::UserImage`, wherever it sits in the conversation.
//! - A target declaring the feature `false` is rejected by name and no fallback moves the turn to
//!   it; an undeclared or `true` target is not rejected.
//! - Several rejections are named in a fixed order, and the no-compatible-target refusal names
//!   each label once and carries no byte of the turn.
//!
//! The unit's own cases put the image last in the conversation and use two-target routes; these
//! use three targets in every order and an image earlier in the history. No network: every model
//! is a scripted fake.

use llm_core::{
    BoxFuture, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, ImageMediaType, ImageSource,
    Item, Model, OutputFormat, Provenance, StopReason, StreamSink, TurnObservation, TurnOutcome,
    TurnRequest, VecSink,
};
use llm_routing::{Catalog, FallbackPolicy, FallbackRun, Halt, Models, Ports, Rejection};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    future::Future,
    sync::atomic::{AtomicUsize, Ordering},
    task::{Context, Poll, Waker},
};

const SPEC_CATALOG: &str = include_str!("../../../spec/domains/catalog.yaml");
const SCHEMA_ROUTE_REJECTION: &str =
    include_str!("../../../contracts/schema/schema/types/llm.catalog.RouteRejection.schema.json");

fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}

/// Three serving models `a`, `b`, `c` on one anonymous endpoint, targets `t0`, `t1`, `t2` at
/// positions 0, 1, 2 of route `code`. Each entry of `declared` is appended to that model's
/// capabilities table as written: `""` leaves both features undeclared.
fn toml(declared: [&str; 3], fallback_enabled: bool) -> String {
    let mut text = String::from(
        r#"format = "llm.catalog/1"

[[providers]]
id = "lab"
category = "self-hosted"

[[accounts]]
id = "local"
provider_id = "lab"
auth_kind = "anonymous"
billing_kind = "self-hosted"

[[endpoints]]
id = "local-vllm"
account_id = "local"
base_url = "http://127.0.0.1:8000/v1"
"#,
    );
    for (index, (serving, extra)) in ["a", "b", "c"].iter().zip(declared).enumerate() {
        write!(
            text,
            r#"
[[models]]
id = "m-{serving}"
upstream_name = "example/{serving}"

[[serving_models]]
id = "{serving}"
endpoint_id = "local-vllm"
model_id = "m-{serving}"
protocol = "chat-completions"
[serving_models.capabilities]
tools = false
tool_choice = false
temperature = false
top_p = false
reasoning_efforts = []
context_window = 32768
max_output_tokens = 8192
{extra}

[[targets]]
id = "t{index}"
route_id = "coding"
serving_model_id = "{serving}"
position = {index}
"#
        )
        .unwrap();
    }
    write!(
        text,
        "\n[[routes]]\nid = \"coding\"\nalias = \"code\"\nfallback_enabled = {fallback_enabled}\n"
    )
    .unwrap();
    text
}

fn catalog(declared: [&str; 3]) -> Catalog {
    Catalog::parse(&toml(declared, true)).unwrap()
}

fn image() -> Item {
    Item::user_image(ImageSource::bytes(ImageMediaType::Png, b"\x89PNG".to_vec()))
}

fn plain() -> TurnRequest {
    let mut turn = TurnRequest::new("code", vec![Item::user("hello")]);
    turn.max_output_tokens = Some(128);
    turn
}

fn with_format(mut turn: TurnRequest) -> TurnRequest {
    turn.controls.output_format =
        Some(OutputFormat::json_schema(json!({"type": "object"})).with_name("place"));
    turn
}

/// A user image last in the conversation, as every case of the unit has it.
fn image_last() -> TurnRequest {
    let mut turn = plain();
    turn.items.push(image());
    turn
}

/// A user image two turns back: the model answered it, and the user asked a follow-up in text.
fn image_in_history() -> TurnRequest {
    let mut turn = TurnRequest::new(
        "code",
        vec![
            Item::user("What is in this picture?"),
            image(),
            Item::assistant("A lighthouse."),
            Item::user("And what colour is it?"),
        ],
    );
    turn.max_output_tokens = Some(128);
    turn
}

fn rejections(catalog: &Catalog, turn: &TurnRequest) -> Vec<Vec<Rejection>> {
    catalog
        .explain(turn, Some(100))
        .unwrap()
        .targets
        .into_iter()
        .map(|target| target.rejections)
        .collect()
}

fn selected(catalog: &Catalog, turn: &TurnRequest) -> Option<Id> {
    catalog.explain(turn, Some(100)).unwrap().selected_target_id
}

const NO_IMAGE: &str = "image_input = false";
const IMAGE: &str = "image_input = true";
const NO_FORMAT: &str = "structured_output = false";
const FORMAT: &str = "structured_output = true";
const UNDECLARED: &str = "";

#[test]
fn an_image_earlier_in_the_conversation_rejects_a_target_declaring_no_image_input() {
    let catalog = catalog([NO_IMAGE, UNDECLARED, IMAGE]);
    let turn = image_in_history();
    assert_eq!(
        rejections(&catalog, &turn),
        [vec![Rejection::ImageInput], vec![], vec![]]
    );
    assert_eq!(selected(&catalog, &turn), Some(id("t1")));
    assert_eq!(
        catalog.resolve(&turn, Some(100)).unwrap().target.id,
        id("t1")
    );
}

#[test]
fn validate_for_refuses_an_image_earlier_in_the_conversation_for_a_binding_declaring_none() {
    let catalog = catalog([NO_IMAGE, UNDECLARED, IMAGE]);
    let binding = catalog.binding(&id("a")).unwrap();
    let mut turn = image_in_history();
    turn.model = binding.provenance().model.as_str().to_owned();
    let error = turn
        .validate_for(binding.provenance(), binding.capabilities())
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(error.message, "binding does not take image input");
}

/// The first target whose rejections are empty is selected, in declared position order,
/// whatever mix of `false`, `true` and undeclared the route holds.
#[test]
fn the_first_eligible_target_is_selected_in_declared_order_for_every_mix() {
    for (declared, expected, picked) in [
        (
            [NO_IMAGE, UNDECLARED, IMAGE],
            [true, false, false],
            Some("t1"),
        ),
        (
            [IMAGE, NO_IMAGE, UNDECLARED],
            [false, true, false],
            Some("t0"),
        ),
        (
            [UNDECLARED, NO_IMAGE, IMAGE],
            [false, true, false],
            Some("t0"),
        ),
        (
            [NO_IMAGE, NO_IMAGE, UNDECLARED],
            [true, true, false],
            Some("t2"),
        ),
        ([NO_IMAGE, IMAGE, NO_IMAGE], [true, false, true], Some("t1")),
        ([NO_IMAGE, NO_IMAGE, NO_IMAGE], [true, true, true], None),
    ] {
        let catalog = catalog(declared);
        for turn in [image_last(), image_in_history()] {
            let got = rejections(&catalog, &turn);
            let want: Vec<Vec<Rejection>> = expected
                .iter()
                .map(|rejected| {
                    if *rejected {
                        vec![Rejection::ImageInput]
                    } else {
                        vec![]
                    }
                })
                .collect();
            assert_eq!(got, want, "{declared:?}");
            assert_eq!(selected(&catalog, &turn), picked.map(id), "{declared:?}");
        }
        // The same declarations concern no turn without an image.
        assert_eq!(selected(&catalog, &plain()), Some(id("t0")), "{declared:?}");
    }
    for (declared, picked) in [
        ([NO_FORMAT, UNDECLARED, FORMAT], Some("t1")),
        ([FORMAT, NO_FORMAT, UNDECLARED], Some("t0")),
        ([NO_FORMAT, NO_FORMAT, FORMAT], Some("t2")),
        ([NO_FORMAT, NO_FORMAT, NO_FORMAT], None),
    ] {
        let catalog = catalog(declared);
        assert_eq!(
            selected(&catalog, &with_format(plain())),
            picked.map(id),
            "{declared:?}"
        );
        assert_eq!(selected(&catalog, &plain()), Some(id("t0")), "{declared:?}");
    }
    let refused = catalog([NO_IMAGE, NO_IMAGE, NO_IMAGE])
        .resolve(&image_in_history(), Some(100))
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::Unsupported);
    assert_eq!(
        refused.message,
        "route has no compatible target: image-input"
    );
}

#[test]
fn a_turn_needing_both_is_rejected_by_each_target_for_the_feature_it_declares_false() {
    let catalog = catalog([
        "structured_output = true\nimage_input = false",
        "structured_output = false\nimage_input = true",
        UNDECLARED,
    ]);
    let both = with_format(image_in_history());
    assert_eq!(
        rejections(&catalog, &both),
        [
            vec![Rejection::ImageInput],
            vec![Rejection::StructuredOutput],
            vec![]
        ]
    );
    assert_eq!(selected(&catalog, &both), Some(id("t2")));
    assert_eq!(selected(&catalog, &image_in_history()), Some(id("t1")));
    assert_eq!(selected(&catalog, &with_format(plain())), Some(id("t0")));
}

/// Every rejection that applies is named, not only the first, in the order the enum declares;
/// the explanation is the same on every call; the refusal names each label once, sorted.
#[test]
fn every_rejection_that_applies_is_named_in_a_fixed_order_and_the_refusal_names_each_once() {
    let both_false = "structured_output = false\nimage_input = false";
    let catalog = catalog([both_false, both_false, both_false]);
    let mut turn = with_format(image_in_history());
    turn.sampling.temperature = Some(0.5);
    turn.max_output_tokens = Some(9000);
    let first = catalog.explain(&turn, Some(100)).unwrap();
    let again = catalog.explain(&turn, Some(100)).unwrap();
    assert_eq!(first.selected_target_id, None);
    for target in &first.targets {
        assert_eq!(
            target.rejections,
            [
                Rejection::Temperature,
                Rejection::OutputLimit,
                Rejection::StructuredOutput,
                Rejection::ImageInput,
            ],
            "{}",
            target.target_id
        );
    }
    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(&again).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&first).unwrap()["targets"][2]["rejections"],
        json!([
            "temperature",
            "output-limit",
            "structured-output",
            "image-input"
        ])
    );
    let error = catalog.resolve(&turn, Some(100)).unwrap_err();
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(
        error.message,
        "route has no compatible target: image-input, output-limit, structured-output, temperature"
    );
}

#[test]
fn the_no_compatible_target_refusal_carries_no_byte_of_the_turn() {
    let catalog = Catalog::parse(&toml(
        [
            "structured_output = false\nimage_input = false",
            "structured_output = false\nimage_input = false",
            "structured_output = false\nimage_input = false",
        ],
        false,
    ))
    .unwrap();
    let image_bytes = b"PRIVATE-IMAGE-BYTES-5521".to_vec();
    let mut turn = TurnRequest::new(
        "code",
        vec![
            Item::user("PRIVATE-USER-TEXT-7731"),
            Item::user_image(ImageSource::bytes(ImageMediaType::Png, image_bytes.clone())),
            Item::user_image(ImageSource::url(
                "https://private-host.example.invalid/private-path-9912.png",
            )),
        ],
    );
    turn.max_output_tokens = Some(128);
    turn.controls.output_format = Some(
        OutputFormat::json_schema(json!({
            "type": "object",
            "properties": {"private_field_3317": {"type": "string"}}
        }))
        .with_name("private_schema_4410"),
    );
    let error = catalog.resolve(&turn, Some(100)).unwrap_err();
    assert_eq!(
        error.message,
        "route has no compatible target: fallback-disabled, image-input, structured-output"
    );
    let base64 = ImageSource::bytes(ImageMediaType::Png, image_bytes)
        .base64()
        .unwrap();
    let debug = format!("{error:?}");
    let written = serde_json::to_string(&error).unwrap();
    for secret in [
        "PRIVATE-USER-TEXT-7731",
        "PRIVATE-IMAGE-BYTES-5521",
        base64.as_str(),
        "private-host",
        "private-path-9912",
        "private_field_3317",
        "private_schema_4410",
    ] {
        for (form, text) in [
            ("message", &error.message),
            ("debug", &debug),
            ("json", &written),
        ] {
            assert!(
                !text.contains(secret),
                "the refusal's {form} carries {secret}"
            );
        }
    }
}

/// Catalog TOML: a declaration that is not a boolean, and a misspelt sibling, are refused rather
/// than read as undeclared; `true` reads back declared and is written, absent is not written.
#[test]
fn a_catalog_declaration_is_a_boolean_or_absent_and_a_misspelt_one_is_refused() {
    for extra in [
        "structured_output = \"true\"",
        "image_input = \"false\"",
        "image_input = 0",
        "image_input = []",
        "image_inputs = false",
        "structured-output = false",
        "image_input = false\nimage_input = true",
    ] {
        let refused = Catalog::parse(&toml([extra, UNDECLARED, UNDECLARED], true))
            .err()
            .unwrap_or_else(|| panic!("`{extra}` was read"));
        assert_eq!(refused.code, ErrorCode::InvalidRequest, "{extra}");
    }
    let declared = catalog([
        "structured_output = true\nimage_input = true",
        UNDECLARED,
        UNDECLARED,
    ]);
    let a = declared.binding(&id("a")).unwrap().capabilities();
    assert_eq!(
        (a.structured_output, a.image_input),
        (Some(true), Some(true))
    );
    let written = serde_json::to_value(declared.explain(&plain(), Some(100)).unwrap()).unwrap();
    assert_eq!(
        written["targets"][0]["capabilities"]["structured_output"],
        true
    );
    assert_eq!(written["targets"][0]["capabilities"]["image_input"], true);
    let undeclared = written["targets"][1]["capabilities"].as_object().unwrap();
    assert!(!undeclared.contains_key("structured_output"));
    assert!(!undeclared.contains_key("image_input"));
    // Declaring `true` is a different document from leaving it out; declaring it changes no
    // selection.
    let none = catalog([UNDECLARED, UNDECLARED, UNDECLARED]);
    assert_ne!(declared.digest(), none.digest());
    let both = with_format(image_in_history());
    assert_eq!(selected(&declared, &both), selected(&none, &both));
}

/// `RouteRejection` names the same variants, in the same order, in the specification and the
/// generated schema, and each is a code `Rejection` whose wire label is its kebab-case name.
#[test]
fn route_rejection_is_the_same_set_in_the_spec_the_schema_and_the_code() {
    let mut lines = SPEC_CATALOG.lines();
    lines
        .find(|line| line.trim() == "- name: llm.catalog.RouteRejection")
        .expect("the spec declares RouteRejection");
    let variants = lines
        .find_map(|line| line.trim().strip_prefix("variants: ["))
        .and_then(|rest| rest.strip_suffix(']'))
        .expect("RouteRejection lists its variants on one line");
    let spec: Vec<&str> = variants.split(',').map(str::trim).collect();
    let schema: serde_json::Value = serde_json::from_str(SCHEMA_ROUTE_REJECTION).unwrap();
    let schema: Vec<&str> = schema["$defs"]["llm.catalog.RouteRejection"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    assert_eq!(spec, schema);
    let code = [
        Rejection::Tools,
        Rejection::ToolChoice,
        Rejection::Temperature,
        Rejection::TopP,
        Rejection::ReasoningEffort,
        Rejection::OutputLimit,
        Rejection::StructuredOutput,
        Rejection::ImageInput,
        Rejection::InputTokensUnknown,
        Rejection::ContextWindow,
        Rejection::OpaqueState,
        Rejection::RefusalText,
        Rejection::FallbackDisabled,
    ];
    let kebab = |name: &str| {
        let mut out = String::new();
        for (index, ch) in name.chars().enumerate() {
            if ch.is_ascii_uppercase() {
                if index > 0 {
                    out.push('-');
                }
                out.push(ch.to_ascii_lowercase());
            } else {
                out.push(ch);
            }
        }
        out
    };
    assert_eq!(
        spec.iter().map(|name| kebab(name)).collect::<Vec<_>>(),
        code.iter()
            .map(|rejection| rejection.label().to_owned())
            .collect::<Vec<_>>()
    );
    for (name, rejection) in spec.iter().zip(code) {
        let read: Rejection = serde_json::from_value(json!(kebab(name))).unwrap();
        assert_eq!(read, rejection, "{name}");
        assert_eq!(serde_json::to_value(rejection).unwrap(), json!(kebab(name)));
    }
}

// --- Fallback over three targets -------------------------------------------------------------

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
    }
}

struct Fake {
    provenance: Provenance,
    capabilities: Capabilities,
    fail: bool,
    calls: AtomicUsize,
}

impl Model for Fake {
    fn provenance(&self) -> &Provenance {
        &self.provenance
    }
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }
    fn turn<'a>(
        &'a self,
        _request: &'a TurnRequest,
        _sink: &'a mut dyn StreamSink,
        _: &'a Cancel,
    ) -> BoxFuture<'a, Result<TurnOutcome, Error>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let mut observation = TurnObservation::new(self.provenance.clone());
            if self.fail {
                return Err(Error::new(ErrorCode::Transport, "scripted failure")
                    .with_dispatch(Dispatch::NotSent));
            }
            observation.final_usage = true;
            Ok(TurnOutcome {
                stop_reason: StopReason::EndTurn,
                items: vec![Item::assistant("done")],
                observation,
            })
        })
    }
}

struct Fleet(BTreeMap<Id, Fake>);

impl Fleet {
    fn new(catalog: &Catalog, failing: &[&str]) -> Self {
        Self(
            ["a", "b", "c"]
                .into_iter()
                .map(|serving| {
                    let binding = catalog.binding(&id(serving)).unwrap();
                    (
                        id(serving),
                        Fake {
                            provenance: binding.provenance().clone(),
                            capabilities: binding.capabilities().clone(),
                            fail: failing.contains(&serving),
                            calls: AtomicUsize::new(0),
                        },
                    )
                })
                .collect(),
        )
    }
    fn calls(&self, serving: &str) -> usize {
        self.0[&id(serving)].calls.load(Ordering::SeqCst)
    }
}

impl Models for Fleet {
    fn model(&self, serving_model_id: &Id) -> Option<&dyn Model> {
        self.0
            .get(serving_model_id)
            .map(|model| model as &dyn Model)
    }
}

fn run(catalog: &Catalog, turn: &TurnRequest, models: &Fleet) -> FallbackRun {
    let cancel = Cancel::new();
    let mut sink = VecSink::new(16, 4096);
    let mut admit = |_: &llm_routing::Selection<'_>| Ok(());
    let ports = Ports {
        models,
        admit: &mut admit,
        sink: &mut sink,
        cancel: &cancel,
        pause: &|_| Box::pin(std::future::ready(())),
    };
    block_on(catalog.run_turn(turn, Some(100), FallbackPolicy::default(), ports)).unwrap()
}

fn attempted(run: &FallbackRun) -> Vec<String> {
    let mut targets: Vec<String> = run
        .attempts
        .iter()
        .map(|attempt| attempt.target_id.as_str().to_owned())
        .collect();
    targets.dedup();
    targets
}

/// After an eligible failure on the first target, fallback passes over the middle target that
/// declares the feature `false` and moves the turn to the last one; for a turn not needing the
/// feature it moves to the middle one.
#[test]
fn fallback_passes_over_a_middle_target_declaring_the_feature_false() {
    for (declared, needing) in [
        ([UNDECLARED, NO_IMAGE, IMAGE], image_in_history()),
        ([FORMAT, NO_FORMAT, UNDECLARED], with_format(plain())),
    ] {
        let catalog = catalog(declared);
        let models = Fleet::new(&catalog, &["a"]);
        let moved = run(&catalog, &needing, &models);
        assert_eq!(moved.halt, Halt::Succeeded, "{declared:?}");
        assert_eq!(attempted(&moved), ["t0", "t2"], "{declared:?}");
        assert_eq!(models.calls("b"), 0, "{declared:?}");

        let models = Fleet::new(&catalog, &["a"]);
        let next = run(&catalog, &plain(), &models);
        assert_eq!(next.halt, Halt::Succeeded, "{declared:?}");
        assert_eq!(attempted(&next), ["t0", "t1"], "{declared:?}");
    }
    // Every alternative declares it false: the run ends on the first target's failure and never
    // calls either alternative.
    let catalog = catalog([UNDECLARED, NO_IMAGE, NO_IMAGE]);
    let models = Fleet::new(&catalog, &["a"]);
    let spent = run(&catalog, &image_in_history(), &models);
    assert_eq!(spent.halt, Halt::Exhausted);
    assert_eq!(attempted(&spent), ["t0"]);
    assert_eq!((models.calls("b"), models.calls("c")), (0, 0));
    assert_eq!(spent.result.unwrap_err().code, ErrorCode::Transport);
}
