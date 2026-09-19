//! Explicit protected mounted files. Files are raw bytes: nothing is trimmed or decoded.
use crate::{ResolvedSecret, SecretError, SecretRef, SecretResolver, local};
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
        Box::pin(async move {
            let path = self
                .bindings
                .get(reference)
                .ok_or(SecretError::Missing)?
                .clone();
            local::blocking(self.permits.clone(), move || read(&path)).await
        })
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
        io::Read,
        os::unix::fs::MetadataExt,
    };
    use zeroize::Zeroizing;

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
    let mut bytes = Zeroizing::new(Vec::with_capacity(size));
    (&file)
        .take((MAX_SECRET_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| SecretError::Unavailable)?;
    if bytes.len() > MAX_SECRET_BYTES {
        return Err(SecretError::TooLarge);
    }
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
