//! An `aws-web-identity` Bedrock endpoint of a routes document. The document names only the
//! kind; the role ARN, the session name, the STS endpoint and the token file come from the caller
//! through `SecretSources::with_web_identity`, as an `aws-container` endpoint's URI and token file
//! come through `SecretSources::with_container_credentials`. With feature `file`, turns run
//! against a loopback STS and a loopback Bedrock fixture.

use llm_core::{ErrorCode, Id, Protocol};
use llm_credentials::SecretResolver;
use llm_http::{HttpClient, Limits};
use llm_models::WebIdentityRole;
use llm_models::routes::{AwsCredentialKind, Routes, SecretKind, SecretSources};
use std::collections::BTreeMap;
use std::sync::Arc;

const ROLE_ARN: &str = "arn:aws:iam::000000000000:role/llm-fixture-role";
const SESSION_NAME: &str = "llm-fixture-session";

const YAML: &str = "format: llm.routes/1
endpoints:
  bedrock-irsa: {protocol: bedrock-converse, bedrock: {region: eu-central-1},
                 auth: {kind: aws-web-identity}}
  open: {base_url: https://open.example.invalid/v1, protocol: messages, auth: {kind: anonymous}}
routes:
  opus: bedrock-irsa/eu.anthropic.claude-opus-5-5
  local: open/claude-haiku-5-5
";

const AUTH: &str = r#"auth = { kind = "aws-web-identity" }"#;

const TOML: &str = r#"format = "llm.routes/1"

[endpoints.bedrock-irsa]
protocol = "bedrock-converse"
bedrock = { region = "eu-central-1" }
auth = { kind = "aws-web-identity" }

[endpoints.open]
base_url = "https://open.example.invalid/v1"
protocol = "messages"
auth = { kind = "anonymous" }

[routes]
opus = "bedrock-irsa/eu.anthropic.claude-opus-5-5"
local = "open/claude-haiku-5-5"
"#;

fn http() -> HttpClient {
    HttpClient::new(Limits::default()).expect("a transport")
}

fn role(sts_endpoint: &str) -> WebIdentityRole {
    WebIdentityRole::new(ROLE_ARN, SESSION_NAME, sts_endpoint)
}

#[test]
fn an_aws_web_identity_endpoint_loads_from_yaml_and_toml_naming_only_its_kind() {
    let yaml = Routes::parse(YAML).unwrap();
    let toml = Routes::parse_toml(TOML).unwrap();
    assert_eq!(yaml.digest(), toml.digest());
    for routes in [&yaml, &toml] {
        let endpoint = routes.bedrock_endpoints().next().unwrap();
        assert_eq!(endpoint.name().as_str(), "bedrock-irsa");
        assert_eq!(endpoint.credential_kind(), AwsCredentialKind::WebIdentity);
        assert_eq!(endpoint.credential_kind().as_str(), "aws-web-identity");
        assert_eq!(endpoint.command(), None);
        assert_eq!(endpoint.secret_kinds(), [] as [SecretKind; 0]);
        let opus = routes.target("opus").unwrap();
        assert_eq!(opus.protocol, Protocol::BedrockConverse);
        assert_eq!(opus.endpoint.as_str(), "bedrock-irsa");
        let catalog: Vec<_> = routes
            .catalog()
            .bindings()
            .map(|(id, _)| id.to_string())
            .collect();
        assert_eq!(catalog, ["local"]);
        assert_eq!(
            routes.binding("opus").unwrap_err().code,
            ErrorCode::Unsupported
        );
    }
    // The kind is part of the document's identity.
    let container =
        Routes::parse_toml(&TOML.replace(AUTH, r#"auth = { kind = "aws-container" }"#)).unwrap();
    assert_ne!(container.digest(), toml.digest());
    assert_eq!(
        container
            .bedrock_endpoints()
            .next()
            .unwrap()
            .credential_kind(),
        AwsCredentialKind::Container
    );
}

/// The role ARN, the session name, the STS endpoint and the token file are the caller's, never
/// document values: each is an unknown `auth` field, refused by the parser without its value.
#[test]
fn a_value_the_caller_supplies_is_refused_as_a_document_field_without_echoing_it() {
    for field in [
        "role_arn",
        "session_name",
        "sts_endpoint",
        "token_file",
        "web_identity_token_file",
        "token",
    ] {
        let toml = TOML.replace(
            AUTH,
            &format!(r#"auth = {{ kind = "aws-web-identity", {field} = "fixture-value" }}"#),
        );
        let error = Routes::parse_toml(&toml).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{field}");
        assert!(
            error
                .message
                .starts_with("invalid llm.routes/1 TOML document at line 6 column "),
            "{field}: {}",
            error.message
        );
        assert!(
            error.message.contains(&format!("unknown field `{field}`")),
            "{field}: {}",
            error.message
        );
        assert!(
            !error.message.contains("fixture-value"),
            "{field}: {}",
            error.message
        );
        let yaml = YAML.replace(
            "auth: {kind: aws-web-identity}",
            &format!("auth: {{kind: aws-web-identity, {field}: fixture-value}}"),
        );
        let error = Routes::parse(&yaml).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{field}");
        assert!(
            error
                .message
                .starts_with("invalid llm.routes/1 YAML document at line 4 column "),
            "{field}: {}",
            error.message
        );
        assert!(
            !error.message.contains("fixture-value"),
            "{field}: {}",
            error.message
        );
    }
}

/// Each refusal names the endpoint and the field by its path, and echoes no value.
#[test]
fn an_aws_web_identity_endpoint_takes_no_other_credential_source_or_auth_field() {
    let cases = [
        (
            "access_key_id",
            "an aws-web-identity endpoint cannot also name `auth.access_key_id`: a Bedrock \
             endpoint has one credential source",
        ),
        (
            "secret_access_key",
            "an aws-web-identity endpoint cannot also name `auth.secret_access_key`",
        ),
        (
            "session_token",
            "an aws-web-identity endpoint cannot also name `auth.session_token`",
        ),
        ("secret", "a Bedrock endpoint takes no `auth.secret`"),
        ("header", "a Bedrock endpoint takes no `auth.header`"),
    ];
    for (field, expected) in cases {
        let toml = TOML.replace(
            AUTH,
            &format!(r#"auth = {{ kind = "aws-web-identity", {field} = "env:FIXTURE_VALUE" }}"#),
        );
        let yaml = YAML.replace(
            "auth: {kind: aws-web-identity}",
            &format!("auth: {{kind: aws-web-identity, {field}: 'env:FIXTURE_VALUE'}}"),
        );
        for error in [
            Routes::parse_toml(&toml).unwrap_err(),
            Routes::parse(&yaml).unwrap_err(),
        ] {
            assert_eq!(error.code, ErrorCode::InvalidRequest, "{field}");
            assert!(
                error.message.starts_with("endpoint `bedrock-irsa`: "),
                "{field}: {}",
                error.message
            );
            assert!(
                error.message.contains(expected),
                "{field}: {}",
                error.message
            );
            assert!(!error.message.contains("FIXTURE"), "{}", error.message);
        }
    }
    let error = Routes::parse_toml(&TOML.replace(
        AUTH,
        r#"auth = { kind = "aws-web-identity", command = ["fixture-value"] }"#,
    ))
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert_eq!(
        error.message,
        "endpoint `bedrock-irsa`: only an aws-process endpoint takes `auth.command`"
    );
}

#[test]
fn only_a_bedrock_endpoint_takes_aws_web_identity_and_every_refusal_lists_it() {
    let http_endpoint = TOML.replace(
        r#"auth = { kind = "anonymous" }"#,
        r#"auth = { kind = "aws-web-identity" }"#,
    );
    let error = Routes::parse_toml(&http_endpoint).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert_eq!(
        error.message,
        "endpoint `open`: only a Bedrock endpoint (one with a `bedrock` table) takes `auth.kind` \
         aws-keys, aws-container, aws-process or aws-web-identity"
    );
    let unknown =
        Routes::parse_toml(&TOML.replace(AUTH, r#"auth = { kind = "aws-sso" }"#)).unwrap_err();
    assert_eq!(unknown.code, ErrorCode::InvalidRequest);
    assert_eq!(
        unknown.message,
        "endpoint `bedrock-irsa`: a Bedrock endpoint's `auth.kind` must be aws-keys, \
         aws-container, aws-process or aws-web-identity"
    );
}

/// Without the caller's web identity values the endpoint gets no resolver, and only its routes
/// are refused, `unauthorized` and by the endpoint's name; a lookup of the caller's own that
/// supplies a resolver but no role is refused the same way.
#[test]
fn without_the_callers_web_identity_only_its_route_is_refused() {
    let routes = Routes::parse_toml(TOML).unwrap();
    let resolvers = routes.resolvers(&SecretSources::new()).unwrap();
    assert_eq!(resolvers.accounts().count(), 0);
    let error = routes.port("opus", http(), &resolvers).err().unwrap();
    assert_eq!(error.code, ErrorCode::Unauthorized);
    assert_eq!(
        error.message,
        "endpoint `bedrock-irsa`: an `aws-web-identity` endpoint needs the web identity role and \
         its token file, and none was supplied"
    );
    let port = routes.port("local", http(), &resolvers).unwrap();
    assert_eq!(port.protocol(), Protocol::Messages);

    let mut own: BTreeMap<Id, Arc<dyn SecretResolver>> = BTreeMap::new();
    own.insert(
        Id::new("bedrock-irsa").unwrap(),
        Arc::new(llm_credentials::AnonymousResolver),
    );
    let error = routes.port("opus", http(), &own).err().unwrap();
    assert_eq!(error.code, ErrorCode::Unauthorized);
    assert_eq!(
        error.message,
        "no web identity role is supplied for account `bedrock-irsa`"
    );
}

#[cfg(not(feature = "file"))]
#[test]
fn without_feature_file_a_web_identity_route_is_refused_by_name() {
    let routes = Routes::parse_toml(TOML).unwrap();
    let sources = SecretSources::new().with_web_identity(
        role("https://sts.eu-central-1.amazonaws.com"),
        "/var/fixture/token",
    );
    let resolvers = routes.resolvers(&sources).unwrap();
    let error = routes.port("opus", http(), &resolvers).err().unwrap();
    assert_eq!(error.code, ErrorCode::Unauthorized);
    assert_eq!(
        error.message,
        "endpoint `bedrock-irsa`: `file:` secrets need feature `file` of llm-models, which this \
         build does not enable"
    );
    routes.port("local", http(), &resolvers).unwrap();
}

/// Turns against a loopback STS and a loopback Bedrock, the token in a file the `file` source
/// reads.
#[cfg(feature = "file")]
mod turns {
    use super::{ROLE_ARN, SESSION_NAME, TOML, http, role};
    use llm_bedrock::eventstream::{HeaderValue, Message, encode_message};
    use llm_core::{Cancel, Error, ErrorCode, Item, Model, Protocol, TurnRequest, VecSink};
    use llm_models::WebIdentityRole;
    use llm_models::routes::{Routes, SecretSources};
    use serde_json::json;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    const WEB_IDENTITY_TOKEN: &str =
        "llm-fixture-web-identity-token.fixture-claims.fixture-signature";
    const ACCESS_KEY_ID: &str = "ASIAROUTESIRSAFIXTURE";
    const SECRET_ACCESS_KEY: &str = "routes/irsa+fixture-secret";
    const SESSION_TOKEN: &str = "routes-irsa-fixture-session-token";
    /// A marker only an STS error message carries.
    const STS_MESSAGE: &str = "llm-fixture-sts-message-private";

    /// Every secret value or answer text a case uses: none may reach a message or a `Debug`
    /// form.
    const SECRETS: [&str; 5] = [
        WEB_IDENTITY_TOKEN,
        ACCESS_KEY_ID,
        SECRET_ACCESS_KEY,
        SESSION_TOKEN,
        STS_MESSAGE,
    ];

    /// The TOML document with `bedrock-irsa` sending to `origin` instead of its Region's host.
    fn document(origin: &str) -> String {
        TOML.replace(
            "[endpoints.bedrock-irsa]\n",
            &format!("[endpoints.bedrock-irsa]\nbase_url = \"{origin}\"\n"),
        )
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

    /// Which secret value or answer text `text` carries, if any.
    fn leak(text: &str) -> Option<&'static str> {
        SECRETS.into_iter().find(|secret| text.contains(secret))
    }

    /// One request as a fixture read it: the request line, the headers lowercased, the body.
    #[derive(Clone, Debug)]
    struct Request {
        line: String,
        headers: Vec<(String, String)>,
        body: String,
    }

    impl Request {
        fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
        }
    }

    fn read_request(socket: &mut std::net::TcpStream) -> Option<Request> {
        let mut reader = BufReader::new(socket);
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        let mut headers = Vec::new();
        loop {
            let mut header = String::new();
            reader.read_line(&mut header).ok()?;
            let header = header.trim_end();
            if header.is_empty() {
                break;
            }
            let (name, value) = header.split_once(':')?;
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
        let length = headers
            .iter()
            .find(|(name, _)| name == "content-length")
            .map_or(0, |(_, value)| value.parse::<usize>().unwrap_or(0));
        let mut body = vec![0; length];
        reader.read_exact(&mut body).ok()?;
        Some(Request {
            line: line.trim_end().to_owned(),
            headers,
            body: String::from_utf8_lossy(&body).into_owned(),
        })
    }

    /// A loopback server answering every connection with the same bytes, keeping each request.
    struct Fixture {
        origin: String,
        requests: Arc<Mutex<Vec<Request>>>,
    }

    impl Fixture {
        fn answering(answer: Vec<u8>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
            let origin = format!("http://{}", listener.local_addr().expect("an address"));
            let requests = Arc::new(Mutex::new(Vec::new()));
            let kept = Arc::clone(&requests);
            std::thread::spawn(move || {
                for socket in listener.incoming() {
                    let Ok(mut socket) = socket else { return };
                    if let Some(request) = read_request(&mut socket) {
                        kept.lock().expect("the requests").push(request);
                    }
                    let _ = socket.write_all(&answer);
                    let _ = socket.flush();
                }
            });
            Self { origin, requests }
        }

        fn requests(&self) -> Vec<Request> {
            self.requests.lock().expect("the requests").clone()
        }
    }

    fn sts_response(status: &str, body: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 {status}\r\ncontent-type: text/xml\r\ncontent-length: {length}\r\n\
             connection: close\r\n\r\n{body}",
            length = body.len()
        )
        .into_bytes()
    }

    /// An `AssumeRoleWithWebIdentity` answer whose credentials expire long after the test.
    fn sts_success() -> Vec<u8> {
        sts_response(
            "200 OK",
            &format!(
                r#"<AssumeRoleWithWebIdentityResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/">
  <AssumeRoleWithWebIdentityResult>
    <Credentials>
      <SessionToken>{SESSION_TOKEN}</SessionToken>
      <SecretAccessKey>{SECRET_ACCESS_KEY}</SecretAccessKey>
      <Expiration>2099-01-01T00:00:00Z</Expiration>
      <AccessKeyId>{ACCESS_KEY_ID}</AccessKeyId>
    </Credentials>
  </AssumeRoleWithWebIdentityResult>
</AssumeRoleWithWebIdentityResponse>
"#
            ),
        )
    }

    /// An STS refusal whose message quotes the token, as a careless identity provider might.
    fn sts_refusal() -> Vec<u8> {
        sts_response(
            "400 Bad Request",
            &format!(
                r#"<ErrorResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/">
  <Error>
    <Type>Sender</Type>
    <Code>InvalidIdentityToken</Code>
    <Message>{STS_MESSAGE} {WEB_IDENTITY_TOKEN}</Message>
  </Error>
</ErrorResponse>
"#
            ),
        )
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

    /// The port is built before the token file exists: building reads nothing, and the first
    /// turn reads the token, asks STS for the caller's role and signs with the answer; a second
    /// turn uses the kept credentials.
    #[test]
    fn a_turn_assumes_the_callers_role_with_the_token_file_and_signs_with_the_answer() {
        let sts = Fixture::answering(sts_success());
        let bedrock = Fixture::answering(converse_stream());
        let directory = tempfile::tempdir().unwrap();
        let token_file = directory.path().join("token");
        let routes = Routes::parse_toml(&document(&bedrock.origin)).unwrap();
        let sources =
            SecretSources::new().with_web_identity(role(&format!("{}/", sts.origin)), &token_file);
        let resolvers = routes.resolvers(&sources).unwrap();
        let accounts: Vec<_> = resolvers.accounts().map(ToString::to_string).collect();
        assert_eq!(accounts, ["bedrock-irsa"]);
        runtime().block_on(async {
            let port = routes.port("opus", http(), &resolvers).expect("a port");
            assert!(port.resolver().is_some());
            assert_eq!(port.protocol(), Protocol::BedrockConverse);
            assert_eq!(port.provenance().endpoint.as_str(), "bedrock-irsa");
            assert!(sts.requests().is_empty(), "building the port sends nothing");
            std::fs::write(&token_file, format!("{WEB_IDENTITY_TOKEN}\n")).unwrap();
            assert_eq!(turn(&port).await.expect("the first turn"), "signed");
            assert_eq!(turn(&port).await.expect("the second turn"), "signed");
        });
        let asked = sts.requests();
        assert_eq!(asked.len(), 1, "the second turn uses the kept credentials");
        assert_eq!(asked[0].line, "POST / HTTP/1.1");
        assert_eq!(
            asked[0].body,
            format!(
                "Action=AssumeRoleWithWebIdentity&Version=2011-06-15\
                 &RoleArn=arn%3Aaws%3Aiam%3A%3A000000000000%3Arole%2Fllm-fixture-role\
                 &RoleSessionName={SESSION_NAME}&WebIdentityToken={WEB_IDENTITY_TOKEN}"
            )
        );
        assert_eq!(asked[0].header("authorization"), None);
        assert!(
            asked[0]
                .headers
                .iter()
                .all(|(name, _)| !name.starts_with("x-amz-")),
            "{:?}",
            asked[0].headers
        );
        let sent = bedrock.requests();
        assert_eq!(sent.len(), 2);
        for request in &sent {
            assert_eq!(
                request.line,
                "POST /model/eu.anthropic.claude-opus-5-5/converse-stream HTTP/1.1"
            );
            let authorization = request.header("authorization").expect("signed");
            assert!(
                authorization.starts_with(&format!("AWS4-HMAC-SHA256 Credential={ACCESS_KEY_ID}/")),
                "{authorization}"
            );
            assert!(
                authorization.contains("/eu-central-1/bedrock/aws4_request"),
                "{authorization}"
            );
            assert_eq!(request.header("x-amz-security-token"), Some(SESSION_TOKEN));
        }
    }

    /// A token file the file source refuses, a role ARN, a session name or an STS endpoint
    /// `WebIdentityCredentials` refuses: each refuses only the web identity route,
    /// `unauthorized`, quoting none of the values, and sends nothing.
    #[test]
    fn a_web_identity_source_that_cannot_be_built_refuses_only_its_own_route() {
        let sts = Fixture::answering(sts_success());
        let bedrock = Fixture::answering(converse_stream());
        let sts_endpoint = format!("{}/", sts.origin);
        let directory = tempfile::tempdir().unwrap();
        let token_file = directory.path().join("token");
        let routes = Routes::parse_toml(&document(&bedrock.origin)).unwrap();
        let cases = [
            (
                SecretSources::new()
                    .with_web_identity(role(&sts_endpoint), "relative/fixture-token"),
                "endpoint `bedrock-irsa`: its web identity token file is not an absolute path \
                 without `.` or `..`",
            ),
            (
                SecretSources::new().with_web_identity(
                    WebIdentityRole::new("fixture-role-value", SESSION_NAME, &sts_endpoint),
                    &token_file,
                ),
                "web identity role ARN is not one STS takes",
            ),
            (
                SecretSources::new().with_web_identity(
                    WebIdentityRole::new(ROLE_ARN, "fixture session value", &sts_endpoint),
                    &token_file,
                ),
                "web identity session name is not one STS takes",
            ),
            (
                SecretSources::new().with_web_identity(
                    WebIdentityRole::new(
                        ROLE_ARN,
                        SESSION_NAME,
                        "http://fixture-sts.example.invalid/",
                    ),
                    &token_file,
                ),
                "web identity STS endpoint is not https or http to a loopback address",
            ),
        ];
        for (sources, expected) in cases {
            let resolvers = routes.resolvers(&sources).unwrap();
            let error = routes.port("opus", http(), &resolvers).err().unwrap();
            assert_eq!(error.code, ErrorCode::Unauthorized, "{expected}");
            assert!(error.message.starts_with(expected), "{}", error.message);
            assert!(!error.message.contains("fixture"), "{}", error.message);
            routes.port("local", http(), &resolvers).unwrap();
        }
        assert!(sts.requests().is_empty());
        assert!(bedrock.requests().is_empty());
    }

    /// The token, the keys and STS's message stay out of every `Debug` form and every refusal: a
    /// token that is not one, and an STS refusal quoting the token, each refuse the turn
    /// `unauthorized` before Bedrock hears anything.
    #[test]
    fn the_token_never_appears_in_a_debug_form_or_an_error() {
        let refusing = Fixture::answering(sts_refusal());
        let bedrock = Fixture::answering(converse_stream());
        let directory = tempfile::tempdir().unwrap();
        let token_file = directory.path().join("token");
        std::fs::write(&token_file, WEB_IDENTITY_TOKEN).unwrap();
        let routes = Routes::parse_toml(&document(&bedrock.origin)).unwrap();
        let role = role(&format!("{}/", refusing.origin));
        let sources = SecretSources::new().with_web_identity(role.clone(), &token_file);
        let resolvers = routes.resolvers(&sources).unwrap();
        let path = token_file.display().to_string();
        for text in [
            format!("{role:?}"),
            format!("{sources:?}"),
            format!("{resolvers:?}"),
            format!("{routes:?}"),
        ] {
            assert_eq!(leak(&text), None, "{text}");
            assert!(!text.contains(&path), "{text}");
        }
        assert!(
            format!("{sources:?}").contains("web_identity: true"),
            "{sources:?}"
        );
        assert!(
            format!("{resolvers:?}").contains("web_identity: true"),
            "{resolvers:?}"
        );
        runtime().block_on(async {
            let port = routes.port("opus", http(), &resolvers).expect("a port");
            let error = turn(&port).await.unwrap_err();
            assert_eq!(error.code, ErrorCode::Unauthorized, "{error:?}");
            assert!(
                error.message.contains("InvalidIdentityToken"),
                "{}",
                error.message
            );
            for text in [error.message.clone(), format!("{error:?}")] {
                assert_eq!(leak(&text), None, "{text}");
            }

            std::fs::write(
                &token_file,
                format!("{WEB_IDENTITY_TOKEN} {WEB_IDENTITY_TOKEN}"),
            )
            .unwrap();
            let port = routes.port("opus", http(), &resolvers).expect("a port");
            let error = turn(&port).await.unwrap_err();
            assert_eq!(error.code, ErrorCode::Unauthorized, "{error:?}");
            for text in [error.message.clone(), format!("{error:?}")] {
                assert_eq!(leak(&text), None, "{text}");
                assert!(!text.contains(&path), "{text}");
            }
        });
        assert_eq!(
            refusing.requests().len(),
            1,
            "a malformed token is never sent"
        );
        assert!(bedrock.requests().is_empty());
    }
}
