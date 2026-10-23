//! `llm` on an `aws-process` Bedrock route, with this test binary as the credential command and a
//! loopback Bedrock fixture as the endpoint.
//!
//! The target has no libtest harness, because libtest prints to stdout before any test runs and
//! a `credential_process` command's stdout must be exactly its answer. Run with
//! `--credential-process-fixture <directory>`, the binary appends a line to `<directory>/runs`,
//! then exits with status 3 when `<directory>/exit` exists and otherwise prints
//! `<directory>/answer`. Run any other way, it runs every case below, or those whose name
//! contains an argument.

use llm_bedrock::eventstream::{HeaderValue, Message, encode_message};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, ExitCode, Output, Stdio};
use std::sync::{Arc, Mutex};

const FIXTURE_FLAG: &str = "--credential-process-fixture";
const ACCESS_KEY_ID: &str = "ASIACOMMANDFIXTURE";
const SECRET_ACCESS_KEY: &str = "command/fixture+secret";
const SESSION_TOKEN: &str = "command-fixture-session-token";
const PRINTED: &str = "fixture-stdout-that-must-stay-private";

type Case = (&'static str, fn());
/// One request head: its header names, lowercased, and values.
type Head = Vec<(String, String)>;

const CASES: &[Case] = &[
    (
        "model_list_shows_the_aws_process_endpoint_and_runs_no_command",
        model_list_shows_the_aws_process_endpoint_and_runs_no_command,
    ),
    (
        "infer_runs_the_command_once_and_signs_with_the_keys_it_printed",
        infer_runs_the_command_once_and_signs_with_the_keys_it_printed,
    ),
    (
        "a_failing_command_is_a_refusal_naming_the_endpoint_and_the_program_and_sends_nothing",
        a_failing_command_is_a_refusal_naming_the_endpoint_and_the_program_and_sends_nothing,
    ),
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some(FIXTURE_FLAG) {
        return fixture(Path::new(&args[2]));
    }
    let filters: Vec<&str> = args[1..]
        .iter()
        .map(String::as_str)
        .filter(|arg| !arg.starts_with('-'))
        .collect();
    let chosen: Vec<&Case> = CASES
        .iter()
        .filter(|(name, _)| filters.is_empty() || filters.iter().any(|f| name.contains(f)))
        .collect();
    println!("\nrunning {} tests", chosen.len());
    let mut failed = 0;
    for (name, case) in chosen.iter().copied() {
        let passed = std::panic::catch_unwind(case).is_ok();
        println!("test {name} ... {}", if passed { "ok" } else { "FAILED" });
        failed += usize::from(!passed);
    }
    println!(
        "\ntest result: {}. {} passed; {failed} failed; 0 ignored; 0 measured; {} filtered out\n",
        if failed == 0 { "ok" } else { "FAILED" },
        chosen.len() - failed,
        CASES.len() - chosen.len()
    );
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn fixture(directory: &Path) -> ExitCode {
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("runs"))
        .expect("the run log");
    log.write_all(b"run\n").expect("a run logged");
    drop(log);
    let mut stdout = std::io::stdout().lock();
    if directory.join("exit").exists() {
        let _ = stdout.write_all(PRINTED.as_bytes());
        return ExitCode::from(3);
    }
    let answer = std::fs::read(directory.join("answer")).unwrap_or_default();
    let _ = stdout.write_all(&answer);
    let _ = stdout.flush();
    ExitCode::SUCCESS
}

fn program() -> String {
    std::env::current_exe()
        .expect("this test binary")
        .display()
        .to_string()
}

/// Writes a configuration with `opus` on `bedrock-dev`, an aws-process endpoint at `origin`
/// whose command is this binary in fixture mode on `directory`, and returns its path.
fn configured(origin: &str, directory: &Path) -> String {
    let command = json!([program(), FIXTURE_FLAG, directory.display().to_string()]);
    let text = format!(
        r#"format = "llm.routes/1"

[endpoints.bedrock-dev]
base_url = "{origin}"
protocol = "bedrock-converse"
bedrock = {{ region = "eu-central-1" }}
auth = {{ kind = "aws-process", command = {command} }}

[routes]
opus = "bedrock-dev/eu.anthropic.claude-opus-5-5"
"#
    );
    let path = directory.join("llm.toml");
    std::fs::write(&path, text).expect("a configuration");
    path.display().to_string()
}

fn printed_answer() -> Vec<u8> {
    json!({
        "Version": 1,
        "AccessKeyId": ACCESS_KEY_ID,
        "SecretAccessKey": SECRET_ACCESS_KEY,
        "SessionToken": SESSION_TOKEN,
    })
    .to_string()
    .into_bytes()
}

fn runs(directory: &Path) -> usize {
    std::fs::read_to_string(directory.join("runs")).map_or(0, |text| text.lines().count())
}

fn llm(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_llm"))
        .args(args)
        .env_remove("LLM_CONFIG")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("llm ran")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A loopback Bedrock fixture answering each connection with one Converse stream, keeping
/// the head of every request it received.
struct Bedrock {
    origin: String,
    heads: Arc<Mutex<Vec<Head>>>,
}

impl Bedrock {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let origin = format!("http://{}", listener.local_addr().expect("an address"));
        let heads = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&heads);
        std::thread::spawn(move || {
            for socket in listener.incoming() {
                let Ok(mut socket) = socket else { return };
                let head = read_head(&mut socket);
                kept.lock().expect("the heads").push(head);
                let _ = socket.write_all(&converse_stream());
                let _ = socket.flush();
            }
        });
        Self { origin, heads }
    }

    fn heads(&self) -> Vec<Head> {
        self.heads.lock().expect("the heads").clone()
    }
}

fn read_head(socket: &mut std::net::TcpStream) -> Head {
    let mut reader = BufReader::new(socket);
    let mut line = String::new();
    reader.read_line(&mut line).expect("a request line");
    let mut headers = Vec::new();
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).expect("a header");
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        let (name, value) = header.split_once(':').expect("a header");
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
    }
    let length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .map_or(0, |(_, value)| value.parse::<usize>().expect("a length"));
    let mut body = vec![0; length];
    reader.read_exact(&mut body).expect("the body");
    headers
}

fn converse_stream() -> Vec<u8> {
    let mut answer = b"HTTP/1.1 200 OK\r\ncontent-type: application/vnd.amazon.eventstream\r\n\
connection: close\r\n\r\n"
        .to_vec();
    for (kind, payload) in [
        ("messageStart", json!({"role": "assistant"})),
        (
            "contentBlockDelta",
            json!({"contentBlockIndex": 0, "delta": {"text": "signed"}}),
        ),
        ("contentBlockStop", json!({"contentBlockIndex": 0})),
        ("messageStop", json!({"stopReason": "end_turn"})),
        (
            "metadata",
            json!({"usage": {"inputTokens": 3, "outputTokens": 1, "totalTokens": 4}}),
        ),
    ] {
        answer.extend(encode_message(&Message {
            headers: [
                (":event-type", kind),
                (":content-type", "application/json"),
                (":message-type", "event"),
            ]
            .iter()
            .map(|(name, value)| ((*name).to_owned(), HeaderValue::String((*value).to_owned())))
            .collect(),
            payload: serde_json::to_vec(&payload).expect("a payload"),
        }));
    }
    answer
}

fn header<'a>(head: &'a [(String, String)], name: &str) -> Option<&'a str> {
    head.iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

fn model_list_shows_the_aws_process_endpoint_and_runs_no_command() {
    let directory = tempfile::tempdir().expect("a directory");
    std::fs::write(directory.path().join("answer"), printed_answer()).expect("an answer");
    let path = configured("http://127.0.0.1:9", directory.path());
    let listed = llm(&["--config", &path, "model", "list"]);
    assert_eq!(listed.status.code(), Some(0), "{}", text(&listed.stderr));
    assert!(
        text(&listed.stdout).contains("opus"),
        "{}",
        text(&listed.stdout)
    );
    let json = llm(&["--config", &path, "model", "list", "--format", "json"]);
    assert_eq!(json.status.code(), Some(0), "{}", text(&json.stderr));
    let rows: Value = serde_json::from_slice(&json.stdout).expect("JSON rows");
    assert_eq!(
        rows,
        json!([
            {"id": "opus", "aliases": [], "protocol": "bedrock-converse",
             "provider": "bedrock-dev", "host": "127.0.0.1", "region": "eu-central-1",
             "model": "eu.anthropic.claude-opus-5-5", "price_per_million": null},
        ])
    );
    assert_eq!(runs(directory.path()), 0, "listing runs no command");
}

fn infer_runs_the_command_once_and_signs_with_the_keys_it_printed() {
    let directory = tempfile::tempdir().expect("a directory");
    std::fs::write(directory.path().join("answer"), printed_answer()).expect("an answer");
    let bedrock = Bedrock::new();
    let path = configured(&bedrock.origin, directory.path());
    let output = llm(&["--config", &path, "infer", "--model", "opus", "hi"]);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "signed\n");
    assert_eq!(runs(directory.path()), 1);
    let heads = bedrock.heads();
    assert_eq!(heads.len(), 1);
    let authorization = header(&heads[0], "authorization").expect("signed");
    assert!(
        authorization.starts_with(&format!("AWS4-HMAC-SHA256 Credential={ACCESS_KEY_ID}/")),
        "{authorization}"
    );
    assert!(
        authorization.contains("/eu-central-1/bedrock/aws4_request"),
        "{authorization}"
    );
    assert_eq!(
        header(&heads[0], "x-amz-security-token"),
        Some(SESSION_TOKEN)
    );
    for secret in [SECRET_ACCESS_KEY, SESSION_TOKEN] {
        assert!(!text(&output.stdout).contains(secret));
        assert!(!text(&output.stderr).contains(secret));
    }
}

fn a_failing_command_is_a_refusal_naming_the_endpoint_and_the_program_and_sends_nothing() {
    let directory = tempfile::tempdir().expect("a directory");
    std::fs::write(directory.path().join("exit"), b"").expect("a failing fixture");
    let bedrock = Bedrock::new();
    let path = configured(&bedrock.origin, directory.path());
    let output = llm(&["--config", &path, "infer", "--model", "opus", "hi"]);
    let message = text(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{message}");
    assert!(output.stdout.is_empty(), "{}", text(&output.stdout));
    assert!(message.contains("unauthorized"), "{message}");
    assert!(message.contains("endpoint `bedrock-dev`"), "{message}");
    assert!(message.contains(&format!("`{}`", program())), "{message}");
    assert!(!message.contains(PRINTED), "{message}");
    assert_eq!(runs(directory.path()), 1);
    assert!(bedrock.heads().is_empty(), "no request was sent");
}
