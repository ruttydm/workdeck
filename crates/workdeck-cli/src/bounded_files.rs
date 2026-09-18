//! Bounded descriptor reads shared by inert repository data adapters.
//! Unix opens each component relative to an already-owned directory descriptor.
//! A parent replacement cannot redirect the descriptor to its replacement.
use anyhow::{Context, Result, bail};
use std::{
    fs::File,
    io::Read,
    path::{Component, Path, PathBuf},
};

pub(crate) struct ReadFile {
    pub bytes: Vec<u8>,
    pub truncated: bool,
    pub size: u64,
}

pub(crate) fn validate_relative(path: &Path, allow_root: bool) -> Result<()> {
    let text = path.to_str().context("repository paths must be UTF-8")?;
    if text.is_empty() && allow_root {
        return Ok(());
    }
    if text.is_empty()
        || text.contains(['\\', ':'])
        || text.chars().any(char::is_control)
        || text
            .trim_end_matches('/')
            .split('/')
            .any(|part| matches!(part, "" | "." | ".."))
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        bail!("expected a portable repository-relative path without traversal");
    }
    Ok(())
}

pub(crate) fn read(root: &Path, relative: &Path, max_bytes: usize) -> Result<ReadFile> {
    validate_relative(relative, false)?;
    let file = open(root, relative, false)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        bail!(
            "only regular repository files can be read: {}",
            relative.display()
        );
    }
    let mut bytes = Vec::new();
    file.take(max_bytes.saturating_add(1) as u64)
        .read_to_end(&mut bytes)?;
    let truncated = bytes.len() > max_bytes;
    bytes.truncate(max_bytes);
    Ok(ReadFile {
        bytes,
        truncated,
        size: metadata.len(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    File,
    Directory,
    Symlink,
    Other,
}
#[derive(Debug)]
pub(crate) struct Entry {
    pub name: String,
    pub kind: Kind,
    pub size: u64,
}

/// Returns at most `limit` entries and an explicit coverage flag. Metadata is
/// obtained relative to the open directory, without following entry symlinks.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn list(root: &Path, relative: &Path, limit: usize) -> Result<(Vec<Entry>, bool)> {
    use std::{
        ffi::{CStr, CString},
        os::fd::{AsRawFd, IntoRawFd},
    };
    validate_relative(relative, true)?;
    let file = open(root, relative, true)?;
    let descriptor = file.as_raw_fd();
    // SAFETY: fdopendir takes ownership only on success.
    let pointer = unsafe { libc::fdopendir(descriptor) };
    if pointer.is_null() {
        return Err(std::io::Error::last_os_error().into());
    }
    let _ = file.into_raw_fd();
    struct Directory(*mut libc::DIR);
    impl Drop for Directory {
        fn drop(&mut self) {
            unsafe {
                libc::closedir(self.0);
            }
        }
    }
    let directory = Directory(pointer);
    let mut entries = Vec::new();
    let mut truncated = false;
    loop {
        #[cfg(target_os = "macos")]
        let errno = unsafe { libc::__error() };
        #[cfg(target_os = "linux")]
        let errno = unsafe { libc::__errno_location() };
        unsafe {
            *errno = 0;
        }
        let entry = unsafe { libc::readdir(directory.0) };
        if entry.is_null() {
            if unsafe { *errno } != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            break;
        }
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
        if matches!(name.to_bytes(), b"." | b"..") {
            continue;
        }
        if entries.len() == limit {
            truncated = true;
            break;
        }
        let name = name
            .to_str()
            .context("repository filename must be UTF-8")?
            .to_owned();
        validate_relative(Path::new(&name), false)?;
        let name_c = CString::new(name.as_bytes())?;
        let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe {
            libc::fstatat(
                descriptor,
                name_c.as_ptr(),
                metadata.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let metadata = unsafe { metadata.assume_init() };
        let kind = match metadata.st_mode & libc::S_IFMT {
            libc::S_IFREG => Kind::File,
            libc::S_IFDIR => Kind::Directory,
            libc::S_IFLNK => Kind::Symlink,
            _ => Kind::Other,
        };
        entries.push(Entry {
            name,
            kind,
            size: metadata.st_size.max(0) as u64,
        });
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok((entries, truncated))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn list(_root: &Path, _relative: &Path, _limit: usize) -> Result<(Vec<Entry>, bool)> {
    bail!("descriptor-bound repository directory enumeration is not supported on this platform")
}

#[cfg(unix)]
fn open(root: &Path, relative: &Path, directory_only: bool) -> Result<File> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::{ffi::OsStrExt, fs::OpenOptionsExt},
        },
    };
    let mut directory = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC)
        .open(root)
        .with_context(|| format!("failed to open source directory {}", root.display()))?;
    let parts = relative.components().collect::<Vec<_>>();
    for (index, part) in parts.iter().enumerate() {
        let name = CString::new(part.as_os_str().as_bytes())?;
        let flags = libc::O_RDONLY
            | libc::O_NOFOLLOW
            | libc::O_CLOEXEC
            | if directory_only || index + 1 < parts.len() {
                libc::O_DIRECTORY
            } else {
                libc::O_NONBLOCK
            };
        let descriptor = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
        if descriptor < 0 {
            return Err(std::io::Error::last_os_error())
                .with_context(|| format!("failed to open {}", root.join(relative).display()));
        }
        directory = unsafe { File::from_raw_fd(descriptor) };
    }
    Ok(directory)
}

#[cfg(not(unix))]
fn open(_root: &Path, _relative: &Path, _directory_only: bool) -> Result<File> {
    bail!("descriptor-bound repository reads are not supported on this platform")
}

/// Read an explicitly selected absolute source, preserving leaf-symlink
/// rejection while allowing canonical system directory aliases such as /var.
pub(crate) fn read_absolute(path: &Path, max_bytes: usize) -> Result<ReadFile> {
    let parent = path
        .parent()
        .context("source file has no parent")?
        .canonicalize()?;
    let name = path.file_name().context("source file has no filename")?;
    read(&parent, &PathBuf::from(name), max_bytes)
}
