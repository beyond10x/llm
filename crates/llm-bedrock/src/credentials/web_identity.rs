//! Credentials from the AWS Security Token Service's `AssumeRoleWithWebIdentity`, as IAM roles
//! for service accounts (IRSA) hand a pod a role ARN and a projected token file.

use super::{
    AwsCredentials, CredentialSource, Fetch, MAX_PART_BYTES, Refreshing, resolve, unauthorized,
    without_line_end,
};
use crate::time::parse_rfc3339_utc;
use llm_core::{BoxFuture, Cancel, Dispatch, Error, ErrorCode};
use llm_credentials::{SecretRef, SecretResolver};
use llm_http::{FormAnswer, HeaderMap, HttpClient, status_error};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_encode};
use std::fmt;
use std::ops::RangeInclusive;
use std::sync::Arc;
use std::time::SystemTime;
use url::{Host, Url};
use zeroize::Zeroizing;

/// The query API version `AssumeRoleWithWebIdentity` is asked in.
const VERSION: &str = "2011-06-15";
/// The longest web identity token STS takes: 20000 characters.
const MAX_TOKEN_BYTES: usize = 20_000;
/// The role ARN lengths STS takes.
const ROLE_ARN_BYTES: RangeInclusive<usize> = 20..=2048;
/// The session name lengths STS takes.
const SESSION_NAME_BYTES: RangeInclusive<usize> = 2..=64;
/// How deep the answer reader follows elements.
const MAX_DEPTH: usize = 16;
/// Every byte of a form value but the unreserved characters of RFC 3986 becomes `%XX`.
const FORM_VALUE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');
/// The path from the answer's root to its `Credentials`, and the four members read there.
const CREDENTIALS: [&str; 3] = [
    "AssumeRoleWithWebIdentityResponse",
    "AssumeRoleWithWebIdentityResult",
    "Credentials",
];
const MEMBERS: [&str; 4] = [
    "AccessKeyId",
    "SecretAccessKey",
    "SessionToken",
    "Expiration",
];
/// The path from an error document's root to its `Error`, whose `Code` is read.
const ERROR: [&str; 2] = ["ErrorResponse", "Error"];
/// The error codes a message may name: those the `AssumeRoleWithWebIdentity` reference lists,
/// and the access and throttling refusals STS answers any action with. Any other code is read
/// for its class only and never written into a message.
const NAMED_CODES: [&str; 10] = [
    "ExpiredToken",
    "IDPCommunicationError",
    "IDPRejectedClaim",
    "InvalidIdentityToken",
    "MalformedPolicyDocument",
    "PackedPolicyTooLarge",
    "RegionDisabledException",
    "AccessDenied",
    "Throttling",
    "ValidationError",
];
/// The one refusal STS documents as worth asking again: it could not reach the identity
/// provider that issued the token.
const IDP_UNREACHABLE: &str = "IDPCommunicationError";

/// Temporary credentials for a role, from STS `AssumeRoleWithWebIdentity`: the source behind IAM
/// roles for service accounts (IRSA), where a pod gets `AWS_ROLE_ARN` and
/// `AWS_WEB_IDENTITY_TOKEN_FILE`.
///
/// Each fetch resolves the token reference again, so a token file the platform rewrote is read
/// as it is now, and sends one unsigned POST to the regional STS endpoint: the form
/// `Action=AssumeRoleWithWebIdentity`, `Version=2011-06-15`, `RoleArn`, `RoleSessionName` and
/// `WebIdentityToken`, each value percent-encoded, with no `authorization` and no `x-amz-*`
/// header. The token travels in the body, never in the URL. One trailing line end of the token is
/// removed; what is left must be non-empty visible ASCII of at most 20000 bytes.
///
/// The XML answer is read once: the four members of its `Credentials`, `AccessKeyId`,
/// `SecretAccessKey`, `SessionToken` and `Expiration`, go straight into zeroizing strings, and
/// no other copy of the answer is built. A member missing, empty or named twice, a second
/// `Credentials`, a member holding an element, a comment, a document type declaration or an
/// entity other than the five predefined ones and character references is refused. The
/// credentials are kept and renewed as [`super::ContainerCredentials`] keeps its own: one fetch
/// at a time, inside five minutes before they expire, on its own task.
///
/// `ExpiredToken`, `InvalidIdentityToken`, every other 4xx, a redirect and an unusable answer
/// are `unauthorized`, final. An endpoint that does not answer, cuts its answer or answers 408
/// or 5xx, and STS's `IDPCommunicationError`, are `transport` and retriable, a `Retry-After`
/// kept. Every failure is dispatch `not-sent`. A message may name an error code STS documents,
/// never the token, the answer, a key or the endpoint.
pub struct WebIdentityCredentials {
    refreshing: Refreshing<WebIdentityFetch>,
}

/// One `AssumeRoleWithWebIdentity` call: where it goes, for which role and session, and the token
/// reference it is asked with.
struct WebIdentityFetch {
    http: HttpClient,
    endpoint: Url,
    role_arn: String,
    session_name: String,
    resolver: Arc<dyn SecretResolver>,
    token: SecretRef,
}

impl WebIdentityCredentials {
    /// A source assuming `role_arn` as `session_name` at the STS `endpoint`, such as
    /// `https://sts.eu-central-1.amazonaws.com`, with the token the resolver gives for `token`.
    ///
    /// The endpoint must be `https`, or `http` to a loopback address for a fixture; a host name
    /// over `http`, `localhost` included, is not admitted. Nothing is fetched before the first
    /// call.
    ///
    /// # Errors
    /// `unauthorized` for an endpoint that does not parse, carries user information, a query or a
    /// fragment, or is neither `https` nor `http` to a loopback address; for a role ARN that is not
    /// 20 to 2048 visible ASCII characters starting `arn:`; and for a session name that is not 2 to
    /// 64 of the characters STS takes, letters, digits and `+=,.@_-`. No message quotes the
    /// endpoint, the role or the session name.
    pub fn new(
        http: HttpClient,
        endpoint: &str,
        role_arn: impl Into<String>,
        session_name: impl Into<String>,
        resolver: Arc<dyn SecretResolver>,
        token: SecretRef,
    ) -> Result<Self, Error> {
        let endpoint = admitted_endpoint(endpoint)?;
        let role_arn = role_arn.into();
        if !ROLE_ARN_BYTES.contains(&role_arn.len())
            || !role_arn.starts_with("arn:")
            || !role_arn.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(unauthorized(
                "web identity role ARN is not one STS takes: 20 to 2048 visible ASCII characters \
                 starting `arn:`",
            ));
        }
        let session_name = session_name.into();
        if !SESSION_NAME_BYTES.contains(&session_name.len())
            || !session_name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"+=,.@_-".contains(&byte))
        {
            return Err(unauthorized(
                "web identity session name is not one STS takes: 2 to 64 letters, digits or \
                 `+=,.@_-`",
            ));
        }
        Ok(Self {
            refreshing: Refreshing::new(WebIdentityFetch {
                http,
                endpoint,
                role_arn,
                session_name,
                resolver,
                token,
            }),
        })
    }

    /// Whether a fetch is in flight now.
    pub fn refreshing(&self) -> bool {
        self.refreshing.in_flight()
    }
}

impl fmt::Debug for WebIdentityCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let fetch = self.refreshing.fetcher();
        f.debug_struct("WebIdentityCredentials")
            .field("endpoint", &fetch.endpoint.origin().ascii_serialization())
            .field("role_arn", &fetch.role_arn)
            .field("session_name", &fetch.session_name)
            .field("token", &fetch.token)
            .finish_non_exhaustive()
    }
}

impl CredentialSource for WebIdentityCredentials {
    fn credentials<'a>(
        &'a self,
        cancel: &'a Cancel,
    ) -> BoxFuture<'a, Result<AwsCredentials, Error>> {
        Box::pin(self.refreshing.credentials(cancel))
    }
}

impl Fetch for WebIdentityFetch {
    fn fetch<'a>(&'a self, cancel: &'a Cancel) -> BoxFuture<'a, Result<AwsCredentials, Error>> {
        Box::pin(async move {
            let form = self.form(cancel).await?;
            let answer = self
                .http
                .post_form(self.endpoint.as_str(), HeaderMap::new(), form, cancel)
                .await
                .map_err(exchange_error)?;
            read_answer(&answer, SystemTime::now())
        })
    }

    fn abandoned(&self) -> Error {
        Error::new(
            ErrorCode::Transport,
            "web identity credentials fetch ended without an answer",
        )
        .with_dispatch(Dispatch::NotSent)
        .with_retriable(true)
    }
}

impl WebIdentityFetch {
    /// The `AssumeRoleWithWebIdentity` form, with the token the resolver gives now. It is written
    /// once into a buffer of its exact length, so no smaller buffer holding part of the token is
    /// freed on the way; the transport zeroizes it, and the resolved token is zeroized on return.
    async fn form(&self, cancel: &Cancel) -> Result<Vec<u8>, Error> {
        let resolved = resolve(self.resolver.as_ref(), &self.token, cancel).await?;
        let token = without_line_end(resolved.secret.expose());
        if token.is_empty()
            || token.len() > MAX_TOKEN_BYTES
            || !token.iter().all(u8::is_ascii_graphic)
        {
            return Err(unauthorized(format!(
                "secret reference `{}` is not a web identity token",
                self.token.as_str()
            )));
        }
        let fields: [(&str, &[u8]); 5] = [
            ("Action", b"AssumeRoleWithWebIdentity"),
            ("Version", VERSION.as_bytes()),
            ("RoleArn", self.role_arn.as_bytes()),
            ("RoleSessionName", self.session_name.as_bytes()),
            ("WebIdentityToken", token),
        ];
        let length = fields
            .iter()
            .map(|(name, value)| {
                name.len()
                    + 1
                    + percent_encode(value, FORM_VALUE)
                        .map(str::len)
                        .sum::<usize>()
            })
            .sum::<usize>()
            + fields.len()
            - 1;
        let mut form = Vec::with_capacity(length);
        for (index, (name, value)) in fields.iter().enumerate() {
            if index > 0 {
                form.push(b'&');
            }
            form.extend_from_slice(name.as_bytes());
            form.push(b'=');
            for piece in percent_encode(value, FORM_VALUE) {
                form.extend_from_slice(piece.as_bytes());
            }
        }
        Ok(form)
    }
}

/// The STS endpoint, refused unless it is `https`, or `http` to a loopback address, with no user
/// information, query or fragment.
fn admitted_endpoint(text: &str) -> Result<Url, Error> {
    let refused = || {
        unauthorized(
            "web identity STS endpoint is not https or http to a loopback address, or carries \
             user information, a query or a fragment",
        )
    };
    let uri = Url::parse(text).map_err(|_| refused())?;
    if !uri.username().is_empty()
        || uri.password().is_some()
        || uri.query().is_some()
        || uri.fragment().is_some()
    {
        return Err(refused());
    }
    let admitted = match (uri.scheme(), uri.host()) {
        ("https", Some(_)) => true,
        ("http", Some(Host::Ipv4(address))) => address.is_loopback(),
        ("http", Some(Host::Ipv6(address))) => address.is_loopback(),
        _ => false,
    };
    if admitted { Ok(uri) } else { Err(refused()) }
}

/// The transport's failure before an answer, as a credential failure: cancellation stays
/// cancellation and a missing runtime stays unsupported; no answer, a cut answer and a late one
/// keep their `transport` or `deadline` class and are retriable; anything else is `unauthorized`.
/// Each is `not-sent`. The transport's messages are fixed texts, never a body.
fn exchange_error(error: Error) -> Error {
    match error.code {
        ErrorCode::Cancelled => Error::cancelled(),
        ErrorCode::Unsupported => error.with_dispatch(Dispatch::NotSent).with_retriable(false),
        ErrorCode::Transport | ErrorCode::Deadline => {
            let mut failed = error;
            failed.message = format!("web identity STS endpoint failed: {}", failed.message);
            failed.with_dispatch(Dispatch::NotSent).with_retriable(true)
        }
        _ => unauthorized(format!(
            "web identity STS endpoint failed: {}",
            error.message
        )),
    }
}

/// What STS answered, as credentials or the failure its status and error code make it.
fn read_answer(answer: &FormAnswer, now: SystemTime) -> Result<AwsCredentials, Error> {
    let status = answer.status();
    if (200..300).contains(&status) {
        return read_credentials(answer.body().unwrap_or_default(), now)
            .map_err(|reason| unauthorized(format!("web identity STS answer {reason}")));
    }
    let failure = status_error(status, answer.headers(), now);
    if !(400..500).contains(&status) || failure.code == ErrorCode::Transport {
        return Err(exchange_error(failure));
    }
    match answer.body().and_then(named_code) {
        Some(IDP_UNREACHABLE) => Err(Error::new(
            ErrorCode::Transport,
            format!("web identity STS endpoint could not reach the identity provider ({IDP_UNREACHABLE})"),
        )
        .with_dispatch(Dispatch::NotSent)
        .with_retriable(true)),
        Some(code) => Err(unauthorized(format!(
            "web identity STS endpoint refused the request ({code}) with status {status}"
        ))),
        None => Err(unauthorized(format!(
            "web identity STS endpoint refused the request with status {status}"
        ))),
    }
}

/// The documented code an STS error document names, if it names one of [`NAMED_CODES`].
fn named_code(body: &[u8]) -> Option<&'static str> {
    let mut code: [Option<Zeroizing<String>>; 1] = [None];
    read_members(body, &ERROR, &["Code"], &mut code).ok()?;
    let [code] = code;
    let code = code?;
    NAMED_CODES
        .iter()
        .copied()
        .find(|named| *named == code.as_str())
}

/// The credentials in an `AssumeRoleWithWebIdentity` answer, or the fixed reason they cannot be
/// used. No reason quotes the answer.
fn read_credentials(body: &[u8], now: SystemTime) -> Result<AwsCredentials, &'static str> {
    let mut members: [Option<Zeroizing<String>>; 4] = [None, None, None, None];
    read_members(body, &CREDENTIALS, &MEMBERS, &mut members)?;
    let [access_key_id, secret_access_key, session_token, expiration] = members;
    let access_key_id = access_key_id
        .filter(|part| usable(part))
        .ok_or("has no usable `AccessKeyId`")?;
    let secret_access_key = secret_access_key
        .filter(|part| usable(part))
        .ok_or("has no usable `SecretAccessKey`")?;
    let session_token = session_token
        .filter(|part| usable(part))
        .ok_or("has no usable `SessionToken`")?;
    let expiration = expiration
        .filter(|value| !value.is_empty())
        .ok_or("has no usable `Expiration`")?;
    let expires =
        parse_rfc3339_utc(&expiration).ok_or("has an `Expiration` that is not a UTC instant")?;
    if expires <= now {
        return Err("has already expired");
    }
    Ok(AwsCredentials {
        access_key_id: access_key_id.as_str().to_owned(),
        secret_access_key,
        session_token: Some(session_token),
        expires: Some(expires),
    })
}

/// A key part: non-empty visible ASCII of at most 16 KiB.
fn usable(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= MAX_PART_BYTES
        && part.bytes().all(|byte| byte.is_ascii_graphic())
}

// ---------------------------------------------------------------------------------------------
// The XML an STS answer is written in, read without a parser dependency.
//
// The subset: an optional XML declaration first, elements with attributes, text, the five
// predefined entities and character references. A comment, a CDATA section, a processing
// instruction after the declaration, a document type declaration and any other entity are
// refused, so no entity is ever expanded. Names are compared as written, without namespaces.
// ---------------------------------------------------------------------------------------------

/// Reads `body` and decodes, into `slots`, the text of each element named in `names` that is a
/// child of the element at `parent` (a path from the root, which must be the document's root).
/// The elements on `parent` and each named member may appear once; a member may hold text only.
/// A member that is present but empty leaves its slot empty.
fn read_members(
    body: &[u8],
    parent: &[&str],
    names: &[&str],
    slots: &mut [Option<Zeroizing<String>>],
) -> Result<(), &'static str> {
    let text = std::str::from_utf8(body).map_err(|_| "is not UTF-8")?;
    let mut reader = Reader::new(text)?;
    let mut path: Vec<&str> = Vec::new();
    let mut opened = vec![false; parent.len()];
    let mut present = vec![false; names.len()];
    let mut rooted = false;
    let member = |path: &[&str]| -> Option<usize> {
        let (last, above) = path.split_last()?;
        (above == parent)
            .then(|| names.iter().position(|name| name == last))
            .flatten()
    };
    while let Some(token) = reader.next()? {
        match token {
            Token::Text(raw) => match member(&path) {
                Some(index) => slots[index] = Some(decode(raw)?),
                None if path.is_empty() && !raw.chars().all(|c| SPACE.contains(&c)) => {
                    return Err("has text outside its root element");
                }
                None => validate(raw)?,
            },
            Token::Open { name, empty } => {
                if path.is_empty() {
                    if rooted {
                        return Err("has more than one root element");
                    }
                    rooted = true;
                    if parent.first() != Some(&name) {
                        return Err("is not the document expected");
                    }
                }
                if path.len() >= MAX_DEPTH {
                    return Err("nests elements too deep");
                }
                if member(&path).is_some() {
                    return Err("has a member holding an element");
                }
                path.push(name);
                if parent.get(..path.len()) == Some(&path[..]) {
                    let seen = &mut opened[path.len() - 1];
                    if *seen {
                        return Err("names an element twice");
                    }
                    *seen = true;
                } else if let Some(index) = member(&path) {
                    if present[index] {
                        return Err("names a member twice");
                    }
                    present[index] = true;
                }
                if empty {
                    path.pop();
                }
            }
            Token::Close(name) => {
                if path.pop() != Some(name) {
                    return Err("closes an element it did not open");
                }
            }
        }
    }
    if !rooted || !path.is_empty() {
        return Err("ends before its root element closes");
    }
    Ok(())
}

/// XML white space.
const SPACE: [char; 4] = [' ', '\t', '\r', '\n'];

/// One piece of the document: a start tag (`empty` when it closes itself), an end tag, or the
/// raw text between two tags.
enum Token<'a> {
    Open { name: &'a str, empty: bool },
    Close(&'a str),
    Text(&'a str),
}

struct Reader<'a> {
    rest: &'a str,
}

impl<'a> Reader<'a> {
    /// A reader past the XML declaration, when the document starts with one.
    fn new(text: &'a str) -> Result<Self, &'static str> {
        let mut rest = text;
        if let Some(declaration) = text.strip_prefix("<?xml")
            && declaration.starts_with(SPACE)
        {
            let end = declaration
                .find("?>")
                .ok_or("has an unterminated declaration")?;
            rest = &declaration[end + 2..];
        }
        Ok(Self { rest })
    }

    fn next(&mut self) -> Result<Option<Token<'a>>, &'static str> {
        if self.rest.is_empty() {
            return Ok(None);
        }
        let Some(tag) = self.rest.strip_prefix('<') else {
            let end = self.rest.find('<').unwrap_or(self.rest.len());
            let (text, rest) = self.rest.split_at(end);
            self.rest = rest;
            return Ok(Some(Token::Text(text)));
        };
        let end = tag.find('>').ok_or("has an unterminated tag")?;
        let (tag, rest) = (&tag[..end], &tag[end + 1..]);
        self.rest = rest;
        if let Some(name) = tag.strip_prefix('/') {
            let name = name.trim_end_matches(SPACE);
            checked_name(name)?;
            return Ok(Some(Token::Close(name)));
        }
        if tag.starts_with(['!', '?']) {
            return Err("has a comment, section, declaration or instruction the reader refuses");
        }
        let (tag, empty) = tag
            .strip_suffix('/')
            .map_or((tag, false), |tag| (tag, true));
        let name_end = tag.find(SPACE).unwrap_or(tag.len());
        let (name, attributes) = tag.split_at(name_end);
        checked_name(name)?;
        checked_attributes(attributes)?;
        Ok(Some(Token::Open { name, empty }))
    }
}

/// An ASCII XML name: a letter, `_` or `:`, then letters, digits, `_`, `-`, `.` or `:`.
fn checked_name(name: &str) -> Result<(), &'static str> {
    let mut bytes = name.bytes();
    let first = bytes.next().ok_or("has an element without a name")?;
    if (first.is_ascii_alphabetic() || matches!(first, b'_' | b':'))
        && bytes
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
    {
        Ok(())
    } else {
        Err("has a name the reader does not take")
    }
}

/// Attributes after a start tag's name: each `name="value"` or `name='value'`, preceded by white
/// space, its value free of `<` and of malformed references. Their values are not kept.
fn checked_attributes(mut rest: &str) -> Result<(), &'static str> {
    const MALFORMED: &str = "has a malformed attribute";
    loop {
        let trimmed = rest.trim_start_matches(SPACE);
        if trimmed.is_empty() {
            return Ok(());
        }
        if trimmed.len() == rest.len() {
            return Err(MALFORMED);
        }
        let equals = trimmed.find('=').ok_or(MALFORMED)?;
        checked_name(trimmed[..equals].trim_end_matches(SPACE)).map_err(|_| MALFORMED)?;
        let value = trimmed[equals + 1..].trim_start_matches(SPACE);
        let quote = value
            .chars()
            .next()
            .filter(|quote| matches!(quote, '"' | '\''))
            .ok_or(MALFORMED)?;
        let end = value[1..].find(quote).ok_or(MALFORMED)?;
        let content = &value[1..=end];
        if content.contains('<') {
            return Err(MALFORMED);
        }
        validate(content)?;
        rest = &value[end + 2..];
    }
}

/// `raw` with its references replaced, in a zeroizing string reserved at `raw`'s length, which
/// the decoded text never exceeds, so it is never reallocated.
fn decode(raw: &str) -> Result<Zeroizing<String>, &'static str> {
    let mut decoded = Zeroizing::new(String::with_capacity(raw.len()));
    unescape(raw, |c| decoded.push(c))?;
    Ok(decoded)
}

/// Checks the references in text whose value is not kept.
fn validate(raw: &str) -> Result<(), &'static str> {
    unescape(raw, |_| ())
}

/// Hands `emit` each character of `raw`, a predefined entity or character reference as the
/// character it names. Any other reference, and a character reference to a character XML does
/// not allow, is refused.
fn unescape(raw: &str, mut emit: impl FnMut(char)) -> Result<(), &'static str> {
    const REFUSED: &str = "has a reference the reader does not take";
    let mut rest = raw;
    while let Some(at) = rest.find('&') {
        rest[..at].chars().for_each(&mut emit);
        let after = &rest[at + 1..];
        let end = after.find(';').ok_or(REFUSED)?;
        let character = match &after[..end] {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            reference => {
                let number = reference.strip_prefix('#').ok_or(REFUSED)?;
                let (digits, radix) = number
                    .strip_prefix('x')
                    .map_or((number, 10), |hex| (hex, 16));
                if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
                    return Err(REFUSED);
                }
                u32::from_str_radix(digits, radix)
                    .ok()
                    .and_then(char::from_u32)
                    .filter(|c| {
                        matches!(c, '\t' | '\n' | '\r')
                            || (*c >= ' ' && !matches!(c, '\u{fffe}' | '\u{ffff}'))
                    })
                    .ok_or(REFUSED)?
            }
        };
        emit(character);
        rest = &after[end + 1..];
    }
    rest.chars().for_each(emit);
    Ok(())
}
