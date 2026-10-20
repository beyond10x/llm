//! Adversary, wave 2026-10-20-w11, unit 2 (story:async-clients-without-a-tokio-runtime), pass 2.
//!
//! `HttpClient::post_json` says a later poll on a thread in no Tokio runtime is refused with
//! dispatch `unknown` while the answer is awaited and `accepted` while its body is read. The
//! unit's cases move a JSON exchange out only before its answer. This one moves it out after the
//! head of a success arrived, while the body is still arriving: the answer was given, so the
//! refusal is `accepted` (a Codex renewal reads that as an uncertain grant). Local sockets only.

use llm_core::{Cancel, Dispatch, Error, ErrorCode};
use llm_http::{HeaderMap, HttpClient, Limits};
use std::{
    any::Any,
    future::Future,
    panic::{AssertUnwindSafe, catch_unwind},
    pin::pin,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    thread::{self, Thread},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const REQUIREMENT: &str = "an llm client needs a Tokio runtime; run the call inside one, or use b10x-llm-blocking for a single model";

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

#[test]
fn a_json_body_read_moved_out_of_its_runtime_is_refused_after_dispatch() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let url = runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a local port");
        let url = format!(
            "http://{}/token",
            listener.local_addr().expect("an address")
        );
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("a connection");
            let mut seen = Vec::new();
            let mut buffer = [0; 4096];
            while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                let length = socket.read(&mut buffer).await.expect("a readable socket");
                assert!(length > 0, "the client closed before sending a request");
                seen.extend_from_slice(&buffer[..length]);
            }
            // A success head and the start of its body; the rest never comes.
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\
                      content-length: 64\r\n\r\n{\"access_token\":",
                )
                .await
                .expect("a head and a partial body written");
            std::future::pending::<()>().await;
        });
        url
    });
    let client = HttpClient::new(Limits::default()).expect("a bounded transport");
    let cancel = Cancel::new();
    let mut call = Box::pin(client.post_json(&url, HeaderMap::new(), b"{}".to_vec(), &cancel));
    // Inside the runtime long enough for the head to arrive: the call then waits for its body.
    let inside = runtime
        .block_on(async { tokio::time::timeout(Duration::from_secs(1), call.as_mut()).await });
    assert!(
        inside.is_err(),
        "fixture: the exchange ended inside the runtime: {inside:?}"
    );
    let outside: Result<Result<serde_json::Value, Error>, String> =
        catch_unwind(AssertUnwindSafe(|| block_on(call.as_mut())))
            .map_err(|payload| panic_message(&*payload));
    match outside {
        Ok(Err(error))
            if error.code == ErrorCode::Unsupported
                && error.dispatch == Dispatch::Accepted
                && !error.may_retry()
                && error.message == REQUIREMENT => {}
        other => panic!("a JSON body read moved out of its runtime: {other:?}"),
    }
    drop(call);
    drop(runtime);
}
