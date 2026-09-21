//! Adversarial cases for the Responses projection.
//!
//! Every case here asserts something `docs/responses.md`, the story's Acceptance statement or
//! `/home/timo/.cache/llm-wave-1/invariants.md` already claims, against a fixture the authored
//! suite does not build. They were written to fail; each one names the claim it drives.

use llm_core::{
    Dispatch, ErrorCode, Id, Item, Protocol, Provenance, Sampling, StopReason, StreamEvent,
    ToolChoice, ToolName, ToolSpec, TurnRequest,
};
use llm_responses::{Binding, decode_stream, ingest_request, project_request};
use serde_json::{Value, json};

fn id(value: &str) -> Id {
    Id::new(value).expect("fixture identifier")
}

fn provenance() -> Provenance {
    Provenance {
        protocol: Protocol::Responses,
        provider: id("my-lab"),
        account: id("local"),
        endpoint: id("local-vllm"),
        model: id("small"),
        binding_revision: id("rev-1"),
    }
}

fn binding() -> Binding {
    Binding::new(provenance(), id("example/Small-Model"))
}

/// The same endpoint after a repoint: every display identifier persists, the revision does not.
fn repointed_binding() -> Binding {
    let mut repointed = provenance();
    repointed.binding_revision = id("rev-2");
    Binding::new(repointed, id("example/Small-Model"))
}

fn text_request() -> TurnRequest {
    TurnRequest {
        model: "small".to_owned(),
        instructions: String::new(),
        items: vec![Item::user("Hi")],
        tools: Vec::new(),
        max_output_tokens: None,
        sampling: Sampling::default(),
        tool_choice: ToolChoice::Auto,
    }
}

/// A terminal object that reports counters and carries one output item.
fn terminal_with(output_item: &Value) -> Value {
    json!({"type": "response.incomplete", "response": {
        "id": "resp_1", "model": "example/Small-Model", "status": "incomplete",
        "incomplete_details": {"reason": "max_output_tokens"},
        "output": [output_item],
        "usage": {"input_tokens": 1788, "output_tokens": 115}
    }})
}

/// Acceptance: "...reject incompatible continuation state **without losing usage or terminal
/// truth**". `docs/responses.md` calls `Error.observation` the last bound evidence, and
/// `llm-core` documents the field as "including partial usage from an interrupted stream".
///
/// `Decoder::terminate` holds the terminal object — counters, model and response id — in its hand
/// while it decodes that object's own `output`, and every refusal raised in that loop is returned
/// with no observation at all. A model that runs out of output tokens mid tool call is the
/// ordinary way to reach it.
#[test]
fn a_terminal_object_the_decoder_refuses_still_reports_the_counters_it_carried() {
    let oversize = format!("{{\"path\":\"{}\"}}", "x".repeat(70_000));
    let cases: [(&str, &str); 3] = [
        ("truncated by the output cap", "{\"path\": \"READ"),
        ("a JSON scalar rather than an object", "\"README.md\""),
        ("over the argument bound", oversize.as_str()),
    ];
    for (why, arguments) in cases {
        let payloads = vec![terminal_with(&json!({
            "type": "function_call", "call_id": "call-1", "name": "file_read",
            "arguments": arguments
        }))];
        let error = decode_stream(&binding(), &payloads)
            .result
            .expect_err("a tool call this decoder cannot read is refused");
        assert_eq!(error.dispatch, Dispatch::Accepted, "{why}");
        let observation = error.observation.as_ref().unwrap_or_else(|| {
            panic!("{why}: the terminal object's own counters must survive its refusal")
        });
        let usage = observation
            .usage
            .as_ref()
            .unwrap_or_else(|| panic!("{why}: reported counters"));
        assert_eq!(usage.input_tokens, Some(1788), "{why}");
        assert_eq!(usage.output_tokens, Some(115), "{why}");
        assert!(observation.final_usage, "{why}");
    }
}

/// `docs/responses.md`: a body carrying anything outside the subset "is refused with
/// `ErrorCode::Unsupported` rather than translated with the excess dropped".
///
/// `strict` is inside the tool envelope this projection writes, always as `false`. Ingress accepts
/// the key with any value and never reads it, so a gateway client asking for a strict schema is
/// answered by a re-projected body that asks for a loose one, with no refusal and no warning.
#[test]
fn a_strict_tool_schema_is_either_carried_or_refused_but_not_quietly_inverted() {
    let body = json!({
        "model": "example/Small-Model",
        "input": [{"type": "message", "role": "user",
                   "content": [{"type": "input_text", "text": "Hi"}]}],
        "tools": [{"type": "function", "name": "file_read", "description": "Read one file",
                   "parameters": {"type": "object"}, "strict": true}],
        "stream": true, "store": false
    });
    match ingest_request(&binding(), &body) {
        Err(error) => assert_eq!(
            error.code,
            ErrorCode::Unsupported,
            "refusing a flag this contract cannot carry is the other acceptable answer"
        ),
        Ok(request) => {
            let reprojected = project_request(&binding(), &request).expect("projects");
            assert_eq!(
                reprojected["tools"][0]["strict"],
                json!(true),
                "ingress accepted `strict: true` and the contract read back a loose schema"
            );
        }
    }
}

/// `docs/responses.md` refusal table: "image or audio content, **on either side** | `Unsupported`",
/// and the pinned stream subset preserves and reports whatever it does not interpret.
///
/// `message_text` keeps `output_text` and `text` parts and drops every other part of an output
/// message on the floor: no refusal, no warning, no opaque item. A model refusal arrives as a
/// `refusal` content part, so the turn reads back as an empty assistant message that ended
/// normally.
#[test]
fn a_content_part_outside_the_pinned_subset_is_not_dropped_without_a_word() {
    let payloads = vec![json!({"type": "response.completed", "response": {
        "id": "resp_1", "model": "example/Small-Model", "status": "completed",
        "output": [{"type": "message", "role": "assistant", "content": [
            {"type": "refusal", "refusal": "I cannot help with that."}
        ]}]
    }})];
    let decoding = decode_stream(&binding(), &payloads);
    let warned = decoding
        .events
        .iter()
        .any(|event| matches!(event, StreamEvent::Warning { .. }));
    match &decoding.result {
        Err(error) => assert_eq!(error.code, ErrorCode::Unsupported),
        Ok(outcome) => assert!(
            warned,
            "a content part outside the subset was dropped silently: \
             items {:?}, stop_reason {:?}, events {:?}",
            outcome.items, outcome.stop_reason, decoding.events
        ),
    }
}

/// `docs/responses.md`: "One contract serves both directions, so a gateway ingress surface and an
/// outgoing client disagree about nothing."
///
/// `project_request` refuses a tool name outside `^[a-zA-Z0-9_-]+$` because this provider answers
/// one with a 400. `ingest_request` applies no such check, so a gateway accepts a request it can
/// never forward. `workspace.read` is the name that produced the recorded 400.
#[test]
fn a_tool_name_ingress_accepts_is_one_egress_can_publish() {
    let body = json!({
        "model": "example/Small-Model",
        "input": [{"type": "message", "role": "user",
                   "content": [{"type": "input_text", "text": "Hi"}]}],
        "tools": [{"type": "function", "name": "workspace.read", "description": "Read one file",
                   "parameters": {"type": "object"}, "strict": false}],
        "stream": true, "store": false
    });
    match ingest_request(&binding(), &body) {
        Err(error) => assert_eq!(
            error.code,
            ErrorCode::Unsupported,
            "refusing on ingress is the other acceptable answer"
        ),
        Ok(request) => {
            project_request(&binding(), &request).expect(
                "a gateway must be able to forward the request its own ingress just accepted",
            );
        }
    }
}

/// Invariant: "Opaque continuation state is bound to its exact target — protocol, provider,
/// account, endpoint, model and binding revision. A mismatch is refused, never silently reused."
///
/// `contracts/responses/scenarios/a-repointed-binding-revision-refuses-opaque-state.yaml` asserts
/// the refusal on the outgoing side. The ingress side of the same contract stamps every unmodelled
/// `input` entry with the binding doing the reading, so the identical payload — minted by `rev-1`,
/// replayed by the client after the endpoint was repointed to `rev-2` — is reinstated as native
/// state and sent.
#[test]
fn ingress_does_not_reinstate_opaque_state_a_repointed_binding_refuses() {
    let mut request = text_request();
    request.items.push(Item::Opaque {
        provenance: provenance(),
        payload: json!({"type": "reasoning", "id": "rs_1", "encrypted_content": "gAAAAA-rev-1"}),
    });
    let body =
        project_request(&binding(), &request).expect("projects under the binding that minted it");
    project_request(&repointed_binding(), &request)
        .expect_err("the outgoing side refuses a repointed binding");

    match ingest_request(&repointed_binding(), &body) {
        Err(error) => assert_eq!(error.code, ErrorCode::Unsupported),
        Ok(returned) => {
            let forwarded = project_request(&repointed_binding(), &returned);
            assert!(
                forwarded.is_err(),
                "state minted by rev-1 became sendable to rev-2 by passing through ingress: {:?}",
                returned.items
            );
        }
    }
}

/// `docs/responses.md`: `response.completed` and `response.incomplete` are terminal truth, and
/// "end of stream must never manufacture success".
///
/// `stop_reason` reads the `status` string inside the payload and never the discriminator of the
/// event that delivered it, so a `response.incomplete` whose object does not repeat `status`
/// decodes as a turn that ended normally, and its `incomplete_details` are discarded.
#[test]
fn the_terminal_event_that_says_incomplete_is_not_read_as_a_completed_turn() {
    let payloads = vec![json!({"type": "response.incomplete", "response": {
        "id": "resp_1", "model": "example/Small-Model",
        "incomplete_details": {"reason": "max_output_tokens"},
        "output": []
    }})];
    let outcome = decode_stream(&binding(), &payloads)
        .result
        .expect("a terminal response");
    assert_ne!(
        outcome.stop_reason,
        StopReason::EndTurn,
        "`response.incomplete` decoded as a turn that ended normally"
    );
}

// The four cases below are green against the tree as handed over. Each one is here because a
// mutation of the line it covers survived the whole authored suite — 43 ESS scenarios and 28 Rust
// cases, all still passing — when it was applied to a scratch copy of this tree. They are the
// cases that would have killed those mutants.

/// Mutation `absent-input-count-becomes-zero` survived: every usage fixture in the authored suite
/// reports `input_tokens`, so `count(...).or(Some(0))` on that one counter is invisible to it.
/// `docs/responses.md` makes each counter independently optional on purpose, and turning an
/// absent count into zero is the defect this repository names first.
#[test]
fn an_unreported_input_count_stays_unknown_and_is_never_zero() {
    let payloads = vec![json!({"type": "response.completed", "response": {
        "status": "completed", "output": [],
        "usage": {"output_tokens": 115, "output_tokens_details": {"reasoning_tokens": 42}}
    }})];
    let outcome = decode_stream(&binding(), &payloads)
        .result
        .expect("a terminal response");
    let usage = outcome.observation.usage.expect("reported counters");
    assert_eq!(
        usage.input_tokens, None,
        "an unreported input count must stay unknown"
    );
    assert_eq!(usage.output_tokens, Some(115));
    assert_eq!(usage.reasoning_output_tokens, Some(42));
}

/// Mutations `ingress-store-guard-removed` and `ingress-stream-guard-removed` both survived: no
/// authored scenario sends `store: true` or `stream: false`, so neither refusal is exercised.
#[test]
fn ingress_refuses_provider_side_storage_and_a_non_streaming_body() {
    let base = json!({
        "model": "example/Small-Model",
        "input": [{"type": "message", "role": "user",
                   "content": [{"type": "input_text", "text": "Hi"}]}]
    });
    for (field, value) in [("store", json!(true)), ("stream", json!(false))] {
        let mut body = base.clone();
        body[field] = value;
        let error = ingest_request(&binding(), &body)
            .expect_err("outside the pinned subset in this version");
        assert_eq!(error.code, ErrorCode::Unsupported, "`{field}`");
    }
}

/// Mutation `published-tool-field-guard-removed` survived: the authored suite refuses a
/// **top-level** field outside the subset and never one inside a published tool.
#[test]
fn ingress_refuses_a_published_tool_field_outside_the_pinned_subset() {
    let body = json!({
        "model": "example/Small-Model", "input": [],
        "tools": [{"type": "function", "name": "file_read", "description": "Read one file",
                   "parameters": {"type": "object"}, "strict": false,
                   "container": {"type": "auto"}}]
    });
    let error = ingest_request(&binding(), &body).expect_err("outside the pinned subset");
    assert_eq!(error.code, ErrorCode::Unsupported);
}

/// Mutation `incomplete-reason-length-bound-widened` survived: the authored case for an unsafe
/// reason uses spaces and capitals, so the 64-byte half of `is_safe_reason` is never read.
#[test]
fn an_over_long_incomplete_reason_is_named_rather_than_relayed() {
    let reason = "a".repeat(65);
    let payloads = vec![json!({"type": "response.incomplete", "response": {
        "status": "incomplete", "incomplete_details": {"reason": reason}, "output": []
    }})];
    let outcome = decode_stream(&binding(), &payloads)
        .result
        .expect("a terminal response");
    assert_eq!(
        outcome.stop_reason,
        StopReason::Incomplete {
            reason: "unrecognized".to_owned()
        }
    );
}

/// The published tool-name class is `^[a-zA-Z0-9_-]+$`, which matches one character or more.
/// `check_tool_names` accepts the empty name because `all` over no bytes is true; nothing else in
/// the projection path re-reads the pattern. Kept as a live case rather than a comment because a
/// later widening of `ToolName` is exactly what would make it reachable.
#[test]
fn the_published_tool_name_class_requires_at_least_one_character() {
    let empty = ToolName::new("");
    if let Ok(name) = empty {
        let mut request = text_request();
        request.tools = vec![ToolSpec {
            name,
            description: "d".to_owned(),
            input_schema: json!({"type": "object"}),
        }];
        let error = project_request(&binding(), &request)
            .expect_err("a name outside the published class is refused");
        assert_eq!(error.code, ErrorCode::Unsupported);
    }
}
