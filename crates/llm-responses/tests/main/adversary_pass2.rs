//! Second adversarial pass over the Responses projection.
//!
//! Every case here drives a sentence of `docs/responses.md`, or the story's Acceptance statement,
//! against the implementation the same unit wrote. Each names the claim it drives. No
//! implementation file is touched by this file or by the pass that wrote it.

use llm_core::{ErrorCode, Id, Protocol, Provenance};
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

/// The terminal object every case below shares: it reports its counters, model and id.
fn completed(output: &Value) -> Value {
    json!({"type": "response.completed", "response": {
        "id": "resp_1", "model": "example/Small-Model", "status": "completed",
        "output": [output],
        "usage": {"input_tokens": 1788, "output_tokens": 115}
    }})
}

/// Acceptance: "...reject incompatible continuation state **without losing usage or terminal
/// truth**". `docs/responses.md`: "**Every refusal carries the evidence it had.** A refusal
/// raised while decoding the terminal object's own output — an unreadable tool call, an oversize
/// argument blob ... is returned with that object's counters ... because a model that runs out of
/// output tokens mid tool call is the ordinary way to reach it and the attempt was still paid
/// for."
///
/// The ordinary way to reach it is the shape this case builds, and it is not the shape any
/// fixture in this crate builds. A real Responses stream announces a finished output item with
/// `response.output_item.done` **before** the terminal object arrives, and `Decoder::apply`
/// decodes that item too. The same unreadable tool call therefore refuses one event earlier,
/// through `bound()` rather than `retain()`, and `decode_stream` returns on the spot — so the
/// `response.completed` sitting in the very same payload list, carrying the counters the attempt
/// was billed for, is never read at all.
///
/// Not one Rust case and not one authored scenario sends `response.output_item.done`, which is
/// why the retained-evidence correction is reachable only from a payload list no server sends.
#[test]
fn an_unreadable_tool_call_keeps_the_terminal_counters_when_the_server_announces_the_item() {
    let oversize = format!("{{\"path\":\"{}\"}}", "x".repeat(70_000));
    let cases: [(&str, &str); 3] = [
        ("truncated by the output cap", "{\"path\": \"READ"),
        ("a JSON scalar rather than an object", "\"README.md\""),
        ("over the argument bound", oversize.as_str()),
    ];
    for (why, arguments) in cases {
        let item = json!({"type": "function_call", "id": "fc_1", "call_id": "call-1",
                          "name": "file_read", "arguments": arguments});
        // The ordinary order one Responses server produces: the item is announced, its arguments
        // stream, the item is announced done, and only then does the terminal object arrive.
        let payloads = vec![
            json!({"type": "response.output_item.added", "item": {
                "type": "function_call", "id": "fc_1", "call_id": "call-1",
                "name": "file_read"}}),
            json!({"type": "response.function_call_arguments.delta",
                   "item_id": "fc_1", "delta": arguments}),
            json!({"type": "response.output_item.done", "item": item.clone()}),
            completed(&item),
        ];
        let error = decode_stream(&binding(), &payloads)
            .result
            .expect_err("a tool call this decoder cannot read is refused");
        let observation = error
            .observation
            .as_ref()
            .unwrap_or_else(|| panic!("{why}: the refusal carries no evidence at all"));
        let usage = observation.usage.as_ref().unwrap_or_else(|| {
            panic!(
                "{why}: the terminal object in the same stream reported 1788/115 and the refusal \
                 reports no counters"
            )
        });
        assert_eq!(usage.input_tokens, Some(1788), "{why}");
        assert_eq!(usage.output_tokens, Some(115), "{why}");
    }
}

/// One minimal wire body inside the pinned subset. A `null` override removes the field.
fn wire_body(fields: &[(&str, Value)]) -> Value {
    let mut body = json!({
        "model": "example/Small-Model",
        "input": [{"type": "message", "role": "user",
                   "content": [{"type": "input_text", "text": "Hi"}]}],
        "tools": [], "stream": true, "store": false
    });
    let object = body.as_object_mut().expect("an object");
    for (field, value) in fields {
        if value.is_null() {
            object.remove(*field);
        } else {
            object.insert((*field).to_owned(), value.clone());
        }
    }
    body
}

/// `docs/responses.md`: "The four fields this projection writes with a **fixed** value —
/// `stream`, `store`, `include` and a tool's `strict` — are refused on ingress when they carry a
/// different one. Accepting `strict: true` and re-projecting `strict: false` answers a client
/// that asked for a strict schema with a loose one and says nothing."
///
/// A body that **omits** one of the four carries a different one, because on this wire an absent
/// field is not an absent value: `stream` absent is a non-streaming request, `store` absent is a
/// stored conversation, and an absent tool `strict` is whatever the endpoint defaults to. The
/// crate reasons this way itself on the other side — `tool_choice` is omitted rather than sent as
/// `"auto"` precisely so the provider's default stays the provider's — and ingress then reads
/// three absences as agreement with the value it was going to write anyway.
///
/// Either answer is acceptable and neither is given: refuse the body, or forward it as it
/// arrived. What happens instead is that the gateway answers a request nobody made.
#[test]
fn a_fixed_field_the_body_omits_is_refused_rather_than_answered_with_this_crates_own_value() {
    let loose = json!([{"type": "function", "name": "file_read", "description": "Read one file",
                        "parameters": {"type": "object"}}]);
    let cases: [(&str, Value, &str); 3] = [
        (
            "`stream` absent is a non-streaming request",
            wire_body(&[("stream", Value::Null)]),
            "stream",
        ),
        (
            "`store` absent is a stored conversation",
            wire_body(&[("store", Value::Null)]),
            "store",
        ),
        (
            "a published tool's `strict` absent is the endpoint's default",
            wire_body(&[("tools", loose)]),
            "tools",
        ),
    ];
    for (why, sent, field) in cases {
        match ingest_request(&binding(), &sent) {
            Err(error) => assert_eq!(error.code, ErrorCode::Unsupported, "{why}"),
            Ok(request) => {
                let out = project_request(&binding(), &request).unwrap_or_else(|error| {
                    panic!("{why}: a gateway forwards what it accepted: {error:?}")
                });
                assert_eq!(
                    out.get(field),
                    sent.get(field),
                    "{why}: accepted, then answered with this crate's own value"
                );
            }
        }
    }
}

/// The same sentence, for the one of the four that carries an explicit different value and is
/// still not refused.
///
/// `include: []` is not absent and is not `["reasoning.encrypted_content"]`. The guard is
/// `values.iter().all(...)`, and `all` over no elements is true — the identical vacuous-truth
/// trap the crate documents and defends against three hundred lines earlier, in
/// `check_tool_names`: "`+`, not `*`: the documented class matches one character or more, and
/// `all` over no bytes is true."
#[test]
fn an_include_carrying_a_different_value_is_refused_rather_than_rewritten() {
    let sent = wire_body(&[("include", json!([]))]);
    match ingest_request(&binding(), &sent) {
        Err(error) => assert_eq!(error.code, ErrorCode::Unsupported),
        Ok(request) => {
            let out = project_request(&binding(), &request)
                .expect("a gateway forwards what its own ingress accepted");
            assert_eq!(
                out.get("include"),
                sent.get("include"),
                "`include: []` was accepted and answered with a different include"
            );
        }
    }
}

/// `docs/responses.md`: "One contract serves both directions, so a gateway ingress surface and an
/// outgoing client disagree about nothing", and the crate's own property case says "Anything
/// ingress accepts, egress can send back — and sends back unchanged."
///
/// Two `input_text` parts in one message is ordinary, entirely inside the pinned subset, and
/// accepted — and comes back out as one part. The four bodies the property case drives all carry
/// exactly one part each, and the conformance adapter's `body_preserved` fact cannot see it
/// either: it compares only the fields the body sent, field by field, against the reprojection.
#[test]
fn a_message_of_two_content_parts_reprojects_as_it_arrived() {
    let sent = wire_body(&[(
        "input",
        json!([{"type": "message", "role": "user", "content": [
            {"type": "input_text", "text": "Hello, "},
            {"type": "input_text", "text": "world"}]}]),
    )]);
    match ingest_request(&binding(), &sent) {
        Err(error) => assert_eq!(error.code, ErrorCode::Unsupported),
        Ok(request) => {
            let out = project_request(&binding(), &request)
                .expect("a gateway forwards what its own ingress accepted");
            assert_eq!(
                out.get("input"),
                sent.get("input"),
                "the parts a client sent were merged on the way back out"
            );
        }
    }
}

/// `docs/responses.md`: "A provider failure is classified from its machine-readable code alone",
/// and "**One** refusal deliberately carries none [no evidence]: when the reported counters are
/// themselves contradictory".
///
/// There is a second. `provider_failure` builds the classified error and then replaces it whole
/// with the counter-validation refusal, so a response the provider explicitly rate-limited is
/// reported as `Protocol` — not retriable — as soon as its usage block disagrees with itself.
/// The code was read, and then discarded.
#[test]
fn a_rate_limited_failure_keeps_its_class_when_its_counters_disagree() {
    let payloads = vec![json!({"type": "response.failed", "response": {
        "id": "resp_1", "model": "example/Small-Model", "status": "failed",
        "error": {"code": "rate_limit_exceeded", "message": "slow down"},
        "usage": {"input_tokens": 10, "input_tokens_details": {"cached_tokens": 11}}
    }})];
    let error = decode_stream(&binding(), &payloads)
        .result
        .expect_err("a failed response refuses");
    assert_eq!(
        error.code,
        ErrorCode::RateLimited,
        "a provider failure is classified from its machine-readable code alone"
    );
}
