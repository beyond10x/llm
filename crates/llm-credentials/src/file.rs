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

/// No filesystem I/O occurs until a known reference is resolved. No files are written.
pub struct FileResolver {
    bindings: BTreeMap<SecretRef, PathBuf>,
    permits: Arc<Semaphore>,
}
impl FileResolver {
    /// # Errors
    /// Refuses relative/traversing paths and more than 4096 bindings. Only Linux file
    /// protection semantics are implemented; other platforms fail closed on resolve.
    pub fn new(bindings: BTreeMap<SecretRef, PathBuf>) -> Result<Self, SecretError> {
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
        local::blocking(self.permits.clone(), move || read(&path)).await
    }
}
impl fmt::Debug for FileResolver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileResolver")
            .field("bindings", &self.bindings.len())
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
fn read(_: &std::path::Path) -> Result<ResolvedSecret, SecretError> {
    Err(SecretError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
fn read(path: &std::path::Path) -> Result<ResolvedSecret, SecretError> {
    use crate::{MAX_SECRET_BYTES, Secret};
    use rustix::{
        fs::{Mode, OFlags, open, openat},
        io::Errno,
        process::geteuid,
    };
    use std::{
        fs::{File, Metadata},
        os::unix::fs::MetadataExt,
    };

    fn failure(error: Errno) -> SecretError {
        match error {
            Errno::NOENT => SecretError::Missing,
            Errno::LOOP | Errno::NOTDIR | Errno::ACCESS | Errno::PERM => SecretError::UnsafeSource,
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
    fn regular(stat: &Metadata) -> Result<(), SecretError> {
        if !stat.is_file() || !trusted_owner(stat) || stat.mode() & 0o7177 != 0 || stat.nlink() != 1
        {
            return Err(SecretError::UnsafeSource);
        }
        Ok(())
    }

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
    regular(&before)?;
    let size = usize::try_from(before.len()).map_err(|_| SecretError::TooLarge)?;
    if size > MAX_SECRET_BYTES {
        return Err(SecretError::TooLarge);
    }
    let mut bytes = read_exact_secret(&file, size)?;
    let after = metadata(&file)?;
    regular(&after)?;
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
