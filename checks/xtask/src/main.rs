//! The gate's test phase.

use std::process::ExitCode;

fn main() -> ExitCode {
    eprintln!("usage: llm-xtask test | llm-xtask report <JUnit report>...");
    ExitCode::from(2)
}
