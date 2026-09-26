//! Adversarial codec cases, written against the documents this unit shipped.
//!
//! These drive the implementation from `docs/messages.md` and from `crates/llm-core/src/item.rs`,
//! which are the specifications the unit claims to satisfy. No implementation file is edited here.
mod support;

use llm_core::{ErrorCode, Id, Item, TurnRequest};
use llm_messages::{decode_request, encode_request};
use serde_json::{Value, json};
use support::binding;

fn wire_with(extra: &[(&str, Value)]) -> Value {
    let mut body = json!({
        "model": "internal-model",
        "max_tokens": 512,
        "messages": [{"role":"user","content":"Summarise the log"}]
    });
    for (name, value) in extra {
        body[*name] = value.clone();
    }
    body
}

fn ingress(body: &Value) -> Result<llm_messages::IngressRequest, llm_core::Error> {
    decode_request(body.to_string().as_bytes(), binding().provenance())
}

/// `docs/messages.md` lines 19-21: "Thinking crosses as an opaque item bound to all six binding
/// coordinates ... Opaque state from any other binding is refused rather than replayed", in a
/// section describing "one codec used in both directions" (line 5).
/// `crates/llm-core/src/item.rs` line 14: "Opaque payloads remain bound to their original serving
/// model."
///
/// Egress honours both. Ingress cannot attribute a `thinking` block that arrived on the wire —
/// the wire carries no binding — and instead of refusing it, `decode_block` stamps the *reading*
/// binding onto it (`crates/llm-messages/src/codec.rs:390`). The state egress refuses in part (a)
/// is therefore sendable after one `decode_request` / `encode_request` round trip.
///
/// Either answer would satisfy this case: refuse the unattributable block, or keep it out of the
/// reading binding's provenance. Stamping the reader is the one answer that launders it.
#[test]
fn ingress_does_not_bind_unattributable_thinking_to_the_reading_binding() {
    let payload = json!({"type":"thinking","thinking":"weighing","signature":"sig-1"});

    // (a) The rule the shipped suite already pins: the same payload, carried as opaque state
    //     bound to another endpoint, is refused before any I/O.
    let mut foreign = binding().provenance().clone();
    foreign.endpoint = Id::new("other-endpoint").unwrap();
    let carried = TurnRequest::new(
        "internal-model",
        vec![
            Item::user("Summarise the log"),
            Item::Opaque {
                provenance: foreign,
                payload: payload.clone(),
            },
        ],
    );
    assert_eq!(
        encode_request(&carried, &binding())
            .expect_err("egress refuses opaque state from another binding")
            .code,
        ErrorCode::Unsupported,
    );

    // (b) The identical bytes arriving as a Messages request body, which is what a gateway reads.
    let body = json!({
        "model": "internal-model",
        "max_tokens": 512,
        "messages": [
            {"role":"user","content":"Summarise the log"},
            {"role":"assistant","content":[payload]}
        ]
    });
    let Ok(decoded) = ingress(&body) else {
        // Refusing what it cannot attribute is a correct answer to this case.
        return;
    };
    let provenance = match decoded.request.items.get(1) {
        Some(Item::Opaque { provenance, .. }) => provenance,
        // Carrying it unattributed is the third correct answer: it names no binding, so egress
        // refuses it until a caller binds it.
        Some(Item::UnattributedOpaque { .. }) => {
            let error = encode_request(&decoded.request, &binding())
                .expect_err("unbound state is not sendable");
            assert_eq!(error.code, ErrorCode::Unsupported);
            assert_eq!(error.message, Item::UNATTRIBUTED_REFUSAL);
            return;
        }
        _ => panic!("ingress kept the thinking block as something other than opaque state"),
    };

    // (c) The demonstration: the round trip made (a) sendable.
    let replayable = encode_request(&decoded.request, &binding()).is_ok();
    assert_ne!(
        provenance,
        binding().provenance(),
        "ingress stamped the reading binding onto a thinking block it could not attribute; \
         encode_request now accepts that block ({replayable}) although it refuses the same \
         payload when it is bound to any other binding",
    );
}

/// The response half of this same codec defines an explicit `null` as absent in every guard it
/// has — `decode.rs::text`, `usage.rs::count`, `lib.rs::absent` and the two
/// `.filter(|v| !v.is_null())` calls — and the unit's own fixtures show this producer spelling
/// inapplicable optional fields that way (`"stop_reason":null`, `"stop_sequence":null` in every
/// scenario under `contracts/messages/scenarios`).
///
/// The request half does not. `system`, `tool_choice`, `output_config`, `tools`, `temperature`,
/// `top_p` and `stream` are all on the accepted key list in `codec.rs::decode_request`, and every
/// one of them is refused when its value is the `null` that means "absent" — several of them as
/// `Unsupported`, the code reserved for something outside the declared subset. Inside one
/// function, `{"type":"auto","name":null}` is accepted through `absent()` while
/// `"tool_choice": null` is refused.
#[test]
fn ingress_reads_an_explicit_null_optional_field_as_absent() {
    let mut refused = Vec::new();
    for name in [
        "system",
        "tool_choice",
        "output_config",
        "tools",
        "temperature",
        "top_p",
        "stream",
    ] {
        if let Err(error) = ingress(&wire_with(&[(name, Value::Null)])) {
            refused.push(format!("{name} => {:?}", error.code));
        }
    }
    assert!(
        refused.is_empty(),
        "an absent optional spelled as an explicit null is refused on the request side of a \
         codec whose response side reads null as absent: {refused:?}",
    );
}
