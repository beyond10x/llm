//! A Codex login's access token, read from the `auth.json` the Codex CLI maintains.
//!
//! The resolver is read-only: it never writes, refreshes or caches the file. Each request reads
//! `/tokens/access_token` again and judges its JWT `exp` claim against the caller's clock. An
//! expired or absent token is refused; the caller renews the login by running `codex`, which owns
//! the file.
//!
//! There is no default location. The caller passes an absolute path; Codex keeps the file at
//! `~/.codex/auth.json`, and expanding that is the embedding application's job, so this crate
//! performs no ambient lookup. A relative path is refused before anything is opened, so it is
//! never resolved against the process working directory.
//!
//! Unlike the `file` adapter, no permission or ownership check is made: the Codex CLI owns the file
//! and its protections, and this adapter only reads it.
use crate::{
    MAX_SECRET_BYTES, ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, local,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use llm_core::BoxFuture;
use serde::{
    Deserialize, Deserializer,
    de::{MapAccess, Visitor, value::MapAccessDeserializer},
};
use std::{
    borrow::Cow,
    fmt,
    fs::File,
    io::Read,
    marker::PhantomData,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};
use tokio::sync::Semaphore;
use zeroize::Zeroizing;

type Clock = Arc<dyn Fn() -> SystemTime + Send + Sync>;

/// Resolves one [`SecretRef`] to the access token of one Codex `auth.json`.
///
/// `Debug` shows the reference and the path, never token material.
pub struct CodexAuthFile {
    reference: SecretRef,
    path: PathBuf,
    clock: Clock,
    permits: Arc<Semaphore>,
}

impl CodexAuthFile {
    /// Binds `reference` to the Codex login at `path`. No I/O happens until a request.
    /// Expiry is judged against the system clock unless [`Self::with_clock`] replaces it.
    /// `path` must be absolute: a relative one is accepted here and refused by every request.
    pub fn new(reference: SecretRef, path: impl Into<PathBuf>) -> Self {
        Self {
            reference,
            path: path.into(),
            clock: Arc::new(SystemTime::now),
            permits: Arc::new(Semaphore::new(local::MAX_BLOCKING_READS)),
        }
    }

    /// Replaces the clock the token's `exp` claim is judged against.
    #[must_use]
    pub fn with_clock(mut self, clock: impl Fn() -> SystemTime + Send + Sync + 'static) -> Self {
        self.clock = Arc::new(clock);
        self
    }

    /// The `auth.json` this resolver reads.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads the access token now, with a refusal that names the file.
    ///
    /// # Errors
    /// `Missing` for another reference, an absent file or an absent token; `Expired` when the
    /// token's integer `exp` is not after the clock; `TooLarge` above 1 MiB; `Unavailable` for a
    /// path that is not absolute (nothing is opened), anything but a regular file at the path (a
    /// FIFO is opened without blocking and not read), or an unreadable file, document or token.
    pub async fn read(&self, reference: &SecretRef) -> Result<ResolvedSecret, CodexAuthError> {
        let refusal = |kind| CodexAuthError {
            kind,
            path: self.path.clone(),
            cause: Cause::Source,
        };
        if *reference != self.reference {
            return Err(refusal(SecretError::Missing));
        }
        if !self.path.is_absolute() {
            return Err(CodexAuthError {
                kind: SecretError::Unavailable,
                path: self.path.clone(),
                cause: Cause::NotAbsolute,
            });
        }
        let path = self.path.clone();
        let now = (self.clock)();
        local::blocking(self.permits.clone(), move || read(&path, now))
            .await
            .map_err(refusal)
    }
}

impl fmt::Debug for CodexAuthFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CodexAuthFile")
            .field("reference", &self.reference)
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl SecretResolver for CodexAuthFile {
    fn resolve<'a>(
        &'a self,
        reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async move { self.read(reference).await.map_err(|error| error.kind) })
    }
}

/// A refused Codex login: the [`SecretError`] and the file it concerns. Carries no token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexAuthError {
    kind: SecretError,
    path: PathBuf,
    cause: Cause,
}

/// Why a refusal happened, beyond its kind: only the message differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cause {
    /// The file, its document or its token.
    Source,
    /// The configured path is relative; nothing was opened.
    NotAbsolute,
}

impl CodexAuthError {
    /// The typed refusal, as [`SecretResolver::resolve`] returns it.
    pub fn kind(&self) -> SecretError {
        self.kind
    }

    /// The `auth.json` the refusal concerns.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl fmt::Display for CodexAuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = self.path.display();
        if self.cause == Cause::NotAbsolute {
            return write!(
                f,
                "the Codex login path {path} is not absolute; pass the absolute path of `auth.json`"
            );
        }
        match self.kind {
            SecretError::Expired => write!(
                f,
                "the Codex access token in {path} has expired; run `codex` to refresh the login"
            ),
            SecretError::Missing => {
                write!(f, "no Codex access token in {path}; run `codex` to log in")
            }
            kind => write!(
                f,
                "the Codex login in {path} cannot be used ({kind}); run `codex` to refresh the login"
            ),
        }
    }
}

impl std::error::Error for CodexAuthError {}

/// A JSON object and nothing else. A derived struct also accepts an array, read positionally, so
/// every structure this module reads is wrapped in this.
struct Object<T>(T);

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Object<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Members<T>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for Members<T> {
            type Value = T;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON object")
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<T, A::Error> {
                T::deserialize(MapAccessDeserializer::new(map))
            }
        }
        deserializer
            .deserialize_map(Members(PhantomData))
            .map(Object)
    }
}

#[derive(Deserialize)]
struct Document<'a> {
    #[serde(borrow)]
    tokens: Option<Object<Tokens<'a>>>,
}

#[derive(Deserialize)]
struct Tokens<'a> {
    #[serde(borrow)]
    access_token: Option<Cow<'a, str>>,
}

#[derive(Deserialize)]
struct Claims {
    exp: serde_json::Number,
}

fn read(path: &Path, now: SystemTime) -> Result<ResolvedSecret, SecretError> {
    let file = open(path)?;
    let metadata = file.metadata().map_err(|_| SecretError::Unavailable)?;
    if !metadata.is_file() {
        return Err(SecretError::Unavailable);
    }
    let size = usize::try_from(metadata.len())
        .ok()
        .filter(|size| *size <= MAX_SECRET_BYTES)
        .ok_or(SecretError::TooLarge)?;
    let bytes = read_exact(&file, size)?;
    let Object(document) = serde_json::from_slice::<Object<Document<'_>>>(&bytes)
        .map_err(|_| SecretError::Unavailable)?;
    let token = document
        .tokens
        .and_then(|Object(tokens)| tokens.access_token)
        .filter(|token| !token.is_empty())
        .ok_or(SecretError::Missing)?;
    let token = Zeroizing::new(token.into_owned());
    if expiry(&token)? <= unix_seconds(now) {
        return Err(SecretError::Expired);
    }
    local::resolved(Secret::new(token.as_bytes().to_vec())?)
}

/// Opens `path` for reading without blocking, so a FIFO or a device at the path cannot stall the
/// resolve; the caller then refuses anything but a regular file. Symlinks are followed: the file
/// and its location are the Codex CLI's.
#[cfg(unix)]
fn open(path: &Path) -> Result<File, SecretError> {
    use rustix::{
        fs::{Mode, OFlags},
        io::Errno,
    };
    let flags = OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK | OFlags::NOCTTY;
    rustix::fs::open(path, flags, Mode::empty())
        .map(File::from)
        .map_err(|error| match error {
            Errno::NOENT => SecretError::Missing,
            _ => SecretError::Unavailable,
        })
}

/// Opens `path` for reading; the caller then refuses anything but a regular file.
#[cfg(not(unix))]
fn open(path: &Path) -> Result<File, SecretError> {
    File::open(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => SecretError::Missing,
        _ => SecretError::Unavailable,
    })
}

/// `time` in whole seconds since the Unix epoch, rounded down, negative before it. An integer `exp`
/// is after `time` exactly when it is greater than this.
fn unix_seconds(time: SystemTime) -> i128 {
    match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(since) => i128::from(since.as_secs()),
        Err(before) => {
            let before = before.duration();
            -i128::from(before.as_secs()) - i128::from(before.subsec_nanos() > 0)
        }
    }
}

/// Reads exactly `size` bytes into storage allocated once, so no reallocation can free an
/// unzeroized copy of the login, then requires end of file. A file that changed length since
/// its size was taken is `Unavailable`.
fn read_exact(mut source: impl Read, size: usize) -> Result<Zeroizing<Vec<u8>>, SecretError> {
    let mut bytes = Zeroizing::new(vec![0; size]);
    source
        .read_exact(&mut bytes)
        .map_err(|_| SecretError::Unavailable)?;
    let mut probe = Zeroizing::new([0; 1]);
    loop {
        match source.read(&mut *probe) {
            Ok(0) => return Ok(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Ok(_) | Err(_) => return Err(SecretError::Unavailable),
        }
    }
}

/// The `exp` claim of a JWT, in seconds since the Unix epoch: any JSON integer, negative ones
/// included. A float, a string, `null`, an absent claim or an integer outside `i64` and `u64` is
/// `Unavailable`. The signature is not checked: the issuer does that, and this only decides
/// whether sending the token is pointless.
fn expiry(token: &str) -> Result<i128, SecretError> {
    let payload = token.split('.').nth(1).ok_or(SecretError::Unavailable)?;
    let claims = Zeroizing::new(
        URL_SAFE_NO_PAD
            .decode(payload.trim_end_matches('='))
            .map_err(|_| SecretError::Unavailable)?,
    );
    let Object(Claims { exp }) =
        serde_json::from_slice(&claims).map_err(|_| SecretError::Unavailable)?;
    exp.as_u64()
        .map(i128::from)
        .or_else(|| exp.as_i64().map(i128::from))
        .ok_or(SecretError::Unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Cursor, time::Duration};

    #[test]
    fn exact_read_keeps_one_allocation_and_refuses_short_or_long_sources() {
        let bytes = read_exact(Cursor::new(b"abc\n".to_vec()), 4).unwrap();
        assert_eq!(bytes.as_slice(), b"abc\n");
        assert_eq!(bytes.capacity(), 4);
        assert!(read_exact(Cursor::new(Vec::new()), 0).unwrap().is_empty());
        assert_eq!(
            read_exact(Cursor::new(b"abc".to_vec()), 4).unwrap_err(),
            SecretError::Unavailable
        );
        assert_eq!(
            read_exact(Cursor::new(b"abcde".to_vec()), 4).unwrap_err(),
            SecretError::Unavailable
        );
    }

    #[test]
    fn exp_is_any_json_integer() {
        let token = |claims: &str| format!("h.{}.s", URL_SAFE_NO_PAD.encode(claims));
        for (claims, exp) in [
            (r#"{"exp":-1}"#, Ok(-1)),
            (r#"{"exp":-9223372036854775808}"#, Ok(i128::from(i64::MIN))),
            (r#"{"exp":0}"#, Ok(0)),
            (r#"{"exp":18446744073709551615}"#, Ok(i128::from(u64::MAX))),
            (
                r#"{"exp":18446744073709551616}"#,
                Err(SecretError::Unavailable),
            ),
            (
                r#"{"exp":-9223372036854775809}"#,
                Err(SecretError::Unavailable),
            ),
            (r#"{"exp":1.0}"#, Err(SecretError::Unavailable)),
            (r#"{"exp":1e3}"#, Err(SecretError::Unavailable)),
            (r#"{"exp":"1"}"#, Err(SecretError::Unavailable)),
            (r#"{"exp":null}"#, Err(SecretError::Unavailable)),
            ("{}", Err(SecretError::Unavailable)),
        ] {
            assert_eq!(expiry(&token(claims)), exp, "{claims}");
        }
    }

    #[test]
    fn unix_seconds_round_down_on_both_sides_of_the_epoch() {
        let epoch = SystemTime::UNIX_EPOCH;
        for (time, seconds) in [
            (epoch, 0),
            (epoch + Duration::from_millis(1500), 1),
            (epoch - Duration::from_secs(100), -100),
            (epoch - Duration::from_millis(100_500), -101),
            (epoch - Duration::from_nanos(1), -1),
        ] {
            assert_eq!(unix_seconds(time), seconds, "{time:?}");
        }
    }

    #[test]
    fn only_objects_are_read_and_an_unescaped_token_is_copied_at_most_once_exactly() {
        let parse = |json: &'static str| serde_json::from_str::<Object<Document<'static>>>(json);
        let Object(document) = parse(r#"{"tokens":{"access_token":"abc"}}"#).unwrap();
        match document.tokens.and_then(|Object(t)| t.access_token) {
            Some(Cow::Borrowed(token)) => assert_eq!(token, "abc"),
            Some(Cow::Owned(token)) => {
                assert_eq!(token, "abc");
                assert_eq!(token.capacity(), 3);
            }
            None => panic!("no token"),
        }
        assert!(parse(r#"[{"access_token":"abc"}]"#).is_err());
        assert!(parse(r#"{"tokens":["abc"]}"#).is_err());
        assert!(serde_json::from_str::<Object<Claims>>("[1]").is_err());
    }
}
