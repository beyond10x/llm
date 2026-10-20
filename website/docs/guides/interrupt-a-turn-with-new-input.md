---
title: Interrupt a turn with new input
sidebar_position: 5
description: Cancel a turn that is still answering, keep the text it already showed as assistant text, and send the new input in the next turn.
lede: No protocol llm speaks carries input into a turn that is still streaming, so new input cancels the running turn and starts the next one with what was already shown.
source: crates/llm-core (port.rs, vocabulary.rs), crates/llm-docs/examples/interrupt_turn.rs, docs/design.md (Input into a running turn)
---

# Interrupt a turn with new input

A voice or chat front end can receive new input while the model is still answering. llm cannot
send that input into the running turn: Responses, Messages and Chat Completions each take one
request and stream one answer, and none accepts input into an answer that is still streaming. So
every model declares `input_during_output: false`.

The supported pattern is cancel, then continue: cancel the running turn, keep the text it already
showed as assistant text, and start a new turn that carries that text and the new input.

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

The model streams in-process, so the run sends nothing and reads no credential. Any `Model`, such
as a `ResponsesClient`, a `MessagesClient` or a `ChatClient`, goes in its place. The sink cancels
the turn after the third word to stand in for input arriving; in a real program the task that
receives the input calls `cancel.cancel()`. The test at the end checks the second request.

```rust title="crates/llm-docs/examples/interrupt_turn.rs"
//! Interrupt a turn that is still answering: cancel it, keep the text it already showed, and send
//! the new input in the next turn.
//!
//! Run with `cargo run --locked -p llm-docs --example interrupt_turn`. No protocol llm speaks
//! carries input into a turn that is still producing output, so this is the supported pattern. The
//! model streams in-process, so nothing is sent and no credential is read; any `Model`, such as a
//! `ResponsesClient` or a `MessagesClient`, goes in its place.
use llm_core::{
    BoxFuture, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, InvalidId, Item, Model,
    Protocol, Provenance, StopReason, StreamEvent, StreamSink, TurnObservation, TurnOutcome,
    TurnRequest,
};
use std::sync::Mutex;

const QUESTION: &str = "What will the weather be like tomorrow?";
const NEW_INPUT: &str = "Sorry, I meant on Sunday.";
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
                // A cancel wins over handing the sink its next event, as on Responses and Messages.
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
    let first = TurnRequest::new("example", vec![Item::user(QUESTION)]);
    let cancel = Cancel::new();
    let mut listener = Listener::new(Some(3), &cancel);
    let returned = model.turn(&first, &mut listener, &cancel).await;

    // The next turn carries the conversation so far, what the first turn showed, then the input.
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
        // It finished before the cancel reached it: its assistant text is the text it showed.
        Ok(outcome) => {
            items.extend(outcome.items);
            Ok(outcome.stop_reason)
        }
        Err(error) => return Err(error),
    };
    items.push(Item::user(NEW_INPUT));
    let interrupted = Turn {
        shown: listener.shown,
        ended,
    };

    let next = TurnRequest::new("example", items);
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
        capabilities: Capabilities::text(4096, 1024),
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

    #[tokio::test]
    async fn the_next_turn_carries_the_shown_text_as_assistant_text_then_the_new_input() {
        let model = model().unwrap();
        let [first, _] = converse(&model).await.unwrap();
        let sent = model.sent.lock().unwrap();
        assert_eq!(sent.len(), 2, "two turns were sent");

        // The first turn was cancelled part-way: it showed some of its answer, not all of it.
        let whole = FIRST_ANSWER.concat();
        assert_eq!(first.ended, Err(ErrorCode::Cancelled));
        assert!(
            !first.shown.is_empty() && first.shown != whole && whole.starts_with(&first.shown),
            "shown: {:?}",
            first.shown
        );

        // The second request: the first one's items, the shown text as assistant text, then the
        // new input, and nothing else.
        let mut expected = sent[0].items.clone();
        expected.push(Item::assistant(first.shown.as_str()));
        expected.push(Item::user(NEW_INPUT));
        assert_eq!(sent[1].items, expected);
    }
}
```

Run its test with `cargo test --locked -p llm-docs --example interrupt_turn`.

## How it behaves

- **The declaration.** `capabilities().input_during_output` is `false` for every client in llm,
  and a binding, catalog or routes document that declares it `true` is refused. A caller that
  schedules from the declaration reads it and uses this pattern.
- **What was shown is final.** No stream event is ever retracted, so the text deltas the sink
  accepted are what the model said. A cancelled turn returns an error with code `cancelled` and
  dispatch `accepted`, and no outcome: the sink is the only record of what it showed.
- **Where the cancel lands.** The Responses and Messages clients check the cancel before each
  event they hand the sink. The Chat Completions client checks it at each read of the stream, so
  the events of the payload it read last still arrive. Whatever arrives was shown, and is carried.
- **The next request.** The conversation so far, then the shown text as one `Item::assistant`,
  then the new input as `Item::user`. A turn cancelled before it showed anything carries no
  assistant item. A turn that finished before the cancel reached it returns its outcome instead:
  carry `outcome.items`, whose assistant text is exactly the text shown.
- **Only text crosses.** A cancelled turn returns no outcome, so no tool call, reasoning or opaque
  state from it reaches the next request.
- **No retry and no fallback.** A cancelled turn is never retried, and a routed run starts no
  further attempt after the cancel.

The decision and the code it rests on are in
[docs/design.md](https://github.com/beyond10x/llm/blob/main/docs/design.md#input-into-a-running-turn).
