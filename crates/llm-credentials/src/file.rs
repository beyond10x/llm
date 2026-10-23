//! Explicit protected mounted files. Files are raw bytes: nothing is trimmed or decoded.
use crate::{ReferenceError, ResolvedSecret, SecretError, SecretRef, SecretResolver, local};
use llm_core::BoxFuture;
use std::{
    collections::BTreeMap,
    fmt,
    path::{Component, PathBuf},
    sync::Arc,
};
use tokio::sync::Semaphore;

/// Which files a resolver accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Policy {
    /// The path itself, no link anywhere on it, a file only its owner can read.
    Strict,
    /// A file a platform mounts for the process, such as a Kubernetes projected volume: links
    /// are followed, but only from directories nobody else can write, and the file others may
    /// read but not write or execute.
    Mounted,
}

/// No filesystem I/O occurs until a known reference is resolved. No files are written.
pub struct FileResolver {
    bindings: BTreeMap<SecretRef, PathBuf>,
    policy: Policy,
    permits: Arc<Semaphore>,
}
impl FileResolver {
    /// # Errors
    /// Refuses relative/traversing paths and more than 4096 bindings. Only Linux file
    /// protection semantics are implemented; other platforms fail closed on resolve.
    pub fn new(bindings: BTreeMap<SecretRef, PathBuf>) -> Result<Self, SecretError> {
        Self::with_policy(bindings, Policy::Strict)
    }

    /// A resolver for files the platform mounts, re-read on every resolve. A Kubernetes
    /// projected volume names its file through links (`token` to `..data/token`, `..data` to a
    /// timestamped directory the kubelet replaces on rotation); this resolver follows them,
    /// at most 40, and accepts a link only in a directory owned by root or the process and
    /// writable by nobody else. The file it reaches must be a regular file with one link, owned
    /// by root or the process, writable and executable by its owner alone, with no set-id or
    /// sticky bit: `0644` passes, `0664` does not. A rotation that removes the file between the
    /// link walk and the read is retried once.
    ///
    /// # Errors
    /// As [`FileResolver::new`].
    pub fn mounted(bindings: BTreeMap<SecretRef, PathBuf>) -> Result<Self, SecretError> {
        Self::with_policy(bindings, Policy::Mounted)
    }

    fn with_policy(
        bindings: BTreeMap<SecretRef, PathBuf>,
        policy: Policy,
    ) -> Result<Self, SecretError> {
        if bindings.len() > local::MAX_BINDINGS {
            return Err(SecretError::TooManyReferences);
        }
        for path in bindings.values() {
            if !path.is_absolute()
                || path.file_name().is_none()
                || path.components().any(|part| {
                    !matches!(
                        part,
                        Component::RootDir | Component::Normal(_) | Component::Prefix(_)
                    )
                })
            {
                return Err(SecretError::UnsafeSource);
            }
        }
        Ok(Self {
            bindings,
            policy,
            permits: Arc::new(Semaphore::new(local::MAX_BLOCKING_READS)),
        })
    }

    /// Resolves as [`SecretResolver::resolve`] does, with a refusal that names `reference`.
    ///
    /// # Errors
    /// The same refusal kinds as `resolve`; the error never carries the path or the value.
    pub async fn read(&self, reference: &SecretRef) -> Result<ResolvedSecret, ReferenceError> {
        self.lookup(reference)
            .await
            .map_err(|kind| ReferenceError::new(kind, reference.clone()))
    }

    async fn lookup(&self, reference: &SecretRef) -> Result<ResolvedSecret, SecretError> {
        let path = self
            .bindings
            .get(reference)
            .ok_or(SecretError::Missing)?
            .clone();
        let policy = self.policy;
        local::blocking(self.permits.clone(), move || read(&path, policy)).await
    }
}
impl fmt::Debug for FileResolver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileResolver")
            .field("bindings", &self.bindings.len())
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}
impl SecretResolver for FileResolver {
    fn resolve<'a>(
        &'a self,
        reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(self.lookup(reference))
    }
}

#[cfg(not(target_os = "linux"))]
fn read(_: &std::path::Path, _: Policy) -> Result<ResolvedSecret, SecretError> {
    Err(SecretError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
fn read(path: &std::path::Path, policy: Policy) -> Result<ResolvedSecret, SecretError> {
    match policy {
        Policy::Strict => linux::read(path, 0o7177),
        Policy::Mounted => match linux::read(&linux::resolve_links(path)?, 0o7133) {
            Err(SecretError::Missing) => linux::read(&linux::resolve_links(path)?, 0o7133),
            read => read,
        },
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use crate::{MAX_SECRET_BYTES, ResolvedSecret, Secret, SecretError, local};
    use rustix::{
        fs::{Mode, OFlags, open, openat},
        io::Errno,
        process::geteuid,
    };
    use std::{
        ffi::OsString,
        fs::{self, File, Metadata},
        io::ErrorKind,
        os::unix::fs::MetadataExt,
        path::{Component, Path, PathBuf},
    };

    /// The most links one mounted path may pass through.
    const MAX_LINKS: usize = 40;

    fn failure(error: Errno) -> SecretError {
        match error {
            Errno::NOENT => SecretError::Missing,
            Errno::LOOP | Errno::NOTDIR | Errno::ACCESS | Errno::PERM => SecretError::UnsafeSource,
            _ => SecretError::Unavailable,
        }
    }
    fn io_failure(error: &std::io::Error) -> SecretError {
        match error.kind() {
            ErrorKind::NotFound => SecretError::Missing,
            ErrorKind::PermissionDenied | ErrorKind::NotADirectory => SecretError::UnsafeSource,
            _ => SecretError::Unavailable,
        }
    }
    fn trusted_owner(stat: &Metadata) -> bool {
        stat.uid() == 0 || stat.uid() == geteuid().as_raw()
    }
    fn directory(stat: &Metadata) -> Result<(), SecretError> {
        let writable = stat.mode() & 0o022 != 0;
        let sticky_root = stat.uid() == 0 && stat.mode() & 0o1000 != 0;
        if !stat.is_dir() || !trusted_owner(stat) || (writable && !sticky_root) {
            return Err(SecretError::UnsafeSource);
        }
        Ok(())
    }
    fn regular(stat: &Metadata, forbidden: u32) -> Result<(), SecretError> {
        if !stat.is_file()
            || !trusted_owner(stat)
            || stat.mode() & forbidden != 0
            || stat.nlink() != 1
        {
            return Err(SecretError::UnsafeSource);
        }
        Ok(())
    }

    enum Step {
        Name(OsString),
        Up,
    }

    /// Pushes a path's steps so the first is popped first.
    fn push(pending: &mut Vec<Step>, path: &Path) {
        let steps: Vec<_> = path
            .components()
            .filter_map(|part| match part {
                Component::Normal(name) => Some(Step::Name(name.to_owned())),
                Component::ParentDir => Some(Step::Up),
                Component::RootDir | Component::CurDir | Component::Prefix(_) => None,
            })
            .collect();
        pending.extend(steps.into_iter().rev());
    }

    /// The path with every link replaced by its target. A link is followed only from a
    /// directory [`directory`] accepts; the directories of the result are checked by [`read`].
    pub(super) fn resolve_links(path: &Path) -> Result<PathBuf, SecretError> {
        let mut pending = Vec::new();
        push(&mut pending, path);
        let mut current = PathBuf::from("/");
        let mut links = 0;
        while let Some(step) = pending.pop() {
            let name = match step {
                Step::Up => {
                    current.pop();
                    continue;
                }
                Step::Name(name) => name,
            };
            let candidate = current.join(&name);
            let stat = fs::symlink_metadata(&candidate).map_err(|error| io_failure(&error))?;
            if !stat.file_type().is_symlink() {
                current = candidate;
                continue;
            }
            links += 1;
            if links > MAX_LINKS {
                return Err(SecretError::UnsafeSource);
            }
            directory(&fs::symlink_metadata(&current).map_err(|error| io_failure(&error))?)?;
            let target = fs::read_link(&candidate).map_err(|error| io_failure(&error))?;
            if target.is_absolute() {
                current = PathBuf::from("/");
            }
            push(&mut pending, &target);
        }
        Ok(current)
    }

    /// Reads the file at `path`, which no link may be on, refusing it when its mode has any of
    /// the `forbidden` bits.
    pub(super) fn read(path: &Path, forbidden: u32) -> Result<ResolvedSecret, SecretError> {
        let flags =
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::NOCTTY;
        let metadata = |file: &File| file.metadata().map_err(|_| SecretError::Unavailable);
        let mut parent =
            File::from(open("/", flags | OFlags::DIRECTORY, Mode::empty()).map_err(failure)?);
        directory(&metadata(&parent)?)?;
        let parts: Vec<_> = path
            .components()
            .filter_map(|part| {
                if let Component::Normal(name) = part {
                    Some(name)
                } else {
                    None
                }
            })
            .collect();
        let (leaf, directories) = parts.split_last().ok_or(SecretError::UnsafeSource)?;
        for component in directories {
            parent = File::from(
                openat(
                    &parent,
                    *component,
                    flags | OFlags::DIRECTORY,
                    Mode::empty(),
                )
                .map_err(failure)?,
            );
            directory(&metadata(&parent)?)?;
        }
        let file = File::from(openat(&parent, *leaf, flags, Mode::empty()).map_err(failure)?);
        let before = metadata(&file)?;
        regular(&before, forbidden)?;
        let size = usize::try_from(before.len()).map_err(|_| SecretError::TooLarge)?;
        if size > MAX_SECRET_BYTES {
            return Err(SecretError::TooLarge);
        }
        let mut bytes = super::read_exact_secret(&file, size)?;
        let after = metadata(&file)?;
        regular(&after, forbidden)?;
        if before.len() != after.len()
            || before.mtime() != after.mtime()
            || before.mtime_nsec() != after.mtime_nsec()
            || before.ctime() != after.ctime()
            || before.ctime_nsec() != after.ctime_nsec()
            || bytes.len() != size
        {
            return Err(SecretError::Unavailable);
        }
        local::resolved(Secret::new(std::mem::take(&mut *bytes))?)
    }
}

/// Reads exactly `size` bytes into storage allocated once, so no reallocation can leave an
/// unzeroized copy of secret material behind, then requires end of file.
#[cfg(any(target_os = "linux", test))]
fn read_exact_secret(
    mut source: impl std::io::Read,
    size: usize,
) -> Result<zeroize::Zeroizing<Vec<u8>>, SecretError> {
    let mut bytes = zeroize::Zeroizing::new(vec![0; size]);
    source
        .read_exact(&mut bytes)
        .map_err(|_| SecretError::Unavailable)?;
    let mut probe = zeroize::Zeroizing::new([0; 1]);
    loop {
        match source.read(&mut *probe) {
            Ok(0) => return Ok(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Ok(_) | Err(_) => return Err(SecretError::Unavailable),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn exact_read_keeps_one_allocation_and_refuses_short_or_long_sources() {
        let bytes = read_exact_secret(Cursor::new(b"abc\n".to_vec()), 4).unwrap();
        assert_eq!(bytes.as_slice(), b"abc\n");
        assert_eq!(bytes.capacity(), 4);
        assert!(
            read_exact_secret(Cursor::new(Vec::new()), 0)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            read_exact_secret(Cursor::new(b"abc".to_vec()), 4).unwrap_err(),
            SecretError::Unavailable
        );
        assert_eq!(
            read_exact_secret(Cursor::new(b"abcde".to_vec()), 4).unwrap_err(),
            SecretError::Unavailable
        );
    }
}
