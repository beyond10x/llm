//! The header-signing cases of the AWS Signature Version 4 test suite (`tests/data/sigv4/`, see
//! `tests/data/README.md`): each case's canonical request, string to sign and signature, then
//! the headers `sign` writes for every case whose request a URL carries as written.

use http::{HeaderMap, HeaderName, HeaderValue, Method};
use llm_bedrock::AwsCredentials;
use llm_bedrock::sigv4::{
    SigningParams, canonical_request, payload_hash, sign, signature, string_to_sign,
};
use llm_core::{Dispatch, Error, ErrorCode};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use url::Url;

/// The case directories the suite holds.
const CASES: usize = 38;
/// The suite's one signing instant, 2015-08-30T12:36:00Z, in seconds since 1970.
const SUITE_SECONDS: u64 = 1_440_938_160;
const SUITE_TIMESTAMP: &str = "2015-08-30T12:36:00Z";

const UNNORMALIZED: &str = "the case signs its path as written; sign normalises every path, the \
                            rule of every service but S3";
const RAW_PATH: &str = "the case signs a raw path encoded once; a URL carries that path already \
                        encoded, and sign encodes the path it carries once more, as the service \
                        does with the path it receives";
const TOKEN_ADDED_AFTER: &str = "the case adds the session token after signing; sign always \
                                 signs the token it adds";

/// Cases whose headers `sign` is not checked on end to end, and why. Their canonical request,
/// string to sign and signature are compared like every other case's; nothing is skipped there.
const NOT_END_TO_END: &[(&str, &str)] = &[
    ("get-relative-relative-unnormalized", UNNORMALIZED),
    ("get-relative-unnormalized", UNNORMALIZED),
    ("get-slash-dot-slash-unnormalized", UNNORMALIZED),
    ("get-slash-pointless-dot-unnormalized", UNNORMALIZED),
    ("get-slash-unnormalized", UNNORMALIZED),
    ("get-slashes-unnormalized", UNNORMALIZED),
    ("get-space-unnormalized", UNNORMALIZED),
    ("get-space-normalized", RAW_PATH),
    ("get-utf8", RAW_PATH),
    ("post-sts-header-after", TOKEN_ADDED_AFTER),
];

const SECRET: &str = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";

/// A request as the suite writes it: a request line, headers (a line that starts with white
/// space continues the header before it), a blank line and the body.
struct Request {
    method: String,
    path: String,
    query: Option<String>,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn parse(text: &str) -> Self {
        let (head, body) = text.split_once("\n\n").unwrap_or((text, ""));
        let mut lines = head.lines();
        let (method, rest) = lines
            .next()
            .and_then(|line| line.split_once(' '))
            .expect("a request line");
        let (target, _version) = rest.rsplit_once(' ').expect("a protocol version");
        let (path, query) = match target.split_once('?') {
            Some((path, query)) => (path, Some(query.to_owned())),
            None => (target, None),
        };
        let mut headers: Vec<(String, String)> = Vec::new();
        for line in lines {
            if line.starts_with([' ', '\t']) {
                let (_, value) = headers.last_mut().expect("a continuation follows a header");
                value.push(' ');
                value.push_str(line);
            } else {
                let (name, value) = line.split_once(':').expect("a header line has a colon");
                headers.push((name.to_owned(), value.to_owned()));
            }
        }
        Self {
            method: method.to_owned(),
            path: path.to_owned(),
            query,
            headers,
            body: body.as_bytes().to_vec(),
        }
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(header, _)| header.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    fn header_map(&self) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in &self.headers {
            map.append(
                HeaderName::from_bytes(name.to_ascii_lowercase().as_bytes())
                    .expect("a header name"),
                HeaderValue::from_str(value).expect("a header value"),
            );
        }
        map
    }
}

struct Case {
    name: String,
    directory: PathBuf,
    context: Value,
    request: Request,
}

impl Case {
    fn read(directory: PathBuf) -> Self {
        let name = directory
            .file_name()
            .and_then(|name| name.to_str())
            .expect("a UTF-8 case name")
            .to_owned();
        let context =
            serde_json::from_str(&read(&directory, "context.json")).expect("context.json is JSON");
        let request = Request::parse(&read(&directory, "request.txt"));
        Self {
            name,
            directory,
            context,
            request,
        }
    }

    fn file(&self, file: &str) -> String {
        read(&self.directory, file)
    }

    fn text(&self, key: &str) -> &str {
        self.context[key]
            .as_str()
            .unwrap_or_else(|| panic!("{}: context `{key}` is a string", self.name))
    }

    fn flag(&self, key: &str) -> bool {
        self.context[key].as_bool().unwrap_or(false)
    }

    fn credential(&self, key: &str) -> &str {
        self.context["credentials"][key]
            .as_str()
            .unwrap_or_else(|| panic!("{}: credential `{key}` is a string", self.name))
    }

    fn token(&self) -> Option<&str> {
        self.context["credentials"]["token"].as_str()
    }

    /// `x-amz-date` written as the suite's timestamp without its separators.
    fn amz_date(&self) -> String {
        self.text("timestamp").replace(['-', ':'], "")
    }

    /// The request's headers and those the suite signs with them: `x-amz-date`,
    /// `x-amz-content-sha256` when the case signs its body, and the session token unless the
    /// case adds it after signing.
    fn signed_headers(&self) -> HeaderMap {
        let mut headers = self.request.header_map();
        headers.insert(
            "x-amz-date",
            HeaderValue::from_str(&self.amz_date()).expect("a date header"),
        );
        self.add_content_hash(&mut headers);
        if let Some(token) = self.token()
            && !self.flag("omit_session_token")
        {
            headers.insert(
                "x-amz-security-token",
                HeaderValue::from_str(token).expect("a token header"),
            );
        }
        headers
    }

    fn add_content_hash(&self, headers: &mut HeaderMap) {
        if self.flag("sign_body") {
            headers.insert(
                "x-amz-content-sha256",
                HeaderValue::from_str(&payload_hash(&self.request.body)).expect("a hash header"),
            );
        }
    }
}

fn read(directory: &Path, file: &str) -> String {
    let path = directory.join(file);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn cases() -> Vec<Case> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/sigv4");
    let mut directories: Vec<PathBuf> = fs::read_dir(&root)
        .expect("the suite directory")
        .map(|entry| entry.expect("a suite entry").path())
        .filter(|path| path.is_dir())
        .collect();
    directories.sort_unstable();
    directories.into_iter().map(Case::read).collect()
}

fn suite_time() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(SUITE_SECONDS)
}

#[test]
fn every_case_matches_its_canonical_request_string_to_sign_and_signature() {
    let cases = cases();
    assert_eq!(cases.len(), CASES, "case directories in the suite");
    let mut compared = 0;
    let mut mismatches = Vec::new();
    for case in &cases {
        let amz_date = case.amz_date();
        let (region, service) = (case.text("region"), case.text("service"));
        let canonical = canonical_request(
            &case.request.method,
            &case.request.path,
            case.request.query.as_deref(),
            &case.signed_headers(),
            &payload_hash(&case.request.body),
            case.flag("normalize"),
        );
        let to_sign = string_to_sign(&amz_date, region, service, &canonical);
        let proof = signature(
            case.credential("secret_access_key"),
            &amz_date,
            region,
            service,
            &to_sign,
        );
        for (file, actual) in [
            ("header-canonical-request.txt", &canonical),
            ("header-string-to-sign.txt", &to_sign),
            ("header-signature.txt", &proof),
        ] {
            let expected = case.file(file);
            if *actual != expected {
                mismatches.push(format!(
                    "{}/{file}\n  expected {expected:?}\n  actual   {actual:?}",
                    case.name
                ));
            }
            compared += 1;
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} of {compared} comparisons differ:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
    assert_eq!(compared, 3 * CASES);
}

#[test]
fn sign_writes_the_suites_headers_for_every_request_a_url_carries_as_written() {
    let cases = cases();
    for (name, _) in NOT_END_TO_END {
        assert!(
            cases.iter().any(|case| case.name == *name),
            "{name} is a suite case"
        );
    }
    let mut signed = 0;
    let mut mismatches = Vec::new();
    for case in cases
        .iter()
        .filter(|case| !NOT_END_TO_END.iter().any(|(name, _)| case.name == *name))
    {
        assert_eq!(case.text("timestamp"), SUITE_TIMESTAMP, "{}", case.name);
        let credentials = AwsCredentials::new(
            case.credential("access_key_id"),
            case.credential("secret_access_key"),
            case.token().map(str::to_owned),
            None,
        );
        let params = SigningParams {
            credentials: &credentials,
            region: case.text("region"),
            service: case.text("service"),
            time: suite_time(),
        };
        let request = &case.request;
        let mut headers = request.header_map();
        case.add_content_hash(&mut headers);
        let target = request.query.as_ref().map_or_else(
            || request.path.clone(),
            |query| format!("{}?{query}", request.path),
        );
        let host = request.header("host").expect("a host header");
        let url = Url::parse(&format!("https://{host}{target}")).expect("a URL");
        let method = Method::from_bytes(request.method.as_bytes()).expect("a method");
        sign(&method, &url, &mut headers, &request.body, &params)
            .unwrap_or_else(|error| panic!("{}: {error}", case.name));
        let expected = Request::parse(&case.file("header-signed-request.txt"));
        for name in ["authorization", "x-amz-date", "x-amz-security-token"] {
            let actual = headers.get(name).and_then(|value| value.to_str().ok());
            let wanted = expected.header(name);
            if actual != wanted {
                mismatches.push(format!(
                    "{} {name}\n  expected {wanted:?}\n  actual   {actual:?}",
                    case.name
                ));
            }
        }
        signed += 1;
    }
    assert!(
        mismatches.is_empty(),
        "{} header(s) differ:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
    assert_eq!(signed, CASES - NOT_END_TO_END.len());
}

fn bedrock_params(credentials: &AwsCredentials) -> SigningParams<'_> {
    SigningParams {
        credentials,
        region: "us-east-1",
        service: "bedrock",
        time: suite_time(),
    }
}

#[test]
fn the_path_is_signed_encoded_once_more_than_it_travels() {
    let headers = HeaderMap::new();
    let empty = payload_hash(b"");
    let uri = |path: &str, normalize: bool| {
        canonical_request("POST", path, None, &headers, &empty, normalize)
            .lines()
            .nth(1)
            .map(str::to_owned)
    };
    assert_eq!(
        uri("/model/a.b-v1%3A0/converse", true).as_deref(),
        Some("/model/a.b-v1%253A0/converse")
    );
    assert_eq!(
        uri("/a:b c/./x/../d/", true).as_deref(),
        Some("/a%3Ab%20c/d/")
    );
    assert_eq!(uri("", true).as_deref(), Some("/"));
    assert_eq!(uri("", false).as_deref(), Some("/"));
    assert_eq!(uri("//a/./b", false).as_deref(), Some("//a/./b"));
}

#[test]
fn sign_adds_host_date_and_token_and_replaces_a_stale_authorization() {
    let credentials = AwsCredentials::new("AKIDEXAMPLE", SECRET, Some("SESSION".to_owned()), None);
    let url = Url::parse(
        "https://bedrock-runtime.us-east-1.amazonaws.com:8443/model/a.b-v1%3A0/converse",
    )
    .expect("a URL");
    let mut headers = HeaderMap::new();
    headers.insert("authorization", HeaderValue::from_static("stale"));
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    sign(
        &Method::POST,
        &url,
        &mut headers,
        b"{}",
        &bedrock_params(&credentials),
    )
    .expect("signed");

    assert_eq!(
        headers["host"],
        "bedrock-runtime.us-east-1.amazonaws.com:8443"
    );
    assert_eq!(headers["x-amz-date"], "20150830T123600Z");
    assert_eq!(headers["x-amz-security-token"], "SESSION");
    assert!(headers["x-amz-security-token"].is_sensitive());
    assert_eq!(headers.get_all("authorization").iter().count(), 1);
    assert!(headers["authorization"].is_sensitive());

    let mut unsigned = headers.clone();
    unsigned.remove("authorization");
    let canonical = canonical_request(
        "POST",
        url.path(),
        url.query(),
        &unsigned,
        &payload_hash(b"{}"),
        true,
    );
    assert!(
        canonical.starts_with("POST\n/model/a.b-v1%253A0/converse\n\ncontent-type:"),
        "{canonical}"
    );
    let to_sign = string_to_sign("20150830T123600Z", "us-east-1", "bedrock", &canonical);
    let proof = signature(SECRET, "20150830T123600Z", "us-east-1", "bedrock", &to_sign);
    assert_eq!(
        headers["authorization"],
        format!(
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/bedrock/aws4_request, \
             SignedHeaders=content-type;host;x-amz-date;x-amz-security-token, Signature={proof}"
        )
        .as_str()
    );
}

#[test]
fn sign_keeps_the_callers_host_and_drops_a_stale_session_token() {
    let credentials = AwsCredentials::new("AKIDEXAMPLE", SECRET, None, None);
    let url = Url::parse("https://bedrock-runtime.us-east-1.amazonaws.com/model/m/converse")
        .expect("a URL");
    let mut headers = HeaderMap::new();
    headers.insert("host", HeaderValue::from_static("vpce.example.test"));
    headers.insert("x-amz-security-token", HeaderValue::from_static("OLD"));
    sign(
        &Method::POST,
        &url,
        &mut headers,
        b"",
        &bedrock_params(&credentials),
    )
    .expect("signed");

    assert_eq!(headers["host"], "vpce.example.test");
    assert!(!headers.contains_key("x-amz-security-token"));
    let authorization = headers["authorization"].to_str().expect("ASCII");
    assert!(
        authorization.contains(", SignedHeaders=host;x-amz-date, "),
        "{authorization}"
    );
}

fn refusal(
    credentials: &AwsCredentials,
    region: &str,
    service: &str,
    headers: &HeaderMap,
) -> Error {
    let url = Url::parse("https://bedrock-runtime.us-east-1.amazonaws.com/model/m/converse")
        .expect("a URL");
    let mut changed = headers.clone();
    let params = SigningParams {
        credentials,
        region,
        service,
        time: suite_time(),
    };
    let error = sign(&Method::POST, &url, &mut changed, b"{}", &params).expect_err("a refusal");
    assert_eq!(&changed, headers, "a refusal leaves the request as it was");
    assert_eq!(error.dispatch, Dispatch::NotSent);
    assert!(!error.retriable);
    for secret in [SECRET, "bad\ntoken", "AKID"] {
        assert!(!error.message.contains(secret), "{}", error.message);
    }
    error
}

#[test]
fn sign_refuses_what_it_cannot_present_before_changing_the_request() {
    let good = AwsCredentials::new("AKIDEXAMPLE", SECRET, None, None);
    let unauthorized = [
        AwsCredentials::new("AKIDEXAMPLE", SECRET, Some("bad\ntoken".to_owned()), None),
        AwsCredentials::new("AKIDEXAMPLE", SECRET, Some(String::new()), None),
        AwsCredentials::new("AKID/EXAMPLE", SECRET, None, None),
        AwsCredentials::new("AKID EXAMPLE", SECRET, None, None),
        AwsCredentials::new("", SECRET, None, None),
        AwsCredentials::new("AKIDEXAMPLE", "", None, None),
    ];
    let none = HeaderMap::new();
    for credentials in &unauthorized {
        let error = refusal(credentials, "us-east-1", "bedrock", &none);
        assert_eq!(error.code, ErrorCode::Unauthorized, "{credentials:?}");
    }
    for (region, service) in [
        ("US-EAST-1", "bedrock"),
        ("us east 1", "bedrock"),
        ("", "bedrock"),
        ("us-east-1", ""),
        ("us-east-1", "bedrock/x"),
    ] {
        let error = refusal(&good, region, service, &none);
        assert_eq!(
            error.code,
            ErrorCode::InvalidRequest,
            "{region:?} {service:?}"
        );
    }
    let mut binary = HeaderMap::new();
    binary.insert(
        "x-binary",
        HeaderValue::from_bytes(b"\xff\xfe").expect("an obs-text value"),
    );
    let error = refusal(&good, "us-east-1", "bedrock", &binary);
    assert_eq!(error.code, ErrorCode::InvalidRequest);

    let url = Url::parse("data:text/plain,signed").expect("a URL without a host");
    let mut headers = HeaderMap::new();
    let error = sign(
        &Method::GET,
        &url,
        &mut headers,
        b"",
        &bedrock_params(&good),
    )
    .expect_err("no host to sign");
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert!(headers.is_empty());
}
