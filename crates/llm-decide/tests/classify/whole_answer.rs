//! What a model classifier takes as its answer. On the JSON-schema path: the one assistant text
//! of a turn that ended normally, read as one JSON value that gives no key twice. On the
//! forced-tool path: the one call of a turn that stopped for its tool call, with no refusal
//! beside it. Every other outcome the routed call hands over abstains `OffLabel`, with the
//! call's record written as succeeded.

use super::{Fake, Fleet, INPUT, Ran, Reply, Scripted, answer, catalog, chain, id, run};
use llm_calls::Outcome;
use llm_core::{CallId, ErrorCode, Item, StopReason, ToolCall, ToolName};
use llm_decide::{AbstentionCause, Classification, ModelClassifier, TOOL_NAME};
use serde_json::{Value, json};

/// One chain of the one classifier `m`, whose route reaches `serving`, which returns `items`
/// under `stop_reason`.
fn ran(
    serving: &'static str,
    classifier: ModelClassifier,
    stop_reason: StopReason,
    items: Vec<Item>,
) -> Ran {
    let catalog = catalog();
    let clock = Scripted::at(1_000);
    let reply = Reply::Outcome(stop_reason, items);
    let fleet = Fleet::of(
        &catalog,
        &clock,
        [(serving, Fake::on(&catalog, serving, &clock, reply))],
    );
    let chain = chain(vec![Box::new(classifier)]);
    run(&chain, &catalog, &fleet, &clock, INPUT, None)
}

/// A JSON-schema classifier: the route's first target, `local-small`, answers.
fn by_schema(stop_reason: StopReason, items: Vec<Item>) -> Ran {
    let classifier = ModelClassifier::json_schema(id("m"), "code");
    ran("local-small", classifier, stop_reason, items)
}

/// A forced-tool classifier: only `remote-large` takes tools.
fn by_tool(stop_reason: StopReason, items: Vec<Item>) -> Ran {
    let classifier = ModelClassifier::forced_tool(id("m"), "code");
    ran("remote-large", classifier, stop_reason, items)
}

/// One text item under a normal end, on the JSON-schema path.
fn by_schema_text(text: &str) -> Ran {
    by_schema(StopReason::EndTurn, vec![Item::assistant(text)])
}

fn incomplete() -> StopReason {
    StopReason::Incomplete {
        reason: "content_filter".into(),
    }
}

#[track_caller]
fn assert_decides(ran: &Ran, label: &str, name: &str) {
    assert_eq!(
        ran.outcome
            .decision()
            .map(|decision| decision.label.as_str()),
        Some(label),
        "{name}: {:?}",
        ran.outcome
    );
}

/// The classifier abstained `OffLabel`, and the call it made succeeded and is recorded.
#[track_caller]
fn assert_off_label(ran: &Ran, name: &str) {
    let Some(Classification::Abstained(abstention)) = ran.outcome.result() else {
        panic!("{name}: taken as a decision: {:?}", ran.outcome);
    };
    assert_eq!(abstention.cause, AbstentionCause::OffLabel, "{name}");
    assert_eq!(abstention.call_record_id, Some(id("req-1/m")), "{name}");
    assert_eq!(ran.records.len(), 1, "{name}");
    assert_eq!(ran.records[0].outcome, Outcome::Succeeded, "{name}");
}

/// The routed call refused the outcome before the classifier read it: `ProviderError` with
/// `protocol`, and the call's record written as failed.
#[track_caller]
fn assert_refused_by_the_call(ran: &Ran, name: &str) {
    let Some(Classification::Abstained(abstention)) = ran.outcome.result() else {
        panic!("{name}: taken as a decision: {:?}", ran.outcome);
    };
    assert_eq!(
        abstention.cause,
        AbstentionCause::ProviderError(ErrorCode::Protocol),
        "{name}"
    );
    assert_eq!(ran.records.len(), 1, "{name}");
    assert_eq!(ran.records[0].outcome, Outcome::Failed, "{name}");
}

/// A whole answer naming `positive`.
fn positive() -> Value {
    answer("positive", 9100, 300, 600)
}

/// A whole answer naming `negative`.
fn negative() -> Value {
    answer("negative", 200, 9700, 100)
}

/// State the provider keeps beside its answer (reasoning arrives as such an item), bound to
/// `serving`.
fn state(serving: &str) -> Item {
    Item::Opaque {
        provenance: catalog()
            .binding(&id(serving))
            .unwrap()
            .provenance()
            .clone(),
        payload: json!({"reasoning": "kept by the provider"}),
    }
}

fn refusal() -> Item {
    Item::AssistantRefusal {
        text: "I cannot classify this.".into(),
    }
}

/// A call `call_id` to the tool `name`.
fn call(call_id: &str, name: &str, arguments: Value) -> Item {
    Item::ToolCall(ToolCall {
        call_id: CallId::new(call_id).unwrap(),
        name: ToolName::new(name).unwrap(),
        arguments,
    })
}

/// The call `call-1` to the classifier's forced tool.
fn classify(arguments: Value) -> Item {
    call("call-1", TOOL_NAME, arguments)
}

#[test]
fn an_answer_that_repeats_label_abstains_off_label() {
    const CONFIDENCES: &str = r#""confidences":{"positive":9100,"negative":300,"neutral":600}"#;
    // Given once, the same members decide.
    let once = format!(r#"{{"label":"positive",{CONFIDENCES}}}"#);
    assert_decides(&by_schema_text(&once), "positive", "label given once");
    for (name, text) in [
        (
            "two labels of the set, the last one first in the set",
            format!(r#"{{"label":"negative","label":"positive",{CONFIDENCES}}}"#),
        ),
        (
            "the same label twice",
            format!(r#"{{"label":"positive","label":"positive",{CONFIDENCES}}}"#),
        ),
        (
            "the second label after the confidences",
            format!(r#"{{"label":"negative",{CONFIDENCES},"label":"positive"}}"#),
        ),
        (
            "the second key written with an escape",
            format!(r#"{{"label":"negative","label":"positive",{CONFIDENCES}}}"#),
        ),
    ] {
        assert_off_label(&by_schema_text(&text), name);
    }
}

#[test]
fn an_answer_that_repeats_confidences_abstains_off_label() {
    let text = concat!(
        r#"{"label":"positive","#,
        r#""confidences":{"positive":100,"negative":9000,"neutral":900},"#,
        r#""confidences":{"positive":9100,"negative":300,"neutral":600}}"#
    );
    assert_off_label(&by_schema_text(text), "confidences given twice");
}

#[test]
fn an_answer_that_repeats_a_key_inside_confidences_abstains_off_label() {
    for (name, confidences) in [
        (
            "the named label's confidence twice",
            r#"{"positive":100,"positive":9100,"negative":300,"neutral":600}"#,
        ),
        (
            "another label's confidence twice",
            r#"{"positive":9100,"negative":300,"neutral":600,"negative":400}"#,
        ),
    ] {
        let text = format!(r#"{{"label":"positive","confidences":{confidences}}}"#);
        assert_off_label(&by_schema_text(&text), name);
    }
}

#[test]
fn an_answer_cut_at_the_output_limit_abstains_off_label() {
    // What arrived would decode as a decision; the turn says it is not all of the answer.
    let text = Item::assistant(positive().to_string());
    assert_decides(
        &by_schema(StopReason::EndTurn, vec![text.clone()]),
        "positive",
        "the same text of a turn that ended normally",
    );
    let cut = by_schema(StopReason::MaxOutputTokens, vec![text]);
    assert_off_label(&cut, "cut at the output limit");
}

#[test]
fn an_answer_of_a_turn_reported_incomplete_abstains_off_label() {
    let text = Item::assistant(negative().to_string());
    assert_decides(
        &by_schema(StopReason::EndTurn, vec![text.clone()]),
        "negative",
        "the same text of a turn that ended normally",
    );
    assert_off_label(&by_schema(incomplete(), vec![text]), "reported incomplete");
}

#[test]
fn a_text_that_is_more_than_the_one_json_value_abstains_off_label() {
    let whole = negative().to_string();
    let spaced = format!(" \n\t{whole}\r\n ");
    assert_decides(
        &by_schema_text(&spaced),
        "negative",
        "JSON whitespace around",
    );
    for (name, text) in [
        ("prose after the value", format!("{whole} I am sure.")),
        ("prose before the value", format!("Answer: {whole}")),
        (
            "a code fence around the value",
            format!("```json\n{whole}\n```"),
        ),
        ("the value twice", format!("{whole}{whole}")),
        ("a second value after it", format!("{whole} {}", positive())),
    ] {
        assert_off_label(&by_schema_text(&text), name);
    }
}

#[test]
fn two_text_items_that_are_an_answer_only_when_joined_abstain_off_label() {
    let whole = positive().to_string();
    let (head, tail) = whole.split_at(whole.len() / 2);
    for (name, texts) in [
        ("one answer split in the middle", [head, tail]),
        (
            "a whole answer and then an empty text",
            [whole.as_str(), ""],
        ),
        (
            "an empty text and then a whole answer",
            ["", whole.as_str()],
        ),
        (
            "a whole answer and then a text of whitespace",
            [whole.as_str(), " \n"],
        ),
        (
            "a text of whitespace and then a whole answer",
            [" \n", whole.as_str()],
        ),
    ] {
        let items = texts.into_iter().map(Item::assistant).collect();
        assert_off_label(&by_schema(StopReason::EndTurn, items), name);
    }
}

#[test]
fn two_text_items_that_are_each_a_whole_answer_abstain_off_label() {
    for (name, first, second) in [
        ("two labels, the first item's first", positive(), negative()),
        (
            "two labels, the second item's first",
            negative(),
            positive(),
        ),
        (
            "one label under two confidences",
            positive(),
            answer("positive", 8000, 1000, 1000),
        ),
    ] {
        let items = vec![
            Item::assistant(first.to_string()),
            Item::assistant(second.to_string()),
        ];
        assert_off_label(&by_schema(StopReason::EndTurn, items), name);
    }
}

#[test]
fn a_refusal_beside_the_text_abstains_off_label() {
    let text = Item::assistant(positive().to_string());
    for (name, items) in [
        ("a refusal after the text", vec![text.clone(), refusal()]),
        ("a refusal before the text", vec![refusal(), text]),
        ("a refusal and no text", vec![refusal()]),
    ] {
        assert_off_label(&by_schema(StopReason::EndTurn, items), name);
    }
}

#[test]
fn an_outcome_without_a_text_item_abstains_off_label() {
    for (name, items) in [
        ("no item", vec![]),
        ("provider state alone", vec![state("local-small")]),
    ] {
        assert_off_label(&by_schema(StopReason::EndTurn, items), name);
    }
}

#[test]
fn provider_state_beside_the_one_text_is_not_part_of_the_answer() {
    let text = Item::assistant(negative().to_string());
    for (name, items) in [
        ("state before", vec![state("local-small"), text.clone()]),
        ("state after", vec![text.clone(), state("local-small")]),
        (
            "state on both sides",
            vec![state("local-small"), text, state("local-small")],
        ),
    ] {
        assert_decides(&by_schema(StopReason::EndTurn, items), "negative", name);
    }
}

#[test]
fn a_call_of_a_turn_cut_at_the_output_limit_abstains_off_label() {
    // The call is complete and its arguments would decode as a decision.
    assert_decides(
        &by_tool(StopReason::ToolCalls, vec![classify(negative())]),
        "negative",
        "the same call of a turn that stopped for it",
    );
    let cut = by_tool(StopReason::MaxOutputTokens, vec![classify(negative())]);
    assert_off_label(&cut, "cut at the output limit");
}

#[test]
fn a_call_of_a_turn_reported_incomplete_abstains_off_label() {
    assert_decides(
        &by_tool(StopReason::ToolCalls, vec![classify(positive())]),
        "positive",
        "the same call of a turn that stopped for it",
    );
    let left = by_tool(incomplete(), vec![classify(positive())]);
    assert_off_label(&left, "reported incomplete");
}

#[test]
fn two_calls_to_the_forced_tool_abstain_off_label() {
    let off_set = || answer("furious", 9000, 500, 500);
    for (name, first, second) in [
        ("two labels of the set", positive(), negative()),
        ("only the first call names a label", positive(), off_set()),
        ("only the second call names a label", off_set(), negative()),
        (
            "one label under two confidences",
            negative(),
            answer("negative", 1000, 8000, 1000),
        ),
    ] {
        let items = vec![
            call("call-1", TOOL_NAME, first),
            call("call-2", TOOL_NAME, second),
        ];
        assert_off_label(&by_tool(StopReason::ToolCalls, items), name);
    }
}

#[test]
fn a_refusal_beside_the_one_call_abstains_off_label() {
    for (name, items) in [
        (
            "a refusal before the call",
            vec![refusal(), classify(positive())],
        ),
        (
            "a refusal after the call",
            vec![classify(negative()), refusal()],
        ),
    ] {
        assert_off_label(&by_tool(StopReason::ToolCalls, items), name);
    }
}

#[test]
fn assistant_text_beside_the_one_call_is_not_part_of_the_answer() {
    let said = || Item::assistant("The review reads as praise.");
    // Text that is itself a whole answer naming another label changes nothing: the call decides.
    let other = || Item::assistant(negative().to_string());
    for (name, items) in [
        ("prose before the call", vec![said(), classify(positive())]),
        ("prose after the call", vec![classify(positive()), said()]),
        (
            "another whole answer as text before the call",
            vec![other(), classify(positive())],
        ),
    ] {
        assert_decides(&by_tool(StopReason::ToolCalls, items), "positive", name);
    }
}

#[test]
fn provider_state_beside_the_one_call_is_not_part_of_the_answer() {
    for (name, items) in [
        (
            "state before the call",
            vec![state("remote-large"), classify(negative())],
        ),
        (
            "state after the call",
            vec![classify(negative()), state("remote-large")],
        ),
    ] {
        assert_decides(&by_tool(StopReason::ToolCalls, items), "negative", name);
    }
}

/// These outcomes never reach the classifier's reading: the routed call holds every outcome to
/// the turn it sent (`TurnOutcome::validate_for`) and fails the ones that break it.
#[test]
fn a_forced_tool_outcome_the_routed_call_refuses_abstains_with_protocol() {
    for (name, stop_reason, items) in [
        (
            "a call under another tool's name",
            StopReason::ToolCalls,
            vec![call("call-1", "label", positive())],
        ),
        (
            "a call under another tool's name beside the forced tool's",
            StopReason::ToolCalls,
            vec![classify(positive()), call("call-2", "label", negative())],
        ),
        (
            "a call of a turn that says it ended normally",
            StopReason::EndTurn,
            vec![classify(positive())],
        ),
        (
            "a stop for a tool call with no call",
            StopReason::ToolCalls,
            vec![Item::assistant(positive().to_string())],
        ),
        (
            "two calls under one call id",
            StopReason::ToolCalls,
            vec![classify(positive()), classify(negative())],
        ),
    ] {
        assert_refused_by_the_call(&by_tool(stop_reason, items), name);
    }
}
