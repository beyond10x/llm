use llm_core::Error;
use std::{
    future::{Future, poll_fn},
    pin::pin,
    task::Poll,
};
use tokio::runtime::Handle;

/// What a call outside a Tokio runtime is told: the requirement, and the two ways out.
const NO_RUNTIME: &str = "an llm client needs a Tokio runtime; run the call inside one, or use llm-blocking for a single model";

/// Refuses a call polled on a thread that is in no Tokio runtime, before it does anything that
/// needs one.
///
/// The transport bounds every exchange with Tokio timers and connects through `reqwest` on
/// Tokio's reactor, and a credential file is read on Tokio's blocking pool; outside a runtime
/// each of them panics. Every protocol client calls this first in its turn, before it validates
/// the request or resolves a credential, and so does every exchange of
/// [`HttpClient`](crate::HttpClient). Each later wait of a turn is guarded by
/// [`within_tokio_runtime`].
///
/// It asks only whether a runtime is entered on the calling thread. A runtime built without its
/// time and IO drivers passes, and Tokio still panics there.
///
/// # Errors
/// `unsupported`, dispatch `not-sent`, not retriable, with a fixed message that names the
/// requirement, running the call inside a runtime, and `llm-blocking` for a single model,
/// when the calling thread is in no Tokio runtime.
pub fn require_tokio_runtime() -> Result<(), Error> {
    if Handle::try_current().is_ok() {
        Ok(())
    } else {
        Err(Error::unsupported(NO_RUNTIME))
    }
}

/// Polls `work` only on a thread that is in a Tokio runtime.
///
/// Every poll first asks [`require_tokio_runtime`]. On a thread in no runtime it returns that
/// refusal without polling `work`, which is dropped with this future. That covers a wait moved
/// out of its runtime part-way: a timer created inside and polled outside does not panic, but it
/// waits on a runtime nobody may be driving any more, and a step after it that creates a timer
/// or reads a credential file panics. Pass a timer as an `async` block, so it is created on the
/// first poll, after the check.
///
/// # Errors
/// The refusal of [`require_tokio_runtime`], dispatch `not-sent`; a caller past dispatch states
/// its own.
pub async fn within_tokio_runtime<F: Future>(work: F) -> Result<F::Output, Error> {
    let mut work = pin!(work);
    poll_fn(|context| match require_tokio_runtime() {
        Ok(()) => work.as_mut().poll(context).map(Ok),
        Err(refusal) => Poll::Ready(Err(refusal)),
    })
    .await
}
