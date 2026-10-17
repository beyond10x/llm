//! Adversary cases for story:neutral-turn-controls, driven from `spec/domains/inference.yaml`
//! (`budget_tokens` "present exactly when the mode is Enabled"), the `TurnDocument` doc comment
//! ("Unknown envelope versions and fields refuse deserialization") and `docs/design.md`
//! "Neutral additions" ("a `/3` document carrying controls is refused").
use llm_core::{Item, Thinking, TurnDocument, TurnRequest};
use serde_json::{Value, json};

fn document(format: &str, controls: Value) -> Value {
    let mut written = serde_json::to_value(TurnDocument::new(TurnRequest::new(
        "code",
        vec![Item::user("hi")],
    )))
    .unwrap();
    written["format"] = json!(format);
    written["request"]["controls"] = controls;
    written
}

#[test]
fn adaptive_thinking_carrying_a_budget_is_refused() {
    let thinking =
        serde_json::from_value::<Thinking>(json!({"mode": "adaptive", "budget_tokens": 2048}));
    assert!(
        thinking.is_err(),
        "a budget beside adaptive thinking was read as {thinking:?}"
    );
    let read = serde_json::from_value::<TurnDocument>(document(
        "llm.turn/4",
        json!({"thinking": {"mode": "adaptive", "budget_tokens": 2048}}),
    ));
    assert!(read.is_err(), "llm.turn/4 read as {read:?}");
}

#[test]
fn disabled_thinking_carrying_a_budget_or_an_unknown_key_is_refused() {
    for thinking in [
        json!({"mode": "disabled", "budget_tokens": 0}),
        json!({"mode": "disabled", "tone": "dry"}),
    ] {
        let read = serde_json::from_value::<TurnDocument>(document(
            "llm.turn/4",
            json!({"thinking": thinking.clone()}),
        ));
        assert!(read.is_err(), "{thinking} read as {read:?}");
    }
}

#[test]
fn an_llm_turn_3_document_carrying_a_controls_key_is_refused() {
    for controls in [
        json!({}),
        json!({"hints": {}}),
        json!({"thinking": null, "parallel_tool_calls": null}),
    ] {
        let read = serde_json::from_value::<TurnDocument>(document("llm.turn/3", controls.clone()));
        assert!(
            read.is_err(),
            "llm.turn/3 with controls {controls} was read; llm 0.6.0 refuses it as an unknown field"
        );
    }
}
