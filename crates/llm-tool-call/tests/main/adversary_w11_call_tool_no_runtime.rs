//! Adversary, wave 2026-10-20-w11, unit 2 (story:async-clients-without-a-tokio-runtime), pass 1.
//!
//! `call_tool` over the Codex Responses preset, polled by a standard-library executor on a thread
//! that is in no Tokio runtime. Its task-local slot needs no runtime; the Responses client's check
//! refuses the turn before the Codex login is read, and `call_tool` hands the refusal on as
//! `ModelError::Model`. Local sockets only; the login path names no file.

use llm_core::{Dispatch, ErrorCode, Item, ToolName, ToolSpec};
use llm_tool_call::{ModelError, call_tool, codex_model_at};
use serde_json::json;
use std::{
    any::Any,
    future::Future,
    io::ErrorKind,
    net::TcpListener,
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    pin::pin,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    thread::{self, Thread},
};

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

#[test]
fn call_tool_outside_a_tokio_runtime_is_the_typed_refusal_before_the_login_is_read() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
    listener
        .set_nonblocking(true)
        .expect("a non-blocking listener");
    let url = format!("http://{}/v1", listener.local_addr().expect("an address"));
    let login = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("adversary-w11-no-login.json");
    let model = codex_model_at("gpt-5.1-codex", &url, &login).expect("a model");
    let tool = ToolSpec {
        name: ToolName::new("propose_protocol").expect("a tool name"),
        description: "Propose one protocol".to_owned(),
        input_schema: json!({"type": "object"}),
    };
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        block_on(call_tool(
            &model,
            "Answer with the tool.",
            vec![Item::user("Hallo")],
            tool,
        ))
    }))
    .map_err(|payload| panic_message(&*payload));
    let mut found = Vec::new();
    match outcome {
        Err(panic) => found.push(format!("panicked: {panic}")),
        Ok(Ok(arguments)) => found.push(format!("returned arguments {arguments}")),
        Ok(Err(ModelError::Model(error))) => {
            if error.code != ErrorCode::Unsupported
                || error.dispatch != Dispatch::NotSent
                || error.may_retry()
                || error.message != REQUIREMENT
            {
                found.push(format!("not the runtime refusal: {error:?}"));
            }
        }
        Ok(Err(other)) => found.push(format!("{other:?}")),
    }
    match listener.accept() {
        Err(error) if error.kind() == ErrorKind::WouldBlock => {}
        Ok(_) => found.push("reached the endpoint".to_owned()),
        Err(error) => panic!("the fixture listener failed: {error}"),
    }
    assert!(
        found.is_empty(),
        "call_tool outside a Tokio runtime:\n{}",
        found.join("\n")
    );
}
