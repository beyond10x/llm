//! The transport driven by an executor that is not Tokio (story:async-clients-without-a-tokio-runtime).
//!
//! Every exchange the transport offers, polled by [`block_on`] below on a thread that is in no
//! Tokio runtime, is refused with the typed error that names the requirement and the ways out of
//! it, without a panic and before anything connects. A stream opened inside a
//! runtime and read outside one is refused the same way, as a failure after dispatch. So is each
//! wait moved out of its runtime part-way: the request before its answer, a stream read, a 429
//! body read for a quota rule. Such a wait does not panic; left alone it waits on a runtime
//! nobody drives, so these cases give up after two seconds outside and say so.
//! `docs/design.md` ("Executor") records what the same calls did before. Local sockets only.

use llm_core::{Cancel, Dispatch, Error, ErrorCode};
use llm_http::{Framing, HeaderMap, HttpClient, Limits, ResponseTap};
use std::{
    any::Any,
    future::{Future, poll_fn},
    io::ErrorKind,
    net::TcpListener,
    panic::{AssertUnwindSafe, catch_unwind},
    pin::pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    thread::{self, Thread},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    runtime::Runtime,
};

/// The error's whole message: the requirement and the two ways out of it.
const REQUIREMENT: &str = "an llm client needs a Tokio runtime; run the call inside one, or use b10x-llm-blocking for a single model";

/// How long a wait moved out of its runtime is polled outside before it counts as stuck.
const OUTSIDE: Duration = Duration::from_secs(2);

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
    block_on_within(future, Duration::MAX).expect("no limit")
}

/// [`block_on`], giving up with `None` once `limit` has passed without an answer.
fn block_on_within<F: Future>(future: F, limit: Duration) -> Option<F::Output> {
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = pin!(future);
    let until = Instant::now().checked_add(limit);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return Some(output);
        }
        match until {
            None => thread::park(),
            Some(until) => thread::park_timeout(until.checked_duration_since(Instant::now())?),
        }
    }
}

/// Polls `future` with [`block_on`] on this thread, which is in no Tokio runtime, and keeps only
/// whether it failed and how. A panic is caught and returned as what happened instead.
fn without_tokio<T>(
    future: impl Future<Output = Result<T, Error>>,
) -> Result<Option<Error>, String> {
    catch_unwind(AssertUnwindSafe(|| block_on(future).err()))
        .map_err(|payload| format!("panicked: {}", panic_message(&*payload)))
}

/// Polls `call` inside `runtime` until a poll leaves it waiting and `move_now` says so, then on
/// this thread, in no runtime, for at most [`OUTSIDE`]. The runtime stays alive, undriven. What
/// the call did outside: its error, `None` for a success, or what happened instead.
fn moved_out<T>(
    runtime: &Runtime,
    call: impl Future<Output = Result<T, Error>>,
    move_now: impl Fn() -> bool,
) -> Result<Option<Error>, String> {
    let mut call = pin!(call);
    let waiting = runtime.block_on(poll_fn(|context| match call.as_mut().poll(context) {
        Poll::Ready(_) => Poll::Ready(false),
        Poll::Pending if move_now() => Poll::Ready(true),
        Poll::Pending => Poll::Pending,
    }));
    assert!(waiting, "fixture: the call ended inside the runtime");
    assert!(
        tokio::runtime::Handle::try_current().is_err(),
        "fixture: this thread is still in a runtime"
    );
    catch_unwind(AssertUnwindSafe(|| block_on_within(call.as_mut(), OUTSIDE)))
        .map_err(|payload| format!("panicked: {}", panic_message(&*payload)))?
        .map(Result::err)
        .ok_or_else(|| format!("still waiting {OUTSIDE:?} after it left the runtime"))
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
        Err(instead) => return vec![format!("{what}: {instead}")],
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

fn runtime() -> Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime")
}

/// A local endpoint nothing accepts from, and its URL.
fn endpoint() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
    listener
        .set_nonblocking(true)
        .expect("a non-blocking listener");
    let url = format!("http://{}/v1", listener.local_addr().expect("an address"));
    (listener, url)
}

/// Inside `runtime`, a server that reads one request, writes `answer` and keeps the connection
/// open; returns its URL.
fn serve_and_hold(runtime: &Runtime, answer: &'static [u8]) -> String {
    runtime.block_on(async {
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
            socket.write_all(answer).await.expect("an answer written");
            std::future::pending::<()>().await;
        });
        url
    })
}

/// Each exchange is refused before it connects: the endpoint sees no connection.
#[test]
fn every_exchange_outside_a_tokio_runtime_is_refused_before_it_connects() {
    let (listener, url) = endpoint();
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
    let runtime = runtime();
    let url = serve_and_hold(
        &runtime,
        b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\r\n",
    );
    let mut stream = runtime
        .block_on(client().post_sse(
            &url,
            HeaderMap::new(),
            b"{}".to_vec(),
            Framing::PayloadsOnly,
            &Cancel::new(),
        ))
        .expect("a stream opened inside the runtime");
    assert_no_findings(&departures(
        "SseStream::next",
        &without_tokio(stream.next()),
        Dispatch::Accepted,
    ));
}

/// A request polled once inside a runtime and then outside it, before its answer arrived, is
/// refused at that wait: dispatch `unknown`, as the request may be on the wire.
#[test]
fn a_request_moved_out_of_its_runtime_before_its_answer_is_refused() {
    let (_listener, url) = endpoint();
    let client = client();
    let cancel = Cancel::new();
    let runtime = runtime();
    let mut found = departures(
        "post_sse",
        &moved_out(
            &runtime,
            client.post_sse(
                &url,
                HeaderMap::new(),
                b"{}".to_vec(),
                Framing::PayloadsOnly,
                &cancel,
            ),
            || true,
        ),
        Dispatch::Unknown,
    );
    found.extend(departures(
        "post_json",
        &moved_out(
            &runtime,
            client.post_json(&url, HeaderMap::new(), b"{}".to_vec(), &cancel),
            || true,
        ),
        Dispatch::Unknown,
    ));
    assert_no_findings(&found);
}

/// A read polled once inside a runtime and then outside it, before its chunk arrived, is
/// refused at that wait, after dispatch.
#[test]
fn a_stream_read_moved_out_of_its_runtime_while_it_waits_is_refused() {
    let runtime = runtime();
    let url = serve_and_hold(
        &runtime,
        b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\r\n",
    );
    let mut stream = runtime
        .block_on(client().post_sse(
            &url,
            HeaderMap::new(),
            b"{}".to_vec(),
            Framing::PayloadsOnly,
            &Cancel::new(),
        ))
        .expect("a stream opened inside the runtime");
    assert_no_findings(&departures(
        "SseStream::next",
        &moved_out(&runtime, stream.next(), || true),
        Dispatch::Accepted,
    ));
}

/// Sets its flag once a response head is shown to it.
#[derive(Default)]
struct SawHead(AtomicBool);

impl ResponseTap for SawHead {
    fn head(&self, _status_line: &str, _headers: &HeaderMap) {
        self.0.store(true, Ordering::SeqCst);
    }
    fn chunk(&self, _bytes: &[u8]) {}
}

/// A 429 whose body a quota rule waits for, moved out of its runtime while the body is still
/// arriving, is refused at that wait, with the 429's own dispatch, `rejected`.
#[test]
fn a_429_body_read_moved_out_of_its_runtime_is_refused() {
    let runtime = runtime();
    let url = serve_and_hold(
        &runtime,
        b"HTTP/1.1 429 Too Many Requests\r\ncontent-type: application/json\r\n\
          content-length: 64\r\n\r\n{\"error\":",
    );
    let saw = Arc::new(SawHead::default());
    let client = client()
        .with_response_tap(saw.clone())
        .with_quota_rule(|_, _| true);
    let cancel = Cancel::new();
    assert_no_findings(&departures(
        "post_sse, 429 body",
        &moved_out(
            &runtime,
            client.post_sse(
                &url,
                HeaderMap::new(),
                b"{}".to_vec(),
                Framing::PayloadsOnly,
                &cancel,
            ),
            || saw.0.load(Ordering::SeqCst),
        ),
        Dispatch::Rejected,
    ));
}
