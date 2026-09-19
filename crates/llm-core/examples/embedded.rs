//! Run with `cargo run -p b10x-llm-core --example embedded`; no gateway or credentials required.
use llm_core::{
    BoxFuture, Cancel, Capabilities, Error, Id, Item, Model, Protocol, Provenance, StopReason,
    StreamEvent, StreamSink, TurnOutcome, TurnRequest, VecSink,
};

struct LocalModel {
    target: Provenance,
    capabilities: Capabilities,
}
impl Model for LocalModel {
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
            tokio::select! {
                biased;
                () = cancel.cancelled() => Err(Error::cancelled()),
                result = async {
                    sink.emit(StreamEvent::TextDelta { text: "An embedded model turn.".into() }).await?;
                    Ok(TurnOutcome { stop_reason: StopReason::EndTurn,
                        items: vec![Item::assistant("An embedded model turn.")], usage: None })
                } => result,
            }
        })
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let model: Box<dyn Model> = Box::new(LocalModel {
        target: Provenance {
            protocol: Protocol::ChatCompletions,
            provider: Id::new("local")?,
            account: Id::new("anonymous")?,
            endpoint: Id::new("in-process")?,
            model: Id::new("example")?,
            binding_revision: Id::new("embedded-model-v1")?,
        },
        capabilities: Capabilities::text(4096, 1024),
    });
    let request = TurnRequest::new("example", vec![Item::user("Hello")]);
    let mut sink = VecSink::new(16, 4096);
    let outcome = model.turn(&request, &mut sink, &Cancel::new()).await?;
    println!(
        "{} ({:?}; usage {:?})",
        sink.text(),
        outcome.stop_reason,
        outcome.usage
    );
    Ok(())
}
