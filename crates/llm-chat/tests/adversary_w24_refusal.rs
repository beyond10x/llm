//! Adversary, wave 2026-10-30-w24, story:chat-counts-every-output-delta.
//!
//! - `docs/chat.md` "Reading a response": "Every field it does read is refused when it
//!   contradicts the wire". The change starts reading `delta.refusal`, `delta.reasoning` and
//!   `message.refusal`; a value there that is not a string is neither refused nor counted.
//! - The gateway now answers a refused turn on this wire with the assistant message's `refusal`
//!   key; the same crate's ingress decoder refuses that message when a client sends it back as
//!   history, so the gateway cannot read the answer it wrote.
mod common;

use common::binding;
use llm_chat::{decode_completion, decode_ingress_request, encode_ingress_completion};
use llm_core::{Item, StopReason, TurnObservation, TurnOutcome};
use serde_json::json;

fn chunk(delta: &str, finish_reason: &str) -> String {
    format!(
        "data: {{\"id\":\"chatcmpl-adv\",\"object\":\"chat.completion.chunk\",\"model\":\"m\",\"choices\":[{{\"index\":0,\"delta\":{delta},\"finish_reason\":{finish_reason}}}]}}\n\n"
    )
}

/// A `refusal` or `reasoning` value that is not a string contradicts the wire and is refused,
/// streamed or not; it is not dropped as if the server had sent nothing.
#[test]
fn a_refusal_or_reasoning_that_is_not_a_string_is_refused_not_dropped() {
    let binding = binding();
    for delta in [
        r#"{"refusal":7}"#,
        r#"{"refusal":{"text":"I can't"}}"#,
        r#"{"reasoning":{"text":"Weighing it"}}"#,
        r#"{"reasoning":["Weighing it"]}"#,
    ] {
        let body = format!(
            "{}{}data: [DONE]\n\n",
            chunk(delta, "null"),
            chunk("{}", "\"stop\"")
        );
        let (events, outcome) =
            llm_chat::project_response_bytes(body.as_bytes(), binding.provenance());
        assert!(
            outcome.is_err(),
            "{delta}: a field the decoder reads, in a shape the wire does not write, was dropped: \
             events {events:?}, outcome {outcome:?}"
        );
    }
    let completion = json!({
        "id": "chatcmpl-adv", "object": "chat.completion", "model": "m",
        "choices": [{"index": 0, "finish_reason": "stop",
            "message": {"role": "assistant", "content": null, "refusal": {"text": "I can't"}}}],
    });
    let outcome = decode_completion(&completion, binding.provenance());
    assert!(
        outcome.is_err(),
        "message.refusal that is not a string was dropped: {outcome:?}"
    );
}

/// What the gateway answers on this wire for a turn the model refused, a client sends back as
/// the next request's history; the gateway reads it as the refusal it wrote.
#[test]
fn a_refused_answer_the_gateway_wrote_is_read_back_as_history() {
    let refusal = Item::AssistantRefusal {
        text: "I can't help with that.".to_owned(),
    };
    let answer = TurnOutcome {
        stop_reason: StopReason::EndTurn,
        items: vec![refusal.clone()],
        observation: TurnObservation::new(binding().provenance().clone()),
    };
    let body = encode_ingress_completion(&answer, "chatcmpl-adv", 1).expect("encoded");
    let echoed = body["choices"][0]["message"].clone();
    assert_eq!(echoed["refusal"], json!("I can't help with that."));
    let next = json!({
        "model": "small",
        "messages": [
            {"role": "user", "content": "Help?"},
            echoed,
            {"role": "user", "content": "Why not?"},
        ],
    });
    let decoded = decode_ingress_request(&next);
    match decoded {
        Ok(ingress) => assert_eq!(
            ingress.request.items,
            vec![Item::user("Help?"), refusal, Item::user("Why not?")]
        ),
        Err(error) => panic!(
            "the gateway refuses the assistant message it wrote itself: {:?} {}",
            error.code, error.message
        ),
    }
}
