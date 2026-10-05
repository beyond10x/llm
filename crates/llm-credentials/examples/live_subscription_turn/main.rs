//! One live Messages turn over the operator's own subscription token, for the qualification
//! report of story:anthropic-access. `docs/live-qualification.md` gives the steps.
//!
//! The token is read from the platform's native keychain through the secrets library, where
//! `secretsctl put` stored it; nothing else is searched and no login file is read. The JSON
//! report goes to stdout and every prompt to stderr. The token is never printed, logged or
//! written. Exit status 0 means every turn completed and a requested rotation was observed.
//!
//! For the operator's own subscription only: llm offers no login and serves no other user's
//! credentials.
mod turn;

use clap::Parser;
use std::process::ExitCode;

/// Asks the operator to rotate the token in the store and waits for Enter.
async fn wait_for_rotation(args: &turn::Args) {
    eprintln!(
        "live_subscription_turn: the first turn is done. Store a new token now, in another \
         terminal:\n  secretsctl put {} --namespace {}\nthen press Enter here.",
        args.name, args.namespace
    );
    let _ = tokio::task::spawn_blocking(|| {
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).map(|_| ())
    })
    .await;
}

fn main() -> ExitCode {
    let args = turn::Args::parse();
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        eprintln!("live_subscription_turn: could not start the async runtime");
        return ExitCode::FAILURE;
    };
    let report = runtime.block_on(turn::run(
        &args,
        llm_credentials::keychain::native_store(),
        async || wait_for_rotation(&args).await,
    ));
    println!("{}", turn::render(&report));
    if report.succeeded() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
