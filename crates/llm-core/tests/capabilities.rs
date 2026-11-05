//! What a model declares about its output and its input: whether its text streams and whether
//! input reaches a turn that is still producing output. Every client in this workspace streams
//! and accepts no input during output, and a document written before the declarations existed
//! reads, and writes, exactly as it did. A model may also declare whether it takes structured
//! output and image input; a document that declares neither leaves both undeclared and keeps
//! its bytes.

use llm_core::{
    Capabilities, ErrorCode, Id, ImageMediaType, ImageSource, Item, OutputFormat, Protocol,
    Provenance, TurnRequest,
};
use serde_json::json;

const PROTOCOLS: [Protocol; 3] = [
    Protocol::Responses,
    Protocol::Messages,
    Protocol::ChatCompletions,
];

/// A capabilities object as an `llm.binding/1` or `llm.catalog/1` document wrote it at 0.7.0.
const BEFORE: &str = r#"{"tools":false,"tool_choice":false,"temperature":false,"top_p":false,"reasoning_efforts":[],"context_window":32768,"max_output_tokens":8192}"#;

#[test]
fn a_text_declaration_streams_its_output_and_takes_no_input_during_output() {
    let text = Capabilities::text(32768, 8192);
    assert!(text.output_streaming);
    assert!(!text.input_during_output);
}

#[test]
fn a_declaration_written_before_the_two_fields_reads_as_the_clients_declare_and_writes_unchanged() {
    let read: Capabilities = serde_json::from_str(BEFORE).unwrap();
    assert!(read.output_streaming);
    assert!(!read.input_during_output);
    assert_eq!(read, Capabilities::text(32768, 8192));
    assert_eq!(serde_json::to_string(&read).unwrap(), BEFORE);
}

#[test]
fn a_default_written_out_is_left_out_again_and_only_a_departure_is_written() {
    let explicit = BEFORE.replace(
        "\"max_output_tokens\":8192}",
        "\"max_output_tokens\":8192,\"output_streaming\":true,\"input_during_output\":false}",
    );
    let read: Capabilities = serde_json::from_str(&explicit).unwrap();
    assert_eq!(serde_json::to_string(&read).unwrap(), BEFORE);

    let departing = Capabilities::text(32768, 8192)
        .with_output_streaming(false)
        .with_input_during_output(true);
    assert!(!departing.output_streaming);
    assert!(departing.input_during_output);
    let written = serde_json::to_value(&departing).unwrap();
    assert_eq!(written["output_streaming"], false);
    assert_eq!(written["input_during_output"], true);
    let back: Capabilities = serde_json::from_value(written).unwrap();
    assert_eq!(back, departing);
}

#[test]
fn a_declaration_that_is_not_a_boolean_is_refused() {
    for (field, value) in [
        ("output_streaming", "null"),
        ("output_streaming", "\"yes\""),
        ("input_during_output", "1"),
    ] {
        let invalid = BEFORE.replace(
            "\"max_output_tokens\":8192}",
            &format!("\"max_output_tokens\":8192,\"{field}\":{value}}}"),
        );
        assert!(
            serde_json::from_str::<Capabilities>(&invalid).is_err(),
            "{field}: {value} was read"
        );
    }
}

#[test]
fn no_protocol_in_this_workspace_carries_input_during_output() {
    let claim = Capabilities::text(32768, 8192).with_input_during_output(true);
    for protocol in PROTOCOLS {
        let error = claim.validate_protocol(protocol).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert_eq!(
            error.message,
            "no protocol in this workspace carries input during output"
        );
    }
}

#[test]
fn every_protocol_in_this_workspace_streams_its_output() {
    let claim = Capabilities::text(32768, 8192).with_output_streaming(false);
    for protocol in PROTOCOLS {
        let error = claim.validate_protocol(protocol).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert_eq!(
            error.message,
            "every protocol in this workspace streams its output"
        );
    }
}

#[test]
fn the_declarations_every_client_makes_are_accepted_on_every_protocol() {
    for protocol in PROTOCOLS {
        Capabilities::text(32768, 8192)
            .validate_protocol(protocol)
            .unwrap();
    }
}

/// Every setting a document can declare, written as `llm.catalog/1` writes it at 0.21.0.
const EVERY_SETTING: &str = r#"{"tools":true,"tool_choice":true,"temperature":true,"top_p":true,"reasoning_efforts":["low","high"],"context_window":200000,"max_output_tokens":64000}"#;

#[test]
fn a_caller_outside_the_crate_builds_every_declaration_and_its_bytes_do_not_move() {
    let built = Capabilities::text(200_000, 64_000)
        .with_tools(true)
        .with_tool_choice(true)
        .with_temperature(true)
        .with_top_p(true)
        .with_reasoning_efforts(["low", "high"]);
    assert!(built.tools && built.tool_choice && built.temperature && built.top_p);
    assert_eq!(built.reasoning_efforts, ["low", "high"]);
    assert_eq!(serde_json::to_string(&built).unwrap(), EVERY_SETTING);
    let read: Capabilities = serde_json::from_str(EVERY_SETTING).unwrap();
    assert_eq!(read, built);

    let withdrawn = built
        .with_tools(false)
        .with_tool_choice(false)
        .with_temperature(false)
        .with_top_p(false)
        .with_reasoning_efforts(Vec::<String>::new());
    assert_eq!(withdrawn, Capabilities::text(200_000, 64_000));
    assert_eq!(
        serde_json::to_string(&withdrawn).unwrap(),
        BEFORE.replace("32768", "200000").replace("8192", "64000")
    );
}

#[test]
fn a_model_built_in_code_keeps_its_own_claim() {
    let claim = Capabilities::text(32768, 8192)
        .with_output_streaming(false)
        .with_input_during_output(true);
    claim.validate().unwrap();
}

#[test]
fn a_declaration_written_before_structured_output_and_image_input_declares_neither_and_keeps_its_bytes()
 {
    for before in [BEFORE, EVERY_SETTING] {
        let read: Capabilities = serde_json::from_str(before).unwrap();
        assert_eq!(read.structured_output, None);
        assert_eq!(read.image_input, None);
        assert_eq!(serde_json::to_string(&read).unwrap(), before);
    }
    let text = Capabilities::text(32768, 8192);
    assert_eq!(text.structured_output, None);
    assert_eq!(text.image_input, None);
}

#[test]
fn each_feature_declaration_is_written_only_when_declared_and_reads_back() {
    for (structured_output, image_input) in [(true, false), (false, true)] {
        let declared = Capabilities::text(32768, 8192)
            .with_structured_output(structured_output)
            .with_image_input(image_input);
        assert_eq!(declared.structured_output, Some(structured_output));
        assert_eq!(declared.image_input, Some(image_input));
        let written = serde_json::to_string(&declared).unwrap();
        assert_eq!(
            written,
            BEFORE.replace(
                "\"max_output_tokens\":8192}",
                &format!(
                    "\"max_output_tokens\":8192,\"structured_output\":{structured_output},\"image_input\":{image_input}}}"
                ),
            )
        );
        let back: Capabilities = serde_json::from_str(&written).unwrap();
        assert_eq!(back, declared);
    }
}

#[test]
fn a_feature_declaration_that_is_not_a_boolean_is_refused() {
    for (field, value) in [
        ("structured_output", "null"),
        ("structured_output", "\"yes\""),
        ("image_input", "null"),
        ("image_input", "0"),
    ] {
        let invalid = BEFORE.replace(
            "\"max_output_tokens\":8192}",
            &format!("\"max_output_tokens\":8192,\"{field}\":{value}}}"),
        );
        assert!(
            serde_json::from_str::<Capabilities>(&invalid).is_err(),
            "{field}: {value} was read"
        );
    }
}

fn target() -> Provenance {
    Provenance {
        protocol: Protocol::Responses,
        provider: Id::new("local").unwrap(),
        account: Id::new("none").unwrap(),
        endpoint: Id::new("in-process").unwrap(),
        model: Id::new("example").unwrap(),
        binding_revision: Id::new("declared-v1").unwrap(),
    }
}

fn asking_for_structured_output() -> TurnRequest {
    let mut request = TurnRequest::new("example", vec![Item::user("Where?")]);
    request.controls.output_format =
        Some(OutputFormat::json_schema(json!({"type": "object"})).with_name("place"));
    request
}

fn carrying_an_image() -> TurnRequest {
    TurnRequest::new(
        "example",
        vec![
            Item::user("What is this?"),
            Item::user_image(ImageSource::bytes(ImageMediaType::Png, b"\x89PNG".to_vec())),
        ],
    )
}

#[test]
fn a_binding_declaring_no_structured_output_refuses_a_turn_asking_for_it_before_io() {
    let declared = Capabilities::text(32768, 8192).with_structured_output(false);
    let error = asking_for_structured_output()
        .validate_for(&target(), &declared)
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(error.message, "binding does not take structured output");
    // The same binding takes a turn that asks for no format, and an image is not its concern.
    TurnRequest::new("example", vec![Item::user("Where?")])
        .validate_for(&target(), &declared)
        .unwrap();
    carrying_an_image()
        .validate_for(&target(), &declared)
        .unwrap();
}

#[test]
fn a_binding_declaring_no_image_input_refuses_a_turn_carrying_one_before_io() {
    let declared = Capabilities::text(32768, 8192).with_image_input(false);
    let error = carrying_an_image()
        .validate_for(&target(), &declared)
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(error.message, "binding does not take image input");
    TurnRequest::new("example", vec![Item::user("What is this?")])
        .validate_for(&target(), &declared)
        .unwrap();
    asking_for_structured_output()
        .validate_for(&target(), &declared)
        .unwrap();
}

#[test]
fn a_binding_that_declares_a_feature_or_leaves_it_undeclared_takes_the_turn() {
    for declared in [
        Capabilities::text(32768, 8192),
        Capabilities::text(32768, 8192)
            .with_structured_output(true)
            .with_image_input(true),
    ] {
        asking_for_structured_output()
            .validate_for(&target(), &declared)
            .unwrap();
        carrying_an_image()
            .validate_for(&target(), &declared)
            .unwrap();
    }
}
