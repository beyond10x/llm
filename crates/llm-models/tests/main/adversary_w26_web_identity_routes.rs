//! Review cases for an `aws-web-identity` routes endpoint, wave 2026-11-05-w26: where a document
//! may not carry the caller's values, which source serves the route when the caller supplies
//! another one, and how the token file a turn reads through the platform-mounted file source
//! behaves. Every file a case writes lives under the build's temporary directory.

use llm_core::{ErrorCode, Id};
use llm_credentials::SecretResolver;
use llm_http::{HttpClient, Limits};
use llm_models::routes::{Routes, SecretSources};
use llm_models::{Resolvers, WebIdentityRole};
use std::sync::Arc;

const ROLE_ARN: &str = "arn:aws:iam::000000000000:role/llm-fixture-role";
const SESSION_NAME: &str = "llm-fixture-session";
/// A marker only a value the caller supplies carries.
const MARKER: &str = "review-private-marker";

const YAML: &str = "format: llm.routes/1
endpoints:
  bedrock-irsa: {protocol: bedrock-converse, bedrock: {region: eu-central-1},
                 auth: {kind: aws-web-identity}}
routes:
  opus: bedrock-irsa/eu.anthropic.claude-opus-5-5
";

const TOML: &str = r#"format = "llm.routes/1"

[endpoints.bedrock-irsa]
protocol = "bedrock-converse"
bedrock = { region = "eu-central-1" }
auth = { kind = "aws-web-identity" }

[routes]
opus = "bedrock-irsa/eu.anthropic.claude-opus-5-5"
"#;

/// The refusal of a web identity route whose resolvers were built without a web identity role.
const REFUSED: &str = "endpoint `bedrock-irsa`: an `aws-web-identity` endpoint needs the web \
                       identity role and its token file, and none was supplied";

fn http() -> HttpClient {
    HttpClient::new(Limits::default()).expect("a transport")
}

fn role(sts_endpoint: &str) -> WebIdentityRole {
    WebIdentityRole::new(ROLE_ARN, SESSION_NAME, sts_endpoint)
}

/// A caller's own lookup that has a resolver for every account and a container credentials
/// endpoint, and no web identity role.
struct ContainerOnly(Arc<dyn SecretResolver>);

impl Resolvers for ContainerOnly {
    fn resolver(&self, _account_id: &Id) -> Option<Arc<dyn SecretResolver>> {
        Some(Arc::clone(&self.0))
    }

    fn container_credentials_uri(&self) -> Option<&str> {
        Some("http://127.0.0.1:9/v1/credentials")
    }
}

/// The role ARN, the session name, the STS endpoint and the token file are refused beside
/// `protocol` and inside the `bedrock` table too, not only in `auth`, and no refusal quotes the
/// value.
#[test]
fn a_caller_value_is_refused_wherever_a_document_could_carry_it() {
    let values = [
        (
            "role_arn",
            format!("arn:aws:iam::000000000000:role/{MARKER}"),
        ),
        ("session_name", MARKER.to_owned()),
        ("sts_endpoint", format!("https://{MARKER}.example.invalid")),
        ("token_file", format!("/var/run/{MARKER}/token")),
        (
            "web_identity_token_file",
            format!("/var/run/{MARKER}/token"),
        ),
    ];
    for (field, value) in &values {
        let documents = [
            (
                "TOML",
                Routes::parse_toml(&TOML.replace(
                    "protocol = \"bedrock-converse\"\n",
                    &format!("protocol = \"bedrock-converse\"\n{field} = \"{value}\"\n"),
                )),
            ),
            (
                "TOML",
                Routes::parse_toml(&TOML.replace(
                    r#"bedrock = { region = "eu-central-1" }"#,
                    &format!(r#"bedrock = {{ region = "eu-central-1", {field} = "{value}" }}"#),
                )),
            ),
            (
                "YAML",
                Routes::parse(&YAML.replace(
                    "auth: {kind: aws-web-identity}}",
                    &format!("auth: {{kind: aws-web-identity}}, {field}: '{value}'}}"),
                )),
            ),
            (
                "YAML",
                Routes::parse(&YAML.replace(
                    "bedrock: {region: eu-central-1}",
                    &format!("bedrock: {{region: eu-central-1, {field}: '{value}'}}"),
                )),
            ),
        ];
        for (syntax, parsed) in documents {
            let error = parsed
                .err()
                .unwrap_or_else(|| panic!("{syntax} {field} is refused"));
            assert_eq!(error.code, ErrorCode::InvalidRequest, "{syntax} {field}");
            assert!(
                error
                    .message
                    .starts_with(&format!("invalid llm.routes/1 {syntax} document at line ")),
                "{syntax} {field}: {}",
                error.message
            );
            assert!(
                !error.message.contains(MARKER),
                "{syntax} {field}: {}",
                error.message
            );
        }
    }
}

/// As `WebIdentityRole` documents it: its `Debug` form shows the role ARN and the session name,
/// never the STS endpoint.
#[test]
fn the_role_debug_form_shows_the_role_and_the_session_and_never_the_sts_endpoint() {
    let endpoint = format!("https://{MARKER}.example.invalid/");
    let supplied = role(&endpoint);
    let text = format!("{supplied:?}");
    assert!(text.contains(ROLE_ARN), "{text}");
    assert!(text.contains(SESSION_NAME), "{text}");
    assert!(!text.contains(MARKER), "{text}");
    assert_eq!(supplied.sts_endpoint(), endpoint);
}

/// A caller's lookup with a resolver and a container credentials endpoint but no role, and
/// resolvers built from a container source alone: either way the web identity route, by its
/// name and reached directly, is refused `unauthorized` when its port is built, so no other
/// source can serve it.
#[test]
fn without_a_web_identity_role_a_container_source_never_serves_the_route() {
    let routes = Routes::parse_toml(TOML).unwrap();
    let own = ContainerOnly(Arc::new(llm_credentials::AnonymousResolver));
    let built = routes
        .resolvers(&SecretSources::new().with_container_credentials(
            "http://127.0.0.1:9/v1/credentials",
            "/var/fixture/container-token",
        ))
        .unwrap();
    for name in ["opus", "bedrock-irsa/eu.anthropic.claude-opus-5-5"] {
        let error = routes.port(name, http(), &own).err().expect("refused");
        assert_eq!(error.code, ErrorCode::Unauthorized, "{name}");
        assert_eq!(
            error.message, "no web identity role is supplied for account `bedrock-irsa`",
            "{name}"
        );
        let error = routes.port(name, http(), &built).err().expect("refused");
        assert_eq!(error.code, ErrorCode::Unauthorized, "{name}");
        assert_eq!(error.message, REFUSED, "{name}");
    }
}

/// Turns against loopback STS, container credentials and Bedrock fixtures, the tokens in real
/// files the `file` source reads.
#[cfg(all(feature = "file", target_os = "linux"))]
mod turns {
    use super::{REFUSED, http, role};
    use llm_bedrock::eventstream::{HeaderValue, Message, encode_message};
    use llm_core::{Cancel, Error, ErrorCode, Item, Model, TurnRequest, VecSink};
    use llm_models::routes::{Routes, SecretSources};
    use serde_json::json;
    use std::fs::{self, Permissions};
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

    const IRSA_KEY_ID: &str = "ASIAREVIEWIRSAFIXTURE";
    const CONTAINER_KEY_ID: &str = "ASIAREVIEWPODSFIXTURE";
    const FIRST_TOKEN: &str = "review-web-identity-token-first";
    const SECOND_TOKEN: &str = "review-web-identity-token-rotated";
    const CONTAINER_TOKEN: &str = "review-container-authorization-token";
    const FAR: &str = "2099-01-01T00:00:00Z";

    /// Two Bedrock endpoints sending to the `bedrock` origin: `bedrock-irsa` on web identity,
    /// `bedrock-pod` on container credentials.
    fn document(bedrock: &str) -> String {
        format!(
            r#"format = "llm.routes/1"

[endpoints.bedrock-irsa]
base_url = "{bedrock}"
protocol = "bedrock-converse"
bedrock = {{ region = "eu-central-1" }}
auth = {{ kind = "aws-web-identity" }}

[endpoints.bedrock-pod]
base_url = "{bedrock}"
protocol = "bedrock-converse"
bedrock = {{ region = "eu-central-1" }}
auth = {{ kind = "aws-container" }}

[routes]
opus = "bedrock-irsa/eu.anthropic.claude-opus-5-5"
pod = "bedrock-pod/eu.anthropic.claude-opus-5-5"
"#
        )
    }

    fn scratch() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("w26-review-")
            .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
            .expect("a directory under the build's temporary directory")
    }

    fn write_token(path: &Path, content: &str, mode: u32) {
        fs::write(path, content).expect("the token file");
        fs::set_permissions(path, Permissions::from_mode(mode)).expect("its mode");
    }

    /// One timestamped directory of a projected volume, holding `token` as the kubelet writes
    /// it.
    fn project(volume: &Path, name: &str, token: &str) {
        let directory = volume.join(name);
        fs::create_dir(&directory).expect("a timestamped directory");
        fs::set_permissions(&directory, Permissions::from_mode(0o755)).expect("its mode");
        write_token(&directory.join("token"), token, 0o644);
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime")
    }

    async fn turn(model: &dyn Model, name: &str) -> Result<String, Error> {
        let request = TurnRequest::new(name, vec![Item::user("Say hello")]);
        let mut sink = VecSink::new(64, 64 * 1024);
        model.turn(&request, &mut sink, &Cancel::new()).await?;
        Ok(sink.text())
    }

    /// One request as a fixture read it: the request line, the headers lowercased, the body.
    #[derive(Clone)]
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

        /// Whether `text` appears anywhere in the request.
        fn carries(&self, text: &str) -> bool {
            self.line.contains(text)
                || self.body.contains(text)
                || self.headers.iter().any(|(_, value)| value.contains(text))
        }

        /// The `WebIdentityToken` form value; the tokens here need no percent-encoding.
        fn web_identity_token(&self) -> Option<&str> {
            self.body
                .split('&')
                .find_map(|pair| pair.strip_prefix("WebIdentityToken="))
        }
    }

    fn signed_by(request: &Request, key_id: &str) -> bool {
        request.header("authorization").is_some_and(|value| {
            value.starts_with(&format!("AWS4-HMAC-SHA256 Credential={key_id}/"))
        })
    }

    fn read_request(socket: &mut TcpStream) -> Option<Request> {
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

    /// A loopback server answering each connection with what `answer` writes at that moment,
    /// keeping each request and saying on the returned channel that one arrived.
    struct Server {
        origin: String,
        requests: Arc<Mutex<Vec<Request>>>,
    }

    impl Server {
        fn answering(
            answer: impl Fn() -> Vec<u8> + Send + 'static,
        ) -> (Self, UnboundedReceiver<()>) {
            let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
            let origin = format!("http://{}", listener.local_addr().expect("an address"));
            let requests = Arc::new(Mutex::new(Vec::new()));
            let kept = Arc::clone(&requests);
            let (heard, hearing) = unbounded_channel();
            std::thread::spawn(move || {
                for socket in listener.incoming() {
                    let Ok(mut socket) = socket else { return };
                    if let Some(request) = read_request(&mut socket) {
                        kept.lock().expect("the requests").push(request);
                        let _ = heard.send(());
                    }
                    let _ = socket.write_all(&answer());
                    let _ = socket.flush();
                }
            });
            (Self { origin, requests }, hearing)
        }

        fn requests(&self) -> Vec<Request> {
            self.requests.lock().expect("the requests").clone()
        }
    }

    fn respond(content_type: &str, body: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\ncontent-length: {length}\r\n\
             connection: close\r\n\r\n{body}",
            length = body.len()
        )
        .into_bytes()
    }

    fn sts_answer(key_id: &str, expires: &str) -> Vec<u8> {
        respond(
            "text/xml",
            &format!(
                r#"<AssumeRoleWithWebIdentityResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/">
  <AssumeRoleWithWebIdentityResult>
    <Credentials>
      <AccessKeyId>{key_id}</AccessKeyId>
      <SecretAccessKey>review-irsa-fixture-secret</SecretAccessKey>
      <SessionToken>review-irsa-fixture-session</SessionToken>
      <Expiration>{expires}</Expiration>
    </Credentials>
  </AssumeRoleWithWebIdentityResult>
</AssumeRoleWithWebIdentityResponse>
"#
            ),
        )
    }

    fn container_answer() -> Vec<u8> {
        respond(
            "application/json",
            &json!({
                "AccessKeyId": CONTAINER_KEY_ID,
                "SecretAccessKey": "review-pod-fixture-secret",
                "Token": "review-pod-fixture-session",
                "Expiration": FAR,
            })
            .to_string(),
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

    /// With a container source alone the web identity route is refused while the container
    /// route builds; with both, each route's turn reads only its own token file, asks only its
    /// own endpoint and signs with what that endpoint answered, and neither token reaches
    /// Bedrock.
    #[test]
    fn each_source_serves_only_its_own_endpoint_and_none_stands_in_for_the_other() {
        let (sts, _) = Server::answering(|| sts_answer(IRSA_KEY_ID, FAR));
        let (container_endpoint, _) = Server::answering(container_answer);
        let (bedrock, _) = Server::answering(converse_stream);
        let directory = scratch();
        let irsa_token = directory.path().join("irsa-token");
        let container_token = directory.path().join("container-token");
        write_token(&irsa_token, FIRST_TOKEN, 0o600);
        write_token(&container_token, CONTAINER_TOKEN, 0o600);
        let routes = Routes::parse_toml(&document(&bedrock.origin)).unwrap();
        let uri = format!("{}/v1/credentials", container_endpoint.origin);
        let container = SecretSources::new()
            .with_container_credentials(uri.as_str(), container_token.as_path());
        let resolvers = routes.resolvers(&container).unwrap();
        let error = routes
            .port("opus", http(), &resolvers)
            .err()
            .expect("refused");
        assert_eq!(error.code, ErrorCode::Unauthorized);
        assert_eq!(error.message, REFUSED);
        routes
            .port("pod", http(), &resolvers)
            .expect("the container route builds");

        let both =
            container.with_web_identity(role(&format!("{}/", sts.origin)), irsa_token.as_path());
        let resolvers = routes.resolvers(&both).unwrap();
        runtime().block_on(async {
            let port = routes
                .port("opus", http(), &resolvers)
                .expect("a web identity port");
            assert_eq!(
                turn(&port, "opus").await.expect("a web identity turn"),
                "signed"
            );
            let port = routes
                .port("pod", http(), &resolvers)
                .expect("a container port");
            assert_eq!(
                turn(&port, "pod").await.expect("a container turn"),
                "signed"
            );
        });
        let asked = sts.requests();
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].web_identity_token(), Some(FIRST_TOKEN));
        assert!(!asked[0].carries(CONTAINER_TOKEN));
        let fetched = container_endpoint.requests();
        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0].header("authorization"), Some(CONTAINER_TOKEN));
        assert!(!fetched[0].carries(FIRST_TOKEN));
        let signed = bedrock.requests();
        assert_eq!(signed.len(), 2);
        assert!(
            signed_by(&signed[0], IRSA_KEY_ID),
            "{:?}",
            signed[0].header("authorization")
        );
        assert!(
            signed_by(&signed[1], CONTAINER_KEY_ID),
            "{:?}",
            signed[1].header("authorization")
        );
        for request in &signed {
            assert!(!request.carries(FIRST_TOKEN) && !request.carries(CONTAINER_TOKEN));
        }
    }

    /// A projected volume as the kubelet writes it, `token` linking to `..data/token` and
    /// `..data` to a timestamped directory: credentials STS answers inside the refresh window
    /// are kept for the turn after the kubelet swaps `..data`, and the refresh that turn starts
    /// reads the rotated token, not the one the first fetch read.
    #[test]
    fn the_token_is_read_again_at_each_fetch_through_a_projected_volume_rotation() {
        let (sts, mut heard) = Server::answering(|| {
            sts_answer(
                IRSA_KEY_ID,
                &rfc3339(SystemTime::now() + Duration::from_secs(60)),
            )
        });
        let (bedrock, _) = Server::answering(converse_stream);
        let directory = scratch();
        let volume = directory.path();
        project(volume, "..2026_11_05_01", FIRST_TOKEN);
        symlink("..2026_11_05_01", volume.join("..data")).unwrap();
        symlink("..data/token", volume.join("token")).unwrap();
        let routes = Routes::parse_toml(&document(&bedrock.origin)).unwrap();
        let sources = SecretSources::new()
            .with_web_identity(role(&format!("{}/", sts.origin)), volume.join("token"));
        let resolvers = routes.resolvers(&sources).unwrap();
        runtime().block_on(async {
            let port = routes.port("opus", http(), &resolvers).expect("a port");
            assert_eq!(turn(&port, "opus").await.expect("the first turn"), "signed");
            project(volume, "..2026_11_05_02", SECOND_TOKEN);
            symlink("..2026_11_05_02", volume.join("..data_tmp")).unwrap();
            fs::rename(volume.join("..data_tmp"), volume.join("..data")).unwrap();
            assert_eq!(
                turn(&port, "opus")
                    .await
                    .expect("the second turn, on the kept credentials"),
                "signed"
            );
            for _ in 0..2 {
                tokio::time::timeout(Duration::from_secs(5), heard.recv())
                    .await
                    .expect("STS is asked again inside the refresh window")
                    .expect("the fixture runs");
            }
        });
        let tokens: Vec<_> = sts
            .requests()
            .iter()
            .map(|request| request.web_identity_token().map(str::to_owned))
            .collect();
        assert_eq!(
            tokens,
            [Some(FIRST_TOKEN.to_owned()), Some(SECOND_TOKEN.to_owned())]
        );
        assert_eq!(bedrock.requests().len(), 2);
    }

    /// A real token file through the file source: one trailing `\n` or `\r\n` is removed and
    /// the rest is sent as written; a second line end, a lone `\r`, a leading space, a blank
    /// line or an empty file is refused `unauthorized` before STS hears anything, naming neither
    /// the path nor the token.
    #[test]
    fn one_line_end_is_removed_and_a_blank_or_padded_token_file_never_reaches_sts() {
        let (sts, _) = Server::answering(|| sts_answer(IRSA_KEY_ID, FAR));
        let (bedrock, _) = Server::answering(converse_stream);
        let directory = scratch();
        let token_file = directory.path().join("token");
        let path = token_file.display().to_string();
        let routes = Routes::parse_toml(&document(&bedrock.origin)).unwrap();
        let sources =
            SecretSources::new().with_web_identity(role(&format!("{}/", sts.origin)), &token_file);
        let resolvers = routes.resolvers(&sources).unwrap();
        let cases = [
            (format!("{FIRST_TOKEN}\r\n"), true),
            (format!("{FIRST_TOKEN}\n"), true),
            (FIRST_TOKEN.to_owned(), true),
            (format!("{FIRST_TOKEN}\n\n"), false),
            (format!("{FIRST_TOKEN}\r"), false),
            (format!(" {FIRST_TOKEN}"), false),
            ("\n".to_owned(), false),
            (String::new(), false),
        ];
        runtime().block_on(async {
            for (content, sent) in &cases {
                write_token(&token_file, content, 0o600);
                let before = sts.requests().len();
                let port = routes.port("opus", http(), &resolvers).expect("a port");
                let answered = turn(&port, "opus").await;
                let asked = sts.requests();
                if *sent {
                    assert_eq!(answered.expect("a turn"), "signed", "{content:?}");
                    assert_eq!(asked.len(), before + 1, "{content:?}");
                    assert_eq!(
                        asked[before].web_identity_token(),
                        Some(FIRST_TOKEN),
                        "{content:?}"
                    );
                } else {
                    let error = answered.expect_err("refused");
                    assert_eq!(error.code, ErrorCode::Unauthorized, "{content:?}");
                    assert!(
                        error.message.contains("is not a web identity token"),
                        "{content:?}: {}",
                        error.message
                    );
                    assert!(
                        !error.message.contains(&path) && !error.message.contains(FIRST_TOKEN),
                        "{}",
                        error.message
                    );
                    assert_eq!(asked.len(), before, "nothing is sent for {content:?}");
                }
            }
        });
        assert!(
            bedrock
                .requests()
                .iter()
                .all(|request| !request.carries(FIRST_TOKEN))
        );
    }

    /// A token file that is missing, that others can write, or that is reached through a link
    /// in a directory others can write is refused `unauthorized` at the turn, naming neither the
    /// path nor the token, and STS hears nothing; a fetch after the file appears reads it.
    #[test]
    fn a_missing_or_unprotected_token_file_never_reaches_sts_and_a_later_fetch_reads_it() {
        let (sts, _) = Server::answering(|| sts_answer(IRSA_KEY_ID, FAR));
        let (bedrock, _) = Server::answering(converse_stream);
        let directory = scratch();
        let scratch_path = directory.path().display().to_string();
        let token_file = directory.path().join("token");
        let open = directory.path().join("open");
        fs::create_dir(&open).unwrap();
        fs::set_permissions(&open, Permissions::from_mode(0o777)).unwrap();
        symlink(&token_file, open.join("token")).unwrap();
        let routes = Routes::parse_toml(&document(&bedrock.origin)).unwrap();
        let sts_endpoint = format!("{}/", sts.origin);
        let port_on = |path: &Path| {
            let sources = SecretSources::new().with_web_identity(role(&sts_endpoint), path);
            let resolvers = routes.resolvers(&sources).unwrap();
            routes.port("opus", http(), &resolvers).expect("a port")
        };
        let refused = |error: &Error, reason: &str| {
            assert_eq!(error.code, ErrorCode::Unauthorized, "{}", error.message);
            assert!(error.message.contains(reason), "{}", error.message);
            for text in [error.message.clone(), format!("{error:?}")] {
                assert!(
                    !text.contains(&scratch_path) && !text.contains(FIRST_TOKEN),
                    "{text}"
                );
            }
        };
        runtime().block_on(async {
            let direct = port_on(&token_file);
            let missing = turn(&direct, "opus").await.expect_err("missing");
            refused(&missing, "was not found");
            assert!(sts.requests().is_empty());
            write_token(&token_file, FIRST_TOKEN, 0o600);
            assert_eq!(
                turn(&direct, "opus")
                    .await
                    .expect("the next fetch reads the file"),
                "signed"
            );
            fs::set_permissions(&token_file, Permissions::from_mode(0o666)).unwrap();
            let writable = turn(&port_on(&token_file), "opus")
                .await
                .expect_err("others can write it");
            refused(&writable, "access protections");
            fs::set_permissions(&token_file, Permissions::from_mode(0o600)).unwrap();
            let linked = turn(&port_on(&open.join("token")), "opus")
                .await
                .expect_err("linked from a directory others can write");
            refused(&linked, "access protections");
        });
        let asked = sts.requests();
        assert_eq!(asked.len(), 1, "only the protected file is sent");
        assert_eq!(asked[0].web_identity_token(), Some(FIRST_TOKEN));
    }
}
