//! AWS Signature Version 4 for request headers, written on `ring`.
//!
//! [`sign`] adds `host` (when absent), `x-amz-date`, `x-amz-security-token` (when the credentials
//! carry a session token) and `authorization` to a request, and signs every header the request
//! then carries. The path is normalised and each segment encoded once more, the rule of every
//! service but S3; the payload hash is the SHA-256 of the whole body.
//!
//! [`canonical_request`], [`string_to_sign`] and [`signature`] are the three intermediate
//! products, public so that the files of the AWS test suite can be compared with each one.

use crate::credentials::AwsCredentials;
use crate::time::amz_date;
use http::{HeaderMap, HeaderName, HeaderValue, Method};
use llm_core::{Error, ErrorCode};
use percent_encoding::{
    AsciiSet, NON_ALPHANUMERIC, percent_decode_str, percent_encode, utf8_percent_encode,
};
use ring::{digest, hmac};
use std::time::SystemTime;
use url::Url;
use zeroize::Zeroizing;

/// The signing algorithm, first in the string to sign and in `authorization`.
const ALGORITHM: &str = "AWS4-HMAC-SHA256";
/// The last part of every credential scope.
const TERMINATOR: &str = "aws4_request";

const HOST: &str = "host";
const AMZ_DATE: &str = "x-amz-date";
const SECURITY_TOKEN: &str = "x-amz-security-token";
const AUTHORIZATION: &str = "authorization";

/// The unreserved characters of RFC 3986 stay as they are; every other byte becomes `%XX` with
/// upper-case hex.
const UNRESERVED: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');
/// [`UNRESERVED`], keeping the slashes between path segments.
const PATH: &AsciiSet = &UNRESERVED.remove(b'/');

const HEX: &[u8; 16] = b"0123456789abcdef";

/// What a signature is made for: whose credentials, which region and service, and when.
#[derive(Debug, Clone, Copy)]
pub struct SigningParams<'a> {
    pub credentials: &'a AwsCredentials,
    pub region: &'a str,
    pub service: &'a str,
    pub time: SystemTime,
}

/// Signs a request in place for `params`.
///
/// Adds `host` when the request has none (the URL's host, with its port when that is not the
/// scheme's default), sets `x-amz-date`, sets `x-amz-security-token` when the credentials carry a
/// session token and removes a stale one when they do not, and replaces any `authorization` with
/// the signature over every header then present, the path of `url` normalised. The token and the
/// signature are marked sensitive.
///
/// # Errors
/// `invalid-request` for a region or service that is not a lower-case AWS name, a URL with no
/// host or a header value that is not UTF-8; `unauthorized` for credentials that cannot be
/// presented in a request. Both are refused before the request is changed.
pub fn sign(
    method: &Method,
    url: &Url,
    headers: &mut HeaderMap,
    body: &[u8],
    params: &SigningParams<'_>,
) -> Result<(), Error> {
    if !is_scope_name(params.region) || !is_scope_name(params.service) {
        return Err(Error::invalid(
            "signing region or service is not a lower-case AWS name",
        ));
    }
    let credentials = params.credentials;
    let access_key_id = credentials.access_key_id();
    if access_key_id.is_empty()
        || !access_key_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        || credentials.secret_access_key().is_empty()
    {
        return Err(unauthorized(
            "AWS access key cannot be presented in a signature",
        ));
    }
    let token = match credentials.session_token() {
        Some("") => return Err(unauthorized("AWS session token is empty")),
        Some(token) => Some(
            HeaderValue::from_str(token)
                .map_err(|_| unauthorized("AWS session token cannot be presented in a header"))?,
        ),
        None => None,
    };
    if headers
        .values()
        .any(|value| std::str::from_utf8(value.as_bytes()).is_err())
    {
        return Err(Error::invalid("a header to sign is not UTF-8"));
    }
    let host = if headers.contains_key(HOST) {
        None
    } else {
        Some(host_value(url)?)
    };
    let date = amz_date(params.time);
    let date_value = HeaderValue::from_str(&date)
        .map_err(|_| Error::invalid("signing time is not a header value"))?;

    headers.remove(AUTHORIZATION);
    if let Some(host) = host {
        headers.insert(HOST, host);
    }
    headers.insert(AMZ_DATE, date_value);
    headers.remove(SECURITY_TOKEN);
    if let Some(mut token) = token {
        token.set_sensitive(true);
        headers.insert(SECURITY_TOKEN, token);
    }

    let canonical = canonical_request(
        method.as_str(),
        url.path(),
        url.query(),
        headers,
        &payload_hash(body),
        true,
    );
    let to_sign = string_to_sign(&date, params.region, params.service, &canonical);
    let proof = signature(
        credentials.secret_access_key(),
        &date,
        params.region,
        params.service,
        &to_sign,
    );
    let mut authorization = HeaderValue::from_str(&format!(
        "{ALGORITHM} Credential={access_key_id}/{scope}, SignedHeaders={signed}, \
         Signature={proof}",
        scope = credential_scope(&date, params.region, params.service),
        signed = signed_headers(headers),
    ))
    .map_err(|_| unauthorized("AWS access key cannot be presented in a signature"))?;
    authorization.set_sensitive(true);
    headers.insert(AUTHORIZATION, authorization);
    Ok(())
}

/// The canonical request: method, canonical path, canonical query, canonical headers, signed
/// header names and payload hash, one per line, with no line end after the hash.
///
/// `path` and `query` are as they travel, percent-encoded (`Url::path`, `Url::query`). With
/// `normalize_path` the path's dot segments are resolved and its empty segments dropped, a
/// trailing slash kept; the path is then percent-encoded once more, so `%20` is signed as
/// `%2520`. Each query key and value is decoded and encoded again and the pairs are sorted.
/// Header names are sorted; a value is trimmed and its runs of white space become one space, and
/// the values of a repeated name are joined with `,` in their order. A value that is not UTF-8 is
/// read lossily here; [`sign`] refuses one before it gets this far.
pub fn canonical_request(
    method: &str,
    path: &str,
    query: Option<&str>,
    headers: &HeaderMap,
    payload_hash: &str,
    normalize_path: bool,
) -> String {
    let path = match (normalize_path, path) {
        (true, path) => normalized(path),
        (false, "") => "/".to_owned(),
        (false, path) => path.to_owned(),
    };
    format!(
        "{method}\n{uri}\n{query}\n{block}\n{signed}\n{payload_hash}",
        uri = utf8_percent_encode(&path, PATH),
        query = canonical_query(query),
        block = canonical_headers(headers),
        signed = signed_headers(headers),
    )
}

/// The string to sign: the algorithm, the request time, the credential scope and the hex
/// SHA-256 of the canonical request, one per line. `amz_date` is the `x-amz-date` value
/// (`20150830T123600Z`); its first eight characters are the date of the scope.
pub fn string_to_sign(
    amz_date: &str,
    region: &str,
    service: &str,
    canonical_request: &str,
) -> String {
    format!(
        "{ALGORITHM}\n{amz_date}\n{scope}\n{hash}",
        scope = credential_scope(amz_date, region, service),
        hash = hex_sha256(canonical_request.as_bytes()),
    )
}

/// The hex HMAC-SHA256 of `string_to_sign` under the signing key derived from the secret access
/// key, the date of `amz_date`, the region and the service. The derived keys are zeroized when
/// dropped; the key schedule inside `ring` is not.
pub fn signature(
    secret_access_key: &str,
    amz_date: &str,
    region: &str,
    service: &str,
    string_to_sign: &str,
) -> String {
    let mut key = Zeroizing::new(Vec::with_capacity(4 + secret_access_key.len()));
    key.extend_from_slice(b"AWS4");
    key.extend_from_slice(secret_access_key.as_bytes());
    for part in [scope_date(amz_date), region, service, TERMINATOR] {
        key = Zeroizing::new(mac(&key, part.as_bytes()));
    }
    hex(&mac(&key, string_to_sign.as_bytes()))
}

/// The hex SHA-256 of a request body: the last line of the canonical request.
pub fn payload_hash(body: &[u8]) -> String {
    hex_sha256(body)
}

/// `path` with `.` and empty segments dropped and each `..` removing the segment before it; a
/// trailing slash stays, and `/` is what is left of a path with no segment.
fn normalized(path: &str) -> String {
    let mut segments = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            segment => segments.push(segment),
        }
    }
    let mut out = String::with_capacity(path.len() + 1);
    for segment in &segments {
        out.push('/');
        out.push_str(segment);
    }
    if segments.is_empty() || path.ends_with('/') {
        out.push('/');
    }
    out
}

fn canonical_query(query: Option<&str>) -> String {
    let mut pairs: Vec<(String, String)> = query
        .unwrap_or_default()
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (reencoded(key), reencoded(value))
        })
        .collect();
    pairs.sort_unstable();
    pairs
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// A query key or value decoded, then encoded with only the unreserved characters left as they are.
fn reencoded(component: &str) -> String {
    let bytes: Vec<u8> = percent_decode_str(component).collect();
    percent_encode(&bytes, UNRESERVED).to_string()
}

/// One `name:value` line per header name, each ending in a line feed.
fn canonical_headers(headers: &HeaderMap) -> String {
    let mut block = String::new();
    for name in sorted_names(headers) {
        block.push_str(name);
        block.push(':');
        for (index, value) in headers.get_all(name).iter().enumerate() {
            if index > 0 {
                block.push(',');
            }
            let value = String::from_utf8_lossy(value.as_bytes());
            for (position, word) in value.split_ascii_whitespace().enumerate() {
                if position > 0 {
                    block.push(' ');
                }
                block.push_str(word);
            }
        }
        block.push('\n');
    }
    block
}

fn signed_headers(headers: &HeaderMap) -> String {
    sorted_names(headers).join(";")
}

/// Each header name once, in byte order. A [`HeaderName`] is lower case already.
fn sorted_names(headers: &HeaderMap) -> Vec<&str> {
    let mut names: Vec<&str> = headers.keys().map(HeaderName::as_str).collect();
    names.sort_unstable();
    names
}

fn credential_scope(amz_date: &str, region: &str, service: &str) -> String {
    format!(
        "{date}/{region}/{service}/{TERMINATOR}",
        date = scope_date(amz_date)
    )
}

/// `20150830` of `20150830T123600Z`.
fn scope_date(amz_date: &str) -> &str {
    amz_date.get(..8).unwrap_or(amz_date)
}

fn host_value(url: &Url) -> Result<HeaderValue, Error> {
    let host = url
        .host_str()
        .ok_or_else(|| Error::invalid("the URL to sign has no host"))?;
    let host = url
        .port()
        .map_or_else(|| host.to_owned(), |port| format!("{host}:{port}"));
    HeaderValue::from_str(&host).map_err(|_| Error::invalid("the URL host is not a header value"))
}

/// A region or service name as it appears in a credential scope: `[a-z0-9-]+`.
fn is_scope_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn mac(key: &[u8], data: &[u8]) -> Vec<u8> {
    hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, key), data)
        .as_ref()
        .to_vec()
}

fn hex_sha256(bytes: &[u8]) -> String {
    hex(digest::digest(&digest::SHA256, bytes).as_ref())
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}

fn unauthorized(message: &'static str) -> Error {
    Error::new(ErrorCode::Unauthorized, message)
}
