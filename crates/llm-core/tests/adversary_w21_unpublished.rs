//! Adversary, wave 2026-10-29-w21, story:unpublished-tool-call-reaches-an-opted-in-caller.
//!
//! Probes the checks the story says the control leaves standing and the unit's tests do not
//! name (foreign and unattributed opaque state, terminal evidence, a tool-calls stop with no
//! call, the item bound), and the `llm.turn/3`/`/4` envelope at the control's edges.

use llm_core::{
    CallId, Controls, ErrorCode, Id, Item, MAX_ITEMS, Protocol, Provenance, StopReason, ToolCall,
    ToolChoice, ToolName, ToolSpec, TurnDocument, TurnObservation, TurnOutcome, TurnRequest,
};
use serde_json::{Value, json};

fn binding() -> Provenance {
    Provenance {
        protocol: Protocol::Messages,
        provider: Id::new("lab").unwrap(),
        account: Id::new("account").unwrap(),
        endpoint: Id::new("endpoint").unwrap(),
        model: Id::new("internal-model").unwrap(),
        binding_revision: Id::new("binding-r1").unwrap(),
    }
}

fn other_account() -> Provenance {
    let mut other = binding();
    other.account = Id::new("other-account").unwrap();
    other
}

fn tool(name: &str) -> ToolSpec {
    ToolSpec {
        name: ToolName::new(name).unwrap(),
        description: format!("The {name} verb."),
        input_schema: json!({"type": "object"}),
    }
}

fn opted_in() -> TurnRequest {
    let mut request = TurnRequest::new("internal-model", vec![Item::user("Summarise the repo")]);
    request.tools = vec![tool("discover"), tool("call")];
    request.controls.return_unpublished_calls = true;
    request
}

fn find() -> Item {
    Item::ToolCall(ToolCall {
        call_id: CallId::new("toolu_1").unwrap(),
        name: ToolName::new("find").unwrap(),
        arguments: json!({"pattern": "*.md"}),
    })
}

fn outcome(stop_reason: StopReason, items: Vec<Item>) -> TurnOutcome {
    let mut observation = TurnObservation::new(binding());
    observation.final_usage = true;
    TurnOutcome {
        stop_reason,
        items,
        observation,
    }
}

fn refused(turn: &TurnOutcome) -> (ErrorCode, String) {
    let error = turn
        .validate_for(&opted_in(), &binding())
        .expect_err("still refused under the control");
    (error.code, error.message)
}

#[test]
fn with_the_control_foreign_opaque_state_beside_an_unpublished_call_still_refuses() {
    let turn = outcome(
        StopReason::ToolCalls,
        vec![
            Item::Opaque {
                provenance: other_account(),
                payload: json!({"type": "thinking", "thinking": "x", "signature": "s"}),
            },
            find(),
        ],
    );
    assert_eq!(
        refused(&turn),
        (
            ErrorCode::Protocol,
            "model output carries foreign opaque state".to_owned()
        )
    );
}

#[test]
fn with_the_control_unattributed_opaque_state_still_refuses() {
    let turn = outcome(
        StopReason::ToolCalls,
        vec![
            find(),
            Item::UnattributedOpaque {
                protocol: Protocol::Messages,
                payload: json!({"type": "thinking"}),
            },
        ],
    );
    assert_eq!(
        refused(&turn),
        (
            ErrorCode::Protocol,
            "model output carries unattributed opaque state".to_owned()
        )
    );
}

#[test]
fn with_the_control_missing_terminal_evidence_still_refuses() {
    let mut turn = outcome(StopReason::ToolCalls, vec![find()]);
    turn.observation.final_usage = false;
    assert_eq!(
        refused(&turn),
        (
            ErrorCode::Protocol,
            "successful outcome lacks terminal evidence".to_owned()
        )
    );
}

#[test]
fn with_the_control_evidence_from_another_binding_still_refuses() {
    let mut turn = outcome(StopReason::ToolCalls, vec![find()]);
    turn.observation.binding = other_account();
    assert_eq!(refused(&turn).0, ErrorCode::Protocol);
}

#[test]
fn with_the_control_a_tool_calls_stop_without_any_call_still_refuses() {
    let turn = outcome(StopReason::ToolCalls, vec![Item::assistant("no call")]);
    assert_eq!(
        refused(&turn),
        (
            ErrorCode::Protocol,
            "model terminal reason contradicts its tool obligations".to_owned()
        )
    );
}

#[test]
fn with_the_control_an_end_turn_under_a_required_choice_still_refuses() {
    let mut request = opted_in();
    request.tool_choice = ToolChoice::Required;
    let turn = outcome(StopReason::EndTurn, vec![Item::assistant("prose")]);
    assert_eq!(
        turn.validate_for(&request, &binding()).unwrap_err().message,
        "model terminal reason contradicts its tool obligations"
    );
}

#[test]
fn with_the_control_the_item_bound_still_refuses() {
    let items = (0..=MAX_ITEMS)
        .map(|n| {
            Item::ToolCall(ToolCall {
                call_id: CallId::new(format!("toolu_{n}")).unwrap(),
                name: ToolName::new("find").unwrap(),
                arguments: json!({}),
            })
        })
        .collect();
    assert_eq!(
        refused(&outcome(StopReason::ToolCalls, items)).0,
        ErrorCode::TooLarge
    );
}

#[test]
fn with_the_control_a_second_unpublished_call_with_the_first_id_refuses_after_a_published_one() {
    let turn = outcome(
        StopReason::ToolCalls,
        vec![
            find(),
            Item::ToolCall(ToolCall {
                call_id: CallId::new("toolu_2").unwrap(),
                name: ToolName::new("discover").unwrap(),
                arguments: json!({}),
            }),
            Item::ToolCall(ToolCall {
                call_id: CallId::new("toolu_1").unwrap(),
                name: ToolName::new("file_read").unwrap(),
                arguments: json!({}),
            }),
        ],
    );
    assert_eq!(
        refused(&turn),
        (
            ErrorCode::Protocol,
            "model output contains a duplicate call or unpublished tool".to_owned()
        )
    );
}

/// The control alone makes the controls non-empty, so a turn carrying only it is `/4`.
#[test]
fn a_turn_whose_only_control_is_the_flag_is_llm_turn_4_and_reads_back() {
    let mut request = TurnRequest::new("internal-model", vec![Item::user("hi")]);
    request.controls.return_unpublished_calls = true;
    assert!(!request.controls.is_empty());
    let written = serde_json::to_value(TurnDocument::new(request.clone())).unwrap();
    assert_eq!(
        written,
        json!({"format": "llm.turn/4", "request": {
            "model": "internal-model", "instructions": "",
            "items": [{"kind": "user-text", "text": "hi"}], "tools": [],
            "controls": {"return_unpublished_calls": true}}})
    );
    let read: TurnDocument = serde_json::from_value(written).unwrap();
    assert_eq!(read.request, request);
}

fn document(format: &str, controls: &Value) -> Value {
    json!({"format": format, "request": {
        "model": "internal-model", "instructions": "",
        "items": [{"kind": "user-text", "text": "hi"}], "tools": [],
        "controls": controls}})
}

/// The flag set under `/3` is a `/4` document mislabelled: refused, never read as `/3`.
#[test]
fn the_flag_under_llm_turn_3_is_refused() {
    let error = serde_json::from_value::<TurnDocument>(document(
        "llm.turn/3",
        &json!({"return_unpublished_calls": true}),
    ))
    .unwrap_err();
    assert!(error.to_string().contains("version disagrees"), "{error}");
}

/// An explicit `false` is empty controls, as an explicit `{}` already is: neither version
/// reads it, because no writer produces it. Pinned so a change to that is a decision.
#[test]
fn an_explicit_false_is_empty_controls_and_neither_version_reads_it() {
    let controls: Controls =
        serde_json::from_value(json!({"return_unpublished_calls": false})).unwrap();
    assert!(controls.is_empty());
    for format in ["llm.turn/3", "llm.turn/4"] {
        let error = serde_json::from_value::<TurnDocument>(document(
            format,
            &json!({"return_unpublished_calls": false}),
        ))
        .unwrap_err();
        assert!(
            error.to_string().contains("version disagrees"),
            "{format}: {error}"
        );
    }
}

/// A `null` is not a value of the control.
#[test]
fn a_null_flag_is_refused() {
    assert!(serde_json::from_value::<Controls>(json!({"return_unpublished_calls": null})).is_err());
}
