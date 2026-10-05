//! Generates the derivable pages of the llm documentation site, checks them for drift, and binds
//! a built site to its source commit. The site build itself is Docusaurus.
//!
//! The command surface is fixed; the generators are not written yet, so every action refuses.

use std::{path::PathBuf, process::ExitCode};

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(about = "Generate and check the derived pages of the llm documentation site")]
struct Cli {
    #[command(subcommand)]
    command: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Regenerate the crate reference and the status page, or check them for drift.
    Generate {
        /// Write nothing; fail when a generated file is missing, stale or hand-edited.
        #[arg(long)]
        check: bool,
        /// Repository root.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Bind a built site to its source commit: write `.well-known/b10x-site.json` into it.
    Provenance {
        /// The built site, `website/build`.
        #[arg(long)]
        site: PathBuf,
        /// The full Git revision the site was built from.
        #[arg(long)]
        commit: String,
    },
}

fn main() -> ExitCode {
    let action = match Cli::parse().command {
        Action::Generate { .. } => "generate",
        Action::Provenance { .. } => "provenance",
    };
    eprintln!("llm-docs {action}: not implemented");
    ExitCode::FAILURE
}
