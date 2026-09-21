//! Adversarial stream-decoder cases. No implementation file is edited here.
mod support;

use llm_core::{Cancel, Error, Item, TurnOutcome, TurnRequest, VecSink};
use llm_messages::decode_stream;
use serde_json::{Value, json};
use support::binding;

fn request() -> TurnRequest {
    TurnRequest::new("internal-model", vec![Item::user("Summarise the log")])
}

fn sse(events: &[Value]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for event in events {
        let name = event["type"].as_str().expect("event type");
        bytes.extend_from_slice(format!("event: {name}\ndata: {event}\n\n").as_bytes());
    }
    bytes
}

async fn decode(events: &[Value]) -> (Result<TurnOutcome, Error>, VecSink) {
    let mut sink = VecSink::new(64, 64 * 1024);
    let outcome = decode_stream(
        &sse(events),
        &request(),
        binding().provenance(),
        &mut sink,
        &Cancel::new(),
    )
    .await;
    (outcome, sink)
}

/// The Implementation contract requires the decoder to "Refuse ... duplicate or out-of-order
/// transitions", and `docs/messages.md` lines 42-45 enumerate the transitions it refuses: a
/// second `message_start`, content before the message started, a delta for a block that never
/// started, two blocks at one index, a block still open at the terminal event, and a payload
/// after it.
///
/// `StreamDecoder` deliberately holds several blocks open at once (`blocks: BTreeMap<u64, Block>`)
/// and appends each one to `items` at its `content_block_stop`
/// (`crates/llm-messages/src/decode.rs:413`). The order of `items` is therefore the order the
/// stops arrived in, not the order of the block indices and not the order of the deltas the
/// caller was already shown. Stopping two open blocks in the wrong order is neither refused nor
/// ordered: it silently reverses the assembled turn, so the same stream yields one order through
/// the sink and the opposite order in the outcome.
///
/// Refusing the out-of-order stop satisfies this case; so does assembling by index. Accepting it
/// and reordering the content does not.
#[tokio::test]
async fn interleaved_content_blocks_keep_the_order_the_caller_was_streamed() {
    let events = vec![
        json!({"type":"message_start","message":{"id":"msg_014a","type":"message",
            "role":"assistant","model":"example-model-20260201","content":[],
            "stop_reason":null,"stop_sequence":null,
            "usage":{"input_tokens":11,"cache_read_input_tokens":0,
                "cache_creation_input_tokens":0,"output_tokens":1}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"text_delta","text":"First"}}),
        json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":1,
            "delta":{"type":"text_delta","text":"Second"}}),
        // The two stops arrive in the opposite order to the two starts.
        json!({"type":"content_block_stop","index":1}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},
            "usage":{"input_tokens":11,"cache_read_input_tokens":0,
                "cache_creation_input_tokens":0,"output_tokens":8}}),
        json!({"type":"message_stop"}),
    ];
    let (outcome, sink) = decode(&events).await;
    let Ok(outcome) = outcome else {
        // Refusing the out-of-order transition is a correct answer to this case.
        return;
    };
    let assembled: String = outcome
        .items
        .iter()
        .filter_map(|item| match item {
            Item::AssistantText { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        assembled,
        sink.text(),
        "the assembled turn is in a different order from the deltas the caller was streamed",
    );
}

/// `docs/messages.md` line 13 states that `StreamDecoder` / `decode_stream` "Decodes the event
/// stream through the same response codec", and both halves share `MESSAGE_FIELDS`.
///
/// They do not share the check that the thing being decoded is a response.
/// `read_message` refuses a message that does not announce itself as an assistant message
/// (`crates/llm-messages/src/decode.rs:160`); `start_message` reads the identical inner object
/// through the identical field list and never looks at `type` or `role`
/// (`crates/llm-messages/src/decode.rs:270`). A stream that announces a *user* message decodes
/// into a completed assistant turn.
#[tokio::test]
async fn a_message_start_that_is_not_an_assistant_message_is_refused() {
    let events = vec![
        json!({"type":"message_start","message":{"id":"msg_014a","type":"error",
            "role":"user","model":"example-model-20260201","content":[],
            "stop_reason":null,"stop_sequence":null,
            "usage":{"input_tokens":11,"cache_read_input_tokens":0,
                "cache_creation_input_tokens":0,"output_tokens":1}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,
            "delta":{"type":"text_delta","text":"Hello"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},
            "usage":{"input_tokens":11,"cache_read_input_tokens":0,
                "cache_creation_input_tokens":0,"output_tokens":8}}),
        json!({"type":"message_stop"}),
    ];
    let (outcome, _) = decode(&events).await;
    let accepted = outcome.map(|outcome| outcome.items);
    assert!(
        accepted.is_err(),
        "a stream whose message_start announces type \"error\" and role \"user\" decoded into a \
         completed assistant turn: {accepted:?}",
    );
}

/// The class that beat two sibling units: usage fixtures that only ever exercise the complete,
/// in-subset case. `unreported-counter-stays-unknown` drops both cache counters together and
/// nothing drops them one at a time, so the three-way guard in `usage.rs::normalized` is only
/// ever observed all-present or all-absent.
///
/// Each of the three components of the inclusive input is dropped here in turn while the other
/// two are present. A dropped component must leave `input_tokens` unknown and must not leave the
/// counters that *were* reported behind.
#[test]
fn each_component_of_the_inclusive_input_absent_in_turn_leaves_it_unknown() {
    let complete = json!({"input_tokens":11,"cache_read_input_tokens":4,
        "cache_creation_input_tokens":6,"output_tokens":8});
    for dropped in [
        "input_tokens",
        "cache_read_input_tokens",
        "cache_creation_input_tokens",
    ] {
        let mut usage = complete.clone();
        usage.as_object_mut().unwrap().remove(dropped);
        let message = json!({"id":"msg_014a","type":"message","role":"assistant",
            "model":"example-model-20260201","content":[{"type":"text","text":"Hello"}],
            "stop_reason":"end_turn","stop_sequence":null,"usage":usage});
        let outcome = llm_messages::decode_message(&message, &request(), binding().provenance())
            .unwrap_or_else(|error| panic!("{dropped} absent: {error:?}"));
        let reported = outcome
            .observation
            .usage
            .expect("the reported counters are kept");
        assert_eq!(
            reported.input_tokens, None,
            "{dropped} was unreported and the inclusive input was still produced",
        );
        // The components that were reported must survive the missing one.
        for (name, value) in [
            ("cache_read_input_tokens", reported.cached_input_tokens),
            (
                "cache_creation_input_tokens",
                reported.cache_creation_input_tokens,
            ),
        ] {
            assert_eq!(
                value.is_some(),
                name != dropped,
                "{name} after dropping {dropped}",
            );
        }
        assert_eq!(reported.output_tokens, Some(8), "after dropping {dropped}");
    }
}
