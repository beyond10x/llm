//! Outgoing projection and stateless gateway ingress of the declared Messages subset.
mod support;

use llm_core::{
    CallId, ErrorCode, Item, Protocol, Sampling, ToolCall, ToolChoice, ToolName, ToolSpec,
    TurnRequest,
};
use llm_messages::{decode_request, encode_request};
use serde_json::{Value, json};
use support::{binding, binding_with, capabilities};

fn wire(request: &TurnRequest) -> Value {
    serde_json::from_slice(&encode_request(request, &binding()).expect("projected")).expect("JSON")
}

fn full_request() -> TurnRequest {
    let mut request = TurnRequest::new(
        "internal-model",
        vec![
            Item::user("Summarise the log"),
            Item::Opaque {
                provenance: binding().provenance().clone(),
                payload: json!({"type":"thinking","thinking":"weighing","signature":"sig-1"}),
            },
            Item::ToolCall(ToolCall {
                call_id: CallId::new("call-1").unwrap(),
                name: ToolName::new("lookup").unwrap(),
                arguments: json!({"query":"errors"}),
            }),
            Item::ToolResult {
                call_id: CallId::new("call-1").unwrap(),
                output: json!("not found"),
                failed: true,
            },
        ],
    );
    "Stay terse".clone_into(&mut request.instructions);
    request.tools = vec![ToolSpec {
        name: ToolName::new("lookup").unwrap(),
        description: "Look up a record".to_owned(),
        input_schema: json!({"type":"object"}),
    }];
    request.tool_choice = ToolChoice::Named(ToolName::new("lookup").unwrap());
    request.max_output_tokens = Some(1024);
    request.sampling = Sampling {
        temperature: Some(0.5),
        top_p: Some(0.8),
        reasoning_effort: Some("high".to_owned()),
    };
    request
}

#[test]
fn ordered_roles_tools_and_thinking_project_onto_one_wire_request() {
    assert_eq!(
        wire(&full_request()),
        json!({
            "model": "example/Model-Revision",
            "max_tokens": 1024,
            "stream": true,
            "system": [{"type":"text","text":"Stay terse","cache_control":{"type":"ephemeral"}}],
            "messages": [
                {"role":"user","content":[{"type":"text","text":"Summarise the log"}]},
                {"role":"assistant","content":[
                    {"type":"thinking","thinking":"weighing","signature":"sig-1"},
                    {"type":"tool_use","id":"call-1","name":"lookup","input":{"query":"errors"}}]},
                {"role":"user","content":[
                    {"type":"tool_result","tool_use_id":"call-1","content":"not found","is_error":true,
                        "cache_control":{"type":"ephemeral"}}]}
            ],
            "tools": [{"name":"lookup","description":"Look up a record","input_schema":{"type":"object"}}],
            "temperature": 0.5,
            "top_p": 0.8,
            "output_config": {"effort":"high"},
            "tool_choice": {"type":"tool","name":"lookup"}
        })
    );
}

const MARKER: &str = "cache_control";

fn ephemeral() -> Value {
    json!({"type":"ephemeral"})
}

fn thinking() -> Item {
    Item::Opaque {
        provenance: binding().provenance().clone(),
        payload: json!({"type":"thinking","thinking":"weighing","signature":"sig-1"}),
    }
}

/// Harness parity M14/M15 (`harness-messages/src/project.rs:253`-`300`, `:535`-`542`).
///
/// The loop is stateless: every turn resends the whole transcript. A breakpoint at the end of
/// `system` caches the constant head (tools, then system); a rolling one on the last block of the
/// last message makes each turn write the prefix the next turn reads back, so the conversation's
/// growth is paid for once rather than on every remaining turn. Two breakpoints, against the
/// route's four, and none anywhere else.
#[test]
fn the_system_prompt_and_the_conversation_tail_carry_the_two_cache_breakpoints() {
    let mut request = TurnRequest::new(
        "internal-model",
        vec![
            Item::user("Summarise the log"),
            Item::assistant("Looking"),
            Item::user("Go on"),
        ],
    );
    "Stay terse".clone_into(&mut request.instructions);
    let body = wire(&request);
    assert_eq!(
        body,
        json!({
            "model": "example/Model-Revision",
            "max_tokens": 2048,
            "stream": true,
            "system": [{"type":"text","text":"Stay terse","cache_control":{"type":"ephemeral"}}],
            "messages": [
                {"role":"user","content":[{"type":"text","text":"Summarise the log"}]},
                {"role":"assistant","content":[{"type":"text","text":"Looking"}]},
                {"role":"user","content":[
                    {"type":"text","text":"Go on","cache_control":{"type":"ephemeral"}}]}
            ]
        })
    );
    // Only the tail moves: an earlier message keeping a marker would stop the cache chaining.
    assert_eq!(body.to_string().matches(MARKER).count(), 2, "{body}");
}

/// A `thinking` block is replayed byte for byte or not at all: its signature covers the block as
/// the model produced it, so a marker added to it is a turn the route rejects. The marker falls
/// back to the last block this projection built itself.
#[test]
fn a_replayed_thinking_block_is_never_the_block_that_gets_marked() {
    let request = TurnRequest::new(
        "internal-model",
        vec![
            Item::user("Summarise the log"),
            Item::assistant("Looking"),
            thinking(),
        ],
    );
    let body = wire(&request);
    let tail = &body["messages"][1]["content"];
    assert_eq!(tail[0]["cache_control"], ephemeral(), "{body}");
    assert_eq!(
        tail[1],
        json!({"type":"thinking","thinking":"weighing","signature":"sig-1"}),
        "a replayed block is sent exactly as it arrived"
    );
    assert_eq!(body.to_string().matches(MARKER).count(), 1, "{body}");
}

/// A missing breakpoint costs money; a modified opaque block costs the turn. A tail of nothing
/// this projection built itself carries no rolling marker at all, and an empty instruction sends
/// no `system` block to mark.
#[test]
fn a_tail_with_nothing_markable_carries_no_rolling_breakpoint() {
    let redacted = json!({"type":"redacted_thinking","data":"AAAA"});
    let mut request = TurnRequest::new(
        "internal-model",
        vec![
            Item::user("Summarise the log"),
            thinking(),
            Item::Opaque {
                provenance: binding().provenance().clone(),
                payload: redacted.clone(),
            },
        ],
    );
    request.tools = full_request().tools;
    "Stay terse".clone_into(&mut request.instructions);
    let body = wire(&request);
    assert_eq!(body["system"][0]["cache_control"], ephemeral(), "{body}");
    assert_eq!(
        body["messages"][1]["content"],
        json!([{"type":"thinking","thinking":"weighing","signature":"sig-1"}, redacted])
    );
    assert_eq!(body.to_string().matches(MARKER).count(), 1, "{body}");

    request.instructions.clear();
    let body = wire(&request);
    assert!(body.get("system").is_none(), "{body}");
    assert_eq!(body.to_string().matches(MARKER).count(), 0, "{body}");
}

/// The projection marks what it sends, and ingress still refuses `cache_control` on an arriving
/// block (`docs/messages.md`): a caller's own breakpoint has no neutral field to travel in, and
/// dropping it would hide a billing decision. So the projected body is not itself ingress input.
#[test]
fn ingress_refuses_the_breakpoints_egress_places() {
    let projected = encode_request(&full_request(), &binding()).expect("projected");
    assert!(
        String::from_utf8_lossy(&projected).contains(MARKER),
        "the projection placed no breakpoint"
    );
    let error = decode_request(&projected, binding().provenance()).expect_err("refused");
    assert_eq!(error.code, ErrorCode::Unsupported);
}

/// The projected body with every breakpoint removed: what the round trips below read back.
fn without_breakpoints(projected: &[u8]) -> Vec<u8> {
    fn strip(value: &mut Value) {
        match value {
            Value::Object(fields) => {
                fields.remove(MARKER);
                fields.values_mut().for_each(strip);
            }
            Value::Array(values) => values.iter_mut().for_each(strip),
            _ => {}
        }
    }
    let mut body: Value = serde_json::from_slice(projected).expect("JSON");
    strip(&mut body);
    serde_json::to_vec(&body).expect("JSON")
}

#[test]
fn an_absent_output_bound_takes_the_bindings_declared_maximum() {
    let mut request = TurnRequest::new("internal-model", vec![Item::user("hello")]);
    request.max_output_tokens = None;
    // The route requires the field; the binding's declared maximum is the only honest default.
    assert_eq!(wire(&request)["max_tokens"], json!(2048));
}

#[test]
fn a_temperature_above_the_routes_maximum_is_refused_before_any_projection() {
    let mut request = TurnRequest::new("internal-model", vec![Item::user("hello")]);
    request.sampling.temperature = Some(1.5);
    let error = encode_request(&request, &binding()).expect_err("refused");
    assert_eq!(error.code, ErrorCode::Unsupported);
}

#[test]
fn opaque_state_from_another_binding_is_refused_rather_than_projected() {
    let mut foreign = binding().provenance().clone();
    foreign.model = llm_core::Id::new("other-model").unwrap();
    let request = TurnRequest::new(
        "internal-model",
        vec![
            Item::user("hello"),
            Item::Opaque {
                provenance: foreign,
                payload: json!({"type":"thinking","thinking":"t","signature":"sig-1"}),
            },
        ],
    );
    let error = encode_request(&request, &binding()).expect_err("refused");
    assert_eq!(error.code, ErrorCode::Unsupported);
}

#[test]
fn a_binding_of_another_protocol_is_refused() {
    let request = TurnRequest::new("internal-model", vec![Item::user("hello")]);
    let responses = {
        let binding = binding();
        let mut document = binding.declaration().clone();
        document.serving.protocol = Protocol::Responses;
        document.bind().unwrap()
    };
    let error = encode_request(&request, &responses).expect_err("refused");
    assert_eq!(error.code, ErrorCode::Unsupported);
}

fn ingress(body: &Value) -> Result<llm_messages::IngressRequest, llm_core::Error> {
    decode_request(
        body.to_string().as_bytes(),
        binding_with("https://messages.example.invalid/v1", capabilities()).provenance(),
    )
}

#[test]
fn ingress_decodes_the_declared_subset_and_preserves_the_stream_flag() {
    let decoded = ingress(&json!({
        "model": "caller-alias",
        "max_tokens": 512,
        "system": [{"type":"text","text":"Stay terse"}],
        "messages": [
            {"role":"user","content":"Summarise the log"},
            {"role":"assistant","content":[
                {"type":"tool_use","id":"call-1","name":"lookup","input":{"query":"errors"}}]},
            {"role":"user","content":[
                {"type":"tool_result","tool_use_id":"call-1","content":"not found","is_error":true}]}
        ],
        "tools": [{"name":"lookup","description":"Look up a record","input_schema":{"type":"object"}}],
        "tool_choice": {"type":"any"},
        "stream": true
    }))
    .expect("decoded");
    assert!(decoded.stream);
    // The wire's model stays the caller's routing name; it is not the binding's upstream name.
    assert_eq!(decoded.request.model, "caller-alias");
    assert_eq!(decoded.request.instructions, "Stay terse");
    assert_eq!(decoded.request.max_output_tokens, Some(512));
    assert_eq!(decoded.request.tool_choice, ToolChoice::Required);
    assert_eq!(decoded.request.items.len(), 3);
    assert_eq!(decoded.request.items[0], Item::user("Summarise the log"));
    assert_eq!(
        decoded.request.items[2],
        Item::ToolResult {
            call_id: CallId::new("call-1").unwrap(),
            output: json!("not found"),
            failed: true,
        }
    );
}

#[test]
fn an_absent_stream_flag_is_not_a_streaming_request() {
    let decoded = ingress(&json!({
        "model":"caller-alias","max_tokens":8,
        "messages":[{"role":"user","content":"hi"}]
    }))
    .expect("decoded");
    assert!(!decoded.stream);
}

#[test]
fn ingress_preserves_the_neutral_request_across_a_round_trip() {
    let mut request = full_request();
    // Thinking is the one item a round trip cannot carry; the case below states why.
    request.items.remove(1);
    let projected = encode_request(&request, &binding()).expect("projected");
    let decoded =
        decode_request(&without_breakpoints(&projected), binding().provenance()).expect("decoded");
    let mut restored = decoded.request;
    // Only the routing alias differs: the wire carries the binding's upstream model name.
    restored.model.clone_from(&request.model);
    assert_eq!(restored, request);
}

/// Thinking survives a gateway hop carried, never laundered.
///
/// Egress refuses opaque state bound to any other binding, so stamping the reading binding onto
/// an arriving `thinking` block would make exactly that state sendable after one round trip.
/// Ingress therefore carries it as `Item::UnattributedOpaque`: held JSON-equal, refused by name
/// on egress, and sendable only after the caller binds it.
#[test]
fn thinking_survives_a_gateway_round_trip_only_through_the_callers_binding() {
    let projected = encode_request(&full_request(), &binding()).expect("egress carries thinking");
    let mut decoded = decode_request(&without_breakpoints(&projected), binding().provenance())
        .expect("ingress carries what it cannot attribute")
        .request;
    assert_eq!(
        decoded.items[1],
        Item::UnattributedOpaque {
            protocol: Protocol::Messages,
            payload: json!({"type":"thinking","thinking":"weighing","signature":"sig-1"}),
        }
    );
    decoded.model = "internal-model".to_owned();
    let error = encode_request(&decoded, &binding()).expect_err("not sendable unbound");
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(error.message, Item::UNATTRIBUTED_REFUSAL);

    assert_eq!(
        decoded
            .bind_unattributed(binding().provenance())
            .expect("bound"),
        1
    );
    let replayed: Value =
        serde_json::from_slice(&encode_request(&decoded, &binding()).expect("sendable once bound"))
            .expect("JSON");
    let sent: Value = serde_json::from_slice(&projected).expect("JSON");
    assert_eq!(
        serde_json::to_string(&replayed["messages"][1]["content"][0]).unwrap(),
        serde_json::to_string(&sent["messages"][1]["content"][0]).unwrap()
    );

    let redacted = json!({"model":"m","max_tokens":8,"messages":[
        {"role":"user","content":"hi"},
        {"role":"assistant","content":[{"type":"redacted_thinking","data":"AAAA"}]}]});
    assert_eq!(
        ingress(&redacted).expect("carried").request.items[1],
        Item::UnattributedOpaque {
            protocol: Protocol::Messages,
            payload: json!({"type":"redacted_thinking","data":"AAAA"}),
        }
    );
}

#[test]
fn ingress_refuses_a_request_field_outside_the_declared_subset() {
    for body in [
        json!({"model":"m","max_tokens":8,"messages":[{"role":"user","content":"hi"}],"metadata":{"user_id":"u"}}),
        json!({"model":"m","max_tokens":8,"messages":[{"role":"user","content":"hi"}],"service_tier":"auto"}),
        json!({"model":"m","max_tokens":8,"messages":[{"role":"user","content":"hi"}],"thinking":{"type":"enabled","budget_tokens":1024}}),
    ] {
        assert_eq!(
            ingress(&body).expect_err("refused").code,
            ErrorCode::Unsupported,
            "{body}"
        );
    }
}

#[test]
fn ingress_refuses_content_outside_the_declared_subset() {
    for content in [
        json!({"type":"image","source":{"type":"base64","media_type":"image/png","data":"AA=="}}),
        json!({"type":"server_tool_use","id":"srvtoolu_1","name":"web_search","input":{}}),
        json!({"type":"text","text":"cached","cache_control":{"type":"ephemeral"}}),
        json!({"type":"text","text":"cited","citations":[{"type":"char_location"}]}),
    ] {
        let body = json!({"model":"m","max_tokens":8,
            "messages":[{"role":"user","content":[content.clone()]}]});
        assert_eq!(
            ingress(&body).expect_err("refused").code,
            ErrorCode::Unsupported,
            "{content}"
        );
    }
}

#[test]
fn ingress_refuses_thinking_that_carries_no_signature() {
    let body = json!({"model":"m","max_tokens":8,"messages":[
        {"role":"user","content":"hi"},
        {"role":"assistant","content":[{"type":"thinking","thinking":"weighing","signature":""}]}]});
    assert_eq!(
        ingress(&body).expect_err("refused").code,
        ErrorCode::Protocol
    );
}

#[test]
fn ingress_refuses_a_request_that_does_not_open_with_caller_content() {
    let body = json!({"model":"m","max_tokens":8,
        "messages":[{"role":"assistant","content":"I began on my own"}]});
    assert_eq!(
        ingress(&body).expect_err("refused").code,
        ErrorCode::InvalidRequest
    );
}

#[test]
fn ingress_refuses_a_zero_or_missing_output_bound() {
    for body in [
        json!({"model":"m","messages":[{"role":"user","content":"hi"}]}),
        json!({"model":"m","max_tokens":0,"messages":[{"role":"user","content":"hi"}]}),
    ] {
        assert_eq!(
            ingress(&body).expect_err("refused").code,
            ErrorCode::InvalidRequest,
            "{body}"
        );
    }
}

#[test]
fn ingress_requires_a_messages_origin_and_bounds_its_input() {
    let mut origin = binding().provenance().clone();
    origin.protocol = Protocol::ChatCompletions;
    let body = json!({"model":"m","max_tokens":8,"messages":[{"role":"user","content":"hi"}]});
    assert_eq!(
        decode_request(body.to_string().as_bytes(), &origin)
            .expect_err("refused")
            .code,
        ErrorCode::Unsupported
    );
    assert_eq!(
        decode_request(&vec![b'x'; llm_core::MAX_REQUEST_BYTES + 1], &origin)
            .expect_err("refused")
            .code,
        ErrorCode::TooLarge
    );
}

/// An arriving request names the field it was refused for, by path from the body, and never the
/// field's value.
#[test]
fn ingress_names_the_path_of_a_field_outside_the_declared_subset() {
    let value = "value-that-no-diagnostic-may-carry";
    let body = || {
        json!({"model":"m","max_tokens":8,
            "system":[{"type":"text","text":"be brief"}],
            "messages":[{"role":"user","content":[{"type":"text","text":"hi"}]}],
            "tools":[{"name":"lookup","input_schema":{"type":"object"}}],
            "tool_choice":{"type":"auto"},
            "output_config":{"effort":"low"}})
    };
    for (pointer, path) in [
        ("", "undeclared"),
        ("/system/0", "system[].undeclared"),
        ("/messages/0", "messages[].undeclared"),
        ("/messages/0/content/0", "messages[].content[].undeclared"),
        ("/tools/0", "tools[].undeclared"),
        ("/tool_choice", "tool_choice.undeclared"),
        ("/output_config", "output_config.undeclared"),
    ] {
        let mut body = body();
        body.pointer_mut(pointer)
            .and_then(Value::as_object_mut)
            .unwrap_or_else(|| panic!("{pointer} is an object"))
            .insert("undeclared".to_owned(), json!(value));
        let error = ingress(&body).expect_err(path);
        assert_eq!(error.code, ErrorCode::Unsupported, "{path}: {error}");
        assert_eq!(
            error.message,
            format!("Messages field is outside the declared subset: {path}")
        );
        assert!(!error.message.contains(value), "{error}");
    }
}

#[test]
fn adversary_field_name_diagnostics_obey_byte_and_character_boundaries() {
    for (name, expected) in [
        (String::new(), "?".to_owned()),
        ("a".repeat(64), "a".repeat(64)),
        ("a".repeat(65), "?".to_owned()),
        ("A9_-".to_owned(), "A9_-".to_owned()),
        ("field.name".to_owned(), "?".to_owned()),
        ("a\nb".to_owned(), "?".to_owned()),
        ("é".repeat(32), "?".to_owned()),
    ] {
        let mut body = json!({"model":"m","max_tokens":8,
            "messages":[{"role":"user","content":"hi"}]});
        body.as_object_mut()
            .unwrap()
            .insert(name, json!("private-value"));
        let error = ingress(&body).expect_err("undeclared field must refuse");
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert_eq!(
            error.message,
            format!("Messages field is outside the declared subset: {expected}")
        );
        assert!(!error.message.contains("private-value"));
    }
}
