#![forbid(unsafe_code)]

//! The `llm` command: lists the models an `llm.routes/1` configuration file declares and runs one
//! turn on any of them.
//!
//! It is a thin layer over `llm-models`: [`Routes::parse_toml`] (or [`Routes::parse`] for a
//! `.yaml` file) reads the file, [`Routes::port`] builds the client a name reaches, and
//! [`Routes::cost`] prices the turn. The file is found by [`locate`].
//!
//! Exit codes: 0 for a completed turn or listing; 2 for a refusal before anything is sent (no
//! configuration file, a document `llm-models` refuses, an unknown name, a credential that does
//! not resolve, an invalid request); 1 for a provider or transport failure, reported by its
//! `llm_core` error kind only; 130 when Ctrl-C cancelled the turn.

use clap::{Parser, Subcommand, ValueEnum};
use llm_core::{
    BoxFuture, Cancel, Dispatch, Error, ErrorCode, Item, Model, Protocol, StreamEvent, StreamSink,
    TurnOutcome, TurnRequest, Usage,
};
use llm_http::{HttpClient, Limits};
use llm_models::routes::{RouteDeclaration, Routes, SecretSources};
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};
use url::Url;

/// The variable that names the configuration file when `--config` is not given.
pub const CONFIG_VARIABLE: &str = "LLM_CONFIG";
const CONTAINER_URI: &str = "AWS_CONTAINER_CREDENTIALS_FULL_URI";
const CONTAINER_TOKEN_FILE: &str = "AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE";

/// Lists the configured models and runs one turn on any of them.
#[derive(Debug, Parser)]
#[command(name = "llm", version, about)]
pub struct Cli {
    /// The configuration file; otherwise `LLM_CONFIG`, `~/.b10x/llm.toml`, then
    /// `$XDG_CONFIG_HOME/b10x/llm.toml` (`~/.config/b10x/llm.toml`).
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// The configured models.
    Model {
        #[command(subcommand)]
        command: ModelCommand,
    },
    /// Sends one user turn and streams the assistant text to stdout.
    Infer(Infer),
}

#[derive(Debug, Subcommand)]
enum ModelCommand {
    /// One row per route. Reads no secret and makes no network call.
    List {
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Format {
    Text,
    Json,
}

#[derive(Debug, clap::Args)]
struct Infer {
    /// A route id, one of its aliases, or `<endpoint>/<model-id>`.
    #[arg(long, value_name = "ID")]
    model: String,
    /// The system prompt.
    #[arg(long)]
    system: Option<String>,
    /// The most output tokens the answer may use.
    #[arg(long, value_name = "N")]
    max_output_tokens: Option<u64>,
    /// The user prompt; read from stdin when absent.
    prompt: Option<String>,
}

/// The process environment the command reads: the configuration lookup, the home a `file:~/`
/// reference expands against, and the container credentials endpoint.
#[derive(Debug, Clone, Default)]
pub struct Environment {
    pub config: Option<OsString>,
    pub home: Option<PathBuf>,
    pub xdg_config_home: Option<PathBuf>,
    pub container_uri: Option<String>,
    pub container_token_file: Option<PathBuf>,
}

impl Environment {
    /// The values this process was started with; an empty value counts as unset.
    pub fn from_process() -> Self {
        let os = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
        Self {
            config: os(CONFIG_VARIABLE),
            home: os("HOME").map(PathBuf::from),
            xdg_config_home: os("XDG_CONFIG_HOME").map(PathBuf::from),
            container_uri: os(CONTAINER_URI).and_then(|value| value.into_string().ok()),
            container_token_file: os(CONTAINER_TOKEN_FILE).map(PathBuf::from),
        }
    }
}

/// A failure the command reports on stderr, with its exit code.
#[derive(Debug)]
pub struct Failure {
    code: u8,
    message: String,
}

impl Failure {
    fn refused(message: impl Into<String>) -> Self {
        Self {
            code: 2,
            message: message.into(),
        }
    }

    /// The process exit code.
    pub fn exit_code(&self) -> u8 {
        self.code
    }

    /// The line written to stderr.
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// One place [`locate`] looks, and what it found there.
#[derive(Debug)]
struct Place {
    label: &'static str,
    path: Option<PathBuf>,
    /// An explicitly named file must exist; a default place may be absent.
    explicit: bool,
}

/// The configuration file: `--config`, then `LLM_CONFIG`, then `~/.b10x/llm.toml`, then
/// `$XDG_CONFIG_HOME/b10x/llm.toml` (`~/.config/b10x/llm.toml` when `XDG_CONFIG_HOME` is unset
/// or not absolute). A file named by `--config` or `LLM_CONFIG` must exist; the two default
/// places are tried in order.
///
/// # Errors
/// A named file that does not exist, or no file in either default place, refused with every
/// place the lookup considered.
pub fn locate(explicit: Option<&Path>, environment: &Environment) -> Result<PathBuf, Failure> {
    let home = environment.home.as_deref();
    let xdg = environment
        .xdg_config_home
        .as_deref()
        .filter(|path| path.is_absolute())
        .map(Path::to_path_buf)
        .or_else(|| home.map(|home| home.join(".config")));
    let places = [
        Place {
            label: "--config",
            path: explicit.map(Path::to_path_buf),
            explicit: true,
        },
        Place {
            label: CONFIG_VARIABLE,
            path: environment.config.as_ref().map(PathBuf::from),
            explicit: true,
        },
        Place {
            label: "~/.b10x/llm.toml",
            path: home.map(|home| home.join(".b10x").join("llm.toml")),
            explicit: false,
        },
        Place {
            label: "$XDG_CONFIG_HOME/b10x/llm.toml",
            path: xdg.map(|xdg| xdg.join("b10x").join("llm.toml")),
            explicit: false,
        },
    ];
    for place in &places {
        let Some(path) = &place.path else { continue };
        if path.is_file() {
            return Ok(path.clone());
        }
        if place.explicit {
            return Err(Failure::refused(format!(
                "llm: no configuration file at {} (from {}); looked at {}",
                path.display(),
                place.label,
                looked(&places)
            )));
        }
    }
    Err(Failure::refused(format!(
        "llm: no configuration file found; looked at {}",
        looked(&places)
    )))
}

fn looked(places: &[Place]) -> String {
    places
        .iter()
        .map(|place| match &place.path {
            Some(path) => format!("{} ({})", place.label, path.display()),
            None => format!("{} (not set)", place.label),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Reads the configuration file: TOML, or YAML when its extension is `.yaml` or `.yml`.
///
/// # Errors
/// A file that cannot be read, and a document `llm-models` refuses, with its error kind and
/// message (which never echo a value).
pub fn load(path: &Path) -> Result<Routes, Failure> {
    let source = std::fs::read_to_string(path).map_err(|error| {
        Failure::refused(format!(
            "llm: cannot read {}: {}",
            path.display(),
            error.kind()
        ))
    })?;
    let yaml = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "yaml" | "yml"));
    let parsed = if yaml {
        Routes::parse(&source)
    } else {
        Routes::parse_toml(&source)
    };
    parsed.map_err(|error| {
        Failure::refused(format!(
            "llm: {}: {}: {}",
            path.display(),
            code(error.code),
            error.message
        ))
    })
}

/// Runs the command line `args` in `environment`.
pub fn run(args: impl IntoIterator<Item = OsString>, environment: &Environment) -> ExitCode {
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            let _ = error.print();
            return ExitCode::from(if error.use_stderr() { 2 } else { 0 });
        }
    };
    match execute(&cli, environment) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            eprintln!("{}", failure.message);
            ExitCode::from(failure.code)
        }
    }
}

fn execute(cli: &Cli, environment: &Environment) -> Result<(), Failure> {
    let path = locate(cli.config.as_deref(), environment)?;
    let routes = load(&path)?;
    match &cli.command {
        Command::Model {
            command: ModelCommand::List { format },
        } => list(&routes, *format),
        Command::Infer(infer) => self::infer(&routes, infer, environment),
    }
}

/// One row of `llm model list`.
struct Row {
    id: String,
    aliases: Vec<String>,
    protocol: &'static str,
    provider: String,
    host: String,
    region: Option<String>,
    model: String,
    price: Option<(String, Option<String>, Option<String>)>,
}

fn rows(routes: &Routes) -> Vec<Row> {
    routes
        .routes()
        .map(|route| {
            let (host, region) = endpoint_of(routes, route);
            Row {
                id: route.name().to_string(),
                aliases: route.aliases().iter().map(ToString::to_string).collect(),
                protocol: protocol(route.protocol()),
                provider: route.endpoint().to_string(),
                host,
                region,
                model: route.upstream_model().to_string(),
                price: route.price().map(|price| {
                    (
                        price.currency().as_str().to_owned(),
                        price.input().map(|amount| amount.to_string()),
                        price.output().map(|amount| amount.to_string()),
                    )
                }),
            }
        })
        .collect()
}

fn endpoint_of(routes: &Routes, route: &RouteDeclaration) -> (String, Option<String>) {
    if let Some(endpoint) = routes
        .bedrock_endpoints()
        .find(|endpoint| endpoint.name() == route.endpoint())
    {
        return (endpoint.host(), Some(endpoint.region().to_owned()));
    }
    let host = routes
        .endpoints()
        .find(|endpoint| endpoint.name() == route.endpoint())
        .and_then(|endpoint| Url::parse(endpoint.base_url().as_str()).ok())
        .and_then(|url| {
            url.host_str().map(|host| match url.port() {
                Some(port) => format!("{host}:{port}"),
                None => host.to_owned(),
            })
        })
        .unwrap_or_default();
    (host, None)
}

const UNKNOWN: &str = "unknown";

fn list(routes: &Routes, format: Format) -> Result<(), Failure> {
    let rows = rows(routes);
    let text = match format {
        Format::Json => {
            let value: Vec<Value> = rows
                .iter()
                .map(|row| {
                    json!({
                        "id": row.id,
                        "aliases": row.aliases,
                        "protocol": row.protocol,
                        "provider": row.provider,
                        "host": row.host,
                        "region": row.region,
                        "model": row.model,
                        "price_per_million": row.price.as_ref().map(|(currency, input, output)| {
                            json!({"currency": currency, "input": input, "output": output})
                        }),
                    })
                })
                .collect();
            let mut text = serde_json::to_string_pretty(&value)
                .map_err(|_| Failure::refused("llm: the listing cannot be written as JSON"))?;
            text.push('\n');
            text
        }
        Format::Text => table(&rows),
    };
    io::stdout()
        .write_all(text.as_bytes())
        .map_err(|_| Failure {
            code: 1,
            message: "llm: stdout is closed".to_owned(),
        })
}

fn table(rows: &[Row]) -> String {
    let header = [
        "ID",
        "ALIASES",
        "PROTOCOL",
        "PROVIDER",
        "HOST",
        "MODEL",
        "PRICE/1M IN/OUT",
    ]
    .map(str::to_owned);
    let mut lines = vec![header];
    for row in rows {
        let aliases = if row.aliases.is_empty() {
            "-".to_owned()
        } else {
            row.aliases.join(",")
        };
        let price = match &row.price {
            None => UNKNOWN.to_owned(),
            Some((currency, input, output)) => format!(
                "{}/{} {currency}",
                input.as_deref().unwrap_or(UNKNOWN),
                output.as_deref().unwrap_or(UNKNOWN)
            ),
        };
        lines.push([
            row.id.clone(),
            aliases,
            row.protocol.to_owned(),
            row.provider.clone(),
            row.host.clone(),
            row.model.clone(),
            price,
        ]);
    }
    let mut widths = [0; 7];
    for line in &lines {
        for (width, cell) in widths.iter_mut().zip(line) {
            *width = (*width).max(cell.chars().count());
        }
    }
    let mut text = String::new();
    for line in &lines {
        let cells: Vec<String> = line
            .iter()
            .zip(widths)
            .map(|(cell, width)| format!("{cell:width$}"))
            .collect();
        text.push_str(cells.join("  ").trim_end());
        text.push('\n');
    }
    text
}

/// The wire name of `protocol`, as an `llm.routes/1` document writes it.
fn protocol(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::Responses => "responses",
        Protocol::Messages => "messages",
        Protocol::ChatCompletions => "chat-completions",
        Protocol::BedrockConverse => "bedrock-converse",
    }
}

/// The kebab-case name of an error kind, as `llm_core` serialises it.
fn code(code: ErrorCode) -> String {
    serde_json::to_value(code)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("{code:?}"))
}

fn infer(routes: &Routes, infer: &Infer, environment: &Environment) -> Result<(), Failure> {
    let name = infer.model.as_str();
    let target = routes.target(name).map_err(|_| {
        Failure::refused(format!(
            "llm: `{name}` is no route, alias or <endpoint>/<model-id> in the configuration"
        ))
    })?;
    let route = target.serving_model.to_string();
    let refused = |error: &Error| {
        Failure::refused(format!(
            "llm: route `{route}`: {}: {}",
            code(error.code),
            error.message
        ))
    };
    let mut sources = SecretSources::new();
    if let Some(home) = &environment.home {
        sources = sources.with_home(home);
    }
    if let (Some(uri), Some(token_file)) = (
        &environment.container_uri,
        &environment.container_token_file,
    ) {
        sources = sources.with_container_credentials(uri, token_file);
    }
    let resolvers = routes
        .resolvers(&sources)
        .map_err(|error| refused(&error))?;
    let prompt = if let Some(prompt) = &infer.prompt {
        prompt.clone()
    } else {
        let mut prompt = String::new();
        io::stdin()
            .read_to_string(&mut prompt)
            .map_err(|_| Failure::refused("llm: the prompt on stdin is not UTF-8 text"))?;
        prompt
    };
    if prompt.trim().is_empty() {
        return Err(Failure::refused(
            "llm: no prompt: give it as an argument or on stdin",
        ));
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| Failure {
            code: 1,
            message: "llm: cannot start the runtime".to_owned(),
        })?;
    runtime.block_on(async {
        let http = HttpClient::new(Limits::default()).map_err(|error| refused(&error))?;
        let port = routes
            .port(name, http, &resolvers)
            .map_err(|error| refused(&error))?;
        let mut request =
            TurnRequest::new(port.provenance().model.as_str(), vec![Item::user(prompt)]);
        if let Some(system) = &infer.system {
            request.instructions.clone_from(system);
        }
        request.max_output_tokens = infer.max_output_tokens;
        match streamed(&port, &request).await {
            Ok(outcome) => {
                let cost = match routes.cost(name, &outcome.observation) {
                    Ok(None) => String::new(),
                    Ok(Some(cost)) => cost.total.complete_total.map_or_else(
                        || " cost=unknown".to_owned(),
                        |total| format!(" cost={total} {}", cost.currency.as_str()),
                    ),
                    Err(_) => " cost=unknown".to_owned(),
                };
                eprintln!(
                    "route={route} protocol={}{}{cost}",
                    protocol(port.protocol()),
                    usage(outcome.observation.usage.as_ref())
                );
                Ok(())
            }
            Err(error) if error.code == ErrorCode::Cancelled => Err(Failure {
                code: 130,
                message: format!("llm: route `{route}`: cancelled"),
            }),
            Err(error) if before_sending(&error) => Err(refused(&error)),
            Err(error) => Err(Failure {
                code: 1,
                message: format!("llm: route `{route}`: {}", code(error.code)),
            }),
        }
    })
}

/// Runs the turn, writing its text to stdout as it arrives; Ctrl-C cancels it through its
/// [`Cancel`], and the turn then ends as the client reports it.
async fn streamed(model: &dyn Model, request: &TurnRequest) -> Result<TurnOutcome, Error> {
    let cancel = Cancel::new();
    let mut sink = Stdout::default();
    let returned = {
        let turn = model.turn(request, &mut sink, &cancel);
        tokio::pin!(turn);
        tokio::select! {
            returned = &mut turn => returned,
            _ = tokio::signal::ctrl_c() => {
                cancel.cancel();
                turn.await
            }
        }
    };
    if sink.shown && !sink.ended_with_newline {
        println!();
    }
    returned
}

/// A refusal the request never left the process for: the caller's own input or credentials.
fn before_sending(error: &Error) -> bool {
    error.dispatch == Dispatch::NotSent
        && matches!(
            error.code,
            ErrorCode::InvalidRequest
                | ErrorCode::Unauthorized
                | ErrorCode::Unsupported
                | ErrorCode::TooLarge
        )
}

fn usage(usage: Option<&Usage>) -> String {
    let count = |value: Option<u64>| value.map_or_else(|| UNKNOWN.to_owned(), |n| n.to_string());
    format!(
        " input_tokens={} output_tokens={}",
        count(usage.and_then(|usage| usage.input_tokens)),
        count(usage.and_then(|usage| usage.output_tokens))
    )
}

/// The sink that writes each text delta to stdout as it arrives.
#[derive(Default)]
struct Stdout {
    shown: bool,
    ended_with_newline: bool,
}

impl StreamSink for Stdout {
    fn emit(&mut self, event: StreamEvent) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            if let StreamEvent::TextDelta { text } = event {
                if text.is_empty() {
                    return Ok(());
                }
                let mut stdout = io::stdout().lock();
                stdout
                    .write_all(text.as_bytes())
                    .and_then(|()| stdout.flush())
                    .map_err(|_| Error::cancelled())?;
                self.shown = true;
                self.ended_with_newline = text.ends_with('\n');
            }
            Ok(())
        })
    }
}
