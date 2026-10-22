//! AWS event-stream framing (`application/vnd.amazon.eventstream`), the binary frames Bedrock
//! Runtime streams its answers in.
//!
//! A frame is a 12-byte prelude (total length and headers length, both big-endian `u32`, then the
//! CRC32 of those eight bytes), typed headers, the payload, and the CRC32 of everything before
//! it. Checks run in the order of the smithy-rs reference decoder (prelude checksum, lengths,
//! headers, message checksum), so each malformed frame is refused by the check it breaks. Every
//! refusal is a fixed message: none quotes a header or payload byte.

use llm_core::{Dispatch, Error};
use llm_http::MAX_STREAM_BYTES;

/// The largest frame accepted, total length included: the format's own bound of 16 mebibytes.
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

const PRELUDE_BYTES: usize = 12;
const CHECKSUM_BYTES: usize = 4;
const MIN_FRAME_BYTES: usize = PRELUDE_BYTES + CHECKSUM_BYTES;

const TRUE: u8 = 0;
const FALSE: u8 = 1;
const BYTE: u8 = 2;
const INT16: u8 = 3;
const INT32: u8 = 4;
const INT64: u8 = 5;
const BYTE_ARRAY: u8 = 6;
const STRING: u8 = 7;
const TIMESTAMP: u8 = 8;
const UUID: u8 = 9;

const TRUNCATED: &str = "event-stream frame is truncated";
const PRELUDE_CHECKSUM: &str = "event-stream prelude checksum does not match";
const MESSAGE_CHECKSUM: &str = "event-stream message checksum does not match";
const FRAME_TOO_SHORT: &str = "event-stream frame length is below the 16-byte minimum";
const FRAME_TOO_LARGE: &str = "event-stream frame exceeds the 16 MiB bound";
const HEADERS_LENGTH: &str = "event-stream headers length does not fit the frame";
const NAME_LENGTH: &str = "event-stream header name length is zero or exceeds the headers";
const NAME_UTF8: &str = "event-stream header name is not UTF-8";
const VALUE_TYPE: &str = "event-stream header value type is unknown";
const VALUE_CUT: &str = "event-stream header value exceeds the headers";
const STRING_LENGTH: &str = "event-stream header string length exceeds the headers";
const STRING_UTF8: &str = "event-stream header string is not UTF-8";
const BYTES_LENGTH: &str = "event-stream header byte-array length exceeds the headers";
const STREAM_TOO_LARGE: &str = "event-stream stream exceeds the byte bound";
const ENDED_INSIDE_FRAME: &str = "event-stream ended inside a frame";
const PREVIOUSLY_FAILED: &str = "event-stream decoder previously failed";

/// One typed header value. `Timestamp` is milliseconds since the Unix epoch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderValue {
    Bool(bool),
    Byte(i8),
    Int16(i16),
    Int32(i32),
    Int64(i64),
    ByteArray(Vec<u8>),
    String(String),
    Timestamp(i64),
    Uuid([u8; 16]),
}

/// One decoded frame: its headers in wire order and its payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Message {
    pub headers: Vec<(String, HeaderValue)>,
    pub payload: Vec<u8>,
}

impl Message {
    /// The first header named `name`.
    pub fn header(&self, name: &str) -> Option<&HeaderValue> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }

    /// The first header named `name`, when its value is a string.
    pub fn string_header(&self, name: &str) -> Option<&str> {
        if let HeaderValue::String(value) = self.header(name)? {
            Some(value.as_str())
        } else {
            None
        }
    }
}

/// Decodes the one frame `bytes` starts with and returns it with the number of bytes it took.
/// Bytes after that frame are left alone.
///
/// # Errors
/// Refuses a short or oversized frame, a checksum mismatch, a headers length that does not fit,
/// and any header that is cut off, untyped or not UTF-8 where it must be. Each refusal is a
/// protocol failure, except a frame past [`MAX_FRAME_BYTES`], which is too large.
pub fn decode_message(bytes: &[u8]) -> Result<(Message, usize), Error> {
    let (total, headers_length) = prelude(bytes)?;
    let frame = bytes
        .get(..total)
        .ok_or_else(|| Error::protocol(TRUNCATED))?;
    let headers_end = PRELUDE_BYTES + headers_length;
    let body_end = total - CHECKSUM_BYTES;
    let headers = decode_headers(&frame[PRELUDE_BYTES..headers_end])?;
    let Some(checksum) = read_u32(frame, body_end) else {
        return Err(Error::protocol(TRUNCATED));
    };
    if crc32(&frame[..body_end]) != checksum {
        return Err(Error::protocol(MESSAGE_CHECKSUM));
    }
    let payload = frame[headers_end..body_end].to_vec();
    Ok((Message { headers, payload }, total))
}

/// Encodes one frame. For test fixtures and the conformance adapter: production never sends one.
///
/// # Panics
/// Panics on a header name longer than 255 bytes, a string or byte-array value longer than
/// 65 535 bytes, or a frame longer than [`MAX_FRAME_BYTES`]: the format cannot carry them.
pub fn encode_message(message: &Message) -> Vec<u8> {
    let mut headers = Vec::new();
    for (name, value) in &message.headers {
        headers.push(u8::try_from(name.len()).expect("a header name fits in 255 bytes"));
        headers.extend_from_slice(name.as_bytes());
        encode_value(value, &mut headers);
    }
    let total = MIN_FRAME_BYTES + headers.len() + message.payload.len();
    assert!(
        total <= MAX_FRAME_BYTES,
        "an event-stream frame fits in 16 MiB"
    );
    let total = u32::try_from(total).expect("a bounded frame length fits in u32");
    let headers_length =
        u32::try_from(headers.len()).expect("a bounded headers length fits in u32");
    let mut frame = Vec::with_capacity(MIN_FRAME_BYTES + headers.len() + message.payload.len());
    frame.extend_from_slice(&total.to_be_bytes());
    frame.extend_from_slice(&headers_length.to_be_bytes());
    frame.extend_from_slice(&crc32(&frame).to_be_bytes());
    frame.extend_from_slice(&headers);
    frame.extend_from_slice(&message.payload);
    frame.extend_from_slice(&crc32(&frame).to_be_bytes());
    frame
}

/// Incremental decoding of a whole response body: frames come out in order however the network
/// split the bytes. Bounded per frame by [`MAX_FRAME_BYTES`], checked as soon as a prelude
/// arrives, and per stream by [`MAX_STREAM_BYTES`]. The first failure is final.
#[derive(Default)]
pub struct MessageDecoder {
    buffer: Vec<u8>,
    /// The checked total length of the frame at the head of `buffer`, once its prelude arrived.
    frame: Option<usize>,
    consumed: usize,
    failed: bool,
}

impl MessageDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the frames `bytes` completed, in order, followed by a failure if one occurred.
    /// Frames completed before a malformed one stay observable whatever the chunking.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Result<Message, Error>> {
        if self.failed {
            return vec![Err(Error::protocol(PREVIOUSLY_FAILED))];
        }
        let room = MAX_STREAM_BYTES - self.consumed;
        let (accepted, excess) = bytes.split_at(bytes.len().min(room));
        self.consumed += accepted.len();
        self.buffer.extend_from_slice(accepted);
        let mut messages = Vec::new();
        let mut start = 0;
        let mut failure = self.decode_ready(&mut start, &mut messages).err();
        self.buffer.drain(..start);
        if failure.is_none() && !excess.is_empty() {
            failure = Some(Error::too_large(STREAM_TOO_LARGE));
        }
        if let Some(error) = failure {
            self.failed = true;
            self.buffer = Vec::new();
            self.frame = None;
            messages.push(Err(error));
        }
        messages
    }

    /// Decodes every whole frame from `start` on, advancing `start` past each.
    fn decode_ready(
        &mut self,
        start: &mut usize,
        messages: &mut Vec<Result<Message, Error>>,
    ) -> Result<(), Error> {
        loop {
            let pending = &self.buffer[*start..];
            let total = if let Some(total) = self.frame {
                total
            } else {
                if pending.len() < PRELUDE_BYTES {
                    return Ok(());
                }
                let (total, _) = prelude(pending)?;
                self.frame = Some(total);
                total
            };
            let Some(frame) = pending.get(..total) else {
                return Ok(());
            };
            let (message, _) = decode_message(frame)?;
            messages.push(Ok(message));
            self.frame = None;
            *start += total;
        }
    }

    /// # Errors
    /// Refuses a stream that ended inside a frame, and repeats an earlier failure.
    pub fn finish(&self) -> Result<(), Error> {
        if self.failed {
            return Err(Error::protocol(PREVIOUSLY_FAILED));
        }
        if !self.buffer.is_empty() {
            // Retriable, as the SSE decoder's: nothing malformed arrived, the peer stopped
            // mid-frame, which is what a dropped connection looks like from here.
            return Err(Error::protocol(ENDED_INSIDE_FRAME)
                .with_dispatch(Dispatch::Unknown)
                .with_retriable(true));
        }
        Ok(())
    }
}

/// Checks a prelude and returns the frame's total length and headers length.
fn prelude(bytes: &[u8]) -> Result<(usize, usize), Error> {
    let (Some(total), Some(headers), Some(checksum)) =
        (read_u32(bytes, 0), read_u32(bytes, 4), read_u32(bytes, 8))
    else {
        return Err(Error::protocol(TRUNCATED));
    };
    if crc32(&bytes[..8]) != checksum {
        return Err(Error::protocol(PRELUDE_CHECKSUM));
    }
    let total = usize::try_from(total).map_err(|_| Error::too_large(FRAME_TOO_LARGE))?;
    let headers = usize::try_from(headers).map_err(|_| Error::protocol(HEADERS_LENGTH))?;
    if total < MIN_FRAME_BYTES {
        return Err(Error::protocol(FRAME_TOO_SHORT));
    }
    if total > MAX_FRAME_BYTES {
        return Err(Error::too_large(FRAME_TOO_LARGE));
    }
    if headers > total - MIN_FRAME_BYTES {
        return Err(Error::protocol(HEADERS_LENGTH));
    }
    Ok((total, headers))
}

fn decode_headers(mut rest: &[u8]) -> Result<Vec<(String, HeaderValue)>, Error> {
    let mut headers = Vec::new();
    while let Some((&name_length, tail)) = rest.split_first() {
        rest = tail;
        let name = take(&mut rest, usize::from(name_length))
            .filter(|name| !name.is_empty())
            .ok_or_else(|| Error::protocol(NAME_LENGTH))?;
        let name = std::str::from_utf8(name).map_err(|_| Error::protocol(NAME_UTF8))?;
        let value = decode_value(&mut rest)?;
        headers.push((name.to_owned(), value));
    }
    Ok(headers)
}

fn decode_value(rest: &mut &[u8]) -> Result<HeaderValue, Error> {
    let [kind] = fixed::<1>(rest)?;
    let value = match kind {
        TRUE => HeaderValue::Bool(true),
        FALSE => HeaderValue::Bool(false),
        BYTE => HeaderValue::Byte(i8::from_be_bytes(fixed(rest)?)),
        INT16 => HeaderValue::Int16(i16::from_be_bytes(fixed(rest)?)),
        INT32 => HeaderValue::Int32(i32::from_be_bytes(fixed(rest)?)),
        INT64 => HeaderValue::Int64(i64::from_be_bytes(fixed(rest)?)),
        BYTE_ARRAY => {
            let bytes = sized(rest).ok_or_else(|| Error::protocol(BYTES_LENGTH))?;
            HeaderValue::ByteArray(bytes.to_vec())
        }
        STRING => {
            let bytes = sized(rest).ok_or_else(|| Error::protocol(STRING_LENGTH))?;
            let text = std::str::from_utf8(bytes).map_err(|_| Error::protocol(STRING_UTF8))?;
            HeaderValue::String(text.to_owned())
        }
        TIMESTAMP => HeaderValue::Timestamp(i64::from_be_bytes(fixed(rest)?)),
        UUID => HeaderValue::Uuid(fixed(rest)?),
        _ => return Err(Error::protocol(VALUE_TYPE)),
    };
    Ok(value)
}

fn encode_value(value: &HeaderValue, out: &mut Vec<u8>) {
    match value {
        HeaderValue::Bool(true) => out.push(TRUE),
        HeaderValue::Bool(false) => out.push(FALSE),
        HeaderValue::Byte(value) => {
            out.push(BYTE);
            out.extend_from_slice(&value.to_be_bytes());
        }
        HeaderValue::Int16(value) => {
            out.push(INT16);
            out.extend_from_slice(&value.to_be_bytes());
        }
        HeaderValue::Int32(value) => {
            out.push(INT32);
            out.extend_from_slice(&value.to_be_bytes());
        }
        HeaderValue::Int64(value) => {
            out.push(INT64);
            out.extend_from_slice(&value.to_be_bytes());
        }
        HeaderValue::ByteArray(bytes) => {
            out.push(BYTE_ARRAY);
            encode_sized(bytes, out);
        }
        HeaderValue::String(text) => {
            out.push(STRING);
            encode_sized(text.as_bytes(), out);
        }
        HeaderValue::Timestamp(millis) => {
            out.push(TIMESTAMP);
            out.extend_from_slice(&millis.to_be_bytes());
        }
        HeaderValue::Uuid(bytes) => {
            out.push(UUID);
            out.extend_from_slice(bytes);
        }
    }
}

fn encode_sized(bytes: &[u8], out: &mut Vec<u8>) {
    let length = u16::try_from(bytes.len()).expect("a header value fits in 65 535 bytes");
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(bytes);
}

/// A fixed-width value, or the refusal for one the headers cut off.
fn fixed<const N: usize>(rest: &mut &[u8]) -> Result<[u8; N], Error> {
    let bytes = *rest;
    let (head, tail) = bytes
        .split_first_chunk::<N>()
        .ok_or_else(|| Error::protocol(VALUE_CUT))?;
    *rest = tail;
    Ok(*head)
}

/// A value with a big-endian `u16` length prefix, or `None` when the headers cut either off.
fn sized<'a>(rest: &mut &'a [u8]) -> Option<&'a [u8]> {
    let bytes = *rest;
    let (length, tail) = bytes.split_first_chunk::<2>()?;
    let (value, tail) = tail.split_at_checked(usize::from(u16::from_be_bytes(*length)))?;
    *rest = tail;
    Some(value)
}

fn take<'a>(rest: &mut &'a [u8], length: usize) -> Option<&'a [u8]> {
    let bytes = *rest;
    let (head, tail) = bytes.split_at_checked(length)?;
    *rest = tail;
    Some(head)
}

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let chunk = bytes.get(at..)?.first_chunk::<4>()?;
    Some(u32::from_be_bytes(*chunk))
}

/// CRC32 (IEEE 802.3, reflected polynomial `0xEDB8_8320`), the checksum both frame fields use.
fn crc32(bytes: &[u8]) -> u32 {
    !bytes.iter().fold(u32::MAX, |crc, &byte| {
        CRC_TABLE[usize::from(crc.to_le_bytes()[0] ^ byte)] ^ (crc >> 8)
    })
}

const CRC_TABLE: [u32; 256] = crc_table();

const fn crc_table() -> [u32; 256] {
    let mut table = [0_u32; 256];
    let mut index = 0;
    let mut byte = 0_u32;
    while index < table.len() {
        let mut crc = byte;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[index] = crc;
        index += 1;
        byte += 1;
    }
    table
}
