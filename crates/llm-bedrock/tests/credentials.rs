//! The two credential sources: container credentials against a local fixture endpoint, and
//! static keys from an injected resolver.

use llm_bedrock::{AwsCredentials, ContainerCredentials, CredentialSource, StaticCredentials};
use llm_core::{BoxFuture, Cancel, Dispatch, Error, ErrorCode};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_http::{HttpClient, Limits};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;

const ACCESS_KEY_ID: &str = "ASIAFIXTUREKEYID";
const RENEWED_KEY_ID: &str = "ASIAFIXTURERENEWED";
const SECRET_ACCESS_KEY: &str = "fixture/secret+access-key";
const SESSION_TOKEN: &str = "fixture-session-token";
const AUTHORIZATION_TOKEN: &str = "fixture-authorization-token";
const TOKEN_REFERENCE: &str = "container-token";

/// Every secret value a test uses: none may reach a message or a `Debug` output.
const SECRETS: [&str; 4] = [
    ACCESS_KEY_ID,
    SECRET_ACCESS_KEY,
    SESSION_TOKEN,
    AUTHORIZATION_TOKEN,
];

/// A resolver over a fixed table; a reference outside it is missing.
struct Table(BTreeMap<&'static str, &'static str>);

impl SecretResolver for Table {
    fn resolve<'a>(
        &'a self,
        reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async move {
            let value = self.0.get(reference.as_str()).ok_or(SecretError::Missing)?;
            Ok(ResolvedSecret {
                secret: Secret::new(value.as_bytes().to_vec())?,
                version: SecretVersion::new("1".to_owned())?,
            })
        })
    }
}

fn table(entries: &[(&'static str, &'static str)]) -> Arc<dyn SecretResolver> {
    Arc::new(Table(entries.iter().copied().collect()))
}

fn reference(name: &str) -> SecretRef {
    SecretRef::new(name).expect("a valid reference")
}

/// A local credentials endpoint: it answers each connection with the next of its canned
/// responses, the last one repeated, and keeps the head of each request it read.
struct Endpoint {
    uri: String,
    requests: Arc<Mutex<Vec<String>>>,
    task: JoinHandle<()>,
}

impl Endpoint {
    async fn answering(response: String) -> Self {
        Self::answering_each(vec![response]).await
    }

    async fn answering_each(responses: Vec<String>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a local port");
        let address = listener.local_addr().expect("its address");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&requests);
        let task = tokio::spawn(async move {
            let mut answered = 0;
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut head = Vec::new();
                let mut buffer = [0_u8; 1024];
                while !head.windows(4).any(|window| window == b"\r\n\r\n") {
                    match stream.read(&mut buffer).await {
                        Ok(0) | Err(_) => break,
                        Ok(count) => head.extend_from_slice(&buffer[..count]),
                    }
                }
                seen.lock()
                    .expect("the request log")
                    .push(String::from_utf8_lossy(&head).into_owned());
                let response = &responses[answered.min(responses.len() - 1)];
                answered += 1;
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });
        Self {
            uri: format!("http://{address}/v1/credentials"),
            requests,
            task,
        }
    }

    async fn json(status: &str, document: &Value) -> Self {
        Self::answering(response(status, &document.to_string())).await
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("the request log").clone()
    }

    fn source(&self, token: &'static str) -> ContainerCredentials {
        container(&self.uri, token)
    }
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// A local credentials endpoint that answers its first connection at once with `first`, and
/// every later one with `later` only once `gate` gives it a permit. Each connection is served on
/// its own task and its request head is logged as soon as it is read, so a stalled refresh and
/// any second refresh are both seen.
struct Gated {
    uri: String,
    requests: Arc<Mutex<Vec<String>>>,
    gate: Arc<Semaphore>,
    task: JoinHandle<()>,
}

impl Gated {
    async fn new(first: &Value, later: &Value) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a local port");
        let address = listener.local_addr().expect("its address");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let gate = Arc::new(Semaphore::new(0));
        let (seen, permits) = (Arc::clone(&requests), Arc::clone(&gate));
        let first = response("200 OK", &first.to_string());
        let later = response("200 OK", &later.to_string());
        let task = tokio::spawn(async move {
            let mut answered = 0_usize;
            while let Ok((mut stream, _)) = listener.accept().await {
                let (seen, gate) = (Arc::clone(&seen), Arc::clone(&permits));
                let (response, gated) = if answered == 0 {
                    (first.clone(), false)
                } else {
                    (later.clone(), true)
                };
                answered += 1;
                tokio::spawn(async move {
                    let mut head = Vec::new();
                    let mut buffer = [0_u8; 1024];
                    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
                        match stream.read(&mut buffer).await {
                            Ok(0) | Err(_) => break,
                            Ok(count) => head.extend_from_slice(&buffer[..count]),
                        }
                    }
                    seen.lock()
                        .expect("the request log")
                        .push(String::from_utf8_lossy(&head).into_owned());
                    if gated {
                        gate.acquire().await.expect("the gate").forget();
                    }
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        Self {
            uri: format!("http://{address}/v1/credentials"),
            requests,
            gate,
            task,
        }
    }

    fn requests(&self) -> usize {
        self.requests.lock().expect("the request log").len()
    }

    /// Lets every stalled and every later refresh answer.
    fn open(&self) {
        self.gate.add_permits(Semaphore::MAX_PERMITS / 2);
    }

    /// Waits, at most five seconds, until the endpoint has read `count` requests.
    async fn read(&self, count: usize) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while self.requests() < count {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("{count} requests read, {} seen", self.requests()));
    }
}

impl Drop for Gated {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Waits, at most five seconds, until `source` has no refresh in flight.
async fn settled(source: &ContainerCredentials) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while source.refreshing() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the refresh settles");
}

/// The endpoint's answer for another key, expiring at `expires`.
fn renewed(expires: &str) -> Value {
    let mut document = answer(expires);
    document["AccessKeyId"] = json!(RENEWED_KEY_ID);
    document
}

fn response(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {length}\r\n\
         connection: close\r\n\r\n{body}",
        length = body.len()
    )
}

fn client() -> HttpClient {
    HttpClient::new(Limits::default()).expect("an HTTP client")
}

fn container(uri: &str, token: &'static str) -> ContainerCredentials {
    ContainerCredentials::new(
        client(),
        uri,
        table(&[(TOKEN_REFERENCE, token)]),
        reference(TOKEN_REFERENCE),
    )
    .expect("an admitted URI")
}

/// The endpoint's answer, expiring at `expires`.
fn answer(expires: &str) -> Value {
    json!({
        "AccessKeyId": ACCESS_KEY_ID,
        "SecretAccessKey": SECRET_ACCESS_KEY,
        "Token": SESSION_TOKEN,
        "Expiration": expires,
        "AccountId": "000000000000",
    })
}

/// A whole second `seconds` from now (before now when negative).
fn in_seconds(seconds: i64) -> SystemTime {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("a clock after 1970")
        .as_secs();
    let now = UNIX_EPOCH + Duration::from_secs(now);
    let offset = Duration::from_secs(seconds.unsigned_abs());
    if seconds >= 0 {
        now + offset
    } else {
        now - offset
    }
}

/// `at` as an RFC 3339 instant in UTC, whole seconds.
fn rfc3339(at: SystemTime) -> String {
    let seconds = at.duration_since(UNIX_EPOCH).expect("after 1970").as_secs();
    let days = i64::try_from(seconds / 86_400).expect("a day count");
    let of_day = seconds % 86_400;
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * from_march + 2) / 5 + 1;
    let month = if from_march < 10 {
        from_march + 3
    } else {
        from_march - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z",
        hour = of_day / 3600,
        minute = of_day % 3600 / 60,
        second = of_day % 60
    )
}

/// The failure carries no secret value and says nothing was sent.
fn assert_quiet(error: &Error) {
    assert_eq!(error.dispatch, Dispatch::NotSent, "{error:?}");
    for secret in SECRETS {
        assert!(!error.message.contains(secret), "{}", error.message);
    }
}

fn assert_unauthorized(error: &Error) {
    assert_eq!(error.code, ErrorCode::Unauthorized, "{error:?}");
    assert!(!error.retriable, "{error:?}");
    assert_quiet(error);
}

#[tokio::test]
async fn the_container_source_sends_the_token_as_authorization_and_reads_the_answer() {
    let expires = in_seconds(3600);
    let endpoint = Endpoint::json("200 OK", &answer(&rfc3339(expires))).await;
    let credentials = endpoint
        .source(AUTHORIZATION_TOKEN)
        .credentials(&Cancel::new())
        .await
        .expect("credentials");

    assert_eq!(credentials.access_key_id(), ACCESS_KEY_ID);
    assert_eq!(credentials.secret_access_key(), SECRET_ACCESS_KEY);
    assert_eq!(credentials.session_token(), Some(SESSION_TOKEN));
    assert_eq!(credentials.expires(), Some(expires));
    let requests = endpoint.requests();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].starts_with("GET /v1/credentials HTTP/1.1\r\n"),
        "{}",
        requests[0]
    );
    assert_eq!(
        header_values(&requests[0], "authorization"),
        [AUTHORIZATION_TOKEN]
    );
}

/// The values of every `name` field in a request head, the name matched in any case.
fn header_values<'a>(head: &'a str, name: &str) -> Vec<&'a str> {
    head.split("\r\n")
        .skip(1)
        .filter_map(|line| line.split_once(':'))
        .filter(|(field, _)| field.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.trim_matches([' ', '\t']))
        .collect()
}

#[tokio::test]
async fn the_token_loses_one_line_end_and_nothing_else() {
    for token in [
        "fixture-authorization-token\n",
        "fixture-authorization-token\r\n",
    ] {
        let endpoint = Endpoint::json("200 OK", &answer(&rfc3339(in_seconds(3600)))).await;
        endpoint
            .source(token)
            .credentials(&Cancel::new())
            .await
            .expect("credentials");
        let requests = endpoint.requests();
        assert_eq!(
            header_values(&requests[0], "authorization"),
            [AUTHORIZATION_TOKEN]
        );
    }
}

#[tokio::test]
async fn credentials_are_kept_until_five_minutes_before_they_expire() {
    for seconds in [3600, 360] {
        let endpoint = Endpoint::json("200 OK", &answer(&rfc3339(in_seconds(seconds)))).await;
        let source = endpoint.source(AUTHORIZATION_TOKEN);
        let cancel = Cancel::new();
        let first = source.credentials(&cancel).await.expect("first");
        let second = source.credentials(&cancel).await.expect("second");
        assert_eq!(second.access_key_id(), first.access_key_id());
        assert_eq!(second.expires(), first.expires());
        assert_eq!(endpoint.requests().len(), 1, "expiring in {seconds} s");
    }
}

#[tokio::test]
async fn credentials_within_five_minutes_of_expiry_are_fetched_again() {
    for seconds in [120, 60] {
        let endpoint = Endpoint::json("200 OK", &answer(&rfc3339(in_seconds(seconds)))).await;
        let source = endpoint.source(AUTHORIZATION_TOKEN);
        let cancel = Cancel::new();
        source.credentials(&cancel).await.expect("first");
        source.credentials(&cancel).await.expect("second");
        settled(&source).await;
        assert_eq!(endpoint.requests().len(), 2, "expiring in {seconds} s");
    }
}

/// Inside the refresh window the kept credentials are answered at once, whatever the refresh is
/// doing: sixteen turns at the same time each get them while the one refresh stalls, and the
/// endpoint sees that one refresh and no other. Once it answers, its credentials are kept.
#[tokio::test]
async fn turns_inside_the_refresh_window_get_the_kept_credentials_while_one_refresh_stalls() {
    let endpoint = Gated::new(
        &answer(&rfc3339(in_seconds(120))),
        &renewed(&rfc3339(in_seconds(3600))),
    )
    .await;
    let source = Arc::new(container(&endpoint.uri, AUTHORIZATION_TOKEN));
    let first = source.credentials(&Cancel::new()).await.expect("first");
    assert_eq!(first.access_key_id(), ACCESS_KEY_ID);

    let turns: Vec<_> = (0..16)
        .map(|_| {
            let source = Arc::clone(&source);
            tokio::spawn(async move {
                tokio::time::timeout(
                    Duration::from_millis(500),
                    source.credentials(&Cancel::new()),
                )
                .await
            })
        })
        .collect();
    for turn in turns {
        let kept = turn
            .await
            .expect("the turn ran")
            .expect("answered without waiting for the refresh")
            .expect("the kept credentials");
        assert_eq!(kept.access_key_id(), ACCESS_KEY_ID);
        assert_eq!(kept.expires(), first.expires());
    }
    endpoint.read(2).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(endpoint.requests(), 2, "one refresh in flight, no other");
    assert!(source.refreshing());

    endpoint.open();
    settled(&source).await;
    let renewed = source.credentials(&Cancel::new()).await.expect("renewed");
    assert_eq!(renewed.access_key_id(), RENEWED_KEY_ID);
    assert_eq!(endpoint.requests(), 2);
}

/// Once the kept credentials have expired, or before there are any, a caller waits for the
/// fetch, and every caller waits for the same one.
#[tokio::test]
async fn without_unexpired_credentials_callers_wait_for_the_one_fetch() {
    let expires = in_seconds(2);
    let endpoint = Gated::new(
        &answer(&rfc3339(expires)),
        &renewed(&rfc3339(in_seconds(3600))),
    )
    .await;
    let source = Arc::new(container(&endpoint.uri, AUTHORIZATION_TOKEN));
    source.credentials(&Cancel::new()).await.expect("first");
    let left = expires
        .duration_since(SystemTime::now())
        .unwrap_or_default();
    tokio::time::sleep(left + Duration::from_millis(100)).await;

    let turns: Vec<_> = (0..8)
        .map(|_| {
            let source = Arc::clone(&source);
            tokio::spawn(async move { source.credentials(&Cancel::new()).await })
        })
        .collect();
    endpoint.read(2).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(turns.iter().all(|turn| !turn.is_finished()), "they wait");
    assert_eq!(endpoint.requests(), 2, "one fetch for every waiting caller");

    endpoint.open();
    for turn in turns {
        let renewed = turn.await.expect("the turn ran").expect("renewed");
        assert_eq!(renewed.access_key_id(), RENEWED_KEY_ID);
    }
    assert_eq!(endpoint.requests(), 2);
}

/// A caller that cancels while it waits for a fetch is answered cancelled at once; the fetch
/// goes on and its credentials are kept for the next caller.
#[tokio::test]
async fn a_caller_that_cancels_its_wait_leaves_the_fetch_to_finish() {
    let expires = in_seconds(2);
    let endpoint = Gated::new(
        &answer(&rfc3339(expires)),
        &renewed(&rfc3339(in_seconds(3600))),
    )
    .await;
    let source = container(&endpoint.uri, AUTHORIZATION_TOKEN);
    source.credentials(&Cancel::new()).await.expect("first");
    let left = expires
        .duration_since(SystemTime::now())
        .unwrap_or_default();
    tokio::time::sleep(left + Duration::from_millis(100)).await;

    let cancel = Cancel::new();
    let waiting = source.credentials(&cancel);
    let canceller = cancel.clone();
    let (error, ()) = tokio::join!(async { waiting.await.expect_err("cancelled") }, async {
        endpoint.read(2).await;
        canceller.cancel();
    });
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert!(source.refreshing());
    endpoint.open();
    settled(&source).await;
    let renewed = source.credentials(&Cancel::new()).await.expect("renewed");
    assert_eq!(renewed.access_key_id(), RENEWED_KEY_ID);
    assert_eq!(endpoint.requests(), 2);
}

/// A refresh inside the five minutes that fails, by a server failure or by a refusal, answers
/// the kept credentials while they have not expired.
#[tokio::test]
async fn a_failed_refresh_answers_the_kept_credentials_until_they_expire() {
    for failure in [
        response("503 Service Unavailable", "{}"),
        response("403 Forbidden", "{}"),
    ] {
        let endpoint = Endpoint::answering_each(vec![
            response("200 OK", &answer(&rfc3339(in_seconds(120))).to_string()),
            failure,
        ])
        .await;
        let source = endpoint.source(AUTHORIZATION_TOKEN);
        let cancel = Cancel::new();
        let first = source.credentials(&cancel).await.expect("first");
        let kept = source
            .credentials(&cancel)
            .await
            .expect("the kept credentials");
        assert_eq!(kept.access_key_id(), first.access_key_id());
        assert_eq!(kept.secret_access_key(), first.secret_access_key());
        assert_eq!(kept.session_token(), first.session_token());
        assert_eq!(kept.expires(), first.expires());
        settled(&source).await;
        assert_eq!(endpoint.requests().len(), 2);
        let again = source.credentials(&cancel).await.expect("still kept");
        assert_eq!(again.access_key_id(), first.access_key_id());
    }
}

/// Once the kept credentials have expired, a failed refresh is answered with its failure.
#[tokio::test]
async fn a_failed_refresh_after_the_kept_credentials_expired_is_the_failure() {
    let expires = in_seconds(2);
    let endpoint = Endpoint::answering_each(vec![
        response("200 OK", &answer(&rfc3339(expires)).to_string()),
        response("503 Service Unavailable", "{}"),
    ])
    .await;
    let source = endpoint.source(AUTHORIZATION_TOKEN);
    let cancel = Cancel::new();
    source.credentials(&cancel).await.expect("first");
    let left = expires
        .duration_since(SystemTime::now())
        .unwrap_or_default();
    tokio::time::sleep(left + Duration::from_millis(100)).await;
    let error = source
        .credentials(&cancel)
        .await
        .expect_err("expired credentials and a failing endpoint");
    assert_eq!(error.code, ErrorCode::Transport, "{error:?}");
    assert_quiet(&error);
    assert_eq!(endpoint.requests().len(), 2);
}

#[test]
fn a_uri_that_is_not_https_or_http_to_an_admitted_address_is_refused() {
    for uri in [
        "http://example.com/v1/credentials",
        "http://localhost/v1/credentials",
        "http://169.254.169.254/latest/meta-data",
        "http://10.0.0.1/v1/credentials",
        "http://[::ffff:127.0.0.1]/v1/credentials",
        "ftp://127.0.0.1/v1/credentials",
        "https://user:fixture-password@example.com/v1/credentials",
        "http://user@127.0.0.1/v1/credentials",
        "https://example.com/v1/credentials#fragment",
        "/v1/credentials",
        "not a URI",
    ] {
        let error =
            ContainerCredentials::new(client(), uri, table(&[]), reference(TOKEN_REFERENCE))
                .expect_err(uri);
        assert_unauthorized(&error);
        for part in ["example", "fixture-password", "169.254.169.254", "10.0.0.1"] {
            assert!(!error.message.contains(part), "{}", error.message);
        }
    }
    for uri in [
        "https://example.com/v1/credentials",
        "http://127.0.0.1:8080/v1/credentials",
        "http://127.1.2.3/v1/credentials",
        "http://[::1]/v1/credentials",
        "http://169.254.170.2/v2/credentials/fixture",
        "http://169.254.170.23/v1/credentials",
        "http://[fd00:ec2::23]/v1/credentials",
    ] {
        ContainerCredentials::new(client(), uri, table(&[]), reference(TOKEN_REFERENCE))
            .unwrap_or_else(|error| panic!("{uri}: {error}"));
    }
}

#[tokio::test]
async fn a_refusing_endpoint_is_unauthorized_and_final() {
    for status in ["403 Forbidden", "401 Unauthorized", "404 Not Found"] {
        let endpoint = Endpoint::json(status, &json!({"message": SECRET_ACCESS_KEY})).await;
        let error = endpoint
            .source(AUTHORIZATION_TOKEN)
            .credentials(&Cancel::new())
            .await
            .expect_err(status);
        assert_unauthorized(&error);
    }
}

#[tokio::test]
async fn a_failing_or_unreachable_endpoint_is_transport_and_retriable() {
    let endpoint = Endpoint::json("503 Service Unavailable", &json!({})).await;
    let error = endpoint
        .source(AUTHORIZATION_TOKEN)
        .credentials(&Cancel::new())
        .await
        .expect_err("a 503");
    assert_eq!(error.code, ErrorCode::Transport, "{error:?}");
    assert!(error.retriable);
    assert_quiet(&error);

    let closed = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local port");
    let address = closed.local_addr().expect("its address");
    drop(closed);
    let error = container(
        &format!("http://{address}/v1/credentials"),
        AUTHORIZATION_TOKEN,
    )
    .credentials(&Cancel::new())
    .await
    .expect_err("nothing listens");
    assert_eq!(error.code, ErrorCode::Transport, "{error:?}");
    assert!(error.retriable);
    assert_quiet(&error);
}

/// The transport's own class and retry hint reach the caller: a failing endpoint's
/// `Retry-After`, and `deadline` for an endpoint that does not answer in time.
#[tokio::test]
async fn an_endpoint_failure_keeps_its_retry_after_and_its_deadline_class() {
    let endpoint = Endpoint::answering(
        "HTTP/1.1 503 Service Unavailable\r\nretry-after: 7\r\ncontent-length: 0\r\n\
         connection: close\r\n\r\n"
            .to_owned(),
    )
    .await;
    let error = endpoint
        .source(AUTHORIZATION_TOKEN)
        .credentials(&Cancel::new())
        .await
        .expect_err("a 503");
    assert_eq!(
        (error.code, error.retry_after_ms, error.retriable),
        (ErrorCode::Transport, Some(7000), true),
        "{error:?}"
    );
    assert_quiet(&error);

    let silent = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local port");
    let address = silent.local_addr().expect("its address");
    let holder = tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((stream, _)) = silent.accept().await {
            held.push(stream);
        }
    });
    let late = ContainerCredentials::new(
        HttpClient::new(Limits {
            response_headers: Duration::from_millis(200),
            idle: Duration::from_secs(1),
            total: Duration::from_secs(2),
        })
        .expect("an HTTP client"),
        &format!("http://{address}/v1/credentials"),
        table(&[(TOKEN_REFERENCE, AUTHORIZATION_TOKEN)]),
        reference(TOKEN_REFERENCE),
    )
    .expect("an admitted URI");
    let error = late
        .credentials(&Cancel::new())
        .await
        .expect_err("no answer in time");
    holder.abort();
    assert_eq!(
        (error.code, error.retriable),
        (ErrorCode::Deadline, true),
        "{error:?}"
    );
    assert_quiet(&error);
}

/// The answer is decoded once into its members: one named twice is refused rather than one of
/// its values chosen, and a JSON array is no answer, whatever it holds.
#[tokio::test]
async fn an_answer_naming_a_member_twice_or_not_an_object_is_unauthorized() {
    let expires = rfc3339(in_seconds(3600));
    let mut documents = Vec::new();
    for twice in ["SecretAccessKey", "Token"] {
        documents.push(format!(
            r#"{{"AccessKeyId":"{ACCESS_KEY_ID}","{twice}":"fixture-other","SecretAccessKey":"{SECRET_ACCESS_KEY}","Token":"{SESSION_TOKEN}","Expiration":"{expires}"}}"#
        ));
    }
    documents.push(json!([ACCESS_KEY_ID, SECRET_ACCESS_KEY, SESSION_TOKEN, expires]).to_string());
    for document in &documents {
        let endpoint = Endpoint::answering(response("200 OK", document)).await;
        let error = endpoint
            .source(AUTHORIZATION_TOKEN)
            .credentials(&Cancel::new())
            .await
            .expect_err("an ambiguous answer");
        assert_unauthorized(&error);
    }
}

#[tokio::test]
async fn an_answer_that_is_not_json_is_unauthorized() {
    let endpoint = Endpoint::answering(response("200 OK", SECRET_ACCESS_KEY)).await;
    let error = endpoint
        .source(AUTHORIZATION_TOKEN)
        .credentials(&Cancel::new())
        .await
        .expect_err("not JSON");
    assert_unauthorized(&error);
}

#[tokio::test]
async fn an_answer_without_four_usable_fields_is_unauthorized() {
    let expires = rfc3339(in_seconds(3600));
    let mut answers = Vec::new();
    for field in ["AccessKeyId", "SecretAccessKey", "Token", "Expiration"] {
        let mut missing = answer(&expires);
        missing.as_object_mut().expect("an object").remove(field);
        answers.push(missing);
        let mut empty = answer(&expires);
        empty[field] = json!("");
        answers.push(empty);
        let mut number = answer(&expires);
        number[field] = json!(7);
        answers.push(number);
    }
    answers.push(answer("tomorrow"));
    answers.push(answer("2015-08-30T12:36:00+02:00"));
    answers.push(answer(&rfc3339(in_seconds(-60))));
    answers.push(json!([answer(&expires)]));
    for document in &answers {
        let endpoint = Endpoint::json("200 OK", document).await;
        let error = endpoint
            .source(AUTHORIZATION_TOKEN)
            .credentials(&Cancel::new())
            .await
            .expect_err("an unusable answer");
        assert_unauthorized(&error);
    }
}

#[tokio::test]
async fn a_token_that_cannot_be_sent_stops_before_the_endpoint() {
    let endpoint = Endpoint::json("200 OK", &answer(&rfc3339(in_seconds(3600)))).await;
    for token in [
        "",
        "\n",
        "fixture\r\ntoken",
        "fixture\ttoken",
        "fixture-token\n\n",
    ] {
        let error = endpoint
            .source(token)
            .credentials(&Cancel::new())
            .await
            .expect_err("an unusable token");
        assert_unauthorized(&error);
        assert!(error.message.contains(TOKEN_REFERENCE), "{}", error.message);
    }
    let unresolved = ContainerCredentials::new(
        client(),
        &endpoint.uri,
        table(&[]),
        reference(TOKEN_REFERENCE),
    )
    .expect("an admitted URI");
    let error = unresolved
        .credentials(&Cancel::new())
        .await
        .expect_err("no token");
    assert_unauthorized(&error);
    assert_eq!(endpoint.requests(), [] as [String; 0]);
}

#[tokio::test]
async fn a_cancelled_caller_is_answered_cancelled_without_a_request() {
    let endpoint = Endpoint::json("200 OK", &answer(&rfc3339(in_seconds(3600)))).await;
    let cancel = Cancel::new();
    cancel.cancel();
    let error = endpoint
        .source(AUTHORIZATION_TOKEN)
        .credentials(&cancel)
        .await
        .expect_err("cancelled");
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert_quiet(&error);
    assert_eq!(endpoint.requests(), [] as [String; 0]);
}

fn static_source(entries: &[(&'static str, &'static str)], token: bool) -> StaticCredentials {
    StaticCredentials::new(
        table(entries),
        reference("aws-access-key-id"),
        reference("aws-secret-access-key"),
        token.then(|| reference("aws-session-token")),
    )
}

#[tokio::test]
async fn static_keys_are_resolved_without_one_line_end() {
    let keys = [
        ("aws-access-key-id", "ASIAFIXTUREKEYID\n"),
        ("aws-secret-access-key", "fixture/secret+access-key\r\n"),
        ("aws-session-token", "fixture-session-token\n"),
    ];
    let cancel = Cancel::new();
    let credentials = static_source(&keys, true)
        .credentials(&cancel)
        .await
        .expect("credentials");
    assert_eq!(credentials.access_key_id(), ACCESS_KEY_ID);
    assert_eq!(credentials.secret_access_key(), SECRET_ACCESS_KEY);
    assert_eq!(credentials.session_token(), Some(SESSION_TOKEN));
    assert_eq!(credentials.expires(), None);

    let without = static_source(&keys, false)
        .credentials(&cancel)
        .await
        .expect("credentials without a token");
    assert_eq!(without.session_token(), None);
    let unset = static_source(&keys[..2], true)
        .credentials(&cancel)
        .await
        .expect("a missing session token is none");
    assert_eq!(unset.session_token(), None);
}

#[tokio::test]
async fn unusable_static_keys_are_unauthorized_and_name_their_reference() {
    let cancel = Cancel::new();
    for value in [
        "",
        "\n",
        "fixture key",
        "fixture\nkey",
        "fixture-key\n\n",
        "fixture\u{e9}",
    ] {
        let keys = [
            ("aws-access-key-id", ACCESS_KEY_ID),
            ("aws-secret-access-key", value),
        ];
        let error = static_source(&keys, false)
            .credentials(&cancel)
            .await
            .expect_err("an unusable secret access key");
        assert_unauthorized(&error);
        assert!(
            error.message.contains("aws-secret-access-key"),
            "{}",
            error.message
        );
        assert!(!error.message.contains("fixture"), "{}", error.message);
    }
    let error = static_source(&[("aws-secret-access-key", SECRET_ACCESS_KEY)], false)
        .credentials(&cancel)
        .await
        .expect_err("no access key id");
    assert_unauthorized(&error);
    assert!(
        error.message.contains("aws-access-key-id"),
        "{}",
        error.message
    );

    cancel.cancel();
    let error = static_source(&[], false)
        .credentials(&cancel)
        .await
        .expect_err("cancelled");
    assert_eq!(error.code, ErrorCode::Cancelled);
}

#[tokio::test]
async fn debug_output_shows_no_secret() {
    let credentials = AwsCredentials::new(
        ACCESS_KEY_ID,
        SECRET_ACCESS_KEY,
        Some(SESSION_TOKEN.to_owned()),
        None,
    );
    let endpoint = Endpoint::json("200 OK", &answer(&rfc3339(in_seconds(3600)))).await;
    let container = endpoint.source(AUTHORIZATION_TOKEN);
    container
        .credentials(&Cancel::new())
        .await
        .expect("credentials, now cached");
    let keys = [
        ("aws-access-key-id", ACCESS_KEY_ID),
        ("aws-secret-access-key", SECRET_ACCESS_KEY),
    ];
    let outputs = [
        format!("{credentials:?}"),
        format!("{container:?}"),
        format!("{:?}", static_source(&keys, true)),
    ];
    for output in &outputs {
        for secret in SECRETS {
            assert!(!output.contains(secret), "{output}");
        }
        assert!(!output.contains("/v1/credentials"), "{output}");
    }
    assert!(outputs[0].contains("[REDACTED]"), "{}", outputs[0]);
    assert!(
        outputs[2].contains("aws-secret-access-key"),
        "{}",
        outputs[2]
    );
}
