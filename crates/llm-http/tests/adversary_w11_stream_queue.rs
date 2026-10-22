//! Adversary, wave 2026-10-20-w11, unit 2 (story:async-clients-without-a-tokio-runtime), pass 1.
//!
//! `SseStream::next` asks for a runtime only before a read that waits for the wire
//! (`crates/llm-http/src/transport.rs`, "Only a read that waits for the wire needs the runtime;
//! queued events do not"; `docs/design.md` "Executor"). The unit's own stream case reads a stream
//! whose body has not started, so the first read outside a runtime always waits for the wire, and
//! a check moved above the queue would leave it green. This case reads an event already decoded
//! from the wire outside a runtime, then the read that has to wait. It also times the check inside
//! a runtime. Local sockets only.

use llm_core::{Cancel, Dispatch, Error, ErrorCode};
use llm_http::{Framing, HeaderMap, HttpClient, Limits, SseEvent, require_tokio_runtime};
use serde_json::json;
use std::{
    any::Any,
    future::Future,
    hint::black_box,
    panic::{AssertUnwindSafe, catch_unwind},
    pin::pin,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    thread::{self, Thread},
    time::{Duration, Instant},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const REQUIREMENT: &str = "an llm client needs a Tokio runtime; run the call inside one, or use llm-blocking for a single model";

struct Unpark(Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// An executor that is not Tokio. Standard library only.
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

fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic without a message".to_owned())
}

fn without_tokio<T>(
    future: impl Future<Output = Result<T, Error>>,
) -> Result<Result<T, Error>, String> {
    catch_unwind(AssertUnwindSafe(|| block_on(future))).map_err(|payload| panic_message(&*payload))
}

/// A stream whose head and first two events arrived together inside a runtime, read once there:
/// the first event is returned and the second is already decoded and queued. The body stays
/// open, so a third read has to wait for the wire. Read outside a runtime, the queued event is
/// handed over and only the third read is refused, after dispatch.
#[test]
fn a_queued_event_is_read_outside_a_runtime_and_only_the_wire_read_is_refused() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let (mut stream, first) = runtime.block_on(async {
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
            // Head and both events in one write: one chunk on the client's side.
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\r\n\
                      data: {\"n\":1}\n\ndata: {\"n\":2}\n\n",
                )
                .await
                .expect("a head and two events written");
            std::future::pending::<()>().await;
        });
        let mut stream = HttpClient::new(Limits::default())
            .expect("a bounded transport")
            .post_sse(
                &url,
                HeaderMap::new(),
                b"{}".to_vec(),
                Framing::PayloadsOnly,
                &Cancel::new(),
            )
            .await
            .expect("a stream opened inside the runtime");
        let first = stream.next().await;
        (stream, first)
    });
    let payload = |n: u64| SseEvent::Payload {
        event: None,
        data: json!({ "n": n }),
    };
    assert_eq!(
        first.expect("the first event inside the runtime"),
        Some(payload(1)),
        "fixture: the first read inside the runtime"
    );

    let mut found = Vec::new();
    match without_tokio(stream.next()) {
        Ok(Ok(Some(event))) if event == payload(2) => {}
        other => found.push(format!(
            "the queued second event, read outside a runtime: {other:?}"
        )),
    }
    match without_tokio(stream.next()) {
        Ok(Err(error))
            if error.code == ErrorCode::Unsupported
                && error.dispatch == Dispatch::Accepted
                && !error.may_retry()
                && error.message == REQUIREMENT => {}
        other => found.push(format!(
            "the read that waits for the wire, outside a runtime: {other:?}"
        )),
    }
    drop(stream);
    drop(runtime);
    assert!(found.is_empty(), "{}", found.join("\n"));
}

/// The check is one thread-local read and one handle clone. Inside a runtime it is asked once
/// per turn, once per exchange and once per wire read of a stream; this bounds it far below the
/// timer every one of those calls creates next to it.
#[tokio::test]
async fn the_check_inside_a_runtime_costs_well_under_a_microsecond() {
    const CALLS: u32 = 200_000;
    let started = Instant::now();
    for _ in 0..CALLS {
        black_box(require_tokio_runtime()).expect("inside a runtime");
    }
    let per_call = started.elapsed() / CALLS;
    eprintln!("require_tokio_runtime inside a current-thread runtime: {per_call:?} per call");
    assert!(per_call < Duration::from_micros(2), "{per_call:?} per call");
}
