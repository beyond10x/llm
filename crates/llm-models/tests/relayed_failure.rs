//! A gateway's HTTP 502 before output, end to end: a real protocol client built from a catalog,
//! routing's retry loop over it, and a local server answering as llm-gateway does
//! (story:relayed-failures-keep-their-retry-decision). With `x-should-retry: false` the target
//! is attempted once, on every wire; without it the same 502 is retried on the target until the
//! retry policy is spent. Local sockets only; nothing waits on a real clock between attempts.

use llm_bedrock::{
    Api, AwsCredentials, BedrockClient, BedrockTarget, CredentialSource,
    eventstream::{HeaderValue, Message, encode_message},
};
use llm_core::{
    BoxFuture, Cancel, Capabilities, Dispatch, Error, ErrorCode, Id, Item, Model, Protocol,
    Provenance, TurnRequest, VecSink,
};
use llm_credentials::SecretResolver;
use llm_http::{HttpClient, Limits};
use llm_models::CatalogModels;
use llm_routing::{
    Catalog, CatalogDocument, FallbackPolicy, FallbackRun, Halt, Models, Ports, RetryPolicy,
    Selection,
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

/// llm-gateway's `upstream-failed` refusal body (`crates/llm-gateway/src/server.rs`
/// `refusal_body`, `crates/llm-gateway/src/error.rs:157`).
const GATEWAY_BODY: &str = r#"{"error":{"code":"upstream-failed","message":"the model target could not be reached or failed; the next request asks for a replacement"}}"#;

/// One anonymous endpoint at `base_url` serving one model over `protocol`, and one route to it.
fn catalog(protocol: &str, base_url: &str) -> Catalog {
    let source = format!(
        r#"format = "llm.catalog/1"

[[providers]]
id = "my-lab"
category = "self-hosted"

[[accounts]]
id = "local"
provider_id = "my-lab"
auth_kind = "anonymous"
billing_kind = "self-hosted"

[[endpoints]]
id = "gateway"
account_id = "local"
base_url = "{base_url}"

[[models]]
id = "small"
upstream_name = "example/Small-Model"

[[serving_models]]
id = "relayed-small"
endpoint_id = "gateway"
model_id = "small"
protocol = "{protocol}"
[serving_models.capabilities]
tools = false
tool_choice = false
temperature = false
top_p = false
reasoning_efforts = []
context_window = 4096
max_output_tokens = 1024

[[routes]]
id = "relayed"
alias = "code"

[[targets]]
id = "relayed-primary"
route_id = "relayed"
serving_model_id = "relayed-small"
position = 0
"#
    );
    CatalogDocument::parse(&source)
        .expect("a catalog document")
        .validate()
        .expect("a valid catalog")
}

/// Reads one request head and the body its `content-length` declares.
async fn read_request(socket: &mut TcpStream) {
    let mut bytes = Vec::new();
    let mut buffer = vec![0; 1 << 16];
    loop {
        let length = socket.read(&mut buffer).await.expect("a readable socket");
        assert!(length > 0, "the client closed before sending a request");
        bytes.extend_from_slice(&buffer[..length]);
        let text = String::from_utf8_lossy(&bytes);
        let Some((head, body)) = text.split_once("\r\n\r\n") else {
            continue;
        };
        let declared = head
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        if body.len() >= declared {
            return;
        }
    }
}

/// Answers every request with the gateway's 502, with or without the do-not-retry signal, and
/// counts the requests.
async fn gateway(signal: bool) -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let signal = if signal {
        "x-should-retry: false\r\n"
    } else {
        ""
    };
    answering(format!(
        "HTTP/1.1 502 Bad Gateway\r\ncontent-type: application/json\r\n{signal}content-length: {}\r\nconnection: close\r\n\r\n{GATEWAY_BODY}",
        GATEWAY_BODY.len()
    ))
    .await
}

/// Answers every request with `answer`, head and body, closes, and counts the requests.
async fn answering(answer: String) -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    answering_bytes(answer.into_bytes()).await
}

/// As [`answering`], for an answer whose body is not text.
async fn answering_bytes(
    answer: Vec<u8>,
) -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local port");
    let url = format!("http://{}/v1", listener.local_addr().expect("an address"));
    let requests = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&requests);
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.expect("an accepted connection");
            read_request(&mut socket).await;
            seen.fetch_add(1, Ordering::SeqCst);
            socket.write_all(&answer).await.expect("answer written");
            let _ = socket.shutdown().await;
        }
    });
    (url, requests, server)
}

/// One routed turn over `protocol` against the gateway; the run and the requests it saw.
async fn routed(protocol: &str, signal: bool) -> (FallbackRun, usize) {
    let (run, requests, _) = routed_against(protocol, gateway(signal).await).await;
    (run, requests)
}

/// One routed turn over `protocol` against `server`; the run, the requests the server saw and
/// what the caller's sink was shown.
async fn routed_against(
    protocol: &str,
    (url, requests, server): (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>),
) -> (FallbackRun, usize, VecSink) {
    let catalog = catalog(protocol, &url);
    let http = HttpClient::new(Limits {
        response_headers: Duration::from_secs(20),
        idle: Duration::from_secs(20),
        total: Duration::from_secs(30),
    })
    .expect("bounded transport");
    let resolvers: BTreeMap<Id, Arc<dyn SecretResolver>> = BTreeMap::new();
    let models = CatalogModels::build(&catalog, &http, &resolvers).expect("ports");
    let mut request = TurnRequest::new("code", vec![Item::user("Say hello")]);
    request.max_output_tokens = Some(64);
    let mut admit = |_: &Selection<'_>| -> Result<(), Error> { Ok(()) };
    let mut sink = VecSink::new(64, 64 * 1024);
    let cancel = Cancel::new();
    let pause = |_: Duration| -> BoxFuture<'static, ()> { Box::pin(std::future::ready(())) };
    let run = catalog
        .run_turn(
            &request,
            Some(16),
            FallbackPolicy::default(),
            Ports {
                models: &models,
                admit: &mut admit,
                sink: &mut sink,
                cancel: &cancel,
                pause: &pause,
            },
        )
        .await
        .expect("a run");
    // A second attempt would already have been answered: the run awaited it.
    server.abort();
    (run, requests.load(Ordering::SeqCst), sink)
}

#[tokio::test]
async fn a_gateway_502_that_says_do_not_retry_is_attempted_once_on_every_wire() {
    for protocol in ["responses", "messages", "chat-completions"] {
        let (run, requests) = routed(protocol, true).await;
        assert_eq!(requests, 1, "{protocol}: the gateway was asked again");
        assert_eq!(run.attempts.len(), 1, "{protocol}: {:?}", run.attempts);
        assert_eq!(run.halt, Halt::AmbiguousDispatch, "{protocol}");
        let error = run.result.expect_err("a 502 is a failure");
        assert_eq!(
            (error.code, error.dispatch),
            (ErrorCode::Transport, Dispatch::Unknown),
            "{protocol}"
        );
        assert!(!error.may_retry(), "{protocol}: {error:?}");
        assert!(
            !format!("{error:?}").contains("upstream-failed"),
            "{protocol}: {error:?}"
        );
    }
}

#[tokio::test]
async fn the_same_502_without_the_signal_is_retried_on_its_target() {
    let attempts = usize::try_from(RetryPolicy::DEFAULT.max_attempts).expect("a small bound");
    for protocol in ["responses", "messages", "chat-completions"] {
        let (run, requests) = routed(protocol, false).await;
        assert_eq!(requests, attempts, "{protocol}");
        assert_eq!(
            run.attempts.len(),
            attempts,
            "{protocol}: {:?}",
            run.attempts
        );
        let error = run.result.expect_err("every attempt failed");
        assert_eq!(error.code, ErrorCode::Transport, "{protocol}");
        assert!(
            !error.may_retry(),
            "{protocol}: spent retries go up final: {error:?}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// story:messages-empty-turn-with-an-unmodelled-stop-reason (issue 83), end to end: a Messages
// far side whose every answer stops without content. The retry is bounded by the policy that
// bounds every other retried failure.
// ---------------------------------------------------------------------------------------------

/// A whole Messages stream answered with status 200: the header with `usage`, no content
/// block, the stop `reason`, the terminal event.
fn stop_without_content(reason: &str, usage: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n\
event: message_start\n\
data: {{\"type\":\"message_start\",\"message\":{{\"id\":\"msg_014a\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"example-model-20260201\",\"content\":[],\"usage\":{usage}}}}}\n\n\
event: message_delta\n\
data: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"{reason}\",\"stop_sequence\":null}},\"usage\":{{\"output_tokens\":1}}}}\n\n\
event: message_stop\n\
data: {{\"type\":\"message_stop\"}}\n\n"
    )
}

const PLAIN_USAGE: &str = "{\"input_tokens\":11,\"output_tokens\":1}";

/// A reason in no row is the far side's failure to answer: asked again on its target until the
/// retry policy is spent, then final. Four requests under the default policy, and no more.
#[tokio::test]
async fn a_messages_stop_without_content_for_a_reason_in_no_row_ends_after_the_policys_attempts() {
    let attempts = usize::try_from(RetryPolicy::DEFAULT.max_attempts).expect("a small bound");
    assert_eq!(attempts, 4);
    for reason in ["pause_turn", "stop_sequence", "a_reason_in_no_list"] {
        let server = answering(stop_without_content(reason, PLAIN_USAGE)).await;
        let (run, requests, _) = routed_against("messages", server).await;
        assert_eq!(requests, attempts, "`{reason}`");
        assert_eq!(
            run.attempts.len(),
            attempts,
            "`{reason}`: {:?}",
            run.attempts
        );
        let error = run
            .result
            .expect_err("every attempt stopped without content");
        assert_eq!(
            (error.code, error.dispatch),
            (ErrorCode::Unavailable, Dispatch::Accepted),
            "`{reason}`"
        );
        assert!(
            !error.may_retry(),
            "`{reason}`: spent retries go up final: {error:?}"
        );
    }
}

/// The two reasons with a final row are asked once.
#[tokio::test]
async fn a_messages_stop_at_the_context_window_or_as_a_refusal_without_content_is_asked_once() {
    for (reason, code) in [
        ("model_context_window_exceeded", ErrorCode::TooLarge),
        ("refusal", ErrorCode::Refused),
    ] {
        let server = answering(stop_without_content(reason, PLAIN_USAGE)).await;
        let (run, requests, _) = routed_against("messages", server).await;
        assert_eq!(requests, 1, "`{reason}`: the far side was asked again");
        assert_eq!(run.attempts.len(), 1, "`{reason}`: {:?}", run.attempts);
        let error = run.result.expect_err("the turn stopped without content");
        assert_eq!(
            (error.code, error.dispatch),
            (code, Dispatch::Accepted),
            "`{reason}`"
        );
        assert!(!error.may_retry(), "`{reason}`: {error:?}");
    }
}

/// A route that reports a cost with its usage is retried the same way: the turn that stops
/// without content shows the caller no warning, so the run never halts on visible output and
/// ends after the policy's attempts.
#[tokio::test]
async fn a_messages_stop_without_content_on_a_route_that_reports_a_cost_is_retried_the_same_way() {
    let attempts = usize::try_from(RetryPolicy::DEFAULT.max_attempts).expect("a small bound");
    let usage = "{\"input_tokens\":11,\"output_tokens\":1,\"cost\":{\"total\":0.5}}";
    let server = answering(stop_without_content("pause_turn", usage)).await;
    let (run, requests, sink) = routed_against("messages", server).await;
    assert_eq!(
        requests, attempts,
        "the far side was asked {requests} times"
    );
    assert_eq!(run.attempts.len(), attempts, "{:?}", run.attempts);
    assert_ne!(run.halt, Halt::VisibleOutput);
    // Routing says each retry; the route's cost is never shown.
    assert_eq!(sink.events().len(), attempts - 1, "{:?}", sink.events());
    assert!(
        sink.events().iter().all(|event| matches!(event,
            llm_core::StreamEvent::Warning { code, .. } if code == "turn-retried")),
        "{:?}",
        sink.events()
    );
    let error = run
        .result
        .expect_err("every attempt stopped without content");
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert!(!error.may_retry(), "{error:?}");
}

// ---------------------------------------------------------------------------------------------
// story:bedrock-converse-empty-turn-with-an-unmodelled-stop-reason: a Bedrock Converse far side
// whose answer stops without content for a reason with no row of its own.
//
// `BedrockClient` asks once a turn and hands the retriable failure to its caller. No code in
// this repository puts a Converse client under the retry policy: a Bedrock route has no catalog
// entry, no catalog can name `bedrock-converse`, `Routes::port` hands out the bare client and
// the `llm` command calls `turn` once. A caller that retries bounds the retry itself. The one
// test here holds the two facts that can be held: one request a turn, and routing's loop
// refusing the client under a catalog target.
// ---------------------------------------------------------------------------------------------

/// Fixed AWS keys for a far side that checks no signature.
struct FixtureKeys;

impl CredentialSource for FixtureKeys {
    fn credentials<'a>(&'a self, _: &'a Cancel) -> BoxFuture<'a, Result<AwsCredentials, Error>> {
        Box::pin(async {
            Ok(AwsCredentials::new(
                "AKIDFIXTURE",
                "fixture-secret",
                None,
                None,
            ))
        })
    }
}

/// The bare Converse client as the one model a run is given, whatever serving model it asks
/// for.
struct Bare(BedrockClient);

impl Models for Bare {
    fn model(&self, _: &Id) -> Option<&dyn Model> {
        Some(&self.0)
    }
}

/// One Converse event-stream frame.
fn converse_event(kind: &str, payload: &serde_json::Value) -> Vec<u8> {
    let header = |name: &str, value: &str| (name.to_owned(), HeaderValue::String(value.to_owned()));
    encode_message(&Message {
        headers: vec![
            header(":event-type", kind),
            header(":content-type", "application/json"),
            header(":message-type", "event"),
        ],
        payload: serde_json::to_vec(payload).expect("a payload"),
    })
}

/// A whole Converse stream answered with status 200: the message opens, no content block, the
/// stop `reason`, the usage report.
fn converse_stop_without_content(reason: &str) -> Vec<u8> {
    let mut answer = b"HTTP/1.1 200 OK\r\ncontent-type: application/vnd.amazon.eventstream\r\n\
        connection: close\r\n\r\n"
        .to_vec();
    answer.extend(converse_event(
        "messageStart",
        &serde_json::json!({"role": "assistant"}),
    ));
    answer.extend(converse_event(
        "messageStop",
        &serde_json::json!({"stopReason": reason}),
    ));
    answer.extend(converse_event(
        "metadata",
        &serde_json::json!({"usage": {"inputTokens": 11, "outputTokens": 0, "totalTokens": 11}}),
    ));
    answer
}

/// A catalog with one target, and a streamed Converse client for the far side at `url` whose
/// provenance is that target's but for the protocol, which no catalog target can have.
fn converse_beside_a_catalog_target(url: &str) -> (Catalog, BedrockClient) {
    let catalog = catalog("messages", url);
    let binding = catalog
        .bindings()
        .next()
        .expect("one serving model")
        .1
        .provenance()
        .clone();
    let origin = url.strip_suffix("v1").expect("the fixture's base URL");
    let target = BedrockTarget::new(
        Provenance {
            protocol: Protocol::BedrockConverse,
            ..binding
        },
        Capabilities::text(4096, 1024).with_output_streaming(true),
        "us-east-1",
        "us.amazon.nova-micro-v1:0",
        Api::Converse,
        true,
        Some(url::Url::parse(origin).expect("an origin")),
    )
    .expect("a Converse target");
    let http = HttpClient::new(Limits {
        response_headers: Duration::from_secs(20),
        idle: Duration::from_secs(20),
        total: Duration::from_secs(30),
    })
    .expect("bounded transport");
    let client = BedrockClient::new(target, http, Arc::new(FixtureKeys)).expect("a client");
    (catalog, client)
}

/// The client itself asks once a turn and hands the retriable failure up: the retry is the
/// caller's. And routing's loop takes the same client under no catalog target, because no
/// catalog names `bedrock-converse`: it refuses the run before any request is sent.
#[tokio::test]
async fn the_converse_client_asks_once_a_turn_and_routing_takes_it_under_no_catalog_target() {
    let answer = converse_stop_without_content("a_reason_in_no_list");
    let (url, requests, server) = answering_bytes(answer.clone()).await;
    let (_, client) = converse_beside_a_catalog_target(&url);
    let mut sink = VecSink::new(64, 64 * 1024);
    let request = TurnRequest::new("small", vec![Item::user("Say hello")]);
    let error = client
        .turn(&request, &mut sink, &Cancel::new())
        .await
        .expect_err("the turn stopped without content");
    server.abort();
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    assert_eq!(
        (error.code, error.dispatch, error.may_retry()),
        (ErrorCode::Unavailable, Dispatch::Accepted, true),
        "{error:?}"
    );
    assert_eq!(sink.events().len(), 0, "{:?}", sink.events());

    let (url, requests, server) = answering_bytes(answer).await;
    let (catalog, client) = converse_beside_a_catalog_target(&url);
    let mut request = TurnRequest::new("code", vec![Item::user("Say hello")]);
    request.max_output_tokens = Some(64);
    let mut admit = |_: &Selection<'_>| -> Result<(), Error> { Ok(()) };
    let mut sink = VecSink::new(64, 64 * 1024);
    let cancel = Cancel::new();
    let pause = |_: Duration| -> BoxFuture<'static, ()> { Box::pin(std::future::ready(())) };
    let run = catalog
        .run_turn(
            &request,
            Some(16),
            FallbackPolicy::default(),
            Ports {
                models: &Bare(client),
                admit: &mut admit,
                sink: &mut sink,
                cancel: &cancel,
                pause: &pause,
            },
        )
        .await;
    server.abort();
    let refused = run.expect_err("a Converse client serves no catalog target");
    assert_eq!(refused.code, ErrorCode::InvalidRequest, "{refused:?}");
    assert_eq!(requests.load(Ordering::SeqCst), 0);
}

// ---------------------------------------------------------------------------------------------
// Adversary, wave 2026-11-25-messages-empty-turn, unit 1
// (story:messages-empty-turn-with-an-unmodelled-stop-reason). The story's acceptance: the failure
// is returned "with its usage". What that usage is worth is what `llm-cost` makes of the attempt
// routing recorded, handed over as `llm-calls` hands it (`crates/llm-calls/src/lib.rs`,
// `attempt_usage`): the finality is transferred from the observation, never inferred
// (`docs/pricing.md`: "A failed attempt can have final usage").
// ---------------------------------------------------------------------------------------------

/// Every input counter, so the uncached input is known: 4000 fresh tokens, none cached.
const ADVERSARY_W30_FULL_USAGE: &str = "{\"input_tokens\":4000,\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":0,\"output_tokens\":1}";

/// The cost record `llm-cost` writes for the one attempt of a routed turn whose answer stopped
/// for `reason` without content, priced at 1 per thousand input and 2 per thousand output tokens.
async fn adversary_w30_cost_of_the_one_attempt(reason: &str) -> llm_cost::Record {
    use llm_cost::{
        AccountingInput, Amount, AttemptUsage, Currency, ModelPrice, Observation, PriceBook,
        PriceDocument, Rate,
    };
    let server = answering(stop_without_content(reason, ADVERSARY_W30_FULL_USAGE)).await;
    let (run, requests, _) = routed_against("messages", server).await;
    assert_eq!(requests, 1, "`{reason}`");
    let attempt = run.attempts.first().expect("one attempt");
    let llm_routing::AttemptResult::Failed {
        dispatch,
        observation: Some(observed),
        ..
    } = &attempt.result
    else {
        panic!("`{reason}`: a failed attempt with its observation: {attempt:?}");
    };
    let id = |value: &str| Id::new(value).expect("an identifier");
    let rate = |amount: &str| {
        Some(Rate {
            amount: Amount::parse(amount).expect("an amount"),
            per_units: 1000,
        })
    };
    let prices = PriceBook::from_document(PriceDocument::new(
        id("prices-r1"),
        "fixture price list",
        1,
        Currency::new("USD").expect("a currency"),
        vec![ModelPrice {
            id: id("price-1"),
            serving_model_id: attempt.serving_model_id.clone(),
            binding_revision: attempt.provenance.binding_revision.clone(),
            upstream_model: id("example-model-20260201"),
            input: rate("1"),
            cached_input: rate("1"),
            cache_creation_input: rate("1"),
            output: rate("2"),
        }],
    ))
    .expect("a price book");
    let usage = AttemptUsage {
        id: id("call-1"),
        usage_record_id: id("call-1"),
        request_id: id("call-1"),
        route_target_id: attempt.target_id.clone(),
        serving_model_id: attempt.serving_model_id.clone(),
        price_id: id("price-1"),
        binding: attempt.provenance.clone(),
        upstream_model: observed.upstream_model.clone(),
        billing: attempt.billing_kind,
        dispatch: *dispatch,
        failed: true,
        final_usage: observed.final_usage,
        usage: observed.usage.clone().expect("the reported counters"),
    };
    let report = prices
        .quote(&AccountingInput::new(vec![Observation::Attempt(usage)]))
        .expect("a quote");
    report.records.into_iter().next().expect("one record")
}

/// A turn that stops at the context window is the turn with the largest input a route bills.
/// Its stream ended with `message_stop` and reported every counter, so the 4000 input tokens are
/// priced and the attempt's cost is complete: 4 for the input, 0.002 for the one output token.
#[tokio::test]
async fn adversary_w30_a_stop_at_the_context_window_without_content_is_priced_from_its_terminal_counters()
 {
    use llm_cost::{Amount, Unit};
    for reason in ["model_context_window_exceeded", "refusal"] {
        let record = adversary_w30_cost_of_the_one_attempt(reason).await;
        let input = record
            .lines
            .iter()
            .find(|line| line.unit == Unit::InputToken)
            .expect("an input line");
        assert_eq!(
            (input.amount, input.unknown),
            (Some(Amount::parse("4").expect("an amount")), None),
            "`{reason}`: the 4000 input tokens the route reported and billed: {record:?}"
        );
        assert_eq!(
            record.total.complete_total,
            Some(Amount::parse("4.002").expect("an amount")),
            "`{reason}`: every counter was reported before `message_stop`: {record:?}"
        );
    }
}
