//! story:anthropic-access, live-run tooling: the `live_subscription_turn` example's logic, run
//! against a loopback Messages fixture and a fresh `keyring_core::mock::Store` per test.
//!
//! The token is written the way `secretsctl put` writes it on the default keychain mount: the
//! library's keychain backend under its default service, at tenant `default`, user `default`, in
//! the example's namespace. The example reads it back through `SecretsResolver::keychain` and
//! runs one subscription turn. No native store is opened, no real credential is read and no
//! provider is contacted.
#![cfg(feature = "secrets")]

#[path = "../examples/live_subscription_turn/turn.rs"]
mod turn;

use clap::Parser;
use keyring_core::{CredentialStore, mock};
use llm_credentials::secrets::{
    keychain::KeychainBackend,
    storage::{Address, SecretStorage, SecretValue, Target},
};
use serde_json::Value;
use std::{collections::BTreeSet, path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const FIRST_TOKEN: &str = "fixture-subscription-token-first-7f3a";
const SECOND_TOKEN: &str = "fixture-subscription-token-second-91c4";
/// Written out rather than imported, so the pin cannot move with the library constant.
const PREAMBLE: &str = "You are Claude Code, Anthropic's official CLI for Claude.";

const STREAM: &[u8] = b"event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_live01\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"example-model-20260201\",\"content\":[],\"usage\":{\"input_tokens\":11,\"cache_creation_input_tokens\":0,\"cache_read_input_tokens\":0,\"output_tokens\":1}}}\n\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"input_tokens\":11,\"output_tokens\":8}}\n\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\n";

/// Headers the transport adds to every request; the Messages client does not choose them.
const TRANSPORT_HEADERS: [&str; 2] = ["content-length", "host"];

fn store() -> Arc<mock::Store> {
    mock::Store::new().unwrap()
}

/// Stores `token` where `secretsctl put <name> --namespace <namespace>` stores it on the default
/// keychain mount: `default/<namespace>/default/<name>` under the library's default service.
async fn put(store: &Arc<mock::Store>, namespace: &str, name: &str, token: &str) {
    KeychainBackend::new(store.clone() as Arc<CredentialStore>)
        .write(
            &Target::unbound(Address::parse("default", namespace, "default", name).unwrap()),
            SecretValue::new(token.as_bytes().to_vec()).unwrap(),
        )
        .await
        .unwrap();
}

fn args(endpoint: &str, extra: &[&str]) -> turn::Args {
    let mut argv = vec!["live_subscription_turn", "--endpoint", endpoint];
    argv.extend_from_slice(extra);
    turn::Args::try_parse_from(argv).unwrap()
}

async fn read_request(socket: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0; 1024];
    loop {
        let length = socket.read(&mut buffer).await.unwrap();
        assert!(length > 0, "the client closed before sending a request");
        bytes.extend_from_slice(&buffer[..length]);
        assert!(bytes.len() < 256 * 1024);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
            let length: usize = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .expect("a bounded body")
                .trim()
                .parse()
                .unwrap();
            if bytes.len() >= end + 4 + length {
                return String::from_utf8(bytes).expect("UTF-8 request");
            }
        }
    }
}

/// A loopback listener and its origin, as `--endpoint` takes it.
async fn serve() -> (String, TcpListener) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    (origin, listener)
}

/// The Messages fixture: answers `turns` requests, one connection each, and returns what each
/// carried.
async fn answer(listener: TcpListener, turns: usize) -> Vec<String> {
    let mut captured = Vec::new();
    for _ in 0..turns {
        let (mut socket, _) = tokio::time::timeout(Duration::from_secs(10), listener.accept())
            .await
            .expect("the client never opened a connection")
            .unwrap();
        captured.push(read_request(&mut socket).await);
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        socket.write_all(STREAM).await.unwrap();
        socket.shutdown().await.unwrap();
    }
    captured
}

/// The request's header names, lowercased, and its parsed body.
fn split(captured: &str) -> (Vec<(String, String)>, Value) {
    let (head, body) = captured.split_once("\r\n\r\n").expect("a head and a body");
    let headers = head
        .lines()
        .skip(1)
        .map(|line| {
            let (name, value) = line.split_once(':').expect("a header line");
            (name.trim().to_ascii_lowercase(), value.trim().to_owned())
        })
        .collect();
    (headers, serde_json::from_str(body).expect("a JSON body"))
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(found, _)| found == name)
        .map(|(_, value)| value.as_str())
}

/// What the example prints, parsed back. Asserting on the printed text is the point: the report
/// is what the operator records as evidence.
fn printed(report: &turn::Report) -> (String, Value) {
    let text = turn::render(report);
    let value = serde_json::from_str(&text).expect("the report is JSON");
    (text, value)
}

#[test]
fn the_defaults_name_the_entry_the_operator_steps_store_and_the_public_endpoint() {
    let defaults = turn::Args::try_parse_from(["live_subscription_turn"]).unwrap();
    assert_eq!(defaults.namespace, "llm");
    assert_eq!(defaults.name, "anthropic-subscription");
    assert_eq!(defaults.endpoint, "https://api.anthropic.com");
    assert_eq!(defaults.model, "claude-haiku-4-5");
    assert!(!defaults.prompt.is_empty());
    assert!(!defaults.rotate_check);

    // The operator steps store the token exactly where the defaults look for it.
    let steps = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/live-qualification.md"),
    )
    .unwrap();
    for command in [
        "secretsctl namespace add llm",
        "secretsctl put anthropic-subscription --namespace llm",
        "--features native-keychain",
        "--example live_subscription_turn",
        "--rotate-check",
        "claude setup-token",
    ] {
        assert!(
            steps.contains(command),
            "docs/live-qualification.md lacks `{command}`"
        );
    }
}

#[tokio::test]
async fn one_turn_reports_the_subscription_contract_it_used_and_never_the_token() {
    let store = store();
    put(&store, "llm", "anthropic-subscription", FIRST_TOKEN).await;
    let (origin, listener) = serve().await;
    let server = tokio::spawn(answer(listener, 1));

    let report = turn::run(
        &args(&origin, &[]),
        Ok(store.clone() as Arc<CredentialStore>),
        async || unreachable!("no rotation was asked for"),
    )
    .await;
    let captured = server.await.unwrap();
    let (text, report_json) = printed(&report);

    // The request the fixture received: the token from the store as a bearer, the OAuth beta,
    // and `system` opening with the preamble.
    let (headers, body) = split(&captured[0]);
    assert_eq!(
        header(&headers, "authorization"),
        Some(format!("Bearer {FIRST_TOKEN}").as_str())
    );
    assert_eq!(header(&headers, "anthropic-beta"), Some("oauth-2025-04-20"));
    assert_eq!(body["system"][0]["text"], PREAMBLE);
    assert_eq!(body["model"], "claude-haiku-4-5");

    // The report says what was used, in its own words.
    assert!(report.succeeded(), "{text}");
    assert_eq!(report_json["endpoint"], format!("{origin}/v1/messages"));
    assert_eq!(report_json["auth_kind"], "subscription-oauth");
    assert_eq!(report_json["billing_kind"], "subscription");
    assert_eq!(report_json["oauth_beta_sent"], true);
    assert_eq!(report_json["system_opens_with_preamble"], true);
    assert_eq!(report_json["model"], "claude-haiku-4-5");
    assert_eq!(report_json["secret"]["namespace"], "llm");
    assert_eq!(report_json["secret"]["name"], "anthropic-subscription");
    assert_eq!(report_json["secret"]["service"], "b10x-secrets");
    let turns = report_json["turns"].as_array().unwrap();
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0]["outcome"], "completed");
    assert_eq!(turns[0]["stop_reason"]["kind"], "end-turn");
    assert_eq!(turns[0]["usage"]["input_tokens"], 11, "{text}");
    assert_eq!(turns[0]["usage"]["output_tokens"], 8);
    assert_eq!(turns[0]["usage"]["cached_input_tokens"], 0);
    assert_eq!(turns[0]["upstream_model"], "example-model-20260201");
    assert!(turns[0]["latency_ms"].is_u64());
    assert_eq!(turns[0]["reply"], "ok");
    assert!(report_json["rotation"].is_null());

    // The header names reported are exactly the ones the client sent, apart from the two the
    // transport adds; no value is reported.
    let reported: BTreeSet<&str> = report_json["request_headers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|name| name.as_str().unwrap())
        .collect();
    let sent: BTreeSet<&str> = headers
        .iter()
        .map(|(name, _)| name.as_str())
        .filter(|name| !TRANSPORT_HEADERS.contains(name))
        .collect();
    assert_eq!(reported, sent);
    assert!(reported.contains("authorization") && reported.contains("anthropic-beta"));

    assert!(!text.contains(FIRST_TOKEN), "the report carries the token");
    assert!(!format!("{report:?}").contains(FIRST_TOKEN));
}

#[tokio::test]
async fn a_missing_secret_is_refused_before_any_request_and_names_only_the_reference() {
    let store = store();
    // A token stored under another name in the same namespace is not a fallback.
    put(&store, "llm", "other-token", FIRST_TOKEN).await;
    let (origin, listener) = serve().await;

    let report = turn::run(
        &args(&origin, &[]),
        Ok(store.clone() as Arc<CredentialStore>),
        async || unreachable!("no rotation was asked for"),
    )
    .await;
    let (text, report_json) = printed(&report);

    assert!(
        tokio::time::timeout(Duration::from_millis(300), listener.accept())
            .await
            .is_err(),
        "a request was sent without a credential"
    );
    assert!(!report.succeeded());
    let turns = report_json["turns"].as_array().unwrap();
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0]["outcome"], "error");
    assert_eq!(turns[0]["error"]["code"], "unauthorized");
    assert_eq!(turns[0]["error"]["dispatch"], "not-sent");
    assert_eq!(
        turns[0]["error"]["message"],
        "secret reference `anthropic-subscription` refused: secret reference was not found"
    );
    assert!(!text.contains(FIRST_TOKEN), "the report carries a token");
}

#[tokio::test]
async fn rotate_check_resolves_twice_and_reports_that_the_version_changed() {
    let store = store();
    put(&store, "llm", "anthropic-subscription", FIRST_TOKEN).await;
    let (origin, listener) = serve().await;
    let server = tokio::spawn(answer(listener, 2));

    let rotated = store.clone();
    let report = turn::run(
        &args(&origin, &["--rotate-check"]),
        Ok(store.clone() as Arc<CredentialStore>),
        async move || put(&rotated, "llm", "anthropic-subscription", SECOND_TOKEN).await,
    )
    .await;
    let captured = server.await.unwrap();
    let (text, report_json) = printed(&report);

    let bearer = |n: usize| {
        let (headers, _) = split(&captured[n]);
        header(&headers, "authorization").map(str::to_owned)
    };
    assert_eq!(bearer(0), Some(format!("Bearer {FIRST_TOKEN}")));
    assert_eq!(bearer(1), Some(format!("Bearer {SECOND_TOKEN}")));

    assert!(report.succeeded(), "{text}");
    let turns = report_json["turns"].as_array().unwrap();
    assert_eq!(turns.len(), 2);
    assert!(turns.iter().all(|turn| turn["outcome"] == "completed"));
    assert_eq!(report_json["rotation"]["resolutions"], 2);
    assert_eq!(report_json["rotation"]["version_changed"], true);
    assert!(!text.contains(FIRST_TOKEN) && !text.contains(SECOND_TOKEN));
}

#[tokio::test]
async fn rotate_check_without_a_rotation_reports_an_unchanged_version_and_fails() {
    let store = store();
    put(&store, "llm", "anthropic-subscription", FIRST_TOKEN).await;
    let (origin, listener) = serve().await;
    let server = tokio::spawn(answer(listener, 2));

    let report = turn::run(
        &args(&origin, &["--rotate-check"]),
        Ok(store.clone() as Arc<CredentialStore>),
        async || {},
    )
    .await;
    server.await.unwrap();
    let (text, report_json) = printed(&report);

    assert_eq!(report_json["rotation"]["resolutions"], 2);
    assert_eq!(report_json["rotation"]["version_changed"], false);
    assert!(!report.succeeded(), "an unrotated token passed: {text}");
}

#[tokio::test]
async fn a_keychain_that_cannot_be_opened_is_refused_before_any_request() {
    let (origin, listener) = serve().await;

    let report = turn::run(
        &args(&origin, &[]),
        Err(llm_credentials::SecretError::Unavailable),
        async || unreachable!("no rotation was asked for"),
    )
    .await;
    let (text, report_json) = printed(&report);

    assert!(
        tokio::time::timeout(Duration::from_millis(300), listener.accept())
            .await
            .is_err(),
        "a request was sent without a keychain"
    );
    assert!(!report.succeeded(), "{text}");
    assert_eq!(report_json["refused"]["code"], "unavailable");
    assert_eq!(report_json["refused"]["dispatch"], "not-sent");
    assert!(report_json["turns"].as_array().unwrap().is_empty());
}

/// `--capture-response` keeps what the route sent back, so a refusal of the live stream can be
/// read against the bytes that caused it: the status line, the response headers and the event
/// stream exactly as it arrived. Nothing the client sent is in it — no request header, no token.
#[tokio::test]
async fn capture_response_writes_what_the_route_sent_and_never_the_request_or_its_token() {
    let store = store();
    put(&store, "llm", "anthropic-subscription", FIRST_TOKEN).await;
    let (origin, listener) = serve().await;
    let server = tokio::spawn(answer(listener, 1));
    let directory = tempfile::tempdir().unwrap();
    let capture = directory.path().join("response.capture");

    let report = turn::run(
        &args(&origin, &["--capture-response", capture.to_str().unwrap()]),
        Ok(store.clone() as Arc<CredentialStore>),
        async || unreachable!("no rotation was asked for"),
    )
    .await;
    let captured = server.await.unwrap();
    let (text, report_json) = printed(&report);
    assert!(report.succeeded(), "{text}");
    // The request really carried the token, so its absence below is not an accident.
    let (headers, _) = split(&captured[0]);
    assert_eq!(
        header(&headers, "authorization"),
        Some(format!("Bearer {FIRST_TOKEN}").as_str())
    );

    let bytes = std::fs::read(&capture).unwrap();
    let written = String::from_utf8(bytes.clone()).unwrap();
    assert!(written.starts_with("HTTP/1.1 200 OK\r\n"), "{written}");
    assert!(
        written.contains("\r\ncontent-type: text/event-stream\r\n"),
        "{written}"
    );
    assert!(
        written.ends_with(std::str::from_utf8(STREAM).unwrap()),
        "{written}"
    );
    let lowered = written.to_ascii_lowercase();
    for sent in [
        "authorization",
        "anthropic-beta",
        "anthropic-version",
        "bearer",
    ] {
        assert!(!lowered.contains(sent), "the capture carries `{sent}`");
    }
    assert!(
        !written.contains(FIRST_TOKEN),
        "the capture carries the token"
    );
    assert_eq!(report_json["capture"]["bytes"], bytes.len());
    assert_eq!(report_json["capture"]["complete"], true);
    assert!(
        !text.contains(capture.to_str().unwrap()),
        "the report names the capture path"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&capture).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

/// An existing file is never overwritten or widened: the run is refused before any request.
#[tokio::test]
async fn capture_response_to_an_existing_file_is_refused_before_any_request() {
    let store = store();
    put(&store, "llm", "anthropic-subscription", FIRST_TOKEN).await;
    let (origin, listener) = serve().await;
    let directory = tempfile::tempdir().unwrap();
    let capture = directory.path().join("response.capture");
    std::fs::write(&capture, b"kept").unwrap();

    let report = turn::run(
        &args(&origin, &["--capture-response", capture.to_str().unwrap()]),
        Ok(store.clone() as Arc<CredentialStore>),
        async || unreachable!("no rotation was asked for"),
    )
    .await;
    let (text, report_json) = printed(&report);

    assert!(
        tokio::time::timeout(Duration::from_millis(300), listener.accept())
            .await
            .is_err(),
        "a request was sent without its capture file"
    );
    assert!(!report.succeeded(), "{text}");
    assert_eq!(report_json["refused"]["code"], "invalid-request");
    assert!(report_json["turns"].as_array().unwrap().is_empty());
    assert_eq!(std::fs::read(&capture).unwrap(), b"kept");
}
