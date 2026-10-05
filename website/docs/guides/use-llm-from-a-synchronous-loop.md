---
title: Use llm from a synchronous loop
sidebar_position: 4
description: Run model turns from blocking code with BlockingModel, see events as they arrive, and serve two threads from one client.
lede: BlockingModel runs one asynchronous turn to completion on the calling thread, so a synchronous agent loop needs no runtime of its own.
source: crates/llm-blocking (lib.rs, tests/blocking.rs), crates/llm-docs/examples/blocking_turn.rs
---

# Use llm from a synchronous loop

Every llm client is asynchronous. An agent loop written as plain blocking code wraps one in
`b10x-llm-blocking`'s `BlockingModel` and calls `turn` like any other function.

## Run it

```bash
cargo run --locked -p llm-docs --example blocking_turn
```

```text
streamed: one blocking turn
stop:     EndTurn
second:   EndTurn
```

## The program

The model streams its answer in-process, so the run sends nothing and reads no credential. Any
`Model`, such as a `ResponsesClient` or a `MessagesClient`, goes in its place.

```rust title="crates/llm-docs/examples/blocking_turn.rs"
//! Run model turns from a synchronous loop, on two threads at once.
//!
//! Run with `cargo run --locked -p llm-docs --example blocking_turn`. The model streams its answer
//! in-process, so nothing is sent and no credential is read; any `Model`, such as a
//! `ResponsesClient` or a `MessagesClient`, goes in its place.
use llm_blocking::BlockingModel;
use llm_core::{
    BoxFuture, Cancel, Capabilities, Error, Id, Item, Model, Protocol, Provenance, StopReason,
    StreamEvent, StreamSink, TurnObservation, TurnOutcome, TurnRequest,
};
use std::sync::Arc;

/// A model that streams its answer word by word.
struct Streaming {
    target: Provenance,
    capabilities: Capabilities,
}

impl Model for Streaming {
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
        _cancel: &'a Cancel,
    ) -> BoxFuture<'a, Result<TurnOutcome, Error>> {
        Box::pin(async move {
            request.validate_for(&self.target, &self.capabilities)?;
            let answer = "one blocking turn";
            for word in answer.split(' ') {
                sink.emit(StreamEvent::TextDelta {
                    text: format!("{word} "),
                })
                .await?;
            }
            Ok(TurnOutcome {
                stop_reason: StopReason::EndTurn,
                items: vec![Item::assistant(answer)],
                observation: TurnObservation {
                    final_usage: true,
                    ..TurnObservation::new(self.target.clone())
                },
            })
        })
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let model: Arc<dyn Model> = Arc::new(Streaming {
        target: Provenance {
            protocol: Protocol::Messages,
            provider: Id::new("local")?,
            account: Id::new("none")?,
            endpoint: Id::new("in-process")?,
            model: Id::new("example")?,
            binding_revision: Id::new("streaming-v1")?,
        },
        capabilities: Capabilities::text(4096, 1024),
    });
    // The adapter owns a one-worker runtime; `BlockingModel::on_runtime` borrows yours instead.
    let blocking = BlockingModel::new(model)?;

    let worker = blocking.fork();
    let other = std::thread::spawn(move || {
        let request = TurnRequest::new("example", vec![Item::user("From a second thread")]);
        worker
            .turn(&request, &mut |_event| Ok(()), &Cancel::new())
            .map(|outcome| outcome.stop_reason)
    });

    let request = TurnRequest::new("example", vec![Item::user("Hello")]);
    let mut shown = String::new();
    let outcome = blocking.turn(
        &request,
        &mut |event| {
            if let StreamEvent::TextDelta { text } = event {
                shown.push_str(&text);
            }
            Ok(())
        },
        &Cancel::new(),
    )?;
    println!("streamed: {}", shown.trim_end());
    println!("stop:     {:?}", outcome.stop_reason);
    let stop = other.join().map_err(|_| "the second thread panicked")??;
    println!("second:   {stop:?}");
    Ok(())
}
```

## How it behaves

- **The runtime.** `BlockingModel::new` builds a runtime with one worker thread that drives the
  turn's I/O. `BlockingModel::on_runtime(model, handle)` borrows yours instead; it must be a
  multi-thread runtime, because a current-thread one is driven only by its own `block_on` and a
  turn on it would never progress, so it is refused as `unsupported`.
- **Events arrive as they are decoded.** Each event reaches the `BlockingSink` on the calling
  thread. A closure `FnMut(StreamEvent) -> Result<(), Error>` is a sink; returning an error ends the
  turn with that error.
- **Concurrent turns.** `turn` takes `&self`, so one adapter serves several threads, and
  `fork` hands out an owned handle on the same client and runtime for a thread of its own. Budgets,
  approvals and cancellation stay with the caller: a fork shares none of them.
- **Cancellation.** Pass a `Cancel`; any thread may fire it, and the turn ends with `cancelled`.
- **Not inside a runtime.** Called from inside an asynchronous runtime, `turn` refuses with
  `unsupported` and sends nothing, because blocking would stall that runtime's thread.
