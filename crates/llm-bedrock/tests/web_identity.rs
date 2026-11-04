//! `WebIdentityCredentials`, the source behind IAM roles for service accounts, against a local
//! fixture of the AWS Security Token Service `AssumeRoleWithWebIdentity` query API.

use llm_bedrock::{CredentialSource, WebIdentityCredentials};
use llm_core::{BoxFuture, Cancel, Dispatch, Error, ErrorCode};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_http::{HttpClient, Limits};
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

const ROLE_ARN: &str = "arn:aws:iam::000000000000:role/llm-fixture-role";
const SESSION_NAME: &str = "llm-fixture-session";
const TOKEN_REFERENCE: &str = "web-identity-token";
const WEB_IDENTITY_TOKEN: &str = "llm-fixture-web-identity-token.fixture-claims.fixture-signature";
const ROTATED_TOKEN: &str = "llm-fixture-web-identity-token.rotated-claims.rotated-signature";
const ACCESS_KEY_ID: &str = "ASIAWEBIDENTITYFIXTURE";
const RENEWED_KEY_ID: &str = "ASIAWEBIDENTITYRENEWED";
const SECRET_ACCESS_KEY: &str = "fixture/web+identity-secret-key";
const SESSION_TOKEN: &str = "fixture-web-identity-session-token";
/// A marker only an STS error message carries.
const STS_MESSAGE: &str = "llm-fixture-sts-message-private";

/// Every secret value or answer text a case uses: none may reach a message or a `Debug` output.
const SECRETS: [&str; 7] = [
    WEB_IDENTITY_TOKEN,
    ROTATED_TOKEN,
    ACCESS_KEY_ID,
    RENEWED_KEY_ID,
    SECRET_ACCESS_KEY,
    SESSION_TOKEN,
    STS_MESSAGE,
];

/// The token file as the resolver reads it: its current content, or missing.
#[derive(Clone)]
struct TokenFile(Arc<Mutex<Option<String>>>);

impl TokenFile {
    fn holding(token: &str) -> Self {
        Self(Arc::new(Mutex::new(Some(token.to_owned()))))
    }

    fn missing() -> Self {
        Self(Arc::new(Mutex::new(None)))
    }

    fn write(&self, token: &str) {
        *self.0.lock().expect("the token file") = Some(token.to_owned());
    }
}

impl SecretResolver for TokenFile {
    fn resolve<'a>(
        &'a self,
        reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async move {
            let content = self.0.lock().expect("the token file").clone();
            let content = content
                .filter(|_| reference.as_str() == TOKEN_REFERENCE)
                .ok_or(SecretError::Missing)?;
            Ok(ResolvedSecret {
                secret: Secret::new(content.into_bytes())?,
                version: SecretVersion::new("1".to_owned())?,
            })
        })
    }
}

fn reference() -> SecretRef {
    SecretRef::new(TOKEN_REFERENCE).expect("a valid reference")
}

fn client() -> HttpClient {
    HttpClient::new(Limits::default()).expect("an HTTP client")
}

fn source(uri: &str, file: &TokenFile) -> WebIdentityCredentials {
    WebIdentityCredentials::new(
        client(),
        uri,
        ROLE_ARN,
        SESSION_NAME,
        Arc::new(file.clone()),
        reference(),
    )
    .expect("an admitted endpoint")
}

/// One request as the fixture read it.
#[derive(Clone, Debug)]
struct Captured {
    head: String,
    body: String,
}

impl Captured {
    fn header(&self, name: &str) -> Vec<&str> {
        self.head
            .split("\r\n")
            .skip(1)
            .filter_map(|line| line.split_once(':'))
            .filter(|(field, _)| field.trim().eq_ignore_ascii_case(name))
            .map(|(_, value)| value.trim())
            .collect()
    }
}

async fn read_request(stream: &mut TcpStream) -> Option<Captured> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    let end = loop {
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break end;
        }
        match stream.read(&mut buffer).await {
            Ok(0) | Err(_) => return None,
            Ok(count) => bytes.extend_from_slice(&buffer[..count]),
        }
    };
    let head = String::from_utf8_lossy(&bytes[..end]).into_owned();
    let length: usize = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().ok())?
        })
        .unwrap_or(0);
    let mut body = bytes[end + 4..].to_vec();
    while body.len() < length {
        match stream.read(&mut buffer).await {
            Ok(0) | Err(_) => return None,
            Ok(count) => body.extend_from_slice(&buffer[..count]),
        }
    }
    Some(Captured {
        head,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

/// A local STS: it answers each connection with the next of its canned responses, the last one
/// repeated, and keeps each request it read.
struct Sts {
    uri: String,
    requests: Arc<Mutex<Vec<Captured>>>,
    task: JoinHandle<()>,
}

impl Sts {
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
                if let Some(request) = read_request(&mut stream).await {
                    seen.lock().expect("the request log").push(request);
                }
                let response = &responses[answered.min(responses.len() - 1)];
                answered += 1;
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });
        Self {
            uri: format!("http://{address}/"),
            requests,
            task,
        }
    }

    fn requests(&self) -> Vec<Captured> {
        self.requests.lock().expect("the request log").clone()
    }
}

impl Drop for Sts {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn response(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\ncontent-type: text/xml\r\ncontent-length: {length}\r\n\
         connection: close\r\n\r\n{body}",
        length = body.len()
    )
}

/// An `AssumeRoleWithWebIdentity` answer whose `Credentials` hold `members` as written.
fn answer_with(members: &[(&str, &str)]) -> String {
    let credentials = members
        .iter()
        .fold(String::new(), |mut credentials, (name, value)| {
            let _ = write!(credentials, "<{name}>{value}</{name}>");
            credentials
        });
    format!(
        r#"<AssumeRoleWithWebIdentityResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/">
  <AssumeRoleWithWebIdentityResult>
    <SubjectFromWebIdentityToken>system:serviceaccount:default:llm</SubjectFromWebIdentityToken>
    <Audience>sts.amazonaws.com</Audience>
    <AssumedRoleUser>
      <Arn>arn:aws:sts::000000000000:assumed-role/llm-fixture-role/llm-fixture-session</Arn>
      <AssumedRoleId>AROAFIXTURE:llm-fixture-session</AssumedRoleId>
    </AssumedRoleUser>
    <Credentials>{credentials}</Credentials>
    <Provider>arn:aws:iam::000000000000:oidc-provider/oidc.fixture.example</Provider>
  </AssumeRoleWithWebIdentityResult>
  <ResponseMetadata>
    <RequestId>00000000-0000-0000-0000-000000000000</RequestId>
  </ResponseMetadata>
</AssumeRoleWithWebIdentityResponse>
"#
    )
}

/// The four members in the order STS writes them.
fn members(key_id: &'static str, expires: &str) -> Vec<(&'static str, String)> {
    vec![
        ("SessionToken", SESSION_TOKEN.to_owned()),
        ("SecretAccessKey", SECRET_ACCESS_KEY.to_owned()),
        ("Expiration", expires.to_owned()),
        ("AccessKeyId", key_id.to_owned()),
    ]
}

fn answer(key_id: &'static str, expires: &str) -> String {
    let members = members(key_id, expires);
    let borrowed: Vec<(&str, &str)> = members
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    answer_with(&borrowed)
}

fn success(expires: SystemTime) -> String {
    response("200 OK", &answer(ACCESS_KEY_ID, &rfc3339(expires)))
}

fn sts_error(status: &str, code: &str) -> String {
    response(
        status,
        &format!(
            r#"<ErrorResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/">
  <Error>
    <Type>Sender</Type>
    <Code>{code}</Code>
    <Message>{STS_MESSAGE} {WEB_IDENTITY_TOKEN}</Message>
  </Error>
  <RequestId>00000000-0000-0000-0000-000000000000</RequestId>
</ErrorResponse>
"#
        ),
    )
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

/// Waits, at most five seconds, until `source` has no refresh in flight.
async fn settled(source: &WebIdentityCredentials) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while source.refreshing() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the refresh settles");
}

/// The failure carries no secret value or answer text, in its message or its debug form, and
/// says nothing was sent.
fn assert_quiet(error: &Error) {
    assert_eq!(error.dispatch, Dispatch::NotSent, "{error:?}");
    let debug = format!("{error:?}");
    for secret in SECRETS {
        assert!(!error.message.contains(secret), "{}", error.message);
        assert!(!debug.contains(secret), "{debug}");
    }
}

fn assert_unauthorized(error: &Error) {
    assert_eq!(error.code, ErrorCode::Unauthorized, "{error:?}");
    assert!(!error.retriable, "{error:?}");
    assert!(!error.may_retry(), "{error:?}");
    assert_quiet(error);
}

fn assert_retriable(error: &Error) {
    assert_eq!(error.code, ErrorCode::Transport, "{error:?}");
    assert!(error.may_retry(), "{error:?}");
    assert_quiet(error);
}

/// The form `AssumeRoleWithWebIdentity` is asked with, each value percent-encoded.
fn expected_form(token: &str) -> String {
    format!(
        "Action=AssumeRoleWithWebIdentity&Version=2011-06-15\
         &RoleArn=arn%3Aaws%3Aiam%3A%3A000000000000%3Arole%2Fllm-fixture-role\
         &RoleSessionName=llm-fixture-session&WebIdentityToken={token}"
    )
}

/// One POST of the five query parameters as a form, unsigned: no `authorization`, no
/// `x-amz-*` header, and the token in the body only, never in the URL.
#[tokio::test]
async fn the_source_posts_the_token_as_an_unsigned_form_and_reads_the_answer() {
    let expires = in_seconds(3600);
    let sts = Sts::answering(success(expires)).await;
    let credentials = source(&sts.uri, &TokenFile::holding(WEB_IDENTITY_TOKEN))
        .credentials(&Cancel::new())
        .await
        .expect("credentials");

    assert_eq!(credentials.access_key_id(), ACCESS_KEY_ID);
    assert_eq!(credentials.secret_access_key(), SECRET_ACCESS_KEY);
    assert_eq!(credentials.session_token(), Some(SESSION_TOKEN));
    assert_eq!(credentials.expires(), Some(expires));
    let requests = sts.requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert!(
        request.head.starts_with("POST / HTTP/1.1\r\n"),
        "{}",
        request.head
    );
    assert_eq!(
        request.header("content-type"),
        ["application/x-www-form-urlencoded; charset=utf-8"]
    );
    assert_eq!(request.body, expected_form(WEB_IDENTITY_TOKEN));
    assert!(
        request.header("authorization").is_empty(),
        "{}",
        request.head
    );
    assert!(
        !request
            .head
            .to_ascii_lowercase()
            .split("\r\n")
            .any(|line| line.starts_with("x-amz-")),
        "{}",
        request.head
    );
    assert!(
        !request.head.contains(WEB_IDENTITY_TOKEN),
        "{}",
        request.head
    );
}

/// The kubelet rewrites the token file before the token expires, so each fetch reads it again:
/// a refresh sends what the file holds then. One trailing line end is removed.
#[tokio::test]
async fn the_token_file_is_read_again_on_every_fetch() {
    let sts = Sts::answering_each(vec![
        success(in_seconds(120)),
        response(
            "200 OK",
            &answer(RENEWED_KEY_ID, &rfc3339(in_seconds(3600))),
        ),
    ])
    .await;
    let file = TokenFile::holding(&format!("{WEB_IDENTITY_TOKEN}\n"));
    let source = source(&sts.uri, &file);
    let cancel = Cancel::new();
    let first = source.credentials(&cancel).await.expect("first");
    assert_eq!(first.access_key_id(), ACCESS_KEY_ID);
    file.write(&format!("{ROTATED_TOKEN}\r\n"));
    let kept = source
        .credentials(&cancel)
        .await
        .expect("inside the window");
    assert_eq!(kept.access_key_id(), ACCESS_KEY_ID);
    settled(&source).await;
    let renewed = source.credentials(&cancel).await.expect("renewed");
    assert_eq!(renewed.access_key_id(), RENEWED_KEY_ID);
    let bodies: Vec<String> = sts.requests().into_iter().map(|r| r.body).collect();
    assert_eq!(
        bodies,
        [
            expected_form(WEB_IDENTITY_TOKEN),
            expected_form(ROTATED_TOKEN)
        ]
    );
}

/// The single-flight refresh `ContainerCredentials` uses: kept until five minutes before they
/// expire, and callers with nothing kept all wait for one request.
#[tokio::test]
async fn credentials_are_kept_and_concurrent_callers_share_one_request() {
    let sts = Sts::answering(success(in_seconds(3600))).await;
    let source = source(&sts.uri, &TokenFile::holding(WEB_IDENTITY_TOKEN));
    let cancel = Cancel::new();
    let (a, b, c) = tokio::join!(
        source.credentials(&cancel),
        source.credentials(&cancel),
        source.credentials(&cancel)
    );
    for credentials in [a, b, c] {
        assert_eq!(
            credentials.expect("credentials").access_key_id(),
            ACCESS_KEY_ID
        );
    }
    source.credentials(&cancel).await.expect("kept");
    assert_eq!(sts.requests().len(), 1);
}

#[test]
fn an_endpoint_that_is_not_https_or_http_to_a_loopback_address_is_refused() {
    for uri in [
        "http://sts.eu-central-1.amazonaws.com/",
        "http://localhost/",
        "http://169.254.170.23/",
        "http://169.254.169.254/",
        "http://10.0.0.1/",
        "http://[::ffff:127.0.0.1]/",
        "ftp://127.0.0.1/",
        "https://user:fixture-password@sts.example.com/",
        "http://user@127.0.0.1/",
        "https://sts.example.com/#fragment",
        "https://sts.example.com/?Action=AssumeRoleWithWebIdentity",
        "/",
        "not a URI",
    ] {
        let error = WebIdentityCredentials::new(
            client(),
            uri,
            ROLE_ARN,
            SESSION_NAME,
            Arc::new(TokenFile::holding(WEB_IDENTITY_TOKEN)),
            reference(),
        )
        .expect_err(uri);
        assert_unauthorized(&error);
        for part in [
            "amazonaws",
            "example",
            "fixture-password",
            "169.254",
            "10.0.0.1",
            "Action",
        ] {
            assert!(!error.message.contains(part), "{}", error.message);
        }
    }
    for uri in [
        "https://sts.eu-central-1.amazonaws.com",
        "https://sts.eu-central-1.amazonaws.com/",
        "http://127.0.0.1:8080/",
        "http://127.1.2.3/",
        "http://[::1]/",
    ] {
        WebIdentityCredentials::new(
            client(),
            uri,
            ROLE_ARN,
            SESSION_NAME,
            Arc::new(TokenFile::holding(WEB_IDENTITY_TOKEN)),
            reference(),
        )
        .unwrap_or_else(|error| panic!("{uri}: {error}"));
    }
}

/// A role ARN or session name STS would refuse is refused when the source is built, by its
/// field and without quoting it.
#[test]
fn a_role_or_session_name_sts_would_refuse_is_refused_when_the_source_is_built() {
    let long_session = "s".repeat(65);
    let long_role = format!("arn:aws:iam::000000000000:role/{}", "r".repeat(2048));
    for (role, session) in [
        ("", SESSION_NAME),
        ("role/llm-fixture-role", SESSION_NAME),
        ("arn:aws:iam::000000000000:role/with space", SESSION_NAME),
        (long_role.as_str(), SESSION_NAME),
        (ROLE_ARN, ""),
        (ROLE_ARN, "s"),
        (ROLE_ARN, long_session.as_str()),
        (ROLE_ARN, "llm/fixture"),
        (ROLE_ARN, "llm fixture"),
    ] {
        let error = WebIdentityCredentials::new(
            client(),
            "https://sts.eu-central-1.amazonaws.com",
            role,
            session,
            Arc::new(TokenFile::holding(WEB_IDENTITY_TOKEN)),
            reference(),
        )
        .expect_err("refused");
        assert_unauthorized(&error);
        assert!(
            !error.message.contains("llm-fixture-role"),
            "{}",
            error.message
        );
    }
    for session in ["ab", "llm_fixture+session=1,2.3@x-y", &"s".repeat(64)] {
        WebIdentityCredentials::new(
            client(),
            "https://sts.eu-central-1.amazonaws.com",
            ROLE_ARN,
            session,
            Arc::new(TokenFile::holding(WEB_IDENTITY_TOKEN)),
            reference(),
        )
        .unwrap_or_else(|error| panic!("{session}: {error}"));
    }
}

/// STS's refusals of the token and every other 4xx are `unauthorized` and final; the message
/// names a code STS documents and never the message STS wrote.
#[tokio::test]
async fn each_refusal_of_the_token_is_unauthorized_and_final() {
    for (status, code, named) in [
        ("400 Bad Request", "ExpiredToken", true),
        ("400 Bad Request", "InvalidIdentityToken", true),
        ("400 Bad Request", "IDPRejectedClaim", true),
        ("403 Forbidden", "AccessDenied", true),
        ("400 Bad Request", "SomethingNotDocumented", false),
        ("404 Not Found", "NotFound", false),
        ("429 Too Many Requests", "TooMany", false),
    ] {
        let sts = Sts::answering(sts_error(status, code)).await;
        let error = source(&sts.uri, &TokenFile::holding(WEB_IDENTITY_TOKEN))
            .credentials(&Cancel::new())
            .await
            .expect_err(code);
        assert_unauthorized(&error);
        assert_eq!(
            error.message.contains(code),
            named,
            "{code}: {}",
            error.message
        );
        assert_eq!(sts.requests().len(), 1, "{code}");
    }
    let sts = Sts::answering(response("401 Unauthorized", "")).await;
    let error = source(&sts.uri, &TokenFile::holding(WEB_IDENTITY_TOKEN))
        .credentials(&Cancel::new())
        .await
        .expect_err("a 401 without a body");
    assert_unauthorized(&error);
}

/// No answer, a cut answer, a 408 or 5xx, and `IDPCommunicationError` are `transport`,
/// retriable and `not-sent`; a 5xx keeps its `Retry-After`.
#[tokio::test]
async fn an_unreachable_idp_a_server_failure_or_no_answer_is_retriable() {
    let sts = Sts::answering(sts_error("400 Bad Request", "IDPCommunicationError")).await;
    let error = source(&sts.uri, &TokenFile::holding(WEB_IDENTITY_TOKEN))
        .credentials(&Cancel::new())
        .await
        .expect_err("the IdP did not answer STS");
    assert_retriable(&error);
    assert!(
        error.message.contains("IDPCommunicationError"),
        "{}",
        error.message
    );

    for status in [
        "500 Internal Server Error",
        "503 Service Unavailable",
        "408 Request Timeout",
    ] {
        let sts = Sts::answering(format!(
            "HTTP/1.1 {status}\r\nretry-after: 7\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
        ))
        .await;
        let error = source(&sts.uri, &TokenFile::holding(WEB_IDENTITY_TOKEN))
            .credentials(&Cancel::new())
            .await
            .expect_err(status);
        assert_retriable(&error);
        assert_eq!(error.retry_after_ms, Some(7000), "{status}");
    }

    let cut = Sts::answering(
        "HTTP/1.1 200 OK\r\ncontent-type: text/xml\r\ncontent-length: 4096\r\n\r\n\
         <AssumeRoleWithWebIdentityResponse>"
            .to_owned(),
    )
    .await;
    let error = source(&cut.uri, &TokenFile::holding(WEB_IDENTITY_TOKEN))
        .credentials(&Cancel::new())
        .await
        .expect_err("a cut answer");
    assert_retriable(&error);

    let closed = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local port");
    let address = closed.local_addr().expect("its address");
    drop(closed);
    let error = source(
        &format!("http://{address}/"),
        &TokenFile::holding(WEB_IDENTITY_TOKEN),
    )
    .credentials(&Cancel::new())
    .await
    .expect_err("nothing listens");
    assert_retriable(&error);
}

/// A redirect is refused as a final `unauthorized`, and the redirect target never hears from
/// the source: the token reaches the configured endpoint only.
#[tokio::test]
async fn a_redirect_is_unauthorized_and_its_target_never_receives_the_token() {
    let target = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local port");
    let location = format!("http://{}/", target.local_addr().expect("its address"));
    let sts = Sts::answering(format!(
        "HTTP/1.1 307 Temporary Redirect\r\nlocation: {location}\r\ncontent-length: 0\r\n\
         connection: close\r\n\r\n"
    ))
    .await;
    let error = source(&sts.uri, &TokenFile::holding(WEB_IDENTITY_TOKEN))
        .credentials(&Cancel::new())
        .await
        .expect_err("a redirect");
    assert_unauthorized(&error);
    assert!(!error.message.contains(&location), "{}", error.message);
    assert!(
        tokio::time::timeout(Duration::from_millis(200), target.accept())
            .await
            .is_err(),
        "the redirect target was contacted"
    );
}

/// The answer is read once into its four members: one missing, empty, named twice or holding
/// an element, two `Credentials`, an answer that is not the document or not well-formed, a
/// declaration or reference the reader does not take, and an expired answer are refused.
#[tokio::test]
async fn an_unusable_answer_is_unauthorized() {
    let expires = rfc3339(in_seconds(3600));
    let all = members(ACCESS_KEY_ID, &expires);
    let as_pairs =
        |list: &[(&'static str, String)]| -> Vec<(&'static str, String)> { list.to_vec() };
    let render = |list: &[(&'static str, String)]| {
        let borrowed: Vec<(&str, &str)> = list.iter().map(|(n, v)| (*n, v.as_str())).collect();
        answer_with(&borrowed)
    };
    let mut bodies = Vec::new();
    for (index, (name, _)) in all.iter().enumerate() {
        let mut missing = as_pairs(&all);
        missing.remove(index);
        bodies.push(render(&missing));
        let mut empty = as_pairs(&all);
        empty[index].1 = String::new();
        bodies.push(render(&empty));
        let mut twice = as_pairs(&all);
        twice.push((*name, "fixture-other".to_owned()));
        bodies.push(render(&twice));
        let mut nested = as_pairs(&all);
        nested[index].1 = format!("<Value>{}</Value>", all[index].1);
        bodies.push(render(&nested));
    }
    let whole = answer(ACCESS_KEY_ID, &expires);
    bodies.push(whole.replace(
        "</Credentials>",
        &format!(
            "</Credentials><Credentials><AccessKeyId>{RENEWED_KEY_ID}</AccessKeyId></Credentials>"
        ),
    ));
    bodies.push(whole.replace("AssumeRoleWithWebIdentityResponse", "AssumeRoleResponse"));
    bodies.push(format!(
        r#"{{"AccessKeyId":"{ACCESS_KEY_ID}","SecretAccessKey":"{SECRET_ACCESS_KEY}","SessionToken":"{SESSION_TOKEN}","Expiration":"{expires}"}}"#
    ));
    bodies.push(format!("{whole}<Trailing/>"));
    bodies.push(whole.replace("</AssumeRoleWithWebIdentityResult>", ""));
    bodies.push(format!(
        "<!DOCTYPE AssumeRoleWithWebIdentityResponse [<!ENTITY k \"{SECRET_ACCESS_KEY}\">]>{}",
        whole.replace(SECRET_ACCESS_KEY, "&k;")
    ));
    bodies.push(whole.replace(SECRET_ACCESS_KEY, "fixture&unknown;secret"));
    bodies.push(whole.replace(SECRET_ACCESS_KEY, "fixture&#0;secret"));
    bodies.push(whole.replace("<Audience>", "<!-- a comment --><Audience>"));
    bodies.push(whole.replace(SECRET_ACCESS_KEY, &"k".repeat(16 * 1024 + 1)));
    bodies.push(whole.replace(SECRET_ACCESS_KEY, "fixture secret"));
    bodies.push(answer(ACCESS_KEY_ID, "tomorrow"));
    bodies.push(answer(ACCESS_KEY_ID, "2015-08-30T12:36:00+02:00"));
    bodies.push(answer(ACCESS_KEY_ID, &rfc3339(in_seconds(-60))));
    bodies.push(String::new());
    for body in &bodies {
        let sts = Sts::answering(response("200 OK", body)).await;
        let error = source(&sts.uri, &TokenFile::holding(WEB_IDENTITY_TOKEN))
            .credentials(&Cancel::new())
            .await
            .expect_err("an unusable answer");
        assert_unauthorized(&error);
    }
}

/// The reader takes what STS writes around the four members: a declaration, a namespace,
/// members in any order, and character references inside a value.
#[tokio::test]
async fn an_answer_with_a_declaration_and_character_references_is_read() {
    let expires = in_seconds(3600);
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{}",
        answer_with(&[
            ("AccessKeyId", ACCESS_KEY_ID),
            ("Expiration", &rfc3339(expires)),
            ("SecretAccessKey", "fixture/web&#x2B;identity-secret-key"),
            (
                "SessionToken",
                "fixture-web-identity&#45;session&#x2d;token"
            ),
        ])
    );
    let sts = Sts::answering(response("200 OK", &body)).await;
    let credentials = source(&sts.uri, &TokenFile::holding(WEB_IDENTITY_TOKEN))
        .credentials(&Cancel::new())
        .await
        .expect("credentials");
    assert_eq!(credentials.secret_access_key(), SECRET_ACCESS_KEY);
    assert_eq!(credentials.session_token(), Some(SESSION_TOKEN));
    assert_eq!(credentials.expires(), Some(expires));

    let escaped = answer_with(&[
        ("AccessKeyId", ACCESS_KEY_ID),
        ("Expiration", &rfc3339(expires)),
        ("SecretAccessKey", "a&amp;b&lt;c&gt;d&quot;e&apos;f"),
        ("SessionToken", SESSION_TOKEN),
    ]);
    let sts = Sts::answering(response("200 OK", &escaped)).await;
    let credentials = source(&sts.uri, &TokenFile::holding(WEB_IDENTITY_TOKEN))
        .credentials(&Cancel::new())
        .await
        .expect("credentials");
    assert_eq!(credentials.secret_access_key(), "a&b<c>d\"e'f");
}

#[tokio::test]
async fn a_token_that_does_not_resolve_or_cannot_be_sent_stops_before_sts() {
    let sts = Sts::answering(success(in_seconds(3600))).await;
    let too_long = "t".repeat(20_001);
    for token in [
        "",
        "\n",
        "fixture token",
        "fixture\ttoken",
        "fixture-token\n\n",
        "fixture-tökén",
        too_long.as_str(),
    ] {
        let error = source(&sts.uri, &TokenFile::holding(token))
            .credentials(&Cancel::new())
            .await
            .expect_err("an unusable token");
        assert_unauthorized(&error);
        assert!(error.message.contains(TOKEN_REFERENCE), "{}", error.message);
        assert!(!error.message.contains("fixture"), "{}", error.message);
    }
    let error = source(&sts.uri, &TokenFile::missing())
        .credentials(&Cancel::new())
        .await
        .expect_err("no token file");
    assert_unauthorized(&error);
    assert!(error.message.contains(TOKEN_REFERENCE), "{}", error.message);
    assert!(sts.requests().is_empty(), "{:?}", sts.requests());

    let longest = "t".repeat(20_000);
    source(&sts.uri, &TokenFile::holding(&longest))
        .credentials(&Cancel::new())
        .await
        .expect("a token of 20000 bytes is sent");
}

#[tokio::test]
async fn a_cancelled_caller_is_answered_cancelled_without_a_request() {
    let sts = Sts::answering(success(in_seconds(3600))).await;
    let cancel = Cancel::new();
    cancel.cancel();
    let error = source(&sts.uri, &TokenFile::holding(WEB_IDENTITY_TOKEN))
        .credentials(&cancel)
        .await
        .expect_err("cancelled");
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert_quiet(&error);
    assert!(sts.requests().is_empty());
}

/// The source's `Debug` form names the endpoint's origin, the role, the session and the token
/// reference, never the token; the credentials' shows no key.
#[tokio::test]
async fn debug_forms_show_no_token_and_no_key() {
    let sts = Sts::answering(success(in_seconds(3600))).await;
    let source = source(&sts.uri, &TokenFile::holding(WEB_IDENTITY_TOKEN));
    let credentials = source
        .credentials(&Cancel::new())
        .await
        .expect("credentials");
    for debug in [format!("{source:?}"), format!("{credentials:?}")] {
        for secret in SECRETS {
            assert!(!debug.contains(secret), "{debug}");
        }
    }
    let debug = format!("{source:?}");
    assert!(debug.contains(TOKEN_REFERENCE), "{debug}");
    assert!(debug.contains(ROLE_ARN), "{debug}");
}

const PROXY_CHILD: &str = "LLM_BEDROCK_WEB_IDENTITY_PROXY_CHILD";

/// No ambient proxy receives the token: this test runs itself again with every proxy variable
/// naming a local listener, and that listener hears nothing of the exchange the child makes.
#[test]
fn the_token_never_reaches_an_ambient_proxy() {
    if std::env::var_os(PROXY_CHILD).is_some() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime");
        runtime.block_on(async {
            let sts = Sts::answering(success(in_seconds(3600))).await;
            source(&sts.uri, &TokenFile::holding(WEB_IDENTITY_TOKEN))
                .credentials(&Cancel::new())
                .await
                .expect("credentials straight from the endpoint");
            assert_eq!(sts.requests().len(), 1);
        });
        return;
    }
    let proxy = std::net::TcpListener::bind("127.0.0.1:0").expect("a local port");
    let proxy_url = format!("http://{}", proxy.local_addr().expect("its address"));
    let (seen, heard) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        while let Ok((mut socket, _)) = proxy.accept() {
            let _ = socket.set_read_timeout(Some(Duration::from_secs(2)));
            let mut bytes = vec![0; 64 * 1024];
            let length = std::io::Read::read(&mut socket, &mut bytes).unwrap_or(0);
            bytes.truncate(length);
            if seen.send(bytes).is_err() {
                break;
            }
        }
    });
    let mut child = std::process::Command::new(std::env::current_exe().expect("this binary"));
    child
        .args(["--exact", "the_token_never_reaches_an_ambient_proxy"])
        .env(PROXY_CHILD, "1")
        .env_remove("NO_PROXY")
        .env_remove("no_proxy");
    for variable in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        child.env(variable, &proxy_url);
    }
    let status = child.status().expect("the child runs");
    assert!(status.success(), "the child failed: {status}");
    while let Ok(bytes) = heard.recv_timeout(Duration::from_millis(500)) {
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            !text.contains(WEB_IDENTITY_TOKEN) && !text.contains("AssumeRoleWithWebIdentity"),
            "an ambient proxy received the exchange (token forwarded: {})",
            text.contains(WEB_IDENTITY_TOKEN)
        );
    }
}
