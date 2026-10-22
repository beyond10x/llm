//! Observe `llm-bedrock`: the Signature Version 4 signer, the event-stream decoder, the container
//! credential source and whole turns through `BedrockClient`.
//!
//! Every fact below is what a public `llm_bedrock` item returned or what a scripted server on
//! 127.0.0.1 received. The inputs are the crate's vendored test data and fixtures written here.
//! Nothing here decides an answer: no expected file of the AWS suite is read, no scenario is
//! named, and nothing leaves the host.

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ess_conformance::target::TargetError;
use llm_bedrock::{
    Api, AwsCredentials, BedrockClient, BedrockTarget, ContainerCredentials, CredentialSource,
    eventstream::{self, HeaderValue as FrameValue, Message, MessageDecoder},
    sigv4::{SigningParams, sign},
};
use llm_core::{
    BoxFuture, Cancel, Capabilities, Error, Id, Item, Model, Protocol, Provenance, StopReason,
    StreamEvent, TurnDocument, TurnOutcome, TurnRequest, VecSink,
};
use llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use llm_http::{HeaderMap, HeaderName, HeaderValue, HttpClient, Limits};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use url::Url;

use crate::target::Observed;

/// Every view name this domain answers through `query_view`.
pub const VIEWS: &[&str] = &[
    "llm.bedrock.LastSigning",
    "llm.bedrock.LastFrames",
    "llm.bedrock.LastTurn",
    "llm.bedrock.LastContainerCredentials",
];

/// Observe one command of this domain, or `None` when the command belongs to another.
pub fn observe(command: &str, input: &Value) -> Option<Result<Observed, TargetError>> {
    let (facts, view, event) = match command {
        "llm.bedrock.SignSuiteCase" => (
            signing(input),
            "llm.bedrock.LastSigning",
            "llm.bedrock.Signed",
        ),
        "llm.bedrock.DecodeEventStream" => (
            frames(input),
            "llm.bedrock.LastFrames",
            "llm.bedrock.Decoded",
        ),
        "llm.bedrock.RunTurn" => (turn(input), "llm.bedrock.LastTurn", "llm.bedrock.Answered"),
        "llm.bedrock.FetchContainerCredentials" => (
            container(input),
            "llm.bedrock.LastContainerCredentials",
            "llm.bedrock.Fetched",
        ),
        _ => return None,
    };
    Some(facts.map(|facts| Observed {
        facts,
        view,
        event,
        field: "accepted",
    }))
}

fn unavailable(error: impl std::fmt::Display) -> TargetError {
    TargetError::unavailable("bedrock observation", error.to_string())
}

fn runtime() -> Result<tokio::runtime::Runtime, TargetError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(unavailable)
}

/// `relative` under the crate's vendored test data, refused unless every component is a plain
/// name, so no input reaches outside that directory.
fn data(relative: &str) -> Result<PathBuf, Error> {
    let path = Path::new(relative);
    if relative.is_empty()
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(Error::invalid(
            "a test data name must stay inside the data directory",
        ));
    }
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/llm-bedrock/tests/data")
        .join(path))
}

/// The bytes of one test data file; an input naming none is an invalid input, not a broken
/// target.
fn read_data(relative: &str) -> Result<Vec<u8>, Error> {
    std::fs::read(data(relative)?).map_err(|_| Error::invalid("the test data file cannot be read"))
}

/// A refusal's code, dispatch and whether `Error::may_retry` lets a caller try again.
fn report_error(error: &Error, facts: &mut Value) {
    facts["error_code"] = json!(error.code);
    facts["dispatch"] = json!(error.dispatch);
    facts["retriable"] = json!(error.may_retry());
}

/// Whether neither the message nor the debug form of `error` contains any of `secrets`.
fn clean(error: &Error, secrets: &[&str]) -> bool {
    let debug = format!("{error:?}");
    secrets
        .iter()
        .all(|secret| !error.message.contains(secret) && !debug.contains(secret))
}

// ---------------------------------------------------------------------------------------------
// Calendar arithmetic for the suite's and the fixtures' UTC instants, whole seconds only.
// ---------------------------------------------------------------------------------------------

/// Days since 1970-01-01 of a proleptic Gregorian date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The proleptic Gregorian date of a day count since 1970-01-01.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// The instant of six numeric fields, each read from `text` at the given byte range.
fn instant(text: &str, fields: [(usize, usize); 6]) -> Option<SystemTime> {
    let mut numbers = [0_i64; 6];
    for (number, (start, end)) in numbers.iter_mut().zip(fields) {
        let digits = text.get(start..end)?;
        if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        *number = digits.parse().ok()?;
    }
    let [year, month, day, hour, minute, second] = numbers;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let seconds = days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second;
    Some(UNIX_EPOCH + Duration::from_secs(u64::try_from(seconds).ok()?))
}

/// `2015-08-30T12:36:00Z`.
fn parse_timestamp(text: &str) -> Option<SystemTime> {
    let shape = text.len() == 20
        && text.as_bytes().get(4) == Some(&b'-')
        && text.as_bytes().get(7) == Some(&b'-')
        && text.as_bytes().get(10) == Some(&b'T')
        && text.as_bytes().get(13) == Some(&b':')
        && text.as_bytes().get(16) == Some(&b':')
        && text.ends_with('Z');
    shape
        .then(|| {
            instant(
                text,
                [(0, 4), (5, 7), (8, 10), (11, 13), (14, 16), (17, 19)],
            )
        })
        .flatten()
}

/// `20150830T123600Z`, the `x-amz-date` form.
fn parse_amz_date(text: &str) -> Option<SystemTime> {
    let shape = text.len() == 16 && text.as_bytes().get(8) == Some(&b'T') && text.ends_with('Z');
    shape
        .then(|| instant(text, [(0, 4), (4, 6), (6, 8), (9, 11), (11, 13), (13, 15)]))
        .flatten()
}

/// `at` as `2026-10-22T12:00:00Z`.
fn format_timestamp(at: SystemTime) -> Option<String> {
    let seconds = i64::try_from(at.duration_since(UNIX_EPOCH).ok()?.as_secs()).ok()?;
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    let rest = seconds.rem_euclid(86_400);
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    ))
}

// ---------------------------------------------------------------------------------------------
// llm.bedrock.SignSuiteCase: one AWS Signature Version 4 suite request through `sign`.
// ---------------------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SignInput {
    case: String,
}

#[derive(Deserialize)]
struct SuiteContext {
    credentials: SuiteCredentials,
    region: String,
    service: String,
    timestamp: String,
    #[serde(default)]
    normalize: bool,
    #[serde(default)]
    sign_body: bool,
    #[serde(default)]
    omit_session_token: bool,
}

#[derive(Deserialize)]
struct SuiteCredentials {
    access_key_id: String,
    secret_access_key: String,
    #[serde(default)]
    token: Option<String>,
}

/// A suite request: the request line, header lines in file order, a blank line and the body.
struct SuiteRequest {
    method: http::Method,
    target: String,
    headers: HeaderMap,
    body: Vec<u8>,
}

fn suite_request(text: &str) -> Result<SuiteRequest, Error> {
    let malformed = || Error::invalid("the suite request is outside what this adapter reads");
    let (head, body) = text.split_once("\n\n").unwrap_or((text, ""));
    let mut lines = head.lines();
    let mut parts = lines.next().unwrap_or_default().split(' ');
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        return Err(malformed());
    };
    let method = http::Method::from_bytes(method.as_bytes()).map_err(|_| malformed())?;
    let mut headers = HeaderMap::new();
    for line in lines {
        // A folded header line is outside what this adapter builds.
        if line.starts_with([' ', '\t']) {
            return Err(malformed());
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(malformed());
        };
        headers.append(
            HeaderName::from_bytes(name.trim().as_bytes()).map_err(|_| malformed())?,
            HeaderValue::from_str(value.trim()).map_err(|_| malformed())?,
        );
    }
    Ok(SuiteRequest {
        method,
        target: target.to_owned(),
        headers,
        body: body.as_bytes().to_vec(),
    })
}

fn signing(input: &Value) -> Result<Value, TargetError> {
    let input: SignInput = serde_json::from_value(input.clone()).map_err(unavailable)?;
    let mut facts = json!({"accepted":false,"error_code":null,"authorization":null,
        "amz_date":null,"security_token_added":null});
    if let Err(error) = sign_case(&input.case, &mut facts) {
        facts["error_code"] = json!(error.code);
    }
    Ok(facts)
}

/// Signs one suite case. A case this adapter cannot read is `invalid`; a refusal by `sign`
/// keeps its own code.
fn sign_case(case: &str, facts: &mut Value) -> Result<(), Error> {
    let read = |file: &str| {
        String::from_utf8(read_data(&format!("sigv4/{case}/{file}"))?)
            .map_err(|_| Error::invalid("the suite file is not UTF-8"))
    };
    let context: SuiteContext = serde_json::from_str(&read("context.json")?)
        .map_err(|_| Error::invalid("the suite context is not readable"))?;
    // `sign` signs headers, normalises the path and signs the token it adds; a case that asks
    // for anything else is not this observation's.
    if !context.normalize || context.sign_body || context.omit_session_token {
        return Err(Error::invalid(
            "the case is outside header signing with a normalised path",
        ));
    }
    let time = parse_timestamp(&context.timestamp)
        .ok_or_else(|| Error::invalid("the case timestamp is not a UTC instant"))?;
    let mut request = suite_request(&read("request.txt")?)?;
    let host = request
        .headers
        .get("host")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| Error::invalid("the suite request names no host"))?
        .to_owned();
    let url = Url::parse(&format!("https://{host}{}", request.target))
        .map_err(|_| Error::invalid("the suite request names no URL"))?;
    let token = context.credentials.token.clone();
    let credentials = AwsCredentials::new(
        context.credentials.access_key_id,
        context.credentials.secret_access_key,
        token.clone(),
        None,
    );
    let params = SigningParams {
        credentials: &credentials,
        region: &context.region,
        service: &context.service,
        time,
    };
    sign(
        &request.method,
        &url,
        &mut request.headers,
        &request.body,
        &params,
    )?;
    let header = |name: &str| {
        request
            .headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    facts["accepted"] = json!(true);
    facts["authorization"] = json!(header("authorization"));
    facts["amz_date"] = json!(header("x-amz-date"));
    facts["security_token_added"] = match (token, header("x-amz-security-token")) {
        (None, None) => Value::Null,
        (Some(token), Some(sent)) => json!(token == sent),
        _ => json!(false),
    };
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// llm.bedrock.DecodeEventStream: one vendored body through `MessageDecoder`.
// ---------------------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FramesInput {
    file: String,
    #[serde(default, deserialize_with = "crate::target::token_bound")]
    truncate_by: Option<u64>,
}

/// The frames `bytes` decode into when they arrive in reads of `split` bytes, and the first
/// failure, `finish` included.
fn decode(bytes: &[u8], split: usize) -> (Vec<Message>, Option<Error>) {
    let mut decoder = MessageDecoder::new();
    let mut messages = Vec::new();
    for chunk in bytes.chunks(split.max(1)) {
        for result in decoder.push(chunk) {
            match result {
                Ok(message) => messages.push(message),
                Err(error) => return (messages, Some(error)),
            }
        }
    }
    let failure = decoder.finish().err();
    (messages, failure)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

/// `<name>:<type>:<value>`, as the domain's `first_headers` names each type.
fn header_label(name: &str, value: &FrameValue) -> String {
    let (kind, value) = match value {
        FrameValue::Bool(value) => ("bool", value.to_string()),
        FrameValue::Byte(value) => ("byte", value.to_string()),
        FrameValue::Int16(value) => ("int16", value.to_string()),
        FrameValue::Int32(value) => ("int32", value.to_string()),
        FrameValue::Int64(value) => ("int64", value.to_string()),
        FrameValue::ByteArray(value) => ("byte-array", hex(value)),
        FrameValue::String(value) => ("string", value.clone()),
        FrameValue::Timestamp(value) => ("timestamp", value.to_string()),
        FrameValue::Uuid(value) => ("uuid", hex(value)),
    };
    format!("{name}:{kind}:{value}")
}

fn frames(input: &Value) -> Result<Value, TargetError> {
    let input: FramesInput = serde_json::from_value(input.clone()).map_err(unavailable)?;
    let mut facts = json!({"accepted":false,"error_code":null,"dispatch":null,"retriable":null,
        "messages":0,"event_types":[],"first_headers":[],"first_payload":null,
        "splits_agree":false,"round_trip":false});
    let mut bytes = match read_data(&input.file) {
        Ok(bytes) => bytes,
        Err(error) => {
            report_error(&error, &mut facts);
            return Ok(facts);
        }
    };
    let cut = usize::try_from(input.truncate_by.unwrap_or(0)).unwrap_or(usize::MAX);
    bytes.truncate(bytes.len().saturating_sub(cut));
    let (messages, failure) = decode(&bytes, bytes.len());
    let code = failure.as_ref().map(|error| error.code);
    facts["splits_agree"] = json!([1, 7, 64].into_iter().all(|split| {
        let (other, other_failure) = decode(&bytes, split);
        other == messages && other_failure.map(|error| error.code) == code
    }));
    let encoded: Vec<u8> = messages
        .iter()
        .flat_map(eventstream::encode_message)
        .collect();
    facts["round_trip"] = json!(bytes.starts_with(&encoded));
    facts["messages"] = json!(messages.len());
    let mut types = BTreeMap::<&str, usize>::new();
    for message in &messages {
        if let Some(kind) = message.string_header(":event-type") {
            *types.entry(kind).or_default() += 1;
        }
    }
    facts["event_types"] = json!(
        types
            .iter()
            .map(|(kind, count)| format!("{kind}:{count}"))
            .collect::<Vec<_>>()
    );
    if let Some(first) = messages.first() {
        facts["first_headers"] = json!(
            first
                .headers
                .iter()
                .map(|(name, value)| header_label(name, value))
                .collect::<Vec<_>>()
        );
        facts["first_payload"] = json!(String::from_utf8(first.payload.clone()).ok());
    }
    match &failure {
        None => facts["accepted"] = json!(true),
        Some(error) => report_error(error, &mut facts),
    }
    Ok(facts)
}

// ---------------------------------------------------------------------------------------------
// Scripted loopback servers shared by the turn and the container credential observations.
// ---------------------------------------------------------------------------------------------

const MAX_CAPTURED_REQUEST_BYTES: usize = 1024 * 1024;

/// One request as the server received it: its head and its body.
#[derive(Clone)]
struct Captured {
    head: String,
    body: Vec<u8>,
}

impl Captured {
    /// The value of every header line named `name`, in order.
    fn header(&self, name: &str) -> Vec<&str> {
        self.head
            .split("\r\n")
            .skip(1)
            .filter_map(|line| line.split_once(':'))
            .filter(|(field, _)| field.trim().eq_ignore_ascii_case(name))
            .map(|(_, value)| value.trim())
            .collect()
    }

    /// `<method> <target>` of the request line.
    fn request_line(&self) -> String {
        self.head
            .split("\r\n")
            .next()
            .unwrap_or_default()
            .rsplit_once(' ')
            .map_or_else(String::new, |(line, _)| line.to_owned())
    }
}

async fn read_captured(socket: &mut TcpStream) -> std::io::Result<Captured> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let length = socket.read(&mut buffer).await?;
        if length == 0 || bytes.len() + length > MAX_CAPTURED_REQUEST_BYTES {
            return Err(std::io::Error::other("unterminated or oversized request"));
        }
        bytes.extend_from_slice(&buffer[..length]);
        let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let head = String::from_utf8_lossy(&bytes[..end]).into_owned();
        let declared = head
            .to_ascii_lowercase()
            .lines()
            .find_map(|line| line.strip_prefix("content-length:"))
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        if bytes.len() >= end + 4 + declared {
            let body = bytes[end + 4..end + 4 + declared].to_vec();
            return Ok(Captured { head, body });
        }
    }
}

/// Serves every connection `answer`'s bytes, after keeping the request it carried.
async fn serve(
    listener: TcpListener,
    captured: Arc<Mutex<Vec<Captured>>>,
    answer: impl Fn() -> Vec<u8>,
) {
    while let Ok((mut socket, _)) = listener.accept().await {
        let Ok(request) = read_captured(&mut socket).await else {
            continue;
        };
        if let Ok(mut captured) = captured.lock() {
            captured.push(request);
        }
        let _written = socket.write_all(&answer()).await;
        let _closed = socket.shutdown().await;
    }
}

fn http_answer(status: &str, headers: &str, body: &[u8]) -> Vec<u8> {
    let mut answer = format!(
        "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    answer.extend_from_slice(body);
    answer
}

fn http_client() -> Result<HttpClient, Error> {
    let total = Duration::from_secs(10);
    HttpClient::new(Limits {
        response_headers: total,
        idle: total,
        total,
    })
}

// ---------------------------------------------------------------------------------------------
// llm.bedrock.RunTurn: one turn through `BedrockClient` against a scripted server.
// ---------------------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TurnInput {
    api: String,
    stream: bool,
    model_id: String,
    request_json: String,
    #[serde(default)]
    recording: Option<String>,
    #[serde(default, deserialize_with = "crate::target::token_bound")]
    keep_frames: Option<u64>,
    #[serde(default)]
    frames_json: Option<String>,
    cut: bool,
    #[serde(default, deserialize_with = "crate::target::token_bound")]
    status: Option<u64>,
    #[serde(default)]
    error_type: Option<String>,
    session_token: bool,
}

/// One frame a scenario adds after the recording's: an event, or an exception.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExtraFrame {
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    payload: Option<Value>,
    #[serde(default)]
    exception: Option<String>,
}

/// The Signature Version 4 suite's example key pair.
const ACCESS_KEY_ID: &str = "AKIDEXAMPLE";
const SECRET_ACCESS_KEY: &str = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";
const SESSION_TOKEN: &str = "llm-fixture-bedrock-session-token";
/// The message every scripted refusal and exception frame carries; no diagnostic may quote it.
const PROVIDER_MESSAGE: &str = "llm-fixture-bedrock-provider-message";
const REGION: &str = "eu-central-1";
const SERVICE: &str = "bedrock";
const MAX_SINK_EVENTS: usize = 4096;
const MAX_SINK_BYTES: usize = 1024 * 1024;

/// Answers the suite's example key pair, with the fixture session token when `token`.
struct FixtureCredentials {
    token: bool,
}

impl FixtureCredentials {
    fn get(&self) -> AwsCredentials {
        AwsCredentials::new(
            ACCESS_KEY_ID,
            SECRET_ACCESS_KEY,
            self.token.then(|| SESSION_TOKEN.to_owned()),
            None,
        )
    }
}

impl CredentialSource for FixtureCredentials {
    fn credentials<'a>(
        &'a self,
        _cancel: &'a Cancel,
    ) -> BoxFuture<'a, Result<AwsCredentials, Error>> {
        let credentials = self.get();
        Box::pin(async move { Ok(credentials) })
    }
}

fn string_header(name: &str, value: &str) -> (String, FrameValue) {
    (name.to_owned(), FrameValue::String(value.to_owned()))
}

fn extra_frame(frame: &ExtraFrame) -> Result<Message, Error> {
    match (&frame.event, &frame.exception) {
        (Some(event), None) => Ok(Message {
            headers: vec![
                string_header(":event-type", event),
                string_header(":content-type", "application/json"),
                string_header(":message-type", "event"),
            ],
            payload: frame
                .payload
                .as_ref()
                .unwrap_or(&json!({}))
                .to_string()
                .into_bytes(),
        }),
        (None, Some(kind)) if frame.payload.is_none() => Ok(Message {
            headers: vec![
                string_header(":exception-type", kind),
                string_header(":content-type", "application/json"),
                string_header(":message-type", "exception"),
            ],
            payload: json!({"message": PROVIDER_MESSAGE})
                .to_string()
                .into_bytes(),
        }),
        _ => Err(Error::invalid(
            "an added frame names either an event or an exception",
        )),
    }
}

/// What the scripted server answers the turn's one request; a scenario this adapter cannot
/// script is `invalid`.
fn turn_answer(input: &TurnInput) -> Result<Vec<u8>, Error> {
    if let Some(status) = input.status {
        let error_type = input.error_type.as_deref().map_or_else(String::new, |kind| {
            format!("x-amzn-errortype: {kind}:http://internal.amazon.com/coral/com.amazon.bedrock/\r\n")
        });
        return Ok(http_answer(
            &format!("{status} Scripted"),
            &format!("Content-Type: application/json\r\n{error_type}"),
            json!({"message": PROVIDER_MESSAGE}).to_string().as_bytes(),
        ));
    }
    let recording = match &input.recording {
        Some(name) => read_data(&format!("frames/{name}"))?,
        None => Vec::new(),
    };
    if !input.stream {
        return Ok(http_answer(
            "200 OK",
            "Content-Type: application/json\r\n",
            &recording,
        ));
    }
    let (messages, failure) = decode(&recording, recording.len());
    if failure.is_some() {
        return Err(Error::invalid("the recording is not an event stream"));
    }
    let keep = input.keep_frames.map_or(messages.len(), |keep| {
        usize::try_from(keep).unwrap_or(usize::MAX)
    });
    let mut frames: Vec<Message> = messages.into_iter().take(keep).collect();
    if let Some(added) = &input.frames_json {
        let added: Vec<ExtraFrame> = serde_json::from_str(added)
            .map_err(|_| Error::invalid("the added frames are not readable"))?;
        for frame in &added {
            frames.push(extra_frame(frame)?);
        }
    }
    let mut body: Vec<u8> = frames
        .iter()
        .flat_map(eventstream::encode_message)
        .collect();
    let stream_type = "Content-Type: application/vnd.amazon.eventstream\r\n";
    if !input.cut {
        return Ok(http_answer("200 OK", stream_type, &body));
    }
    // Half of one more frame, then the connection closes: the body has no declared length, so
    // only the event-stream decoder can tell that it ended inside a frame.
    let partial = eventstream::encode_message(&Message {
        headers: vec![
            string_header(":event-type", "contentBlockDelta"),
            string_header(":content-type", "application/json"),
            string_header(":message-type", "event"),
        ],
        payload: br#"{"contentBlockIndex":0,"delta":{"text":"cut"}}"#.to_vec(),
    });
    body.extend_from_slice(&partial[..partial.len() / 2]);
    let mut answer =
        format!("HTTP/1.1 200 OK\r\n{stream_type}Connection: close\r\n\r\n").into_bytes();
    answer.extend_from_slice(&body);
    Ok(answer)
}

fn turn_facts() -> Value {
    json!({"accepted":false,"error_code":null,"dispatch":null,"retriable":null,
        "diagnostics_safe":true,"requests":0,"request_line":null,"request_body":null,
        "credential_scope":null,"signature_verifies":null,"security_token_sent":false,
        "items":[],"text":"","reasoning":"","tool_stream":[],"warnings":[],"stop_reason":null,
        "upstream_model":null,"response_id":null,"final_usage":null,"usage_json":null})
}

fn turn(input: &Value) -> Result<Value, TargetError> {
    let input: TurnInput = serde_json::from_value(input.clone()).map_err(unavailable)?;
    let prepared = serde_json::from_str::<TurnDocument>(&input.request_json)
        .map_err(|_| Error::invalid("the request is not a turn document"))
        .and_then(|document| turn_answer(&input).map(|answer| (document, answer)));
    match prepared {
        Ok((document, answer)) => {
            runtime()?.block_on(turn_on_loopback(&input, &document.request, answer))
        }
        Err(error) => {
            let mut facts = turn_facts();
            report_error(&error, &mut facts);
            Ok(facts)
        }
    }
}

async fn turn_on_loopback(
    input: &TurnInput,
    request: &TurnRequest,
    answer: Vec<u8>,
) -> Result<Value, TargetError> {
    let mut facts = turn_facts();
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(unavailable)?;
    let endpoint = Url::parse(&format!(
        "http://{}",
        listener.local_addr().map_err(unavailable)?
    ))
    .map_err(unavailable)?;
    let captured = Arc::new(Mutex::new(Vec::new()));
    let server = tokio::spawn(serve(listener, captured.clone(), move || answer.clone()));
    let mut sink = VecSink::new(MAX_SINK_EVENTS, MAX_SINK_BYTES);
    let outcome = run_turn(input, request, endpoint, &mut sink).await;
    server.abort();
    report_turn_sink(&sink, &mut facts);
    match &outcome {
        Ok(outcome) => {
            facts["accepted"] = json!(true);
            report_turn_outcome(outcome, &mut facts);
        }
        Err(error) => {
            report_error(error, &mut facts);
            facts["diagnostics_safe"] = json!(clean(
                error,
                &[PROVIDER_MESSAGE, SECRET_ACCESS_KEY, SESSION_TOKEN]
            ));
            if let Some(observation) = &error.observation {
                facts["upstream_model"] = json!(observation.upstream_model);
                facts["response_id"] = json!(observation.response_id);
                facts["final_usage"] = json!(observation.final_usage);
                facts["usage_json"] = json!(usage_json(observation.usage.as_ref()));
            }
        }
    }
    let captured = captured
        .lock()
        .map(|captured| captured.clone())
        .unwrap_or_default();
    facts["requests"] = json!(captured.len());
    if let Some(first) = captured.first() {
        report_request(first, input.session_token, &mut facts);
    }
    Ok(facts)
}

async fn run_turn(
    input: &TurnInput,
    request: &TurnRequest,
    endpoint: Url,
    sink: &mut VecSink,
) -> Result<TurnOutcome, Error> {
    let (api, protocol) = match input.api.as_str() {
        "Converse" => (Api::Converse, Protocol::BedrockConverse),
        "AnthropicInvoke" => (Api::AnthropicInvoke, Protocol::Messages),
        _ => return Err(Error::invalid("unknown Bedrock API")),
    };
    let id = |value: &str| Id::new(value).map_err(|_| Error::invalid("fixture identifier"));
    let target = BedrockTarget {
        provenance: Provenance {
            protocol,
            provider: id("bedrock")?,
            account: id("fixture-account")?,
            endpoint: id("loopback")?,
            model: id(&input.model_id)?,
            binding_revision: id("fixture-revision")?,
        },
        capabilities: Capabilities {
            tools: true,
            tool_choice: true,
            temperature: true,
            top_p: true,
            reasoning_efforts: Vec::new(),
            context_window: 200_000,
            max_output_tokens: 4096,
            output_streaming: input.stream,
            input_during_output: false,
        },
        region: REGION.to_owned(),
        model_id: input.model_id.clone(),
        api,
        stream: input.stream,
        endpoint: Some(endpoint),
    };
    let client = BedrockClient::new(
        target,
        http_client()?,
        Arc::new(FixtureCredentials {
            token: input.session_token,
        }),
    )?;
    client.turn(request, sink, &Cancel::new()).await
}

/// The request as the server received it, and whether its signature is the one `sign` gives.
fn report_request(request: &Captured, token: bool, facts: &mut Value) {
    facts["request_line"] = json!(request.request_line());
    facts["request_body"] = json!(
        serde_json::from_slice::<Value>(&request.body)
            .ok()
            .map(|body| body.to_string())
    );
    facts["security_token_sent"] = json!(request.header("x-amz-security-token") == [SESSION_TOKEN]);
    let authorization = request.header("authorization");
    let [authorization] = authorization.as_slice() else {
        return;
    };
    facts["credential_scope"] = json!(credential_scope(authorization));
    facts["signature_verifies"] = json!(verifies(request, authorization, token));
}

/// `AKIDEXAMPLE/<date>/eu-central-1/bedrock/aws4_request` of an `authorization` value.
fn credential_scope(authorization: &str) -> Option<String> {
    let credential = authorization
        .split_once("Credential=")?
        .1
        .split(',')
        .next()?
        .trim();
    let mut parts: Vec<&str> = credential.split('/').collect();
    let date = parts.get_mut(1)?;
    *date = "<date>";
    Some(parts.join("/"))
}

/// Whether signing the headers the request names as signed, its target, its body and its
/// `x-amz-date` again gives back the `authorization` it carried.
fn verifies(request: &Captured, authorization: &str, token: bool) -> bool {
    let check = || -> Option<bool> {
        let signed = authorization
            .split_once("SignedHeaders=")?
            .1
            .split(',')
            .next()?
            .trim();
        let mut headers = HeaderMap::new();
        for name in signed.split(';') {
            let header = HeaderName::from_bytes(name.as_bytes()).ok()?;
            for value in request.header(name) {
                headers.append(header.clone(), HeaderValue::from_str(value).ok()?);
            }
        }
        let time = parse_amz_date(request.header("x-amz-date").first()?)?;
        let host = *request.header("host").first()?;
        let line = request.request_line();
        let (method, target) = line.split_once(' ')?;
        let url = Url::parse(&format!("http://{host}{target}")).ok()?;
        let method = http::Method::from_bytes(method.as_bytes()).ok()?;
        let credentials = FixtureCredentials { token }.get();
        let params = SigningParams {
            credentials: &credentials,
            region: REGION,
            service: SERVICE,
            time,
        };
        sign(&method, &url, &mut headers, &request.body, &params).ok()?;
        Some(headers.get("authorization")?.to_str().ok()? == authorization)
    };
    check().unwrap_or(false)
}

fn report_turn_sink(sink: &VecSink, facts: &mut Value) {
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut tool_stream = Vec::new();
    let mut warnings = Vec::new();
    for event in sink.events() {
        match event {
            StreamEvent::TextDelta { text: delta } => text.push_str(delta),
            StreamEvent::ReasoningDelta { text: delta } => reasoning.push_str(delta),
            StreamEvent::ToolCallStarted { call_id, name } => {
                tool_stream.push(format!("started:{call_id}:{name}"));
            }
            StreamEvent::ToolArgumentsDelta { call_id, delta } => {
                tool_stream.push(format!("arguments:{call_id}:{delta}"));
            }
            StreamEvent::Warning { code, message } => warnings.push(format!("{code}:{message}")),
        }
    }
    facts["text"] = json!(text);
    facts["reasoning"] = json!(reasoning);
    facts["tool_stream"] = json!(tool_stream);
    facts["warnings"] = json!(warnings);
}

fn usage_json(usage: Option<&llm_core::Usage>) -> Option<String> {
    usage.and_then(|usage| serde_json::to_string(usage).ok())
}

fn report_turn_outcome(outcome: &TurnOutcome, facts: &mut Value) {
    facts["items"] = json!(outcome.items.iter().map(label).collect::<Vec<_>>());
    facts["stop_reason"] = json!(stop_label(&outcome.stop_reason));
    facts["upstream_model"] = json!(outcome.observation.upstream_model);
    facts["response_id"] = json!(outcome.observation.response_id);
    facts["final_usage"] = json!(outcome.observation.final_usage);
    facts["usage_json"] = json!(usage_json(outcome.observation.usage.as_ref()));
}

/// An item labelled as `llm.messages.CodecInspection` labels one.
fn label(item: &Item) -> String {
    match item {
        Item::UserText { text } => format!("user-text:{text}"),
        Item::AssistantText { text } => format!("assistant-text:{text}"),
        Item::ToolCall(call) => format!(
            "tool-call:{}:{}:{}",
            call.name, call.call_id, call.arguments
        ),
        Item::ToolResult {
            call_id,
            output,
            failed,
        } => format!("tool-result:{call_id}:{failed}:{output}"),
        Item::Opaque {
            provenance,
            payload,
        } => format!(
            "opaque:{}:{payload}",
            serde_json::to_string(provenance).unwrap_or_default()
        ),
        Item::UnattributedOpaque { protocol, payload } => format!(
            "unattributed-opaque:{}:{payload}",
            serde_json::to_string(protocol).unwrap_or_default()
        ),
    }
}

fn stop_label(stop: &StopReason) -> String {
    match stop {
        StopReason::EndTurn => "end-turn".to_owned(),
        StopReason::ToolCalls => "tool-calls".to_owned(),
        StopReason::MaxOutputTokens => "max-output-tokens".to_owned(),
        StopReason::Incomplete { reason } => format!("incomplete:{reason}"),
    }
}

// ---------------------------------------------------------------------------------------------
// llm.bedrock.FetchContainerCredentials: `ContainerCredentials` against a scripted endpoint.
// ---------------------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ContainerInput {
    #[serde(default)]
    uri: Option<String>,
    answer: String,
    token: bool,
    #[serde(deserialize_with = "crate::target::token_bound")]
    fetches: Option<u64>,
}

const MAX_FETCHES: u64 = 4;
const TOKEN_REFERENCE: &str = "container-token";
const CONTAINER_TOKEN: &str = "llm-fixture-container-authorization-token";
const CONTAINER_KEY_ID: &str = "ASIAFIXTUREKEYID";
const CONTAINER_SECRET: &str = "llm-fixture-container-secret-access-key";
const CONTAINER_SESSION: &str = "llm-fixture-container-session-token";
const CONTAINER_REFUSAL: &str = "llm-fixture-container-refusal-text";

/// Answers the container token for its reference while `known`, and reports it missing
/// otherwise.
struct TokenResolver {
    known: bool,
}

impl SecretResolver for TokenResolver {
    fn resolve<'a>(
        &'a self,
        reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        let known = self.known && reference.as_str() == TOKEN_REFERENCE;
        Box::pin(async move {
            if !known {
                return Err(SecretError::Missing);
            }
            Ok(ResolvedSecret {
                secret: Secret::new(CONTAINER_TOKEN.as_bytes().to_vec())?,
                version: SecretVersion::new("generation-1".to_owned())?,
            })
        })
    }
}

/// What the scripted endpoint answers each request, its expiry counted from the request.
fn container_answer(answer: &str) -> Vec<u8> {
    let expiring = |seconds: u64| {
        format_timestamp(SystemTime::now() + Duration::from_secs(seconds)).unwrap_or_default()
    };
    let credentials = |expiration: String| {
        json!({"Code":"Success","Type":"AWS-HMAC","AccessKeyId":CONTAINER_KEY_ID,
            "SecretAccessKey":CONTAINER_SECRET,"Token":CONTAINER_SESSION,
            "Expiration":expiration})
    };
    let json_header = "Content-Type: application/json\r\n";
    match answer {
        "Valid" => http_answer(
            "200 OK",
            json_header,
            credentials(expiring(3600)).to_string().as_bytes(),
        ),
        "ExpiresSoon" => http_answer(
            "200 OK",
            json_header,
            credentials(expiring(120)).to_string().as_bytes(),
        ),
        "MissingSecret" => {
            let mut body = credentials(expiring(3600));
            if let Some(object) = body.as_object_mut() {
                object.remove("SecretAccessKey");
            }
            http_answer("200 OK", json_header, body.to_string().as_bytes())
        }
        _ => http_answer(
            "401 Unauthorized",
            json_header,
            json!({"message": CONTAINER_REFUSAL}).to_string().as_bytes(),
        ),
    }
}

fn container_facts() -> Value {
    json!({"accepted":false,"error_code":null,"dispatch":null,"retriable":null,
        "requests":0,"token_presented":null,"access_key_id":null,"session_token":null,
        "debug_redacted":null,"diagnostics_safe":true})
}

fn container(input: &Value) -> Result<Value, TargetError> {
    let input: ContainerInput = serde_json::from_value(input.clone()).map_err(unavailable)?;
    let fetches = input.fetches.unwrap_or(0);
    let scripted = matches!(
        input.answer.as_str(),
        "Valid" | "ExpiresSoon" | "Unauthorized" | "MissingSecret" | "Unreachable"
    );
    if fetches == 0 || fetches > MAX_FETCHES || !scripted {
        let mut facts = container_facts();
        report_error(
            &Error::invalid("the fetch count or the scripted answer is outside this adapter"),
            &mut facts,
        );
        return Ok(facts);
    }
    runtime()?.block_on(fetch_on_loopback(&input, fetches))
}

async fn fetch_on_loopback(input: &ContainerInput, fetches: u64) -> Result<Value, TargetError> {
    let mut facts = container_facts();
    let resolver = Arc::new(TokenResolver { known: input.token });
    let reference = SecretRef::new(TOKEN_REFERENCE).map_err(unavailable)?;
    let http = http_client().map_err(unavailable)?;
    let secrets = [
        CONTAINER_TOKEN,
        CONTAINER_KEY_ID,
        CONTAINER_SECRET,
        CONTAINER_SESSION,
        CONTAINER_REFUSAL,
    ];
    if let Some(uri) = &input.uri {
        // A literal URI is only given to the constructor: nothing is fetched from it.
        match ContainerCredentials::new(http, uri, resolver, reference) {
            Ok(_) => facts["accepted"] = json!(true),
            Err(error) => {
                report_error(&error, &mut facts);
                facts["diagnostics_safe"] =
                    json!(clean(&error, &secrets) && !error.message.contains(uri.as_str()));
            }
        }
        return Ok(facts);
    }
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(unavailable)?;
    let address = listener.local_addr().map_err(unavailable)?;
    let captured = Arc::new(Mutex::new(Vec::new()));
    let server = if input.answer == "Unreachable" {
        // Nothing listens on the address once the listener is gone.
        drop(listener);
        None
    } else {
        let answer = input.answer.clone();
        Some(tokio::spawn(serve(listener, captured.clone(), move || {
            container_answer(&answer)
        })))
    };
    let uri = format!("http://{address}/v1/credentials");
    let outcome = match ContainerCredentials::new(http, &uri, resolver, reference) {
        Ok(source) => {
            let cancel = Cancel::new();
            let mut last = None;
            let mut failure = None;
            for _ in 0..fetches {
                match source.credentials(&cancel).await {
                    Ok(credentials) => last = Some(credentials),
                    Err(error) => {
                        failure = Some(error);
                        break;
                    }
                }
            }
            (last, failure)
        }
        Err(error) => (None, Some(error)),
    };
    if let Some(server) = server {
        server.abort();
    }
    let captured = captured
        .lock()
        .map(|captured| captured.clone())
        .unwrap_or_default();
    facts["requests"] = json!(captured.len());
    if !captured.is_empty() {
        facts["token_presented"] = json!(
            captured
                .iter()
                .all(|request| request.header("authorization") == [CONTAINER_TOKEN])
        );
    }
    let (last, failure) = outcome;
    if let Some(credentials) = &last {
        let debug = format!("{credentials:?}");
        facts["access_key_id"] = json!(credentials.access_key_id());
        facts["session_token"] = json!(credentials.session_token() == Some(CONTAINER_SESSION));
        facts["debug_redacted"] =
            json!(!debug.contains(CONTAINER_SECRET) && !debug.contains(CONTAINER_SESSION));
    }
    match &failure {
        None => facts["accepted"] = json!(true),
        Some(error) => {
            report_error(error, &mut facts);
            facts["diagnostics_safe"] = json!(clean(error, &secrets));
        }
    }
    Ok(facts)
}
