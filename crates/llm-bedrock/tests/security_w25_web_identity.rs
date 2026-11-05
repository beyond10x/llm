//! Security review of `WebIdentityCredentials`, wave 2026-11-04-w25: invariants the unit's own
//! suite does not measure. Form encoding cannot add or replace a parameter; a failed fetch keeps
//! no token; no failure names the endpoint; the endpoint guard refuses the addresses it must;
//! a late STS is a retriable deadline; an answer of unknown length is bounded while it is read;
//! an `https` endpoint is reached directly whatever proxy the environment names.
//!
//! Every value here is a fixture marker, never a credential.

use llm_bedrock::{CredentialSource, WebIdentityCredentials};
use llm_core::{BoxFuture, Cancel, Dispatch, Error, ErrorCode};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_http::{HttpClient, Limits, MAX_EXCHANGE_BYTES};
use percent_encoding::percent_decode_str;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

const ROLE_ARN: &str = "arn:aws:iam::000000000000:role/llm-fixture-role";
const SESSION_NAME: &str = "llm-fixture-session";
const REFERENCE: &str = "web-identity-token";
const TOKEN: &str = "llm-fixture-sec-w25-token-marker";
const ACCESS_KEY_ID: &str = "ASIASECWFIXTURE";
const SECRET_ACCESS_KEY: &str = "llm-fixture-sec-w25-secret-key-marker";
const SESSION_TOKEN: &str = "llm-fixture-sec-w25-session-token-marker";
const STS_MESSAGE: &str = "llm-fixture-sec-w25-sts-message-marker";
const EXPIRATION: &str = "2100-01-01T00:00:00Z";
/// Every marker a case uses: none may reach a message or a `Debug` form.
const MARKERS: [&str; 5] = [
    TOKEN,
    ACCESS_KEY_ID,
    SECRET_ACCESS_KEY,
    SESSION_TOKEN,
    STS_MESSAGE,
];

/// The token file as a resolver reads it, counting every read.
#[derive(Clone, Default)]
struct TokenFile {
    content: Arc<Mutex<Option<String>>>,
    reads: Arc<AtomicUsize>,
}

impl TokenFile {
    fn holding(token: &str) -> Self {
        let file = Self::default();
        file.write(token);
        file
    }

    fn write(&self, token: &str) {
        *self.content.lock().expect("the token file") = Some(token.to_owned());
    }

    fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}

impl SecretResolver for TokenFile {
    fn resolve<'a>(
        &'a self,
        reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async move {
            self.reads.fetch_add(1, Ordering::SeqCst);
            let content = self.content.lock().expect("the token file").clone();
            let content = content
                .filter(|_| reference.as_str() == REFERENCE)
                .ok_or(SecretError::Missing)?;
            Ok(ResolvedSecret {
                secret: Secret::new(content.into_bytes())?,
                version: SecretVersion::new("1".to_owned())?,
            })
        })
    }
}

fn reference() -> SecretRef {
    SecretRef::new(REFERENCE).expect("a valid reference")
}

fn client() -> HttpClient {
    HttpClient::new(Limits::default()).expect("an HTTP client")
}

fn source(uri: &str, file: &TokenFile, http: HttpClient) -> WebIdentityCredentials {
    WebIdentityCredentials::new(
        http,
        uri,
        ROLE_ARN,
        SESSION_NAME,
        Arc::new(file.clone()),
        reference(),
    )
    .expect("an admitted endpoint")
}

/// One request as the fixture read it.
#[derive(Clone)]
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

/// What the fixture writes on one connection; `hold` keeps the connection open afterwards
/// without writing more.
#[derive(Clone)]
struct Reply {
    bytes: Vec<u8>,
    hold: bool,
}

/// A whole answer with a declared length, the connection closed after it.
fn reply(status: &str, body: &str) -> Reply {
    Reply {
        bytes: format!(
            "HTTP/1.1 {status}\r\ncontent-type: text/xml\r\ncontent-length: {length}\r\n\
             connection: close\r\n\r\n{body}",
            length = body.len()
        )
        .into_bytes(),
        hold: false,
    }
}

/// `bytes` written as they are, then the connection held open.
fn held(bytes: &str) -> Reply {
    Reply {
        bytes: bytes.as_bytes().to_vec(),
        hold: true,
    }
}

/// A 200 whose body is `body`, sent with chunked transfer coding in 4 KiB chunks: no length is
/// declared.
fn chunked(body: &str) -> Reply {
    let mut bytes = String::from(
        "HTTP/1.1 200 OK\r\ncontent-type: text/xml\r\ntransfer-encoding: chunked\r\n\
         connection: close\r\n\r\n",
    );
    for piece in body.as_bytes().chunks(4096) {
        let piece = std::str::from_utf8(piece).expect("an ASCII body");
        let _ = write!(bytes, "{:x}\r\n{piece}\r\n", piece.len());
    }
    bytes.push_str("0\r\n\r\n");
    Reply {
        bytes: bytes.into_bytes(),
        hold: false,
    }
}

/// An `AssumeRoleWithWebIdentity` answer holding the fixture credentials.
fn answer() -> String {
    format!(
        "<AssumeRoleWithWebIdentityResponse xmlns=\"https://sts.amazonaws.com/doc/2011-06-15/\">\
         <AssumeRoleWithWebIdentityResult><Credentials>\
         <AccessKeyId>{ACCESS_KEY_ID}</AccessKeyId>\
         <SecretAccessKey>{SECRET_ACCESS_KEY}</SecretAccessKey>\
         <SessionToken>{SESSION_TOKEN}</SessionToken>\
         <Expiration>{EXPIRATION}</Expiration>\
         </Credentials></AssumeRoleWithWebIdentityResult>\
         </AssumeRoleWithWebIdentityResponse>"
    )
}

fn success() -> Reply {
    reply("200 OK", &answer())
}

fn sts_error(status: &str, code: &str) -> Reply {
    reply(
        status,
        &format!(
            "<ErrorResponse><Error><Type>Sender</Type><Code>{code}</Code>\
             <Message>{STS_MESSAGE} {TOKEN}</Message></Error></ErrorResponse>"
        ),
    )
}

/// A local STS: each connection gets the next reply, the last one repeated; every request read
/// is kept.
struct Sts {
    address: String,
    requests: Arc<Mutex<Vec<Captured>>>,
    task: JoinHandle<()>,
}

impl Sts {
    async fn answering(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a local port");
        let address = listener.local_addr().expect("its address").to_string();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&requests);
        let task = tokio::spawn(async move {
            let mut answered = 0;
            while let Ok((mut stream, _)) = listener.accept().await {
                if let Some(request) = read_request(&mut stream).await {
                    seen.lock().expect("the request log").push(request);
                }
                let reply = &replies[answered.min(replies.len() - 1)];
                answered += 1;
                let _ = stream.write_all(&reply.bytes).await;
                if reply.hold {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                }
                let _ = stream.shutdown().await;
            }
        });
        Self {
            address,
            requests,
            task,
        }
    }

    fn uri(&self) -> String {
        format!("http://{}/", self.address)
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

/// The failure says nothing was sent and carries no marker, in its message or `Debug` form.
fn assert_quiet(error: &Error) {
    assert_eq!(error.dispatch, Dispatch::NotSent, "{error:?}");
    let debug = format!("{error:?}");
    for marker in MARKERS {
        assert!(!error.message.contains(marker), "{}", error.message);
        assert!(!debug.contains(marker), "{debug}");
    }
}

/// The `WebIdentityToken` value a form carries, percent-decoded.
fn token_sent(body: &str) -> String {
    let encoded = body
        .split('&')
        .find_map(|pair| pair.strip_prefix("WebIdentityToken="))
        .expect("a WebIdentityToken parameter");
    percent_decode_str(encoded)
        .decode_utf8()
        .expect("UTF-8")
        .into_owned()
}

/// A token, a role ARN and a session name holding `&`, `=`, `+`, `%`, `#` and `?` stay inside
/// their own parameter: the body is exactly the five parameters in order, each value only
/// unreserved characters and `%XX`, decoding back to what was given; nothing reaches the
/// request target.
#[tokio::test]
async fn a_token_role_or_session_name_holding_form_delimiters_adds_or_replaces_no_parameter() {
    let token = "llm-fixture&RoleArn=arn:aws:iam::000000000000:role/other\
                 &Action=GetCallerIdentity+%26;#?";
    let role = "arn:aws:iam::000000000000:role/llm-fixture&Version=1999-01-01";
    let session = "llm+fixture=session,1@x";
    let sts = Sts::answering(vec![success()]).await;
    let credentials = WebIdentityCredentials::new(
        client(),
        &sts.uri(),
        role,
        session,
        Arc::new(TokenFile::holding(token)),
        reference(),
    )
    .expect("an admitted source")
    .credentials(&Cancel::new())
    .await
    .expect("credentials");
    assert_eq!(credentials.access_key_id(), ACCESS_KEY_ID);

    let requests = sts.requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert!(
        request.head.starts_with("POST / HTTP/1.1\r\n"),
        "{}",
        request.head
    );
    assert!(!request.head.contains("llm-fixture"), "{}", request.head);
    let pairs: Vec<&str> = request.body.split('&').collect();
    let names: Vec<&str> = pairs
        .iter()
        .map(|pair| pair.split('=').next().unwrap_or_default())
        .collect();
    assert_eq!(
        names,
        [
            "Action",
            "Version",
            "RoleArn",
            "RoleSessionName",
            "WebIdentityToken"
        ],
        "{}",
        request.body
    );
    let given = [
        "AssumeRoleWithWebIdentity",
        "2011-06-15",
        role,
        session,
        token,
    ];
    for (pair, value) in pairs.iter().zip(given) {
        let (_, encoded) = pair.split_once('=').expect("name=value");
        assert!(
            encoded
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-._~%".contains(&byte)),
            "a delimiter left unencoded: {pair}"
        );
        assert_eq!(
            percent_decode_str(encoded).decode_utf8().expect("UTF-8"),
            value
        );
    }
}

/// The token is not kept past a fetch that failed: a missing file, STS `ExpiredToken` and a 503
/// each leave the next fetch to read the file again and send what it holds then. Kept
/// credentials read nothing.
#[tokio::test]
async fn after_a_failed_fetch_the_next_one_reads_the_token_file_again() {
    let sts = Sts::answering(vec![
        sts_error("400 Bad Request", "ExpiredToken"),
        reply("503 Service Unavailable", ""),
        success(),
    ])
    .await;
    let file = TokenFile::default();
    let source = source(&sts.uri(), &file, client());
    let cancel = Cancel::new();

    let missing = source.credentials(&cancel).await.expect_err("no token yet");
    assert_eq!(missing.code, ErrorCode::Unauthorized, "{missing:?}");
    assert!(sts.requests().is_empty());

    let tokens = [
        "llm-fixture-sec-w25-first",
        "llm-fixture-sec-w25-second",
        "llm-fixture-sec-w25-third",
    ];
    file.write(tokens[0]);
    let expired = source.credentials(&cancel).await.expect_err("ExpiredToken");
    assert_eq!(expired.code, ErrorCode::Unauthorized, "{expired:?}");
    file.write(tokens[1]);
    let unavailable = source.credentials(&cancel).await.expect_err("a 503");
    assert!(unavailable.may_retry(), "{unavailable:?}");
    file.write(tokens[2]);
    source.credentials(&cancel).await.expect("credentials");

    assert_eq!(file.reads(), 4);
    let sent: Vec<String> = sts.requests().iter().map(|r| token_sent(&r.body)).collect();
    assert_eq!(sent, tokens);

    source.credentials(&cancel).await.expect("kept credentials");
    assert_eq!(file.reads(), 4);
    assert_eq!(sts.requests().len(), 3);
}

/// The failure one fetch against a fixture answering `replies` ends in, and the fixture's
/// address.
async fn failure_against(replies: Vec<Reply>, limits: Limits) -> (Error, String) {
    let sts = Sts::answering(replies).await;
    let http = HttpClient::new(limits).expect("an HTTP client");
    let error = source(&sts.uri(), &TokenFile::holding(TOKEN), http)
        .credentials(&Cancel::new())
        .await
        .expect_err("a failure");
    (error, sts.address.clone())
}

fn short_limits() -> Limits {
    Limits {
        response_headers: Duration::from_millis(300),
        idle: Duration::from_millis(300),
        total: Duration::from_secs(2),
    }
}

/// `docs/bedrock.md` says a web identity message "never quotes the endpoint": no failure, from
/// a redirect to a stalled head, names the endpoint's address or the redirect target's.
#[tokio::test]
async fn no_failure_names_the_endpoint_or_a_redirect_target() {
    let elsewhere = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local port");
    let target = elsewhere.local_addr().expect("its address").to_string();
    let redirect = Reply {
        bytes: format!(
            "HTTP/1.1 307 Temporary Redirect\r\nlocation: http://{target}/\r\n\
             content-length: 0\r\nconnection: close\r\n\r\n"
        )
        .into_bytes(),
        hold: false,
    };
    let oversized = format!(
        "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n<A>",
        MAX_EXCHANGE_BYTES + 1
    );
    let cases: Vec<(&str, Reply, Limits)> = vec![
        ("redirect", redirect, Limits::default()),
        (
            "503",
            reply("503 Service Unavailable", STS_MESSAGE),
            Limits::default(),
        ),
        (
            "documented 400",
            sts_error("400 Bad Request", "ExpiredToken"),
            Limits::default(),
        ),
        (
            "undocumented 400",
            sts_error("400 Bad Request", "FixtureUndocumented"),
            Limits::default(),
        ),
        (
            "unusable answer",
            reply("200 OK", "<Nothing/>"),
            Limits::default(),
        ),
        ("oversized answer", held(&oversized), short_limits()),
        (
            "cut answer",
            Reply {
                bytes: b"HTTP/1.1 200 OK\r\ncontent-length: 4096\r\n\r\n<A>".to_vec(),
                hold: false,
            },
            Limits::default(),
        ),
        ("stalled head", held("HTTP/1.1 200 OK\r\n"), short_limits()),
    ];
    for (case, replies, limits) in cases {
        let (error, address) = failure_against(vec![replies], limits).await;
        assert_quiet(&error);
        for text in [error.message.clone(), format!("{error:?}")] {
            assert!(!text.contains("127.0.0.1"), "{case}: {text}");
            assert!(!text.contains(&address), "{case}: {text}");
            assert!(!text.contains(&target), "{case}: {text}");
        }
    }
    let closed = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local port");
    let address = closed.local_addr().expect("its address").to_string();
    drop(closed);
    let error = source(
        &format!("http://{address}/"),
        &TokenFile::holding(TOKEN),
        client(),
    )
    .credentials(&Cancel::new())
    .await
    .expect_err("nothing listens");
    assert_quiet(&error);
    assert!(!error.message.contains("127.0.0.1"), "{}", error.message);
}

/// Addresses the guard must refuse that the unit's cases do not name: the container agent's
/// other addresses, the unspecified addresses, a loopback name over `http`, other schemes, an
/// empty query or fragment and a password without a user. Each is refused when the source is
/// built, `unauthorized` and final, quoting none of it.
#[test]
fn more_endpoints_the_guard_must_refuse_are_refused_when_the_source_is_built() {
    for uri in [
        "http://[fd00:ec2::23]/",
        "http://169.254.170.2/",
        "http://0.0.0.0/",
        "http://[::]/",
        "http://[::ffff:7f00:1]/",
        "http://127.0.0.1.fixture.example/",
        "HTTP://LOCALHOST/",
        "ws://127.0.0.1/",
        "wss://sts.fixture.example/",
        "file:///var/run/secrets/token",
        "https://sts.fixture.example/?",
        "https://sts.fixture.example/#",
        "http://127.0.0.1/?Action=AssumeRoleWithWebIdentity",
        "https://:fixture-password@sts.fixture.example/",
    ] {
        let error = WebIdentityCredentials::new(
            client(),
            uri,
            ROLE_ARN,
            SESSION_NAME,
            Arc::new(TokenFile::holding(TOKEN)),
            reference(),
        )
        .expect_err(uri);
        assert_eq!(error.code, ErrorCode::Unauthorized, "{uri}: {error:?}");
        assert!(!error.may_retry(), "{uri}: {error:?}");
        for part in ["fixture", "127.0", "169.254", "fd00", "LOCALHOST", "Action"] {
            assert!(!error.message.contains(part), "{uri}: {}", error.message);
        }
    }
}

/// `docs/bedrock.md`: an endpoint that answers after the transport's deadline is `deadline`,
/// retriable, `not-sent`, whether its head or its body is late, and the fetch ends within the
/// client's bounds.
#[tokio::test]
async fn an_sts_endpoint_that_answers_too_late_is_a_retriable_deadline() {
    for (case, late) in [
        ("head", held("")),
        (
            "body",
            held(
                "HTTP/1.1 200 OK\r\ncontent-length: 4096\r\n\r\n<AssumeRoleWithWebIdentityResponse>",
            ),
        ),
    ] {
        let started = Instant::now();
        let (error, _) = failure_against(vec![late], short_limits()).await;
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{case}: {:?}",
            started.elapsed()
        );
        assert_eq!(error.code, ErrorCode::Deadline, "{case}: {error:?}");
        // The producer's mark the page states; `may_retry` never admits `deadline` by design.
        assert!(error.retriable, "{case}: {error:?}");
        assert_quiet(&error);
    }
}

/// An answer that declares no length is bounded while it is read: 64 KiB inclusive, sent in
/// chunks, is read; one byte more is refused, `unauthorized`, `not-sent` and quiet.
#[tokio::test]
async fn an_answer_of_unknown_length_is_bounded_while_it_is_read() {
    let whole = answer();
    let padded = format!("{whole}{}", " ".repeat(MAX_EXCHANGE_BYTES - whole.len()));
    let sts = Sts::answering(vec![chunked(&padded)]).await;
    let credentials = source(&sts.uri(), &TokenFile::holding(TOKEN), client())
        .credentials(&Cancel::new())
        .await
        .expect("an answer of exactly the bound");
    assert_eq!(credentials.secret_access_key(), SECRET_ACCESS_KEY);

    let sts = Sts::answering(vec![chunked(&format!("{padded} "))]).await;
    let error = source(&sts.uri(), &TokenFile::holding(TOKEN), client())
        .credentials(&Cancel::new())
        .await
        .expect_err("one byte past the bound");
    assert_eq!(error.code, ErrorCode::Unauthorized, "{error:?}");
    assert!(!error.may_retry(), "{error:?}");
    assert_quiet(&error);
}

const HTTPS_PROXY_CHILD: &str = "LLM_BEDROCK_SEC_W25_HTTPS_PROXY_CHILD";

/// The child half: an `https` endpoint on 127.0.0.1 that is a plain listener. The client's
/// first byte there must be a TLS handshake record (0x16), so the client spoke TLS to the
/// endpoint itself rather than `CONNECT` to a proxy.
fn reach_an_https_endpoint_directly() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    runtime.block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a local port");
        let address = listener.local_addr().expect("its address");
        let direct = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("a direct connection");
            let mut first = [0_u8; 1];
            stream
                .read_exact(&mut first)
                .await
                .expect("the client's first byte");
            first[0]
        });
        let error = WebIdentityCredentials::new(
            client(),
            &format!("https://{address}/"),
            ROLE_ARN,
            SESSION_NAME,
            Arc::new(TokenFile::holding(TOKEN)),
            reference(),
        )
        .expect("an https endpoint is admitted")
        .credentials(&Cancel::new())
        .await
        .expect_err("no TLS server answers");
        assert_quiet(&error);
        let first = tokio::time::timeout(Duration::from_secs(5), direct)
            .await
            .expect("the endpoint itself was reached")
            .expect("the listener task");
        assert_eq!(first, 0x16, "the first byte was not a TLS handshake record");
    });
}

/// The unit's proxy case asks an `http` endpoint, so only `HTTP_PROXY` and `ALL_PROXY` could
/// apply. The production endpoint is `https`: with `HTTPS_PROXY` and `ALL_PROXY` naming a local
/// listener and `NO_PROXY` unset, the source reaches its `https` endpoint directly and the
/// proxy hears no connection.
#[test]
fn an_https_sts_endpoint_is_reached_directly_and_an_ambient_proxy_hears_nothing() {
    if std::env::var_os(HTTPS_PROXY_CHILD).is_some() {
        reach_an_https_endpoint_directly();
        return;
    }
    let proxy = std::net::TcpListener::bind("127.0.0.1:0").expect("a local port");
    let proxy_url = format!("http://{}", proxy.local_addr().expect("its address"));
    let (seen, heard) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        while let Ok((socket, _)) = proxy.accept() {
            drop(socket);
            if seen.send(()).is_err() {
                break;
            }
        }
    });
    let mut child = std::process::Command::new(std::env::current_exe().expect("this binary"));
    child
        .args([
            "--exact",
            "an_https_sts_endpoint_is_reached_directly_and_an_ambient_proxy_hears_nothing",
        ])
        .env(HTTPS_PROXY_CHILD, "1")
        .env_remove("NO_PROXY")
        .env_remove("no_proxy");
    for variable in [
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ] {
        child.env(variable, &proxy_url);
    }
    let status = child.status().expect("the child runs");
    assert!(status.success(), "the child failed: {status}");
    assert!(
        heard.recv_timeout(Duration::from_millis(300)).is_err(),
        "an ambient proxy received a connection"
    );
}
