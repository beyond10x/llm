//! Adversary, wave 2026-10-30-w24, story:chat-counts-every-output-delta.
//!
//! The Chat client now carries refusal text to its caller (`StreamEvent::RefusalDelta`,
//! `Item::AssistantRefusal`). `llm infer`, the workspace's own caller, writes only
//! `StreamEvent::TextDelta` (`crates/llm-cli/src/lib.rs`, `Stdout::emit`) and reads no item of
//! the outcome, so a turn the model refused prints nothing, exits 0 and reports a normal turn.
//! A loopback fixture server; no provider is reached.

use std::{
    fmt::Write as _,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    process::{Command, Stdio},
    thread,
};

const SSE: &str = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n";

/// Answers one connection with `answer` after reading the request.
fn serve(answer: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    thread::spawn(move || {
        let Ok((mut socket, _)) = listener.accept() else {
            return;
        };
        let mut reader = BufReader::new(&mut socket);
        let mut length = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some((name, value)) = line.split_once(':')
                && name.trim().eq_ignore_ascii_case("content-length")
            {
                length = value.trim().parse().unwrap();
            }
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        let _ = socket.write_all(&answer);
        let _ = socket.flush();
    });
    origin
}

/// A Chat Completions stream in which the model declines, as `fixtures/openai-refusal-and-reasoning.sse`.
fn refused_stream() -> Vec<u8> {
    let mut body = String::new();
    for delta in [
        r#"{"role":"assistant","content":"","refusal":null}"#,
        r#"{"refusal":"I can't"}"#,
        r#"{"refusal":" help with that."}"#,
    ] {
        let _ = write!(
            body,
            "data: {{\"id\":\"chatcmpl-adv\",\"object\":\"chat.completion.chunk\",\"model\":\"small\",\"choices\":[{{\"index\":0,\"delta\":{delta},\"finish_reason\":null}}]}}\n\n"
        );
    }
    body.push_str("data: {\"id\":\"chatcmpl-adv\",\"object\":\"chat.completion.chunk\",\"model\":\"small\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n");
    body.push_str("data: [DONE]\n\n");
    let mut answer = SSE.as_bytes().to_vec();
    answer.extend_from_slice(body.as_bytes());
    answer
}

/// The refusal text reaches the person running `llm infer`, on stdout or stderr; it is not
/// dropped behind an exit status of 0 and an ordinary usage line.
#[test]
fn infer_on_a_turn_the_model_refused_shows_the_refusal() {
    let origin = serve(refused_stream());
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("llm.toml");
    std::fs::write(
        &path,
        format!(
            r#"format = "llm.routes/1"

[endpoints.lab]
base_url = "{origin}/v1"
protocol = "chat-completions"
auth = {{ kind = "anonymous" }}

[routes.small]
model = "lab/small"
"#
        ),
    )
    .unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_llm"));
    command
        .env_clear()
        .args([
            "--config",
            path.to_str().unwrap(),
            "infer",
            "--model",
            "small",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"a prompt the model declines")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stdout.contains("I can't help with that.") || stderr.contains("I can't help with that."),
        "the refusal was dropped: exit {:?}, stdout {stdout:?}, stderr {stderr:?}",
        output.status.code()
    );
}
