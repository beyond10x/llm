//! Bounds adapted from Harness; encodings are counted without allocating a second payload.
use serde::Serialize;
use std::io::{self, Write};

pub const MAX_TOOL_ARGUMENT_BYTES: usize = 64 * 1024;
pub const MAX_TOOL_RESULT_BYTES: usize = 256 * 1024;
pub const MAX_INSTRUCTION_BYTES: usize = 256 * 1024;
pub const MAX_TOOL_DESCRIPTION_BYTES: usize = 16 * 1024;
pub const MAX_TOOLS: usize = 512;
pub const MAX_ITEMS: usize = 4096;
pub const MAX_REQUEST_BYTES: usize = 16 * 1024 * 1024;

struct Counter {
    length: usize,
    limit: usize,
}
impl Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.length = self
            .length
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::other("encoding too large"))?;
        if self.length > self.limit {
            return Err(io::Error::other("encoding too large"));
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Encoded size within a limit; `None` means oversized or unencodable.
pub fn encoded_len<T: Serialize + ?Sized>(value: &T, limit: usize) -> Option<usize> {
    let mut counter = Counter { length: 0, limit };
    serde_json::to_writer(&mut counter, value).ok()?;
    Some(counter.length)
}

pub fn exceeds<T: Serialize + ?Sized>(value: &T, limit: usize) -> bool {
    encoded_len(value, limit).is_none()
}
