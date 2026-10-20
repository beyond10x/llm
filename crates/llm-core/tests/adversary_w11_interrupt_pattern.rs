//! Adversary cases for story:input-into-a-running-turn, driven from `docs/design.md` "Input into a
//! running turn", step 3, and from `website/docs/guides/interrupt-a-turn-with-new-input.md`, "The
//! next request": a turn that finished before the cancel reached it is carried as its outcome's
//! items, then a result for every call it proposed (here each rejected: a failed result carrying
//! the caller's reason), then the new input as `Item::UserText`.
//!
//! Round 0 built the next request without the results, as the documents then said, and the first
//! case was red: `every prior tool call needs a result before the next turn`.
//!
//! Every client checks the request with `TurnRequest::validate_for` before any I/O (Responses
//! `crates/llm-responses/src/client.rs:171`, Messages `crates/llm-messages/src/codec.rs:115`,
//! Chat Completions `crates/llm-chat/src/outgoing.rs:35`), so the request the pattern builds is
//! either the one sent or the one refused here.
use llm_core::{
    CallId, Capabilities, Id, Item, Protocol, Provenance, StopReason, ToolCall, ToolName, ToolSpec,
    TurnObservation, TurnOutcome, TurnRequest,
};
use serde_json::json;

const QUESTION: &str = "What will the weather be like tomorrow?";
const NEW_INPUT: &str = "Sorry, I meant on Sunday.";
const SHOWN: &str = "Let me look that up.";

fn target(protocol: Protocol) -> Provenance {
    Provenance {
        protocol,
        provider: Id::new("local").unwrap(),
        account: Id::new("none").unwrap(),
        endpoint: Id::new("in-process").unwrap(),
        model: Id::new("example").unwrap(),
        binding_revision: Id::new("speaking-v1").unwrap(),
    }
}

fn with_tools() -> Capabilities {
    Capabilities {
        tools: true,
        ..Capabilities::text(8192, 1024)
    }
}

/// The first turn of a caller that offers the model one tool.
fn first_turn() -> TurnRequest {
    let mut first = TurnRequest::new("example", vec![Item::user(QUESTION)]);
    first.tools = vec![ToolSpec {
        name: ToolName::new("forecast").unwrap(),
        description: "The forecast for one day.".to_owned(),
        input_schema: json!({"type": "object", "properties": {"day": {"type": "string"}}}),
    }];
    first
}

/// The reason the caller gives for a call it rejects because new input changed the request.
const NOT_RUN: &str = "Not run: the user changed the question.";

/// The next turn's items exactly as the design and the guide prescribe them: the conversation so
/// far; then the returned outcome's items followed by a rejected result for each call they
/// propose, or, for a cancelled turn, the shown text as one assistant item when anything was
/// shown; then the new input.
fn next_turn(first: &TurnRequest, returned: Option<&TurnOutcome>, shown: &str) -> TurnRequest {
    let mut items = first.items.clone();
    match returned {
        Some(outcome) => {
            items.extend(outcome.items.iter().cloned());
            for call in outcome.items.iter().filter_map(Item::as_tool_call) {
                items.push(Item::ToolResult {
                    call_id: call.call_id.clone(),
                    output: json!(NOT_RUN),
                    failed: true,
                });
            }
        }
        None if !shown.is_empty() => items.push(Item::assistant(shown)),
        None => {}
    }
    items.push(Item::user(NEW_INPUT));
    let mut next = first.clone();
    next.items = items;
    next
}

/// The turn finished, with the text it showed and the call it proposed, before the caller's cancel
/// reached it, so the client returned its outcome. The pattern carries `outcome.items` and appends
/// the new input; on every protocol that request must be one the client sends.
#[test]
fn a_turn_that_ended_in_a_tool_call_before_the_cancel_reached_it_continues_into_a_sendable_turn() {
    for protocol in [
        Protocol::Responses,
        Protocol::Messages,
        Protocol::ChatCompletions,
    ] {
        let target = target(protocol);
        let first = first_turn();
        first
            .validate_for(&target, &with_tools())
            .expect("the first turn is sendable");
        let outcome = TurnOutcome {
            stop_reason: StopReason::ToolCalls,
            items: vec![
                Item::assistant(SHOWN),
                Item::ToolCall(ToolCall {
                    call_id: CallId::new("call-1").unwrap(),
                    name: ToolName::new("forecast").unwrap(),
                    arguments: json!({"day": "tomorrow"}),
                }),
            ],
            observation: TurnObservation {
                final_usage: true,
                ..TurnObservation::new(target.clone())
            },
        };
        outcome
            .validate_for(&first, &target)
            .expect("an outcome a client returns for this request");

        let next = next_turn(&first, Some(&outcome), SHOWN);
        let sent = next.validate_for(&target, &with_tools());
        assert!(
            sent.is_ok(),
            "{protocol:?}: the next turn the documented pattern builds from a turn that ended in a \
             tool call is refused before it is sent: {sent:?}"
        );
    }
}

/// The control: the same caller, the same tool, but the cancel landed first. Only the shown text
/// crosses, and the next turn is sendable on every protocol.
#[test]
fn the_same_turn_cancelled_part_way_continues_into_a_sendable_turn() {
    for protocol in [
        Protocol::Responses,
        Protocol::Messages,
        Protocol::ChatCompletions,
    ] {
        let target = target(protocol);
        let first = first_turn();
        let next = next_turn(&first, None, SHOWN);
        assert_eq!(
            next.items,
            vec![
                Item::user(QUESTION),
                Item::assistant(SHOWN),
                Item::user(NEW_INPUT)
            ]
        );
        next.validate_for(&target, &with_tools())
            .expect("the cancelled branch is sendable");
    }
}
