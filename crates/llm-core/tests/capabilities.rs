//! What a model declares about its output and its input: whether its text streams and whether
//! input reaches a turn that is still producing output. Every client in this workspace streams
//! and accepts no input during output, and a document written before the declarations existed
//! reads, and writes, exactly as it did.

use llm_core::{Capabilities, ErrorCode, Protocol};

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

#[test]
fn a_model_built_in_code_keeps_its_own_claim() {
    let claim = Capabilities::text(32768, 8192)
        .with_output_streaming(false)
        .with_input_during_output(true);
    claim.validate().unwrap();
}
