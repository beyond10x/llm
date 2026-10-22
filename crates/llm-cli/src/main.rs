#![forbid(unsafe_code)]

//! The `llm` binary; [`llm_cli`] holds the command.

use std::process::ExitCode;

fn main() -> ExitCode {
    llm_cli::run(std::env::args_os(), &llm_cli::Environment::from_process())
}
