//! The transport driven by an executor that is not Tokio (story:async-clients-without-a-tokio-runtime).
//!
//! Every exchange the transport offers, polled by [`block_on`] below on a thread that is in no
//! Tokio runtime, is refused with the typed error that names the requirement and
//! `b10x-llm-blocking`, without a panic and before anything connects. A stream opened inside a
//! runtime and read outside one is refused the same way, as a failure after dispatch.
//! `docs/design.md` ("Executor") records what the same calls did before. Local sockets only.

use llm_core::{Cancel, Dispatch, Error, ErrorCode};
use llm_http::{Framing, HeaderMap, HttpClient, Limits};
use std::{
    any::Any,
    future::Future,
    io::ErrorKind,
    net::TcpListener,
    panic::{AssertUnwindSafe, catch_unwind},
    pin::pin,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    thread::{self, Thread},
    time::{Duration, Instant},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// The error's whole message: the requirement, and the adapter a caller without Tokio uses.
const REQUIREMENT: &str =
    "an llm client needs a Tokio runtime; a caller without one uses b10x-llm-blocking";

/// Wakes the thread [`block_on`] parked.
struct Unpark(Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// An executor that is not Tokio: polls `future` on the calling thread and parks the thread
/// until the future's waker is called. Standard library only.
fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
        thread::park();
    }
}

/// Polls `future` with [`block_on`] on this thread, which is in no Tokio runtime, and keeps only
/// whether it failed and how. A panic is caught and returned as its message.
fn without_tokio<T>(
    future: impl Future<Output = Result<T, Error>>,
) -> Result<Option<Error>, String> {
    catch_unwind(AssertUnwindSafe(|| block_on(future).err()))
        .map_err(|payload| panic_message(&*payload))
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic without a message".to_owned())
}

/// Every way `outcome` departs from the refusal a call outside a Tokio runtime gets: typed,
/// final, with `dispatch`, and naming both the requirement and `b10x-llm-blocking`.
fn departures(
    what: &str,
    outcome: &Result<Option<Error>, String>,
    dispatch: Dispatch,
) -> Vec<String> {
    let error = match outcome {
        Err(panic) => return vec![format!("{what}: panicked: {panic}")],
        Ok(None) => return vec![format!("{what}: succeeded")],
        Ok(Some(error)) => error,
    };
    let mut found = Vec::new();
    if error.code != ErrorCode::Unsupported {
        found.push(format!("{what}: code {:?}", error.code));
    }
    if error.dispatch != dispatch {
        found.push(format!("{what}: dispatch {:?}", error.dispatch));
    }
    if error.retriable || error.may_retry() {
        found.push(format!("{what}: retriable"));
    }
    if error.observation.is_some() {
        found.push(format!("{what}: carries an observation"));
    }
    if error.message != REQUIREMENT {
        found.push(format!("{what}: message {:?}", error.message));
    }
    found
}

fn assert_no_findings(found: &[String]) {
    assert!(
        found.is_empty(),
        "outside a Tokio runtime:\n{}",
        found.join("\n")
    );
}

fn client() -> HttpClient {
    HttpClient::new(Limits::default()).expect("a bounded transport")
}

/// Each exchange is refused before it connects: the endpoint sees no connection.
#[test]
fn every_exchange_outside_a_tokio_runtime_is_refused_before_it_connects() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
    listener
        .set_nonblocking(true)
        .expect("a non-blocking listener");
    let url = format!("http://{}/v1", listener.local_addr().expect("an address"));
    let client = client();
    let cancel = Cancel::new();
    let mut found = departures(
        "post_sse",
        &without_tokio(client.post_sse(
            &url,
            HeaderMap::new(),
            b"{}".to_vec(),
            Framing::PayloadsOnly,
            &cancel,
        )),
        Dispatch::NotSent,
    );
    found.extend(departures(
        "post_sse_until",
        &without_tokio(client.post_sse_until(
            &url,
            HeaderMap::new(),
            b"{}".to_vec(),
            Framing::PayloadsOnly,
            &cancel,
            Some(Instant::now() + Duration::from_secs(30)),
        )),
        Dispatch::NotSent,
    ));
    found.extend(departures(
        "post_json",
        &without_tokio(client.post_json(&url, HeaderMap::new(), b"{}".to_vec(), &cancel)),
        Dispatch::NotSent,
    ));
    match listener.accept() {
        Err(error) if error.kind() == ErrorKind::WouldBlock => {}
        Ok(_) => found.push("an exchange reached the endpoint".to_owned()),
        Err(error) => panic!("the fixture listener failed: {error}"),
    }
    assert_no_findings(&found);
}

/// A stream whose head arrived inside a runtime, read on a thread outside one, ends with the
/// same refusal, after dispatch: the request was answered.
#[test]
fn a_stream_read_outside_a_tokio_runtime_is_refused_after_dispatch() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let mut stream = runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a local port");
        let url = format!("http://{}/v1", listener.local_addr().expect("an address"));
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("a connection");
            let mut seen = Vec::new();
            let mut buffer = [0; 4096];
            while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                let length = socket.read(&mut buffer).await.expect("a readable socket");
                assert!(length > 0, "the client closed before sending a request");
                seen.extend_from_slice(&buffer[..length]);
            }
            socket
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\r\n")
                .await
                .expect("a head written");
            // The body stays open: the next read has to wait for the wire.
            std::future::pending::<()>().await;
        });
        client()
            .post_sse(
                &url,
                HeaderMap::new(),
                b"{}".to_vec(),
                Framing::PayloadsOnly,
                &Cancel::new(),
            )
            .await
            .expect("a stream opened inside the runtime")
    });
    assert_no_findings(&departures(
        "SseStream::next",
        &without_tokio(stream.next()),
        Dispatch::Accepted,
    ));
}
