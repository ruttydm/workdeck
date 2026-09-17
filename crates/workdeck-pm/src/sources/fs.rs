//! Descriptor-bound metadata capture. A captured path is never authority to follow a link.
use crate::{ErrorCode, PmError, Result};
use std::{
    fs::File,
    io::Read,
    path::{Component, Path, PathBuf},
    time::SystemTime,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Identity(pub u64, pub u64);

/// Metadata that can be checked without rereading a file's content. The
/// change-time tuple catches ordinary same-size edits even when an editor
/// preserves the modification timestamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stamp {
    pub(crate) identity: Identity,
    pub(crate) bytes: u64,
    pub(crate) modified: Option<SystemTime>,
    pub(crate) changed: Option<(i64, i64)>,
}

/// Reusable descriptor for the parent of a sequence of relative paths. The
/// cache is only an I/O optimization; callers still perform a fresh final
/// source guard before publication, so a replaced directory cannot be treated
/// as the same planning source.
#[derive(Default)]
pub(crate) struct RelativeOpenCache {
    parent: Option<(PathBuf, File)>,
}
#[cfg(unix)]
fn identity(metadata: &std::fs::Metadata) -> Identity {
    use std::os::unix::fs::MetadataExt;
    Identity(metadata.dev(), metadata.ino())
}

#[cfg(unix)]
pub(crate) fn open(path: &Path, directory: bool) -> Result<File> {
    if !path.is_absolute() {
        return Err(unsafe_path(path));
    }
    let file = File::open("/").map_err(|e| PmError::io(path, e))?;
    open_components(file, path, path, directory)
}

/// Open a path relative to an already-open, descriptor-bound directory. The
/// root descriptor is reused by source-capture workers, avoiding a full
/// absolute-path traversal for every small authority file while retaining
/// O_NOFOLLOW and O_DIRECTORY checks for every component.
#[cfg(unix)]
pub(crate) fn open_relative(
    root: &File,
    root_path: &Path,
    relative: &Path,
    directory: bool,
) -> Result<File> {
    if relative.is_absolute() {
        return Err(unsafe_path(&root_path.join(relative)));
    }
    let base = root
        .try_clone()
        .map_err(|error| PmError::io(root_path, error))?;
    open_components(base, relative, &root_path.join(relative), directory)
}

#[cfg(unix)]
pub(crate) fn open_relative_cached(
    cache: &mut RelativeOpenCache,
    root: &File,
    root_path: &Path,
    relative: &Path,
    directory: bool,
) -> Result<File> {
    if relative.is_absolute() {
        return Err(unsafe_path(&root_path.join(relative)));
    }
    let mut components = relative.components();
    let Some(last) = components.next_back() else {
        return Err(unsafe_path(&root_path.join(relative)));
    };
    if !matches!(last, Component::Normal(_))
        || components.any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(unsafe_path(&root_path.join(relative)));
    }
    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    let refresh = cache
        .parent
        .as_ref()
        .is_none_or(|(cached_parent, _)| cached_parent != parent);
    if refresh {
        let descriptor = if parent.as_os_str().is_empty() {
            root.try_clone()
                .map_err(|error| PmError::io(root_path, error))?
        } else {
            open_relative(root, root_path, parent, true)?
        };
        cache.parent = Some((parent.to_owned(), descriptor));
    }
    let descriptor = cache
        .parent
        .as_ref()
        .expect("relative parent descriptor is initialized")
        .1
        .try_clone()
        .map_err(|error| PmError::io(root_path, error))?;
    open_components(
        descriptor,
        Path::new(last.as_os_str()),
        &root_path.join(relative),
        directory,
    )
}

#[cfg(unix)]
fn open_components(mut file: File, path: &Path, display: &Path, directory: bool) -> Result<File> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::ffi::OsStrExt,
        },
    };
    let parts = path
        .components()
        .filter(|p| !matches!(p, Component::RootDir))
        .collect::<Vec<_>>();
    for (i, part) in parts.iter().enumerate() {
        let Component::Normal(part) = part else {
            return Err(unsafe_path(display));
        };
        let name = CString::new(part.as_bytes()).map_err(|_| unsafe_path(display))?;
        let flags = libc::O_RDONLY
            | libc::O_CLOEXEC
            | libc::O_NOFOLLOW
            | libc::O_NONBLOCK
            | if i + 1 < parts.len() || directory {
                libc::O_DIRECTORY
            } else {
                0
            };
        let fd = unsafe { libc::openat(file.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            let error = std::io::Error::last_os_error();
            return Err(
                if matches!(error.raw_os_error(), Some(libc::ELOOP | libc::ENOTDIR)) {
                    unsafe_path(display)
                } else {
                    PmError::io(display, error)
                },
            );
        }
        file = unsafe { File::from_raw_fd(fd) };
    }
    Ok(file)
}
pub(super) fn unsafe_path(path: &Path) -> PmError {
    PmError::new(
        ErrorCode::UnsafePath,
        "source path must contain ordinary directories and a regular file",
    )
    .at(path)
}
#[cfg(unix)]
pub(crate) fn directory(path: &Path) -> Result<Identity> {
    let file = open(path, true)?;
    file.metadata()
        .map(|m| identity(&m))
        .map_err(|e| PmError::io(path, e))
}
#[cfg(unix)]
pub(crate) fn read(path: &Path, limit: usize) -> Result<(Vec<u8>, Identity)> {
    let file = open(path, false)?;
    read_opened(path, file, limit)
}

#[cfg(unix)]
pub(crate) fn read_relative(
    root: &File,
    root_path: &Path,
    relative: &Path,
    limit: usize,
) -> Result<(Vec<u8>, Stamp)> {
    let mut cache = RelativeOpenCache::default();
    read_relative_cached(&mut cache, root, root_path, relative, limit)
}

#[cfg(unix)]
pub(crate) fn read_relative_cached(
    cache: &mut RelativeOpenCache,
    root: &File,
    root_path: &Path,
    relative: &Path,
    limit: usize,
) -> Result<(Vec<u8>, Stamp)> {
    let path = root_path.join(relative);
    let file = open_relative_cached(cache, root, root_path, relative, false)?;
    read_opened_stamp(&path, file, limit)
}

#[cfg(unix)]
fn read_opened(path: &Path, file: File, limit: usize) -> Result<(Vec<u8>, Identity)> {
    let (bytes, stamp) = read_opened_stamp(path, file, limit)?;
    Ok((bytes, stamp.identity))
}

#[cfg(unix)]
fn read_opened_stamp(path: &Path, file: File, limit: usize) -> Result<(Vec<u8>, Stamp)> {
    let before = file.metadata().map_err(|e| PmError::io(path, e))?;
    if !before.is_file() {
        return Err(unsafe_path(path));
    }
    if before.len() > limit as u64 {
        return Err(
            PmError::new(ErrorCode::InvalidInput, "source file exceeds capture bound").at(path),
        );
    }
    let mut bytes = Vec::new();
    (&file)
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| PmError::io(path, e))?;
    let after = file.metadata().map_err(|e| PmError::io(path, e))?;
    if bytes.len() > limit {
        return Err(
            PmError::new(ErrorCode::InvalidInput, "source file exceeds capture bound").at(path),
        );
    }
    let before = stamp(&before);
    let after = stamp(&after);
    if before != after || bytes.len() as u64 != after.bytes {
        return Err(PmError::new(ErrorCode::StaleSource, "source changed while reading").at(path));
    }
    Ok((bytes, after))
}

#[cfg(unix)]
fn stamp(metadata: &std::fs::Metadata) -> Stamp {
    use std::os::unix::fs::MetadataExt;
    Stamp {
        identity: identity(metadata),
        bytes: metadata.len(),
        modified: metadata.modified().ok(),
        changed: Some((metadata.ctime(), metadata.ctime_nsec())),
    }
}

#[cfg(unix)]
pub(crate) fn stamp_relative_cached(
    cache: &mut RelativeOpenCache,
    root: &File,
    root_path: &Path,
    relative: &Path,
) -> Result<Stamp> {
    let path = root_path.join(relative);
    let file = open_relative_cached(cache, root, root_path, relative, false)?;
    let metadata = file.metadata().map_err(|error| PmError::io(&path, error))?;
    if !metadata.is_file() {
        return Err(unsafe_path(&path));
    }
    Ok(stamp(&metadata))
}
#[cfg(not(unix))]
pub(crate) fn directory(_: &Path) -> Result<Identity> {
    Err(PmError::new(
        ErrorCode::Unsupported,
        "source capture requires qualified Unix descriptor reads",
    ))
}
#[cfg(not(unix))]
pub(crate) fn read(_: &Path, _: usize) -> Result<(Vec<u8>, Identity)> {
    Err(PmError::new(
        ErrorCode::Unsupported,
        "source capture requires qualified Unix descriptor reads",
    ))
}
#[cfg(not(unix))]
pub(crate) fn read_relative(_: &File, _: &Path, _: &Path, _: usize) -> Result<(Vec<u8>, Stamp)> {
    Err(PmError::new(
        ErrorCode::Unsupported,
        "source capture requires qualified Unix descriptor reads",
    ))
}
#[cfg(not(unix))]
pub(crate) fn read_relative_cached(
    _: &mut RelativeOpenCache,
    _: &File,
    _: &Path,
    _: &Path,
    _: usize,
) -> Result<(Vec<u8>, Stamp)> {
    Err(PmError::new(
        ErrorCode::Unsupported,
        "source capture requires qualified Unix descriptor reads",
    ))
}
#[cfg(not(unix))]
pub(crate) fn stamp_relative_cached(
    _: &mut RelativeOpenCache,
    _: &File,
    _: &Path,
    _: &Path,
) -> Result<Stamp> {
    Err(PmError::new(
        ErrorCode::Unsupported,
        "source capture requires qualified Unix descriptor reads",
    ))
}
