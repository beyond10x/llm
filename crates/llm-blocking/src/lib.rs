#![forbid(unsafe_code)]

//! A blocking turn adapter over any asynchronous [`llm_core::Model`], for a synchronous agent loop.
//!
//! [`BlockingModel::turn`] runs one turn to completion on the calling thread and hands each event
//! to a [`BlockingSink`] as it is decoded. The adapter owns a small runtime ([`BlockingModel::new`])
//! or borrows a caller's multi-thread one ([`BlockingModel::on_runtime`]); either way the runtime's
//! worker drives the I/O while the calling thread polls the turn. A turn takes `&self`, so one
//! adapter serves several threads at once, and [`BlockingModel::fork`] hands out an owned handle
//! on the same client and runtime for a thread of its own. Cancellation is the caller's
//! [`Cancel`], which any thread may trigger.

use llm_core::{
    BoxFuture, Cancel, Capabilities, Error, ErrorCode, Model, Provenance, StreamEvent, StreamSink,
    TurnOutcome, TurnRequest,
};
use std::{fmt, sync::Arc};
use tokio::runtime::{Builder, Handle, Runtime, RuntimeFlavor};

/// Receives a turn's events on the thread that called [`BlockingModel::turn`].
///
/// Refusal is observable, never dropped: an error ends the turn with that error.
pub trait BlockingSink: Send {
    /// # Errors
    /// Returns the failure that ends the turn when this sink cannot accept `event`.
    fn emit(&mut self, event: StreamEvent) -> Result<(), Error>;
}

impl<F> BlockingSink for F
where
    F: FnMut(StreamEvent) -> Result<(), Error> + Send,
{
    fn emit(&mut self, event: StreamEvent) -> Result<(), Error> {
        self(event)
    }
}

/// A blocking adapter over one model client. Shareable across threads and cheap to fork.
pub struct BlockingModel {
    model: Arc<dyn Model>,
    handle: Handle,
    /// Present when the adapter built its runtime; the last fork to drop shuts it down.
    owned: Option<Arc<OwnedRuntime>>,
}

/// A runtime the adapter built. Shut down in the background on drop, so dropping the last
/// handle inside an asynchronous context does not panic.
struct OwnedRuntime(Option<Runtime>);

impl Drop for OwnedRuntime {
    fn drop(&mut self) {
        if let Some(runtime) = self.0.take() {
            runtime.shutdown_background();
        }
    }
}

impl BlockingModel {
    /// An adapter that owns a runtime with one worker thread to drive the turn's I/O.
    ///
    /// # Errors
    /// `Unavailable` when the runtime cannot be started.
    pub fn new(model: Arc<dyn Model>) -> Result<Self, Error> {
        let runtime = Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("llm-blocking")
            .enable_all()
            .build()
            .map_err(|error| {
                Error::new(
                    ErrorCode::Unavailable,
                    format!("the blocking adapter could not start its runtime: {error}"),
                )
            })?;
        Ok(Self {
            model,
            handle: runtime.handle().clone(),
            owned: Some(Arc::new(OwnedRuntime(Some(runtime)))),
        })
    }

    /// An adapter that borrows the caller's runtime, which must outlive every turn.
    ///
    /// # Errors
    /// `Unsupported` unless `runtime` is a multi-thread runtime: a current-thread runtime is
    /// driven only by its own `block_on`, so a turn on a handle to one would never progress.
    pub fn on_runtime(model: Arc<dyn Model>, runtime: Handle) -> Result<Self, Error> {
        if runtime.runtime_flavor() != RuntimeFlavor::MultiThread {
            return Err(Error::unsupported(
                "a blocking adapter needs a multi-thread runtime to drive its I/O",
            ));
        }
        Ok(Self {
            model,
            handle: runtime,
            owned: None,
        })
    }

    pub fn provenance(&self) -> &Provenance {
        self.model.provenance()
    }

    pub fn capabilities(&self) -> &Capabilities {
        self.model.capabilities()
    }

    /// An owned handle on the same client and runtime, for a loop on another thread.
    ///
    /// Budgets, approvals and cancellation stay with the caller: a fork shares none of them.
    #[must_use]
    pub fn fork(&self) -> Self {
        Self {
            model: Arc::clone(&self.model),
            handle: self.handle.clone(),
            owned: self.owned.clone(),
        }
    }

    /// Runs exactly one turn to completion, blocking the calling thread.
    ///
    /// # Errors
    /// The model's own failure, the sink's refusal, `Cancelled` once `cancel` fires, or
    /// `Unsupported` (nothing sent) when called from inside an asynchronous runtime, where
    /// blocking would stall that runtime's thread.
    pub fn turn(
        &self,
        request: &TurnRequest,
        sink: &mut dyn BlockingSink,
        cancel: &Cancel,
    ) -> Result<TurnOutcome, Error> {
        if Handle::try_current().is_ok() {
            return Err(Error::unsupported(
                "a blocking turn cannot run inside an asynchronous runtime; await the model instead",
            ));
        }
        let mut sink = Forward(sink);
        self.handle
            .block_on(self.model.turn(request, &mut sink, cancel))
    }
}

impl fmt::Debug for BlockingModel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BlockingModel")
            .field("provenance", self.provenance())
            .field("owns_runtime", &self.owned.is_some())
            .finish_non_exhaustive()
    }
}

/// Presents a blocking sink to the asynchronous port.
struct Forward<'a>(&'a mut dyn BlockingSink);

impl StreamSink for Forward<'_> {
    fn emit(&mut self, event: StreamEvent) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move { self.0.emit(event) })
    }
}
