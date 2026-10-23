//! Credentials an operator's command prints, on the AWS `credential_process` contract.

use super::{AwsCredentials, CredentialSource, Fetch, MAX_PART_BYTES, Refreshing, once};
use crate::time::parse_rfc3339;
use llm_core::{BoxFuture, Cancel, Dispatch, Error, ErrorCode};
use serde::de::{Deserialize, Deserializer, IgnoredAny, MapAccess, Visitor};
use std::fmt;
use std::process::Stdio;
use std::time::{Duration, SystemTime};
use tokio::io::AsyncReadExt;
use tokio::process::{ChildStdout, Command};
use zeroize::Zeroizing;

/// How long one run may take before it is killed.
const TIME_LIMIT: Duration = Duration::from_mins(1);
/// The most a command may print: 64 KiB.
const MAX_ANSWER_BYTES: usize = 64 * 1024;
/// How long output is still read once the command has exited.
const EXIT_GRACE: Duration = Duration::from_millis(250);

/// Credentials a command prints on its standard output, as the AWS CLI's `credential_process`
/// setting runs one: for example `aws configure export-credentials --profile dev`.
///
/// The program runs with its arguments and no shell, with standard input closed and standard
/// error passed through, and must exit with status 0 within one minute after printing at most
/// 64 KiB: one JSON object with `Version` 1, a non-empty `AccessKeyId` and `SecretAccessKey`, and
/// optionally `SessionToken` and an RFC 3339 `Expiration` (any offset) that has not passed. Other
/// members are ignored. A command still running at the limit, or when the source is dropped, is
/// killed. Once the command has exited its output is read for a further quarter second at most,
/// so a process it left behind holding standard output open does not hold the turn.
///
/// Credentials with an `Expiration` are kept and renewed as [`super::ContainerCredentials`]
/// renews its own: one run at a time, inside five minutes before they expire, on its own task,
/// with callers waiting only when nothing unexpired is kept and a failed run leaving the kept
/// credentials in use. Credentials without one are kept for the life of the source, so the
/// command runs once.
///
/// A command that cannot start, fails, overruns or prints an unusable answer is `unauthorized`,
/// `not-sent` and final. Two failures are not: a cancelled caller gets `cancelled`, and a run
/// that ended without an answer, its task gone before it finished, is `transport`, `not-sent`
/// and retriable. Every message names the program and never carries what the command printed,
/// a key or the arguments.
pub struct ProcessCredentials {
    refreshing: Refreshing<ProcessFetch>,
}

/// One command and the time each run of it may take.
struct ProcessFetch {
    program: String,
    args: Vec<String>,
    limit: Duration,
}

impl ProcessCredentials {
    /// A source running `program` with `args`, each run limited to one minute.
    ///
    /// # Errors
    /// `unauthorized` for an empty program. Nothing runs before the first call.
    pub fn new(
        program: impl Into<String>,
        args: impl IntoIterator<Item = String>,
    ) -> Result<Self, Error> {
        Self::with_time_limit(program, args, TIME_LIMIT)
    }

    /// [`ProcessCredentials::new`] with each run limited to `limit` instead of one minute.
    ///
    /// # Errors
    /// `unauthorized` for an empty program.
    pub fn with_time_limit(
        program: impl Into<String>,
        args: impl IntoIterator<Item = String>,
        limit: Duration,
    ) -> Result<Self, Error> {
        let program = program.into();
        if program.is_empty() {
            return Err(Error::new(
                ErrorCode::Unauthorized,
                "credential process has an empty program",
            ));
        }
        Ok(Self {
            refreshing: Refreshing::new(ProcessFetch {
                program,
                args: args.into_iter().collect(),
                limit,
            }),
        })
    }

    /// The program each run starts.
    pub fn program(&self) -> &str {
        &self.refreshing.fetcher().program
    }

    /// Whether a run is in flight now.
    pub fn refreshing(&self) -> bool {
        self.refreshing.in_flight()
    }
}

impl fmt::Debug for ProcessCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let fetch = self.refreshing.fetcher();
        f.debug_struct("ProcessCredentials")
            .field("program", &fetch.program)
            .field("arguments", &fetch.args.len())
            .field("time_limit", &fetch.limit)
            .finish_non_exhaustive()
    }
}

impl CredentialSource for ProcessCredentials {
    fn credentials<'a>(
        &'a self,
        cancel: &'a Cancel,
    ) -> BoxFuture<'a, Result<AwsCredentials, Error>> {
        Box::pin(self.refreshing.credentials(cancel))
    }
}

impl Fetch for ProcessFetch {
    fn fetch<'a>(&'a self, cancel: &'a Cancel) -> BoxFuture<'a, Result<AwsCredentials, Error>> {
        Box::pin(async move {
            let run = tokio::time::timeout(self.limit, self.run());
            let printed = tokio::select! {
                biased;
                () = cancel.cancelled() => return Err(Error::cancelled()),
                ran = run => match ran {
                    Ok(printed) => printed?,
                    Err(_) => {
                        return Err(self.refused(&format!(
                            "did not finish within {:?} and was killed",
                            self.limit
                        )));
                    }
                },
            };
            read_answer(&printed, SystemTime::now()).map_err(|reason| self.refused(reason))
        })
    }

    fn abandoned(&self) -> Error {
        Error::new(
            ErrorCode::Transport,
            format!(
                "credential process `{}` ended without an answer",
                self.program
            ),
        )
        .with_dispatch(Dispatch::NotSent)
        .with_retriable(true)
    }
}

impl ProcessFetch {
    /// One run: what the command printed once it exited with status 0. The child is killed when
    /// this future is dropped before it exits.
    async fn run(&self) -> Result<Zeroizing<Vec<u8>>, Error> {
        let mut child = Command::new(&self.program)
            .args(&self.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| self.refused(&format!("could not be started ({})", error.kind())))?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| self.refused("has no output to read"))?;
        // The whole bound is reserved up front, so no smaller buffer holding part of the answer
        // is freed without being zeroized.
        let mut printed = Zeroizing::new(Vec::with_capacity(MAX_ANSWER_BYTES + 1));
        let ended = {
            let reading = drain(&mut stdout, &mut printed);
            tokio::pin!(reading);
            tokio::select! {
                read = &mut reading => Ended::Closed(read),
                status = child.wait() => {
                    // What the command printed is in the pipe. A process it left behind may
                    // hold the pipe open, so the rest is read only for a short grace.
                    let rest = tokio::time::timeout(EXIT_GRACE, &mut reading).await;
                    Ended::Exited(status, rest.unwrap_or(Ok(())))
                }
            }
        };
        let (status, read) = match ended {
            Ended::Closed(read) => {
                if printed.len() > MAX_ANSWER_BYTES {
                    return Err(self.refused("printed more than 64 KiB"));
                }
                (child.wait().await, read)
            }
            Ended::Exited(status, read) => (status, read),
        };
        read.map_err(|_| self.refused("output could not be read"))?;
        if printed.len() > MAX_ANSWER_BYTES {
            return Err(self.refused("printed more than 64 KiB"));
        }
        let status = status.map_err(|_| self.refused("could not be waited for"))?;
        if !status.success() {
            return Err(self.refused(&status.code().map_or_else(
                || "ended without an exit code".to_owned(),
                |code| format!("exited with code {code}"),
            )));
        }
        Ok(printed)
    }

    fn refused(&self, reason: &str) -> Error {
        Error::new(
            ErrorCode::Unauthorized,
            format!("credential process `{}` {reason}", self.program),
        )
    }
}

/// How a run's output ended: the command closed it, or the command exited first.
enum Ended {
    Closed(std::io::Result<()>),
    Exited(
        std::io::Result<std::process::ExitStatus>,
        std::io::Result<()>,
    ),
}

/// Reads `stdout` into `printed` until it closes or holds one byte more than the bound. Each
/// read is cancel-safe and its bytes are appended at once, so a read stopped at the grace keeps
/// everything read before it; `printed` never grows past the capacity reserved for it.
async fn drain(stdout: &mut ChildStdout, printed: &mut Vec<u8>) -> std::io::Result<()> {
    const CHUNK: usize = 8 * 1024;
    let mut chunk = Zeroizing::new([0_u8; CHUNK]);
    loop {
        let room = (MAX_ANSWER_BYTES + 1).saturating_sub(printed.len());
        if room == 0 {
            return Ok(());
        }
        let read = stdout.read(&mut chunk[..room.min(CHUNK)]).await?;
        if read == 0 {
            return Ok(());
        }
        printed.extend_from_slice(&chunk[..read]);
    }
}

/// A `credential_process` answer: the five members read, each one at most once. The secret
/// members are zeroized on drop from the moment they are decoded. Other members are skipped
/// unread.
struct Answer {
    version: Option<u64>,
    access_key_id: Option<Zeroizing<String>>,
    secret_access_key: Option<Zeroizing<String>>,
    session_token: Option<Nullable<Zeroizing<String>>>,
    expiration: Option<Nullable<String>>,
}

/// A member that was named, possibly as `null`, which counts as absent.
struct Nullable<T>(Option<T>);

fn named<T>(member: Option<Nullable<T>>) -> Option<T> {
    member.and_then(|Nullable(value)| value)
}

impl<'de> Deserialize<'de> for Answer {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(AnswerVisitor)
    }
}

struct AnswerVisitor;

impl<'de> Visitor<'de> for AnswerVisitor {
    type Value = Answer;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a credential_process object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Answer, A::Error> {
        let mut answer = Answer {
            version: None,
            access_key_id: None,
            secret_access_key: None,
            session_token: None,
            expiration: None,
        };
        while let Some(name) = map.next_key::<String>()? {
            match name.as_str() {
                "Version" => once(&mut answer.version, "Version", || map.next_value::<u64>())?,
                "AccessKeyId" => once(&mut answer.access_key_id, "AccessKeyId", || {
                    map.next_value::<String>().map(Zeroizing::new)
                })?,
                "SecretAccessKey" => {
                    once(&mut answer.secret_access_key, "SecretAccessKey", || {
                        map.next_value::<String>().map(Zeroizing::new)
                    })?;
                }
                "SessionToken" => once(&mut answer.session_token, "SessionToken", || {
                    map.next_value::<Option<String>>()
                        .map(|token| Nullable(token.map(Zeroizing::new)))
                })?,
                "Expiration" => once(&mut answer.expiration, "Expiration", || {
                    map.next_value::<Option<String>>().map(Nullable)
                })?,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(answer)
    }
}

/// The credentials in what a command printed, or the fixed reason they cannot be used. No
/// reason quotes the output: a decoder's message could.
fn read_answer(printed: &[u8], now: SystemTime) -> Result<AwsCredentials, &'static str> {
    let text = std::str::from_utf8(printed).map_err(|_| "printed text that is not UTF-8")?;
    let answer: Answer =
        serde_json::from_str(text).map_err(|_| "printed no credential_process JSON object")?;
    if answer.version != Some(1) {
        return Err("printed an answer whose `Version` is not 1");
    }
    let access_key_id = answer
        .access_key_id
        .filter(|part| usable(part))
        .ok_or("printed no usable `AccessKeyId`")?;
    let secret_access_key = answer
        .secret_access_key
        .filter(|part| usable(part))
        .ok_or("printed no usable `SecretAccessKey`")?;
    let session_token = match named(answer.session_token) {
        None => None,
        Some(token) if usable(&token) => Some(token),
        Some(_) => return Err("printed an unusable `SessionToken`"),
    };
    let expires = match named(answer.expiration) {
        None => None,
        Some(text) => {
            let expires = parse_rfc3339(&text)
                .ok_or("printed an `Expiration` that is not an RFC 3339 instant")?;
            if expires <= now {
                return Err("printed credentials that have already expired");
            }
            Some(expires)
        }
    };
    Ok(AwsCredentials {
        access_key_id: access_key_id.as_str().to_owned(),
        secret_access_key,
        session_token,
        expires,
    })
}

/// A key part: non-empty visible ASCII of at most 16 KiB.
fn usable(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= MAX_PART_BYTES
        && part.bytes().all(|byte| byte.is_ascii_graphic())
}
