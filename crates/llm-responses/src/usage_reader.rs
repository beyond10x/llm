//! The usage an answer reported, read from its bytes. Not implemented yet: reads nothing.

use llm_core::ReportedUsage;

/// Reads nothing yet.
#[derive(Default)]
pub struct UsageReader {
    report: ReportedUsage,
}

impl UsageReader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        let _ = bytes;
    }

    pub fn finish(self) -> ReportedUsage {
        self.report
    }
}

pub fn read_usage(body: &[u8]) -> ReportedUsage {
    let mut reader = UsageReader::new();
    reader.feed(body);
    reader.finish()
}
