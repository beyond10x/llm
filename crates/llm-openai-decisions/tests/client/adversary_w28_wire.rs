//! Adversary cases of wave 28 against the decision endpoint client: wire edges the suite's own
//! fixture server cannot produce (it always writes a whole `application/json` body of the
//! length it declares), and guards for mutants the suite does not catch. Loopback only.

use super::{
    Answer, EXAMPLE_RESPONSE, Frozen, LONG, catalog, catalog_toml, cause, classify, example, http,
    id, labels, no_prices, recorded,
};
use llm_calls::{CallIdentity, CallLog, CallRecord, Outcome, Recording};
use llm_core::{BoxFuture, Cancel, Dispatch, Error, ErrorCode, Usage};
use llm_cost::{Amount, Currency, ModelPrice, PriceBook, PriceDocument, Rate};
use llm_credentials::{
    AnonymousResolver, ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver,
    SecretVersion,
};
use llm_decide::AbstentionCause;
use llm_openai_decisions::{DecisionRequest, DecisionResponse, OpenAiDecisions, Question};
use llm_routing::Catalog;
use std::{
    net::SocketAddr,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const BEARER: &str =
    "auth_kind = \"bearer\"\nbilling_kind = \"metered\"\nsecret_reference_id = \"decide-token\"";

/// What the raw fixture does once it has read one whole request.
#[derive(Clone)]
struct Raw {
    /// Written byte for byte: the status line, the headers and whatever body follows.
    bytes: Vec<u8>,
    /// Keeps the connection open after writing instead of closing it.
    stall: bool,
}

/// Reads one request to the end of its declared body; returns its head in lower case.
async fn read_whole_request(socket: &mut TcpStream) -> Option<String> {
    let mut bytes = Vec::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    let end = loop {
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break end;
        }
        let read = socket.read(&mut buffer).await.ok()?;
        if read == 0 {
            return None;
        }
        bytes.extend_from_slice(&buffer[..read]);
    };
    let head = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
    let length: usize = head
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .and_then(|value| value.trim().parse().ok())?;
    let mut received = bytes.len() - (end + 4);
    while received < length {
        let read = socket.read(&mut buffer).await.ok()?;
        if read == 0 {
            return None;
        }
        received += read;
    }
    Some(head)
}

/// A server that answers every request with `raw`, and the head of each request it read.
async fn serve_raw(raw: Raw) -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let heads = Arc::new(Mutex::new(Vec::new()));
    let shared = heads.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let Some(head) = read_whole_request(&mut socket).await else {
                continue;
            };
            shared
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(head);
            let _ = socket.write_all(&raw.bytes).await;
            let _ = socket.flush().await;
            if raw.stall {
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    drop(socket);
                });
            } else {
                let _ = socket.shutdown().await;
            }
        }
    });
    (address, heads)
}

fn heads(seen: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
    seen.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// One call of the reference example against a raw answer.
async fn decide_raw(bytes: Vec<u8>) -> Result<DecisionResponse, Error> {
    let (address, _) = serve_raw(Raw {
        bytes,
        stall: false,
    })
    .await;
    let catalog = catalog(address);
    let request = example();
    let selection = OpenAiDecisions::select(&catalog, &request, Some(100)).unwrap();
    OpenAiDecisions::new(http(), Arc::new(AnonymousResolver))
        .decide(&selection, &request, &Cancel::new())
        .await
}

/// One recorded call of `request` through `client`, under `prices`.
async fn record_call(
    client: &OpenAiDecisions,
    catalog: &Catalog,
    request: &DecisionRequest,
    prices: Option<&PriceBook>,
    cancel: &Cancel,
) -> (Result<DecisionResponse, Error>, Vec<CallRecord>) {
    let selection = OpenAiDecisions::select(catalog, request, Some(100)).unwrap();
    let log = CallLog::new();
    let clock = Frozen(1_700_000_000_000);
    let result = client
        .decide_recorded(
            &selection,
            request,
            cancel,
            CallIdentity::primary(id("req-1")),
            Recording {
                sink: &log,
                clock: &clock,
                prices,
            },
        )
        .await;
    (result, log.records())
}

/// A resolver that counts what it is asked and answers every reference alike.
struct Counting {
    asked: AtomicUsize,
    refuses: bool,
}

impl SecretResolver for Counting {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async {
            self.asked.fetch_add(1, Ordering::SeqCst);
            if self.refuses {
                return Err(SecretError::Missing);
            }
            Ok(ResolvedSecret {
                secret: Secret::new(b"fixture-token".to_vec())?,
                version: SecretVersion::new("v1".to_owned())?,
            })
        })
    }
}

/// The design: "A body cut short ... [is] refused `protocol` with dispatch `accepted`". Where the
/// end of the connection is the end of the body, the cut is visible only in the document.
#[tokio::test]
async fn adversary_w28_a_body_cut_where_the_connection_ends_it_is_refused_protocol() {
    let half = &EXAMPLE_RESPONSE[..EXAMPLE_RESPONSE.len() / 2];
    let bytes = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\n\r\n{half}"
    );
    let error = decide_raw(bytes.into_bytes()).await.unwrap_err();
    assert_eq!(
        (error.code, error.dispatch),
        (ErrorCode::Protocol, Dispatch::Accepted)
    );
}

/// The same cut where the answer declared its length, or its chunks: the transport reports the
/// body as cut, and the client hands that failure on as the transport's own (design "Typed
/// decisions", the crate documentation, the changelog line "a body the transport reports as cut").
/// First written against `protocol`, which those documents then stated for every cut.
#[tokio::test]
async fn adversary_w28_a_body_cut_short_of_its_declared_length_is_a_retriable_transport_failure() {
    let half = &EXAMPLE_RESPONSE[..EXAMPLE_RESPONSE.len() / 2];
    let declared = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{half}",
        EXAMPLE_RESPONSE.len()
    );
    let chunked = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n{:x}\r\n{half}\r\n",
        half.len()
    );
    let mut refused = Vec::new();
    for (framing, bytes) in [("content-length", declared), ("chunked", chunked)] {
        let error = decide_raw(bytes.into_bytes()).await.unwrap_err();
        refused.push((framing, error.code, error.dispatch, error.retriable));
    }
    assert_eq!(
        refused,
        [
            (
                "content-length",
                ErrorCode::Transport,
                Dispatch::Accepted,
                true
            ),
            ("chunked", ErrorCode::Transport, Dispatch::Accepted, true),
        ]
    );
}

/// The suite's `repeated_escaped` body is the same text as its `repeated_root`: no member of it
/// is escaped. This one is.
#[tokio::test]
async fn adversary_w28_a_member_repeated_under_an_escaped_name_is_refused() {
    let whole = "decision answer is not one whole JSON document with every member once, nested at most 16 deep";
    let root = EXAMPLE_RESPONSE.replacen('{', "{\"mod\\u0065l\": \"gpt-6-luna\",", 1);
    assert!(root.contains("\\u0065"), "the escape is in the bytes sent");
    let inside = EXAMPLE_RESPONSE.replace(
        r#""probability": 0.95"#,
        "\"prob\\u0061bility\": 0.05, \"probability\": 0.95",
    );
    for text in [root, inside] {
        let head = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            text.len()
        );
        let error = decide_raw(format!("{head}{text}").into_bytes())
            .await
            .unwrap_err();
        assert_eq!(
            (error.code, error.dispatch, error.message.as_str()),
            (ErrorCode::Protocol, Dispatch::Accepted, whole),
            "{text}"
        );
    }
    let request = "{\"model\":\"decide\",\"input\":\"x\",\"inp\\u0075t\":\"y\",\"questions\":[{\"type\":\"predicate\",\"instructions\":\"?\"}]}";
    let error = DecisionRequest::from_json(request).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert!(
        error.message.contains("not one whole JSON document"),
        "{}",
        error.message
    );
}

/// "A call refused before its credential is resolved writes none"; one that reached the
/// credential and was refused there writes one: not sent, and not priced.
#[tokio::test]
async fn adversary_w28_a_call_whose_credential_is_refused_writes_one_unsent_unpriced_record() {
    let (address, seen) = serve_raw(Raw {
        bytes: Vec::new(),
        stall: false,
    })
    .await;
    let catalog = Catalog::parse(&catalog_toml(address, "responses", BEARER)).unwrap();
    let prices = super::prices(&catalog);
    let resolver = Arc::new(Counting {
        asked: AtomicUsize::new(0),
        refuses: true,
    });
    let client = OpenAiDecisions::new(http(), resolver.clone());
    let (result, records) = record_call(
        &client,
        &catalog,
        &example(),
        prices.as_ref(),
        &Cancel::new(),
    )
    .await;
    let error = result.unwrap_err();
    assert_eq!(
        (error.code, error.dispatch),
        (ErrorCode::Unauthorized, Dispatch::NotSent)
    );
    assert_eq!(resolver.asked.load(Ordering::SeqCst), 1);
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0].outcome, Outcome::Failed);
    assert_eq!(records[0].error_code, Some(ErrorCode::Unauthorized));
    assert_eq!(records[0].dispatch, Some(Dispatch::NotSent));
    assert_eq!(records[0].attempts, 1);
    assert_eq!(records[0].usage, Usage::default());
    assert_eq!(records[0].upstream_model, None);
    assert_eq!(
        records[0].cost, None,
        "a call that was not sent cost nothing"
    );
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(heads(&seen).is_empty(), "nothing was sent");
}

/// The design: a call refused before its credential (a body over the bound among them) writes
/// no record. The suite sends the body over the bound through `decide` only, with an anonymous
/// account.
#[tokio::test]
async fn adversary_w28_a_body_over_the_bound_never_reaches_the_credential_or_the_record() {
    let (address, seen) = serve_raw(Raw {
        bytes: Vec::new(),
        stall: false,
    })
    .await;
    let catalog = Catalog::parse(&catalog_toml(address, "responses", BEARER)).unwrap();
    let resolver = Arc::new(Counting {
        asked: AtomicUsize::new(0),
        refuses: false,
    });
    let client = OpenAiDecisions::new(http(), resolver.clone());
    // Every text inside its own bound: 10485760 characters of input and six instructions of
    // 1048576, together more than the 16777216 bytes one request may carry.
    let question = Question::Predicate {
        name: None,
        instructions: "q".repeat(1_048_576),
    };
    let request = DecisionRequest::new("decide", "i".repeat(10_485_760), vec![question; 6]);
    assert!(request.validate().is_ok());
    let (result, records) = record_call(&client, &catalog, &request, None, &Cancel::new()).await;
    let error = result.unwrap_err();
    assert_eq!(
        (error.code, error.dispatch),
        (ErrorCode::TooLarge, Dispatch::NotSent)
    );
    assert_eq!(
        resolver.asked.load(Ordering::SeqCst),
        0,
        "the credential was resolved for a call refused before I/O"
    );
    assert!(records.is_empty(), "{records:?}");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(heads(&seen).is_empty(), "nothing was sent");
}

/// The answer, the record and the price follow the model the body named. In the suite every
/// body names the model the catalog asks for, so the two cannot be told apart there.
#[tokio::test]
async fn adversary_w28_the_model_the_body_names_is_returned_recorded_and_priced() {
    let dated = "gpt-6-luna-2026-11-01";
    let answer = EXAMPLE_RESPONSE.replacen("\"gpt-6-luna\"", &format!("\"{dated}\""), 1);
    let two_prices = |catalog: &Catalog| {
        let binding = catalog.binding(&id("decider")).unwrap();
        let rate = |amount: &str| {
            Some(Rate {
                amount: Amount::parse(amount).unwrap(),
                per_units: 1000,
            })
        };
        let price = |price_id: &str, upstream: &str, input: &str| ModelPrice {
            id: id(price_id),
            serving_model_id: id("decider"),
            binding_revision: binding.provenance().binding_revision.clone(),
            upstream_model: id(upstream),
            input: rate(input),
            cached_input: rate(input),
            cache_creation_input: rate(input),
            output: rate(input),
        };
        Some(
            PriceBook::from_document(PriceDocument::new(
                id("prices-r1"),
                "fixture price list",
                1,
                Currency::new("USD").unwrap(),
                // A price book orders its prices by id: the one naming the reported model is
                // the second, so "the first declared" is not it.
                vec![
                    price("price-a-luna", "gpt-6-luna", "1"),
                    price("price-b-dated", dated, "3"),
                ],
            ))
            .unwrap(),
        )
    };
    let (result, records, seen) =
        recorded(vec![Answer::Body(answer)], &example(), two_prices).await;
    assert_eq!(
        seen.bodies()[0]["model"],
        "gpt-6-luna",
        "the catalog's upstream model was asked"
    );
    assert_eq!(result.unwrap().model, dated);
    assert_eq!(records[0].upstream_model, Some(id(dated)));
    let cost = records[0].cost.as_ref().expect("a priced call has a cost");
    // 42 input tokens at 3 per 1000, the price that names the reported model.
    assert!(cost.complete, "{cost:?}");
    assert_eq!(cost.known_subtotal, Amount::parse("0.126").unwrap());
}

/// The request names the media type it accepts, and an answer of another one is not read.
#[tokio::test]
async fn adversary_w28_the_request_asks_for_json_and_another_media_type_is_refused() {
    let head = |media: &str| {
        format!(
            "HTTP/1.1 200 OK\r\ncontent-type: {media}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{EXAMPLE_RESPONSE}",
            EXAMPLE_RESPONSE.len()
        )
        .into_bytes()
    };
    let (address, seen) = serve_raw(Raw {
        bytes: head("application/json; charset=utf-8"),
        stall: false,
    })
    .await;
    let catalog = catalog(address);
    let request = example();
    let selection = OpenAiDecisions::select(&catalog, &request, Some(100)).unwrap();
    OpenAiDecisions::new(http(), Arc::new(AnonymousResolver))
        .decide(&selection, &request, &Cancel::new())
        .await
        .unwrap();
    let sent = heads(&seen);
    assert_eq!(sent.len(), 1);
    assert!(
        sent[0]
            .lines()
            .any(|line| line == "accept: application/json"),
        "{}",
        sent[0]
    );

    for media in ["text/plain", "text/event-stream", "application/jsonl"] {
        let error = decide_raw(head(media)).await.unwrap_err();
        assert_eq!(
            (error.code, error.dispatch),
            (ErrorCode::Protocol, Dispatch::Accepted),
            "{media}"
        );
    }
}

/// A cancel while the body arrives: the answer was accepted, the call is recorded cancelled.
#[tokio::test]
async fn adversary_w28_a_call_cancelled_while_its_body_arrives_is_recorded_cancelled() {
    let half = &EXAMPLE_RESPONSE[..EXAMPLE_RESPONSE.len() / 2];
    let bytes = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{half}",
        EXAMPLE_RESPONSE.len()
    );
    let (address, seen) = serve_raw(Raw {
        bytes: bytes.into_bytes(),
        stall: true,
    })
    .await;
    let catalog = catalog(address);
    let cancel = Cancel::new();
    let stopper = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        stopper.cancel();
    });
    let client = OpenAiDecisions::new(http(), Arc::new(AnonymousResolver));
    let (result, records) = record_call(&client, &catalog, &example(), None, &cancel).await;
    let error = result.unwrap_err();
    assert_eq!(
        (error.code, error.dispatch),
        (ErrorCode::Cancelled, Dispatch::Accepted)
    );
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0].outcome, Outcome::Cancelled);
    assert_eq!(records[0].dispatch, Some(Dispatch::Accepted));
    assert_eq!(records[0].usage, Usage::default());
    assert_eq!(heads(&seen).len(), 1, "one request");
}

/// The suite holds every character bound with a two-byte character only, where the byte length
/// is far past the bound on both sides. Texts of one-byte characters stand at the edge of the
/// byte-length shortcut itself.
#[test]
fn adversary_w28_a_text_of_one_byte_characters_is_held_to_the_same_bounds() {
    let outside = |request: &DecisionRequest| {
        let error = request.validate().expect_err("outside its bounds");
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        error.message
    };
    let asking = |instructions: String| {
        DecisionRequest::new(
            "decide",
            "x",
            vec![Question::Predicate {
                name: None,
                instructions,
            }],
        )
    };
    assert!(asking("q".repeat(1_048_576)).validate().is_ok());
    assert_eq!(
        outside(&asking("q".repeat(1_048_577))),
        "decision request is outside the reference's bounds: questions[0].instructions"
    );
    let mut request = asking(String::new());
    request.input = "i".repeat(10_485_760);
    assert!(request.validate().is_ok());
    request.input = "i".repeat(10_485_761);
    assert_eq!(
        outside(&request),
        "decision request is outside the reference's bounds: input"
    );
    let mut request = asking(String::new());
    request.safety_identifier = Some("s".repeat(128));
    request.prompt_cache_key = Some("k".repeat(64));
    assert!(request.validate().is_ok());
    request.prompt_cache_key = Some("k".repeat(65));
    assert_eq!(
        outside(&request),
        "decision request is outside the reference's bounds: prompt_cache_key"
    );
    request.prompt_cache_key = None;
    request.safety_identifier = Some("s".repeat(129));
    assert_eq!(
        outside(&request),
        "decision request is outside the reference's bounds: safety_identifier"
    );
}

/// One attempt whatever the status: a 5xx, a redirect and a 408 are each one request and one
/// record.
#[tokio::test]
async fn adversary_w28_a_refusing_status_is_one_request_and_one_record() {
    for status in [500, 503, 529, 408, 307, 302] {
        let (result, records, seen) =
            recorded(vec![Answer::Status(status)], &example(), no_prices).await;
        assert!(result.is_err(), "{status}");
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(seen.requests().len(), 1, "{status}");
        assert_eq!(records.len(), 1, "{status}");
        assert_eq!(records[0].outcome, Outcome::Failed, "{status}");
        assert_eq!(records[0].attempts, 1, "{status}");
    }
}

/// A probability is the double its text names, whatever form the text has: the basis points
/// follow that double, and a text that is no number is no probability.
#[tokio::test]
async fn adversary_w28_a_probability_is_read_as_the_double_its_text_names() {
    let weighing = |probability: &str| {
        Answer::Body(format!(
            r#"{{"model":"gpt-6-luna","answers":[{{"type":"choice","name":null,"choice":"angry","probabilities":[{{"value":"angry","probability":{probability}}}],"confidence":0.5}}],"usage":{{}}}}"#
        ))
    };
    for (text, expected) in [
        ("-0.0", Ok(0)),
        ("-0", Ok(0)),
        ("0e0", Ok(0)),
        ("1E0", Ok(10_000)),
        ("10000e-4", Ok(10_000)),
        ("5e-324", Ok(0)),
        ("1e-400", Ok(0)),
        // The double nearest to this text is 1.
        ("0.99999999999999999", Ok(10_000)),
        // 0.00005 parses above half a basis point; 0.00015 parses below one and a half.
        ("0.00005", Ok(1)),
        ("0.00015", Ok(1)),
        ("0.03125", Ok(313)),
        // The double next above 1, and the largest finite one.
        ("1.0000000000000002", Err(AbstentionCause::OffLabel)),
        ("1.7976931348623157e308", Err(AbstentionCause::OffLabel)),
        ("-1e-400", Ok(0)),
        ("-5e-324", Err(AbstentionCause::OffLabel)),
        (
            "\"0.5\"",
            Err(AbstentionCause::ProviderError(ErrorCode::Protocol)),
        ),
        (
            "1e400",
            Err(AbstentionCause::ProviderError(ErrorCode::Protocol)),
        ),
    ] {
        let classified =
            classify(vec![weighing(text)], labels(&["angry", "calm"]), LONG, None).await;
        let got = match classified.outcome.decision() {
            Some(decision) => Ok(decision.confidences[0].confidence_bps.expect("weighed")),
            None => Err(cause(&classified).expect("an abstention has a cause")),
        };
        assert_eq!(got, expected, "{text}");
    }
}
