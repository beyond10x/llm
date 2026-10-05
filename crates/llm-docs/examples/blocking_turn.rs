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
