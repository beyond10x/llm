//! Adversary cases for `WebIdentityCredentials` (wave 2026-11-04-w25, pass 1): the form encoding
//! of reserved token bytes, the XML reader against decoys, sections and size, the endpoint guard,
//! a redirect to a host that is not local, and the single-flight refresh around failures.

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

const ROLE_ARN: &str = "arn:aws:iam::000000000000:role/adversary-fixture-role";
const SESSION_NAME: &str = "adversary-session";
const TOKEN_REFERENCE: &str = "web-identity-token";
const TOKEN: &str = "adversary-fixture-token.claims.signature";
const ACCESS_KEY_ID: &str = "ASIA-adversary-fixture";
const DECOY_KEY_ID: &str = "ASIA-adversary-decoy";
const SECRET_ACCESS_KEY: &str = "adversary/fixture+secret-key";
const DECOY_SECRET: &str = "adversary-decoy-secret";
const SESSION_TOKEN: &str = "adversary-fixture-session-token";
const STS_MESSAGE: &str = "adversary-fixture-sts-message";

const SECRETS: [&str; 6] = [
    TOKEN,
    ACCESS_KEY_ID,
    SECRET_ACCESS_KEY,
    DECOY_SECRET,
    SESSION_TOKEN,
    STS_MESSAGE,
];

#[derive(Clone)]
struct TokenFile(Arc<Mutex<Option<String>>>);

impl TokenFile {
    fn holding(token: &str) -> Self {
        Self(Arc::new(Mutex::new(Some(token.to_owned()))))
    }

    fn set(&self, token: Option<&str>) {
        *self.0.lock().expect("the token file") = token.map(str::to_owned);
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

fn client() -> HttpClient {
    HttpClient::new(Limits {
        response_headers: Duration::from_secs(10),
        idle: Duration::from_secs(10),
        total: Duration::from_secs(10),
    })
    .expect("an HTTP client")
}

fn build(uri: &str, file: &TokenFile) -> Result<WebIdentityCredentials, Error> {
    WebIdentityCredentials::new(
        client(),
        uri,
        ROLE_ARN,
        SESSION_NAME,
        Arc::new(file.clone()),
        SecretRef::new(TOKEN_REFERENCE).expect("a reference"),
    )
}

fn source(uri: &str, file: &TokenFile) -> WebIdentityCredentials {
    build(uri, file).expect("an admitted endpoint")
}

#[derive(Clone, Debug)]
struct Captured {
    head: String,
    body: String,
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

/// A local STS answering each connection with the next canned response, the last repeated.
struct Sts {
    uri: String,
    requests: Arc<Mutex<Vec<Captured>>>,
    task: JoinHandle<()>,
}

impl Sts {
    async fn answering(response: Vec<u8>) -> Self {
        Self::answering_each(vec![response]).await
    }

    async fn answering_each(responses: Vec<Vec<u8>>) -> Self {
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
                let _ = stream.write_all(response).await;
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

fn response(status: &str, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 {status}\r\ncontent-type: text/xml\r\ncontent-length: {length}\r\n\
         connection: close\r\n\r\n{body}",
        length = body.len()
    )
    .into_bytes()
}

fn chunked(status: &str, body: &str) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 {status}\r\ncontent-type: text/xml\r\ntransfer-encoding: chunked\r\n\
         connection: close\r\n\r\n"
    );
    for piece in body.as_bytes().chunks(8192) {
        let _ = write!(out, "{:x}\r\n", piece.len());
        out.push_str(std::str::from_utf8(piece).expect("ASCII body"));
        out.push_str("\r\n");
    }
    out.push_str("0\r\n\r\n");
    out.into_bytes()
}

fn in_seconds(seconds: u64) -> SystemTime {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("a clock after 1970")
        .as_secs();
    UNIX_EPOCH + Duration::from_secs(now + seconds)
}

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

/// A success document with `before` placed inside the result ahead of `Credentials`, and the
/// credentials' inner XML as given.
fn document(before: &str, credentials: &str) -> String {
    format!(
        "<AssumeRoleWithWebIdentityResponse xmlns=\"https://sts.amazonaws.com/doc/2011-06-15/\">\
         <AssumeRoleWithWebIdentityResult>{before}<Credentials>{credentials}</Credentials>\
         </AssumeRoleWithWebIdentityResult>\
         <ResponseMetadata><RequestId>00000000-0000-0000-0000-000000000000</RequestId></ResponseMetadata>\
         </AssumeRoleWithWebIdentityResponse>"
    )
}

fn members(expires: &str) -> String {
    format!(
        "<AccessKeyId>{ACCESS_KEY_ID}</AccessKeyId><SecretAccessKey>{SECRET_ACCESS_KEY}</SecretAccessKey>\
         <SessionToken>{SESSION_TOKEN}</SessionToken><Expiration>{expires}</Expiration>"
    )
}

fn success_in(seconds: u64) -> Vec<u8> {
    response(
        "200 OK",
        &document("", &members(&rfc3339(in_seconds(seconds)))),
    )
}

fn sts_error(status: &str, code: &str) -> Vec<u8> {
    response(
        status,
        &format!(
            "<ErrorResponse xmlns=\"https://sts.amazonaws.com/doc/2011-06-15/\"><Error>\
             <Type>Sender</Type><Code>{code}</Code><Message>{STS_MESSAGE} {TOKEN}</Message>\
             </Error><RequestId>00000000-0000-0000-0000-000000000000</RequestId></ErrorResponse>"
        ),
    )
}

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
    assert_quiet(error);
}

async fn settled(source: &WebIdentityCredentials) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while source.refreshing() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the refresh settles");
}

/// Every form byte that is not unreserved is percent-encoded: `+`, `/`, `=`, `&`, `%` and `:`
/// in the token cannot split or alter a form field. The unit's own suite sends only
/// unreserved token bytes, so an encoder that leaves these raw passes it.
#[tokio::test]
async fn reserved_token_bytes_are_percent_encoded_in_the_form() {
    let sts = Sts::answering(success_in(3600)).await;
    let token = "ab+cd/ef=gh&Action=Other&ij%41kl:mn~op";
    source(&sts.uri, &TokenFile::holding(token))
        .credentials(&Cancel::new())
        .await
        .expect("credentials");
    let requests = sts.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].body,
        "Action=AssumeRoleWithWebIdentity&Version=2011-06-15\
         &RoleArn=arn%3Aaws%3Aiam%3A%3A000000000000%3Arole%2Fadversary-fixture-role\
         &RoleSessionName=adversary-session\
         &WebIdentityToken=ab%2Bcd%2Fef%3Dgh%26Action%3DOther%26ij%2541kl%3Amn~op"
    );
    assert_eq!(
        requests[0].head.split("\r\n").next(),
        Some("POST / HTTP/1.1")
    );
}

/// A `Credentials` element under another parent is a decoy: the reader takes the members of
/// the one under `AssumeRoleWithWebIdentityResult` only, whichever comes first.
#[tokio::test]
async fn credentials_under_another_parent_are_ignored() {
    let expires = rfc3339(in_seconds(3600));
    let decoy = format!(
        "<AssumedRoleUser><Credentials><AccessKeyId>{DECOY_KEY_ID}</AccessKeyId>\
         <SecretAccessKey>{DECOY_SECRET}</SecretAccessKey><SessionToken>{DECOY_SECRET}</SessionToken>\
         <Expiration>{expires}</Expiration></Credentials></AssumedRoleUser>"
    );
    let sts = Sts::answering(response("200 OK", &document(&decoy, &members(&expires)))).await;
    let credentials = source(&sts.uri, &TokenFile::holding(TOKEN))
        .credentials(&Cancel::new())
        .await
        .expect("credentials");
    assert_eq!(credentials.access_key_id(), ACCESS_KEY_ID);
    assert_eq!(credentials.secret_access_key(), SECRET_ACCESS_KEY);
    assert_eq!(credentials.session_token(), Some(SESSION_TOKEN));
}

/// A member in the wrong parent does not stand in for the one missing from `Credentials`.
#[tokio::test]
async fn a_member_in_the_wrong_parent_does_not_fill_a_missing_one() {
    let expires = rfc3339(in_seconds(3600));
    let inner = format!(
        "<AccessKeyId>{ACCESS_KEY_ID}</AccessKeyId><SessionToken>{SESSION_TOKEN}</SessionToken>\
         <Expiration>{expires}</Expiration>"
    );
    let before = format!("<SecretAccessKey>{SECRET_ACCESS_KEY}</SecretAccessKey>");
    let sts = Sts::answering(response("200 OK", &document(&before, &inner))).await;
    let error = source(&sts.uri, &TokenFile::holding(TOKEN))
        .credentials(&Cancel::new())
        .await
        .expect_err("no SecretAccessKey under Credentials");
    assert_unauthorized(&error);
}

/// A CDATA section, a processing instruction inside the document and a prefixed root are
/// outside the reader's subset and refused, never read around.
#[tokio::test]
async fn sections_instructions_and_prefixed_names_are_refused() {
    let expires = rfc3339(in_seconds(3600));
    let whole = document("", &members(&expires));
    let bodies = [
        whole.replace(
            SECRET_ACCESS_KEY,
            &format!("<![CDATA[{SECRET_ACCESS_KEY}]]>"),
        ),
        whole.replace("<Credentials>", "<?pi data?><Credentials>"),
        whole
            .replace(
                "<AssumeRoleWithWebIdentityResponse ",
                "<sts:AssumeRoleWithWebIdentityResponse xmlns:sts=\"urn:x\" ",
            )
            .replace(
                "</AssumeRoleWithWebIdentityResponse>",
                "</sts:AssumeRoleWithWebIdentityResponse>",
            ),
        format!("\u{feff}{whole}"),
        format!("<?xml version=\"1.0\"?><?xml version=\"1.0\"?>{whole}"),
    ];
    for body in &bodies {
        let sts = Sts::answering(response("200 OK", body)).await;
        let error = source(&sts.uri, &TokenFile::holding(TOKEN))
            .credentials(&Cancel::new())
            .await
            .expect_err("outside the subset");
        assert_unauthorized(&error);
    }
}

/// What STS writes in the field: CRLF line ends and indentation between members, single-quoted
/// attributes, a declaration with `standalone`, and an `Expiration` with milliseconds.
#[tokio::test]
async fn a_pretty_printed_answer_with_a_fractional_expiration_is_read() {
    let expires = in_seconds(3600);
    let stamp = rfc3339(expires).replace('Z', ".000Z");
    let body = format!(
        "<?xml version='1.0' encoding='UTF-8' standalone='yes'?>\r\n\
         <AssumeRoleWithWebIdentityResponse xmlns='https://sts.amazonaws.com/doc/2011-06-15/'>\r\n\
         \t<AssumeRoleWithWebIdentityResult>\r\n\
         \t\t<Credentials>\r\n\
         \t\t\t<AccessKeyId>{ACCESS_KEY_ID}</AccessKeyId>\r\n\
         \t\t\t<SecretAccessKey>{SECRET_ACCESS_KEY}</SecretAccessKey>\r\n\
         \t\t\t<SessionToken>{SESSION_TOKEN}</SessionToken>\r\n\
         \t\t\t<Expiration>{stamp}</Expiration>\r\n\
         \t\t</Credentials >\r\n\
         \t</AssumeRoleWithWebIdentityResult>\r\n\
         </AssumeRoleWithWebIdentityResponse>\r\n"
    );
    let sts = Sts::answering(response("200 OK", &body)).await;
    let credentials = source(&sts.uri, &TokenFile::holding(TOKEN))
        .credentials(&Cancel::new())
        .await
        .expect("credentials");
    assert_eq!(credentials.access_key_id(), ACCESS_KEY_ID);
    assert_eq!(credentials.expires(), Some(expires));
}

/// A success past 64 KiB, declared or chunked, is an unusable answer: `unauthorized`, final,
/// not a retriable cut.
#[tokio::test]
async fn a_success_past_the_bound_is_unauthorized_and_final() {
    let expires = rfc3339(in_seconds(3600));
    let padding = format!("<Padding>{}</Padding>", "p".repeat(70 * 1024));
    let body = document(&padding, &members(&expires));
    for answer in [response("200 OK", &body), chunked("200 OK", &body)] {
        let sts = Sts::answering(answer).await;
        let error = source(&sts.uri, &TokenFile::holding(TOKEN))
            .credentials(&Cancel::new())
            .await
            .expect_err("too large");
        assert_unauthorized(&error);
    }
}

/// An answer whose declared length ends inside a member is whole as far as HTTP goes and an
/// unusable document: `unauthorized`, final.
#[tokio::test]
async fn a_document_truncated_inside_a_member_is_unauthorized() {
    let whole = document("", &members(&rfc3339(in_seconds(3600))));
    let cut = &whole[..whole.find(SESSION_TOKEN).expect("the token") + 5];
    let sts = Sts::answering(response("200 OK", cut)).await;
    let error = source(&sts.uri, &TokenFile::holding(TOKEN))
        .credentials(&Cancel::new())
        .await
        .expect_err("truncated");
    assert_unauthorized(&error);
}

/// STS throttling answers 400 `Throttling`; a 429 is the same class here. Both are named and
/// final as the brief claims.
#[tokio::test]
async fn throttling_is_unauthorized_final_and_named() {
    for status in ["400 Bad Request", "429 Too Many Requests"] {
        let sts = Sts::answering(sts_error(status, "Throttling")).await;
        let error = source(&sts.uri, &TokenFile::holding(TOKEN))
            .credentials(&Cancel::new())
            .await
            .expect_err(status);
        assert_unauthorized(&error);
        assert!(error.message.contains("Throttling"), "{}", error.message);
    }
}

/// `IDPCommunicationError` in a document with a declaration and CRLF line ends is still read as
/// retriable; named twice it is not read as a named code.
#[tokio::test]
async fn an_idp_communication_error_with_a_declaration_is_retriable() {
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<ErrorResponse \
         xmlns=\"https://sts.amazonaws.com/doc/2011-06-15/\">\r\n  <Error>\r\n    \
         <Type>Sender</Type>\r\n    <Code>IDPCommunicationError</Code>\r\n    \
         <Message>{STS_MESSAGE}</Message>\r\n  </Error>\r\n  \
         <RequestId>00000000-0000-0000-0000-000000000000</RequestId>\r\n</ErrorResponse>\r\n"
    );
    let sts = Sts::answering(response("400 Bad Request", &body)).await;
    let error = source(&sts.uri, &TokenFile::holding(TOKEN))
        .credentials(&Cancel::new())
        .await
        .expect_err("retriable");
    assert_eq!(error.code, ErrorCode::Transport, "{error:?}");
    assert!(error.retriable, "{error:?}");
    assert_quiet(&error);
}

/// The guard: scheme and host case do not matter, every loopback spelling the URL standard
/// normalises is admitted over `http`, and a name, the unspecified address or empty user
/// information is not.
#[test]
fn the_endpoint_guard_reads_normalised_addresses() {
    let file = TokenFile::holding(TOKEN);
    for uri in [
        "HTTPS://STS.EU-CENTRAL-1.AMAZONAWS.COM/",
        "HTTP://127.0.0.1:9/",
        "http://[0:0:0:0:0:0:0:1]:9/",
        "http://0x7f.1:9/",
        "http://2130706433:9/",
    ] {
        build(uri, &file).unwrap_or_else(|error| panic!("{uri}: {error}"));
    }
    for uri in [
        "http://LOCALHOST/",
        "http://localhost./",
        "http://127.0.0.1.nip.io/",
        "http://0.0.0.0/",
        "http://[::]/",
        "http://user:@127.0.0.1/",
        "https://sts.eu-central-1.amazonaws.com/?",
        "https://sts.eu-central-1.amazonaws.com/#",
        "http:127.0.0.1@10.0.0.1/",
    ] {
        let error = build(uri, &file).expect_err(uri);
        assert_unauthorized(&error);
    }
}

/// A redirect to a host that is not local is never followed: following it would end in a
/// name lookup or connection failure, which is `transport` and retriable. It is `unauthorized`
/// and final, and the target is not named.
#[tokio::test]
async fn a_redirect_to_a_remote_host_is_not_followed() {
    let location = "https://sts-redirect.adversary.invalid/steal";
    for status in [
        "301 Moved Permanently",
        "302 Found",
        "303 See Other",
        "307 Temporary Redirect",
        "308 Permanent Redirect",
    ] {
        let sts = Sts::answering(
            format!(
                "HTTP/1.1 {status}\r\nlocation: {location}\r\ncontent-length: 0\r\n\
                 connection: close\r\n\r\n"
            )
            .into_bytes(),
        )
        .await;
        let error = source(&sts.uri, &TokenFile::holding(TOKEN))
            .credentials(&Cancel::new())
            .await
            .expect_err(status);
        assert_unauthorized(&error);
        assert!(
            !error.message.contains("adversary.invalid"),
            "{}",
            error.message
        );
        assert_eq!(sts.requests().len(), 1, "{status}");
    }
}

/// Concurrent callers with nothing kept share one failing request and all see its class; the
/// failure is not kept, so the next call asks again.
#[tokio::test]
async fn concurrent_callers_share_one_failure_and_the_next_call_asks_again() {
    let sts = Sts::answering_each(vec![
        sts_error("400 Bad Request", "InvalidIdentityToken"),
        success_in(3600),
    ])
    .await;
    let source = source(&sts.uri, &TokenFile::holding(TOKEN));
    let cancel = Cancel::new();
    let (a, b, c) = tokio::join!(
        source.credentials(&cancel),
        source.credentials(&cancel),
        source.credentials(&cancel)
    );
    for answer in [a, b, c] {
        let error = answer.expect_err("the shared failure");
        assert_unauthorized(&error);
        assert!(
            error.message.contains("InvalidIdentityToken"),
            "{}",
            error.message
        );
    }
    assert_eq!(sts.requests().len(), 1);
    let credentials = source.credentials(&cancel).await.expect("asked again");
    assert_eq!(credentials.access_key_id(), ACCESS_KEY_ID);
    assert_eq!(sts.requests().len(), 2);
}

/// A refresh inside the window that fails at the token file leaves the kept credentials in use
/// and sends nothing; once the file is back, the next refresh sends its token.
#[tokio::test]
async fn a_refresh_whose_token_file_is_gone_keeps_the_credentials() {
    let sts = Sts::answering(success_in(120)).await;
    let file = TokenFile::holding(TOKEN);
    let source = source(&sts.uri, &file);
    let cancel = Cancel::new();
    let first = source.credentials(&cancel).await.expect("first");
    assert_eq!(first.access_key_id(), ACCESS_KEY_ID);
    file.set(None);
    let kept = source.credentials(&cancel).await.expect("kept");
    assert_eq!(kept.secret_access_key(), SECRET_ACCESS_KEY);
    settled(&source).await;
    assert_eq!(
        sts.requests().len(),
        1,
        "a refresh without a token sent a request"
    );
    file.set(Some("adversary-fixture-token.rotated.signature"));
    source.credentials(&cancel).await.expect("kept again");
    settled(&source).await;
    let requests = sts.requests();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1]
            .body
            .ends_with("WebIdentityToken=adversary-fixture-token.rotated.signature"),
        "{}",
        requests[1].body
    );
}
