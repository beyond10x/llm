---
title: Interrupt a turn with new input
sidebar_position: 5
description: Cancel a turn that is still answering, keep the text it already showed as assistant text, and send the new input in the next turn.
lede: llm's clients cannot send input into an answer that is still streaming, so new input cancels the running turn and starts the next one with what the first turn left.
source: crates/llm-core (port.rs, vocabulary.rs, turn.rs), crates/llm-docs/examples/interrupt_turn.rs, docs/design.md (Input into a running turn)
---

# Interrupt a turn with new input

A voice or chat front end can receive new input while the model is still answering. llm cannot
send that input into the running turn. Each client sends one request over HTTP and reads the answer
as server-sent events, and over that exchange none of Responses, Messages and Chat Completions
accepts input into an answer that is still streaming. So every client in llm declares
`input_during_output: false`; a `Model` built in code keeps its own claim.

:::note[Not offered: Responses steering]
OpenAI documents [mid-turn steering](https://developers.openai.com/api/docs/guides/steering) for
Responses: a `response.steer` event sent on a Responses WebSocket connection, for the GPT-6 model
family. llm's Responses client does not implement WebSocket mode, so llm does not offer steering.
Offering it is planned.
:::

The supported pattern is cancel, then continue: cancel the running turn, keep what it already
showed, and start a new turn that carries it and the new input.

## Run it

```bash
cargo run --locked -p llm-docs --example interrupt_turn
```

```text
input during output: false
turn 1 showed: "Tomorrow starts dry,"
turn 1 ended:  Err(Cancelled)
turn 2 sends:  {"kind":"user-text","text":"What will the weather be like tomorrow?"}
               {"kind":"assistant-text","text":"Tomorrow starts dry,"}
               {"kind":"user-text","text":"Sorry, I meant on Sunday."}
turn 2 showed: "On Sunday it stays dry all day."
turn 2 ended:  Ok(EndTurn)
```

The first turn is cancelled after its third word. The second request carries the question, the
three words the caller was shown as assistant text, and the new input, and the model answers it in
full.

## The program

The model streams in-process, so the run sends nothing and reads no credential. The first request
offers one tool, `forecast`, so a turn can also end in a tool call. Any `Model` that supports
tools, such as a `ResponsesClient`, a `MessagesClient` or a `ChatClient` whose binding declares
them, goes in its place; a model whose capabilities declare no tools refuses that request before
sending it. The sink cancels the turn after
the third word to stand in for input arriving; in a real program the task that receives the input
calls `cancel.cancel()`. The tests at the end check each way the first turn can end.

```rust title="crates/llm-docs/examples/interrupt_turn.rs"
//! Interrupt a turn that is still answering: cancel it, keep the text it already showed, and send
//! the new input in the next turn.
//!
//! Run with `cargo run --locked -p llm-docs --example interrupt_turn`. llm's clients send a request
//! and read its streamed answer over HTTP, and cannot send input into an answer still streaming, so
//! this is the supported pattern. The model streams in-process, so nothing is sent and no
//! credential is read. Any `Model` that supports tools, such as a `ResponsesClient` or a
//! `MessagesClient` whose binding declares them, goes in its place: the first request offers one
//! tool, and a model whose capabilities declare no tools refuses it before sending.
use llm_core::{
    BoxFuture, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, InvalidId, Item, Model,
    Protocol, Provenance, StopReason, StreamEvent, StreamSink, ToolName, ToolSpec, TurnObservation,
    TurnOutcome, TurnRequest,
};
use serde_json::json;
use std::sync::Mutex;

const QUESTION: &str = "What will the weather be like tomorrow?";
const NEW_INPUT: &str = "Sorry, I meant on Sunday.";
/// The reason this caller gives for a tool call it rejects because the user changed the request.
const NOT_RUN: &str = "Not run: the user changed the question.";
/// The text deltas of the model's first answer, and of every later one.
const FIRST_ANSWER: &[&str] = &[
    "Tomorrow", " starts", " dry,", " then", " rain", " moves", " in", " from", " the", " west.",
];
const LATER_ANSWER: &[&str] = &["On", " Sunday", " it", " stays", " dry", " all", " day."];

/// A model that streams its answer word by word and keeps every request it is sent.
struct Speaking {
    target: Provenance,
    capabilities: Capabilities,
    sent: Mutex<Vec<TurnRequest>>,
}

impl Model for Speaking {
    fn provenance(&self) -> &Provenance {
        &self.target
    }
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }
    fn turn<'a>(
        &'a self,
        request: &'a TurnRequest,
        sink: &'a mut dyn StreamSink,
        cancel: &'a Cancel,
    ) -> BoxFuture<'a, Result<TurnOutcome, Error>> {
        Box::pin(async move {
            request.validate_for(&self.target, &self.capabilities)?;
            let answer = {
                let mut sent = self
                    .sent
                    .lock()
                    .map_err(|_| Error::protocol("a turn panicked"))?;
                sent.push(request.clone());
                if sent.len() == 1 {
                    FIRST_ANSWER
                } else {
                    LATER_ANSWER
                }
            };
            for word in answer {
                // As on Responses and Messages: the cancel races the sink's `emit` and wins a tie.
                tokio::select! {
                    biased;
                    () = cancel.cancelled() => {
                        return Err(Error::cancelled().with_dispatch(Dispatch::Accepted));
                    }
                    shown = sink.emit(StreamEvent::TextDelta { text: (*word).to_owned() }) => shown?,
                }
            }
            Ok(TurnOutcome {
                stop_reason: StopReason::EndTurn,
                items: vec![Item::assistant(answer.concat())],
                observation: TurnObservation {
                    final_usage: true,
                    ..TurnObservation::new(self.target.clone())
                },
            })
        })
    }
}

/// The caller's side of the stream: it keeps the text it was shown, and cancels the turn when new
/// input arrives. Here the input arrives after `interrupt_after` text deltas; in a real program the
/// task that receives the input fires the `Cancel`.
struct Listener {
    shown: String,
    deltas: usize,
    interrupt_after: Option<usize>,
    cancel: Cancel,
}

impl Listener {
    fn new(interrupt_after: Option<usize>, cancel: &Cancel) -> Self {
        Self {
            shown: String::new(),
            deltas: 0,
            interrupt_after,
            cancel: cancel.clone(),
        }
    }
}

impl StreamSink for Listener {
    fn emit(&mut self, event: StreamEvent) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            // A delta is shown once this `emit` completes: a client may drop an `emit` still in
            // progress when the cancel fires, so a sink that shows text across an `.await`
            // records it after that `.await`. This one shows it at once.
            if let StreamEvent::TextDelta { text } = event {
                self.shown.push_str(&text);
                self.deltas += 1;
                if self.interrupt_after == Some(self.deltas) {
                    self.cancel.cancel();
                }
            }
            Ok(())
        })
    }
}

/// What one turn showed its caller, and how it ended.
struct Turn {
    shown: String,
    ended: Result<StopReason, ErrorCode>,
}

/// Asks a question, interrupts the answer with new input after three words, and continues.
async fn converse(model: &dyn Model) -> Result<[Turn; 2], Error> {
    let mut first = TurnRequest::new("example", vec![Item::user(QUESTION)]);
    first.tools = vec![ToolSpec {
        name: ToolName::new("forecast").map_err(|_| Error::invalid("tool name"))?,
        description: "The forecast for one day.".to_owned(),
        input_schema: json!({"type": "object", "properties": {"day": {"type": "string"}}}),
    }];
    let cancel = Cancel::new();
    let mut listener = Listener::new(Some(3), &cancel);
    let returned = model.turn(&first, &mut listener, &cancel).await;

    // The next turn carries the conversation so far, what the first turn left, then the input.
    let mut items = first.items.clone();
    let ended = match returned {
        // Cancelled part-way: the text it showed is what the model said, so it is carried as
        // assistant text. A turn cancelled before it showed anything carries no assistant item.
        Err(error) if error.code == ErrorCode::Cancelled => {
            if !listener.shown.is_empty() {
                items.push(Item::assistant(listener.shown.as_str()));
            }
            Err(error.code)
        }
        // It finished before the cancel reached it, so its outcome's items are carried: each
        // message carries the text shown for it (the server's, where none was shown), unless a
        // `shown-text-unpaired` warning said shown text could not be paired with one message.
        // Every call it proposed needs a result before the next turn, or that turn is refused
        // before it is sent. This caller runs none of them now, so each is rejected with its
        // reason, as a failed result.
        Ok(outcome) => {
            let open: Vec<_> = outcome
                .items
                .iter()
                .filter_map(Item::as_tool_call)
                .map(|call| call.call_id.clone())
                .collect();
            items.extend(outcome.items);
            for call_id in open {
                items.push(Item::ToolResult {
                    call_id,
                    output: json!(NOT_RUN),
                    failed: true,
                });
            }
            Ok(outcome.stop_reason)
        }
        Err(error) => return Err(error),
    };
    items.push(Item::user(NEW_INPUT));
    let interrupted = Turn {
        shown: listener.shown,
        ended,
    };

    let mut next = first.clone();
    next.items = items;
    let cancel = Cancel::new();
    let mut listener = Listener::new(None, &cancel);
    let outcome = model.turn(&next, &mut listener, &cancel).await?;
    let answered = Turn {
        shown: listener.shown,
        ended: Ok(outcome.stop_reason),
    };
    Ok([interrupted, answered])
}

fn model() -> Result<Speaking, InvalidId> {
    Ok(Speaking {
        target: Provenance {
            protocol: Protocol::Messages,
            provider: Id::new("local")?,
            account: Id::new("none")?,
            endpoint: Id::new("in-process")?,
            model: Id::new("example")?,
            binding_revision: Id::new("speaking-v1")?,
        },
        capabilities: Capabilities {
            tools: true,
            ..Capabilities::text(4096, 1024)
        },
        sent: Mutex::new(Vec::new()),
    })
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let model = model()?;
    let declared = model.capabilities().input_during_output;
    println!("input during output: {declared}");
    let [first, second] = converse(&model).await?;
    println!("turn 1 showed: {:?}", first.shown);
    println!("turn 1 ended:  {:?}", first.ended);
    let sent = model.sent.lock().map_err(|_| "a turn panicked")?;
    for (index, item) in sent[1].items.iter().enumerate() {
        let label = if index == 0 { "turn 2 sends:" } else { "" };
        println!("{label:14} {}", serde_json::to_string(item)?);
    }
    println!("turn 2 showed: {:?}", second.shown);
    println!("turn 2 ended:  {:?}", second.ended);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use llm_core::{CallId, ToolCall};

    #[tokio::test]
    async fn the_next_turn_carries_exactly_the_text_shown_then_the_new_input() {
        let model = model().unwrap();
        let [first, _] = converse(&model).await.unwrap();
        assert_eq!(first.ended, Err(ErrorCode::Cancelled));
        assert_eq!(first.shown, "Tomorrow starts dry,");
        let sent = model.sent.lock().unwrap();
        assert_eq!(
            sent[1].items,
            vec![
                Item::user(QUESTION),
                Item::assistant("Tomorrow starts dry,"),
                Item::user(NEW_INPUT)
            ]
        );
    }

    /// How the first turn of a [`Racing`] model ends.
    #[derive(Clone, Copy)]
    enum First {
        /// It shows three words and finishes before the caller's cancel reaches it.
        Finished,
        /// The same, ending in a call to the `forecast` tool.
        FinishedInACall,
        /// The same, ending in two calls to it.
        FinishedInTwoCalls,
        /// It is cancelled before it shows anything.
        CancelledAtOnce,
    }

    /// A model whose first turn ends as [`First`] says; later turns answer in full. Like a
    /// client, it refuses a request it could not send and checks its own outcome.
    struct Racing {
        inner: Speaking,
        first: First,
    }

    /// A call to the `forecast` tool, and the result that rejects it.
    fn call(id: &str) -> ToolCall {
        ToolCall {
            call_id: CallId::new(id).unwrap(),
            name: ToolName::new("forecast").unwrap(),
            arguments: json!({"day": id}),
        }
    }
    fn rejected(id: &str) -> Item {
        Item::ToolResult {
            call_id: CallId::new(id).unwrap(),
            output: json!(NOT_RUN),
            failed: true,
        }
    }

    impl Model for Racing {
        fn provenance(&self) -> &Provenance {
            &self.inner.target
        }
        fn capabilities(&self) -> &Capabilities {
            &self.inner.capabilities
        }
        fn turn<'a>(
            &'a self,
            request: &'a TurnRequest,
            sink: &'a mut dyn StreamSink,
            _cancel: &'a Cancel,
        ) -> BoxFuture<'a, Result<TurnOutcome, Error>> {
            Box::pin(async move {
                request.validate_for(&self.inner.target, &self.inner.capabilities)?;
                let number = {
                    let mut sent = self.inner.sent.lock().unwrap();
                    sent.push(request.clone());
                    sent.len()
                };
                if number == 1 && matches!(self.first, First::CancelledAtOnce) {
                    return Err(Error::cancelled().with_dispatch(Dispatch::Accepted));
                }
                let answer: &[&str] = if number == 1 {
                    &FIRST_ANSWER[..3]
                } else {
                    LATER_ANSWER
                };
                for word in answer {
                    sink.emit(StreamEvent::TextDelta {
                        text: (*word).to_owned(),
                    })
                    .await?;
                }
                let mut outcome = TurnOutcome {
                    stop_reason: StopReason::EndTurn,
                    items: vec![Item::assistant(answer.concat())],
                    observation: TurnObservation {
                        final_usage: true,
                        ..TurnObservation::new(self.inner.target.clone())
                    },
                };
                let calls: &[&str] = match (number, self.first) {
                    (1, First::FinishedInACall) => &["call-1"],
                    (1, First::FinishedInTwoCalls) => &["call-1", "call-2"],
                    _ => &[],
                };
                for id in calls {
                    outcome.stop_reason = StopReason::ToolCalls;
                    outcome.items.push(Item::ToolCall(call(id)));
                }
                outcome.validate_for(request, &self.inner.target)?;
                Ok(outcome)
            })
        }
    }

    #[tokio::test]
    async fn a_turn_that_finished_before_the_cancel_reached_it_is_carried_as_its_outcome() {
        let model = Racing {
            inner: model().unwrap(),
            first: First::Finished,
        };
        let [first, _] = converse(&model).await.unwrap();
        assert_eq!(first.ended, Ok(StopReason::EndTurn));
        let sent = model.inner.sent.lock().unwrap();
        assert_eq!(
            sent[1].items,
            vec![
                Item::user(QUESTION),
                Item::assistant("Tomorrow starts dry,"),
                Item::user(NEW_INPUT)
            ]
        );
    }

    #[tokio::test]
    async fn a_call_a_finished_turn_proposed_is_answered_as_rejected_before_the_new_input() {
        let model = Racing {
            inner: model().unwrap(),
            first: First::FinishedInACall,
        };
        let [first, _] = converse(&model).await.unwrap();
        assert_eq!(first.ended, Ok(StopReason::ToolCalls));
        let sent = model.inner.sent.lock().unwrap();
        assert_eq!(
            sent[1].items,
            vec![
                Item::user(QUESTION),
                Item::assistant("Tomorrow starts dry,"),
                Item::ToolCall(call("call-1")),
                rejected("call-1"),
                Item::user(NEW_INPUT)
            ]
        );
    }

    #[tokio::test]
    async fn every_call_a_finished_turn_proposed_is_answered_before_the_new_input() {
        let model = Racing {
            inner: model().unwrap(),
            first: First::FinishedInTwoCalls,
        };
        let [first, _] = converse(&model).await.unwrap();
        assert_eq!(first.ended, Ok(StopReason::ToolCalls));
        let sent = model.inner.sent.lock().unwrap();
        assert_eq!(
            sent[1].items,
            vec![
                Item::user(QUESTION),
                Item::assistant("Tomorrow starts dry,"),
                Item::ToolCall(call("call-1")),
                Item::ToolCall(call("call-2")),
                rejected("call-1"),
                rejected("call-2"),
                Item::user(NEW_INPUT)
            ]
        );
    }

    #[tokio::test]
    async fn a_turn_cancelled_before_it_showed_anything_carries_no_assistant_item() {
        let model = Racing {
            inner: model().unwrap(),
            first: First::CancelledAtOnce,
        };
        let [first, _] = converse(&model).await.unwrap();
        assert_eq!(first.ended, Err(ErrorCode::Cancelled));
        assert_eq!(first.shown, "");
        let sent = model.inner.sent.lock().unwrap();
        assert_eq!(
            sent[1].items,
            vec![Item::user(QUESTION), Item::user(NEW_INPUT)]
        );
    }
}
```

Run its tests with `cargo test --locked -p llm-docs --example interrupt_turn`.

## How it behaves

- **The declaration.** `capabilities().input_during_output` is `false` for every client in llm,
  and a binding, catalog or routes document that declares it `true` is refused. A caller that
  schedules from the declaration reads it and uses this pattern.
- **What was shown is final.** No stream event is ever retracted, so the text deltas the sink
  accepted are what the model said. A cancelled turn returns an error with code `cancelled` and
  dispatch `accepted`, and no outcome: the sink is the only record of what it showed.
- **Where the cancel lands.** Every client checks the cancel at each read of the stream. The
  Responses and Messages clients also race it against each `emit`, the cancel first, and drop an
  `emit` still in progress when it fires; the Chat Completions client hands over the events of the
  payload it read last. So a delta counts as shown only once its `emit` completes: a sink that
  shows text across an `.await` records it after that `.await`.
- **The next request.** The conversation so far, then what the first turn left, then the new input
  as `Item::user`. What the first turn left depends on how it ended:
  - **Cancelled:** the shown text as one `Item::assistant`, or nothing when nothing was shown.
  - **Finished before the cancel reached it:** `outcome.items`. Each assistant message carries the
    text deltas shown for it, and a message no delta was shown for carries the server's text. On
    Responses, shown text the client cannot pair with exactly one message is carried in none, and
    a `shown-text-unpaired` warning on the sink says so.
  - **Finished in tool calls:** `outcome.items`, then a result for every call, because a request
    with an unanswered call is refused before it is sent. Run the tool and carry its result, or
    reject the call with a failed `Item::ToolResult` whose output is your reason, as the program
    does.
- **Only text crosses from a cancelled turn.** It returns no outcome, so no tool call, reasoning
  or opaque state from it reaches the next request.
- **No retry and no fallback.** A cancelled turn is never retried, and a routed run starts no
  further attempt after the cancel.

The decision and the code it rests on are in
[docs/design.md](https://github.com/beyond10x/llm/blob/main/docs/design.md#input-into-a-running-turn).
