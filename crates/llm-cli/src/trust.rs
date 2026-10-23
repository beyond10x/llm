//! The check a routes file passes before a command it names may run.
//!
//! A routes file with an `aws-process` endpoint makes `llm` start a program, so whoever can write
//! the file, or swap a directory or link on its way, can run code as the user. On Unix the file
//! must be a regular file owned by the user or root that neither group nor others can write, and
//! every directory on the way to it, and every directory a link on the way sits in, must be owned
//! by the user or root and writable by nobody else (root's sticky directories, such as `/tmp`,
//! excepted), the rule the mounted `file:` source applies. Other platforms are not checked.

use std::fs::File;
use std::path::Path;

/// Why a routes file may not start programs; a fixed text that quotes no part of the file.
pub(crate) type Refusal = &'static str;

/// Checks the file at `path` that `opened` is, as the module describes.
///
/// # Errors
/// The rule the file or a directory on its way breaks, or that the file `opened` is no longer
/// the one at `path`.
#[cfg(unix)]
pub(crate) fn check(path: &Path, opened: &File) -> Result<(), Refusal> {
    use std::os::unix::fs::MetadataExt;
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| "its directory cannot be read")?
            .join(path)
    };
    let resolved = unix::resolve_links(&absolute)?;
    let mut directory = resolved.clone();
    while directory.pop() {
        unix::directory(&std::fs::symlink_metadata(&directory).map_err(|_| unreadable())?)?;
    }
    let file = std::fs::symlink_metadata(&resolved).map_err(|_| unreadable())?;
    unix::regular(&file)?;
    let held = opened.metadata().map_err(|_| unreadable())?;
    if held.dev() != file.dev() || held.ino() != file.ino() {
        return Err("it changed while it was read");
    }
    Ok(())
}

/// Other platforms are not checked.
#[cfg(not(unix))]
#[expect(clippy::unnecessary_wraps, reason = "the Unix check can refuse")]
pub(crate) fn check(_: &Path, _: &File) -> Result<(), Refusal> {
    Ok(())
}

#[cfg(unix)]
fn unreadable() -> Refusal {
    "a directory or link on its way cannot be read"
}

#[cfg(unix)]
mod unix {
    use super::{Refusal, unreadable};
    use rustix::process::geteuid;
    use std::fs::{self, Metadata};
    use std::os::unix::fs::MetadataExt;
    use std::path::{Component, Path, PathBuf};

    /// The most links one path may pass through.
    const MAX_LINKS: usize = 40;

    fn trusted_owner(stat: &Metadata) -> bool {
        stat.uid() == 0 || stat.uid() == geteuid().as_raw()
    }

    pub(super) fn directory(stat: &Metadata) -> Result<(), Refusal> {
        let writable = stat.mode() & 0o022 != 0;
        let sticky_root = stat.uid() == 0 && stat.mode() & 0o1000 != 0;
        if !stat.is_dir() || !trusted_owner(stat) || (writable && !sticky_root) {
            return Err(
                "a directory on its way is not owned by you or root, or others can write it",
            );
        }
        Ok(())
    }

    pub(super) fn regular(stat: &Metadata) -> Result<(), Refusal> {
        if !stat.is_file() {
            return Err("it is not a regular file");
        }
        if !trusted_owner(stat) {
            return Err("it is not owned by you or root");
        }
        if stat.mode() & 0o022 != 0 {
            return Err("its group or others can write it");
        }
        Ok(())
    }

    enum Step {
        Name(std::ffi::OsString),
        Up,
    }

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

    /// The path with every link replaced by its target, following a link only from a
    /// directory [`directory`] accepts.
    pub(super) fn resolve_links(path: &Path) -> Result<PathBuf, Refusal> {
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
            let stat = fs::symlink_metadata(&candidate).map_err(|_| unreadable())?;
            if !stat.file_type().is_symlink() {
                current = candidate;
                continue;
            }
            links += 1;
            if links > MAX_LINKS {
                return Err("its path passes through too many links");
            }
            directory(&fs::symlink_metadata(&current).map_err(|_| unreadable())?)?;
            let target = fs::read_link(&candidate).map_err(|_| unreadable())?;
            if target.is_absolute() {
                current = PathBuf::from("/");
            }
            push(&mut pending, &target);
        }
        Ok(current)
    }
}
