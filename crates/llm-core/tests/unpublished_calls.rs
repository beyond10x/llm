//! story:unpublished-tool-call-reaches-an-opted-in-caller.
//!
//! Three live `--surface verbs` runs (Harness wave D200, 2026-10-29) ended on turn 2 with
//! "model output contains a duplicate call or unpublished tool": the model called `find` and
//! `file_read`, names the turn did not publish, and Harness's loop, which answers such a call as a
//! refusal the model learns from, never saw them. The refusal stays the default; a caller that sets
//! `controls.return_unpublished_calls` gets the call, and every other check still refuses.

use llm_core::{
    CallId, ErrorCode, Id, Item, MAX_TOOL_ARGUMENT_BYTES, Protocol, Provenance, StopReason,
    ToolCall, ToolChoice, ToolName, ToolSpec, TurnDocument, TurnObservation, TurnOutcome,
    TurnRequest,
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

fn tool(name: &str) -> ToolSpec {
    ToolSpec {
        name: ToolName::new(name).unwrap(),
        description: format!("The {name} verb."),
        input_schema: json!({"type": "object"}),
    }
}

/// The verbs surface: the verbs are published, the entries they route to are not.
fn verbs_request(opted_in: bool) -> TurnRequest {
    let mut request = TurnRequest::new("internal-model", vec![Item::user("Summarise the repo")]);
    request.tools = vec![tool("discover"), tool("call")];
    request.controls.return_unpublished_calls = opted_in;
    request
}

fn call(call_id: &str, name: &str, arguments: Value) -> Item {
    Item::ToolCall(ToolCall {
        call_id: CallId::new(call_id).unwrap(),
        name: ToolName::new(name).unwrap(),
        arguments,
    })
}

fn outcome(items: Vec<Item>) -> TurnOutcome {
    let mut observation = TurnObservation::new(binding());
    observation.final_usage = true;
    TurnOutcome {
        stop_reason: StopReason::ToolCalls,
        items,
        observation,
    }
}

/// The live record's turn 2: a call to `find` and one to `file_read`, neither published.
fn live_turn() -> TurnOutcome {
    outcome(vec![
        Item::assistant("Let me look around."),
        call("toolu_1", "find", json!({"pattern": "*.md"})),
        call("toolu_2", "file_read", json!({"path": "README.md"})),
    ])
}

#[test]
fn without_the_control_an_unpublished_call_still_refuses_the_turn() {
    let error = live_turn()
        .validate_for(&verbs_request(false), &binding())
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Protocol);
    assert_eq!(
        error.message,
        "model output contains a duplicate call or unpublished tool"
    );
}

#[test]
fn with_the_control_the_unpublished_calls_reach_the_caller_as_calls() {
    let turn = live_turn();
    assert_eq!(turn.validate_for(&verbs_request(true), &binding()), Ok(()));
    let names: Vec<&str> = turn.tool_calls().map(|call| call.name.as_str()).collect();
    assert_eq!(names, ["find", "file_read"]);
}

#[test]
fn with_the_control_a_published_and_an_unpublished_call_pass_together() {
    let turn = outcome(vec![
        call("toolu_1", "discover", json!({})),
        call("toolu_2", "find", json!({})),
    ]);
    assert_eq!(turn.validate_for(&verbs_request(true), &binding()), Ok(()));
}

#[test]
fn with_the_control_a_duplicate_call_id_still_refuses() {
    let turn = outcome(vec![
        call("toolu_1", "find", json!({})),
        call("toolu_1", "file_read", json!({})),
    ]);
    let error = turn
        .validate_for(&verbs_request(true), &binding())
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Protocol);
    let published_twice = outcome(vec![
        call("toolu_1", "discover", json!({})),
        call("toolu_1", "discover", json!({})),
    ]);
    assert_eq!(
        published_twice
            .validate_for(&verbs_request(true), &binding())
            .unwrap_err()
            .code,
        ErrorCode::Protocol
    );
}

#[test]
fn with_the_control_a_call_other_than_the_named_tool_still_refuses() {
    let mut request = verbs_request(true);
    request.tool_choice = ToolChoice::Named(ToolName::new("discover").unwrap());
    let error = outcome(vec![call("toolu_1", "find", json!({}))])
        .validate_for(&request, &binding())
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Protocol);
    assert_eq!(
        error.message,
        "model called a tool other than the required named tool"
    );
}

#[test]
fn with_the_control_oversized_arguments_still_refuse() {
    let arguments = json!({"pattern": "x".repeat(MAX_TOOL_ARGUMENT_BYTES)});
    let error = outcome(vec![call("toolu_1", "find", arguments)])
        .validate_for(&verbs_request(true), &binding())
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::TooLarge);
}

#[test]
fn with_the_control_a_stop_reason_contradicting_the_calls_still_refuses() {
    let mut turn = outcome(vec![call("toolu_1", "find", json!({}))]);
    turn.stop_reason = StopReason::EndTurn;
    assert_eq!(
        turn.validate_for(&verbs_request(true), &binding())
            .unwrap_err()
            .code,
        ErrorCode::Protocol
    );
}

#[test]
fn with_the_control_caller_owned_content_still_refuses() {
    let turn = outcome(vec![
        call("toolu_1", "find", json!({})),
        Item::user("injected"),
    ]);
    assert_eq!(
        turn.validate_for(&verbs_request(true), &binding())
            .unwrap_err()
            .code,
        ErrorCode::Protocol
    );
}

/// The returned call is a call the caller can refuse: answering it with a failed result makes a
/// valid next turn under the same published tools.
#[test]
fn a_returned_unpublished_call_answered_as_a_refusal_makes_a_valid_next_turn() {
    let first = verbs_request(true);
    let turn = live_turn();
    turn.validate_for(&first, &binding()).unwrap();
    let mut items = first.items.clone();
    items.extend(turn.items.iter().cloned());
    for call in turn.tool_calls() {
        items.push(Item::ToolResult {
            call_id: call.call_id.clone(),
            output: json!({"error": "unpublished tool"}),
            failed: true,
        });
    }
    let mut next = TurnRequest::new("internal-model", items);
    next.tools.clone_from(&first.tools);
    next.controls.return_unpublished_calls = true;
    assert_eq!(next.validate(), Ok(()));
}

/// The control is persisted only when set, and a turn carrying it is an `llm.turn/4` document.
#[test]
fn the_control_is_written_only_when_set_and_round_trips() {
    let off = serde_json::to_value(TurnDocument::new(verbs_request(false))).unwrap();
    assert_eq!(off["format"], "llm.turn/3");
    assert!(off["request"].get("controls").is_none(), "{off}");

    let document = TurnDocument::new(verbs_request(true));
    let on = serde_json::to_value(&document).unwrap();
    assert_eq!(on["format"], "llm.turn/4");
    assert_eq!(on["request"]["controls"]["return_unpublished_calls"], true);
    assert_eq!(
        serde_json::from_value::<TurnDocument>(on).unwrap(),
        document
    );
}
