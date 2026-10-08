mod budgets;
mod chat;
mod fallback;
mod gate;
mod inference;
mod messages;
mod models;
mod pricing;
mod providers;
mod responses;
mod secrets;
mod target;
mod transport;

use ess_conformance::{
    AdmittedSuite, Clock, CountReport, CountRun, CountStatus, Ids, Runner, RunnerConfig,
};
use ess_primitives::time::Timestamp;
use serde::Deserialize;
use std::{
    error::Error,
    fs,
    path::Path,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

// Evidence carries the real observation time. Elapsed time is monotonic even
// when the wall clock adjusts; no reference runner's fixed 2023 epoch leaks into AEP.
struct EvidenceClock {
    epoch_ms: u64,
    started: Instant,
}
impl Clock for EvidenceClock {
    fn now(&mut self) -> Timestamp {
        Timestamp::from_epoch_millis(
            self.epoch_ms.saturating_add(
                u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX),
            ),
        )
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Baseline {
    answered_floor: u64,
    total_floor: u64,
    skipped_ceiling: u64,
}

#[derive(clap::Parser)]
#[command(name = "b10x-llm-conformance")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(clap::Subcommand)]
enum Cmd {
    /// Check ESS projection drift and run the suite three times against the baseline.
    Check,
    /// Run one suite against a baseline and write its report.
    Run {
        suite: String,
        baseline: String,
        output_dir: std::path::PathBuf,
        source_identity: String,
    },
    /// Run one transport program in this process and print its facts; the transport lane
    /// starts it with ambient proxy variables set so they never reach another lane.
    #[command(hide = true)]
    TransportChild { program_json: String },
    /// Resolve one environment-variable probe in this process and print its facts; the
    /// secrets lane starts it with the fixture variable set, since no lane changes its own
    /// environment.
    #[command(hide = true)]
    SecretsEnvironmentChild { input_json: String },
}

fn main() -> Result<(), Box<dyn Error>> {
    match <Cli as clap::Parser>::parse().command {
        Cmd::Check => gate::check(),
        Cmd::Run {
            suite,
            baseline,
            output_dir,
            source_identity,
        } => execute(&suite, &baseline, &output_dir, &source_identity),
        Cmd::TransportChild { program_json } => {
            // A child that did not receive every proxy variable proves nothing about the client
            // ignoring them, so it refuses rather than answering.
            if transport::PROXY_VARIABLES
                .iter()
                .any(|variable| std::env::var_os(variable).is_none_or(|value| value.is_empty()))
            {
                return Err("transport-child started without its proxy environment".into());
            }
            println!("{}", transport::run_here(&program_json));
            Ok(())
        }
        Cmd::SecretsEnvironmentChild { input_json } => {
            println!("{}", secrets::environment_here(&input_json)?);
            Ok(())
        }
    }
}

fn execute(
    suite_path: &str,
    baseline_path: &str,
    output: &Path,
    revision: &str,
) -> Result<(), Box<dyn Error>> {
    let suite = AdmittedSuite::from_json(&fs::read_to_string(suite_path)?)?;
    let target = target::CatalogTarget::new(revision.to_owned());
    let clock = EvidenceClock {
        epoch_ms: u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?,
        started: Instant::now(),
    };
    let run = Runner::new(
        RunnerConfig::default(),
        clock,
        Ids::for_suite(suite.suite()),
    )
    .run_admitted(&suite, &target);
    let report = CountReport::from_run(&run, &suite)?;
    let detail = CountRun::from_run(&run, &suite)?;
    fs::create_dir_all(output)?;
    fs::write(output.join("report.json"), report.to_canonical_json()?)?;
    fs::write(output.join("run.json"), detail.to_canonical_json()?)?;
    // Re-admit the persisted report paired with the exact suite before judging it.
    let report = CountReport::from_json(&fs::read_to_string(output.join("report.json"))?, &suite)?;
    let baseline: Baseline = serde_json::from_str(&fs::read_to_string(baseline_path)?)?;
    let counts = report.counts();
    println!("{}", serde_json::to_string(counts)?);
    let answered = counts.passed + counts.failed;
    if counts.total == 0
        || answered == 0
        || answered < baseline.answered_floor
        || counts.total < baseline.total_floor
        || counts.skipped > baseline.skipped_ceiling
        || counts.unsupported != 0
        || counts.error != 0
        || counts.failed != 0
        || report.execution_status() != CountStatus::Passed
        || report.conformance_status() != CountStatus::Passed
    {
        return Err(format!(
            "conformance gate refused; see {}",
            output.join("run.json").display()
        )
        .into());
    }
    Ok(())
}
