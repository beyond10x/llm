use llm_core::Error;
use tokio::runtime::Handle;

/// What a call outside a Tokio runtime is told: the requirement, and the adapter that serves a
/// caller without one.
const NO_RUNTIME: &str =
    "an llm client needs a Tokio runtime; a caller without one uses b10x-llm-blocking";

/// Refuses a call polled on a thread that is in no Tokio runtime, before it does anything that
/// needs one.
///
/// The transport bounds every exchange with Tokio timers and connects through `reqwest` on
/// Tokio's reactor, and a credential file is read on Tokio's blocking pool; outside a runtime
/// each of them panics. Every protocol client calls this first in its turn, before it resolves a
/// credential, and so does every exchange of [`HttpClient`](crate::HttpClient) and every read of
/// an [`SseStream`](crate::SseStream) that has to wait for the wire.
///
/// It asks only whether a runtime is entered on the calling thread. A runtime built without its
/// time and IO drivers passes, and Tokio still panics there.
///
/// # Errors
/// `unsupported`, dispatch `not-sent`, not retriable, with a fixed message that names the
/// requirement and `b10x-llm-blocking`, when the calling thread is in no Tokio runtime.
pub fn require_tokio_runtime() -> Result<(), Error> {
    if Handle::try_current().is_ok() {
        Ok(())
    } else {
        Err(Error::unsupported(NO_RUNTIME))
    }
}
