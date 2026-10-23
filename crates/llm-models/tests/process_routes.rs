//! An `aws-process` Bedrock endpoint's routes against a loopback Bedrock fixture, with this test
//! binary as the credential command.
//!
//! The target has no libtest harness, because libtest prints to stdout before any test runs and
//! a `credential_process` command's stdout must be exactly its answer. Run with
//! `--credential-process-fixture <directory>`, the binary appends a line to `<directory>/runs`,
//! then exits with status 3 when `<directory>/exit` exists and otherwise prints
//! `<directory>/answer`. Run any other way, it runs every case below, or those whose name
//! contains an argument.

use llm_bedrock::eventstream::{HeaderValue, Message, encode_message};
use llm_core::{Cancel, Dispatch, Error, ErrorCode, Item, Model, TurnRequest, VecSink};
use llm_http::{HttpClient, Limits};
use llm_models::routes::{AwsCredentialKind, Routes, SecretSources};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

const FIXTURE_FLAG: &str = "--credential-process-fixture";
const ACCESS_KEY_ID: &str = "ASIAROUTESFIXTURE";
const SECRET_ACCESS_KEY: &str = "routes/fixture+secret";
const SESSION_TOKEN: &str = "routes-fixture-session-token";
const PRINTED: &str = "fixture-stdout-that-must-stay-private";

type Case = (&'static str, fn());
/// One request head: its header names, lowercased, and values.
type Head = Vec<(String, String)>;

const CASES: &[Case] = &[
    (
        "a_turn_runs_the_command_once_and_signs_with_the_keys_it_printed",
        a_turn_runs_the_command_once_and_signs_with_the_keys_it_printed,
    ),
    (
        "each_unusable_answer_refuses_the_turn_naming_the_endpoint_and_the_program_and_sends_nothing",
        each_unusable_answer_refuses_the_turn_naming_the_endpoint_and_the_program_and_sends_nothing,
    ),
    (
        "without_the_callers_permission_the_route_is_refused_and_nothing_runs",
        without_the_callers_permission_the_route_is_refused_and_nothing_runs,
    ),
    (
        "building_the_catalog_and_the_resolvers_runs_nothing",
        building_the_catalog_and_the_resolvers_runs_nothing,
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

/// A routes document with `bedrock-dev`, an aws-process endpoint at `origin` whose command is
/// this binary in fixture mode on `directory`, and `open`, an anonymous HTTP endpoint.
fn document(origin: &str, directory: &Path) -> String {
    let command = json!([program(), FIXTURE_FLAG, directory.display().to_string()]);
    format!(
        r#"format = "llm.routes/1"

[endpoints.bedrock-dev]
base_url = "{origin}"
protocol = "bedrock-converse"
bedrock = {{ region = "eu-central-1" }}
auth = {{ kind = "aws-process", command = {command} }}

[endpoints.open]
base_url = "https://open.example.invalid/v1"
protocol = "messages"
auth = {{ kind = "anonymous" }}

[routes]
opus = "bedrock-dev/eu.anthropic.claude-opus-5-5"
local = "open/claude-haiku-5-5"
"#
    )
}

fn runs(directory: &Path) -> usize {
    std::fs::read_to_string(directory.join("runs")).map_or(0, |text| text.lines().count())
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

fn http() -> HttpClient {
    HttpClient::new(Limits::default()).expect("a transport")
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime")
}

async fn turn(model: &dyn Model) -> Result<String, Error> {
    let request = TurnRequest::new("opus", vec![Item::user("Say hello")]);
    let mut sink = VecSink::new(64, 64 * 1024);
    model.turn(&request, &mut sink, &Cancel::new()).await?;
    Ok(sink.text())
}

fn header<'a>(head: &'a [(String, String)], name: &str) -> Option<&'a str> {
    head.iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

fn a_turn_runs_the_command_once_and_signs_with_the_keys_it_printed() {
    let directory = tempfile::tempdir().expect("a directory");
    std::fs::write(directory.path().join("answer"), printed_answer()).expect("an answer");
    let bedrock = Bedrock::new();
    let routes = Routes::parse_toml(&document(&bedrock.origin, directory.path())).expect("routes");
    let resolvers = routes
        .resolvers(&SecretSources::new())
        .expect("resolvers")
        .allow_credential_process();
    runtime().block_on(async {
        let port = routes.port("opus", http(), &resolvers).expect("a port");
        assert!(port.resolver().is_none());
        assert_eq!(runs(directory.path()), 0, "building the port runs nothing");
        assert_eq!(turn(&port).await.expect("the first turn"), "signed");
        assert_eq!(runs(directory.path()), 1);
        assert_eq!(turn(&port).await.expect("the second turn"), "signed");
        assert_eq!(runs(directory.path()), 1, "the second turn runs no command");
    });
    let heads = bedrock.heads();
    assert_eq!(heads.len(), 2);
    for head in &heads {
        let authorization = header(head, "authorization").expect("signed");
        assert!(
            authorization.starts_with(&format!("AWS4-HMAC-SHA256 Credential={ACCESS_KEY_ID}/")),
            "{authorization}"
        );
        assert!(
            authorization.contains("/eu-central-1/bedrock/aws4_request"),
            "{authorization}"
        );
        assert_eq!(header(head, "x-amz-security-token"), Some(SESSION_TOKEN));
    }
}

fn each_unusable_answer_refuses_the_turn_naming_the_endpoint_and_the_program_and_sends_nothing() {
    let valid: Value = serde_json::from_slice(&printed_answer()).expect("JSON");
    let mut wrong_version = valid.clone();
    wrong_version["Version"] = json!(2);
    let mut no_key = valid.clone();
    no_key
        .as_object_mut()
        .expect("an object")
        .remove("SecretAccessKey");
    let answers: Vec<(&str, Option<Vec<u8>>)> = vec![
        (
            "wrong Version",
            Some(wrong_version.to_string().into_bytes()),
        ),
        ("missing key", Some(no_key.to_string().into_bytes())),
        ("invalid JSON", Some(format!("{PRINTED} {{").into_bytes())),
        (
            "over 64 KiB",
            Some(format!("{}{valid}{PRINTED}", " ".repeat(64 * 1024)).into_bytes()),
        ),
        ("non-zero exit", None),
    ];
    let bedrock = Bedrock::new();
    for (case, answer) in answers {
        let directory = tempfile::tempdir().expect("a directory");
        match answer {
            Some(answer) => std::fs::write(directory.path().join("answer"), answer),
            None => std::fs::write(directory.path().join("exit"), b""),
        }
        .expect("a fixture");
        let routes =
            Routes::parse_toml(&document(&bedrock.origin, directory.path())).expect("routes");
        let resolvers = routes
            .resolvers(&SecretSources::new())
            .expect("resolvers")
            .allow_credential_process();
        let error = runtime().block_on(async {
            let port = routes.port("opus", http(), &resolvers).expect("a port");
            turn(&port).await.expect_err(case)
        });
        assert_eq!(error.code, ErrorCode::Unauthorized, "{case}: {error:?}");
        assert_eq!(error.dispatch, Dispatch::NotSent, "{case}");
        assert!(!error.retriable, "{case}");
        assert!(
            error.message.starts_with("endpoint `bedrock-dev`: "),
            "{case}: {}",
            error.message
        );
        assert!(
            error.message.contains(&format!("`{}`", program())),
            "{case}: {}",
            error.message
        );
        for secret in [PRINTED, ACCESS_KEY_ID, SECRET_ACCESS_KEY, SESSION_TOKEN] {
            assert!(!error.message.contains(secret), "{case}: {}", error.message);
        }
        assert_eq!(runs(directory.path()), 1, "{case}");
    }
    assert!(bedrock.heads().is_empty(), "no request was sent");
}

fn without_the_callers_permission_the_route_is_refused_and_nothing_runs() {
    let directory = tempfile::tempdir().expect("a directory");
    std::fs::write(directory.path().join("answer"), printed_answer()).expect("an answer");
    let bedrock = Bedrock::new();
    let routes = Routes::parse_toml(&document(&bedrock.origin, directory.path())).expect("routes");
    let resolvers = routes.resolvers(&SecretSources::new()).expect("resolvers");
    runtime().block_on(async {
        let error = routes
            .port("opus", http(), &resolvers)
            .err()
            .expect("refused");
        assert_eq!(error.code, ErrorCode::Unauthorized, "{error:?}");
        assert!(
            error.message.starts_with("endpoint `bedrock-dev`: "),
            "{}",
            error.message
        );
        assert!(
            error.message.contains("does not allow"),
            "{}",
            error.message
        );
        let direct = routes
            .port(
                "bedrock-dev/eu.anthropic.claude-sonnet-5-5",
                http(),
                &resolvers,
            )
            .err()
            .expect("refused by its direct name too");
        assert_eq!(direct.code, ErrorCode::Unauthorized);
        routes
            .port("local", http(), &resolvers)
            .expect("another endpoint still builds");
        let none: std::collections::BTreeMap<
            llm_core::Id,
            Arc<dyn llm_credentials::SecretResolver>,
        > = std::collections::BTreeMap::new();
        let error = routes.port("opus", http(), &none).err().expect("refused");
        assert_eq!(
            error.code,
            ErrorCode::Unauthorized,
            "a bare resolver map allows none"
        );
    });
    assert_eq!(runs(directory.path()), 0);
    assert!(bedrock.heads().is_empty());
}

fn building_the_catalog_and_the_resolvers_runs_nothing() {
    let directory = tempfile::tempdir().expect("a directory");
    std::fs::write(directory.path().join("answer"), printed_answer()).expect("an answer");
    let routes =
        Routes::parse_toml(&document("http://127.0.0.1:9", directory.path())).expect("routes");
    let endpoint = routes
        .bedrock_endpoints()
        .find(|endpoint| endpoint.name().as_str() == "bedrock-dev")
        .expect("the endpoint");
    assert_eq!(endpoint.credential_kind(), AwsCredentialKind::Process);
    assert_eq!(
        endpoint.command().expect("a command"),
        [
            program(),
            FIXTURE_FLAG.to_owned(),
            directory.path().display().to_string()
        ]
    );
    assert!(endpoint.secret_kinds().is_empty());
    let catalog: Vec<_> = routes
        .catalog()
        .bindings()
        .map(|(id, _)| id.to_string())
        .collect();
    assert_eq!(catalog, ["local"]);
    let resolvers = routes
        .resolvers(&SecretSources::new())
        .expect("resolvers")
        .allow_credential_process();
    assert_eq!(resolvers.accounts().count(), 0);
    routes.target("opus").expect("a target");
    assert_eq!(runs(directory.path()), 0);
}
