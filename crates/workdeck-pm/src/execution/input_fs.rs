//! No-follow descriptor capture used by execution input manifests.
use crate::*;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub(crate) const EXCLUDED: &[&str] = &[
    ".git",
    ".workdeck/.local",
    ".workdeck/.tmp",
    ".workdeck/.index",
    ".workdeck/operations",
    ".workdeck/runs",
    ".workdeck/config.local.yml",
    ".workdeck/config.local.toml",
    ".workdeck/settings.local.yml",
];
pub(crate) fn excluded(path: &Path) -> bool {
    EXCLUDED.iter().any(|p| path.starts_with(p))
}
pub(crate) fn stale() -> PmError {
    PmError::new(
        ErrorCode::StaleSource,
        "selected execution inputs changed during capture or after planning",
    )
}
fn unsafe_path(path: &Path) -> PmError {
    PmError::new(
        ErrorCode::UnsafePath,
        "execution inputs must be safe regular files/directories beneath the bound worktree",
    )
    .at(path)
}
fn limit(path: &Path) -> PmError {
    PmError::new(ErrorCode::InvalidInput,"complete execution input capture exceeds configured bounds; select explicit source/dependency/toolchain inputs").at(path)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileIdentity {
    pub dev: u64,
    pub ino: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HashedFile {
    pub content: ContentHash,
    pub size: u64,
    pub executable: bool,
    pub identity: FileIdentity,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InputFiles {
    pub entries: Vec<InputEntry>,
    pub identities: BTreeMap<PathBuf, FileIdentity>,
    pub root_identity: FileIdentity,
    pub total_bytes: u64,
}

#[cfg(unix)]
mod unix {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::{
        ffi::{CStr, CString},
        fs::File,
        io::Read,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::{ffi::OsStrExt, fs::MetadataExt},
        },
        path::Component,
    };
    pub(super) fn identity(meta: &std::fs::Metadata) -> FileIdentity {
        FileIdentity {
            dev: meta.dev(),
            ino: meta.ino(),
        }
    }
    fn raw_open(parent: &File, name: &std::ffi::OsStr, directory: bool) -> std::io::Result<File> {
        let name = CString::new(name.as_bytes())?;
        let flags = libc::O_RDONLY
            | libc::O_NOFOLLOW
            | libc::O_CLOEXEC
            | libc::O_NONBLOCK
            | if directory { libc::O_DIRECTORY } else { 0 };
        let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(unsafe { File::from_raw_fd(fd) })
        }
    }
    pub(super) fn open(root: &File, path: &Path) -> std::io::Result<File> {
        let parts = path
            .components()
            .filter(|c| !matches!(c, Component::CurDir))
            .collect::<Vec<_>>();
        let mut file = root.try_clone()?;
        for (index, part) in parts.iter().enumerate() {
            let Component::Normal(name) = part else {
                return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput));
            };
            file = raw_open(&file, name, index + 1 < parts.len())?;
        }
        Ok(file)
    }
    pub(super) fn root(path: &Path) -> Result<File> {
        if !path.is_absolute() {
            return Err(unsafe_path(path));
        }
        let base = File::open("/").map_err(|e| PmError::io(path, e))?;
        let relative = path.strip_prefix("/").map_err(|_| unsafe_path(path))?;
        let file = open(&base, relative).map_err(|_| unsafe_path(path))?;
        if !file.metadata().map_err(|e| PmError::io(path, e))?.is_dir() {
            return Err(unsafe_path(path));
        }
        Ok(file)
    }
    fn names(file: &File, path: &Path, max_entries: usize) -> Result<Vec<String>> {
        // opendir(".") produces an independent directory stream; dup would share offsets.
        let directory =
            raw_open(file, std::ffi::OsStr::new("."), true).map_err(|e| PmError::io(path, e))?;
        use std::os::fd::IntoRawFd;
        let fd = directory.into_raw_fd();
        let ptr = unsafe { libc::fdopendir(fd) };
        if ptr.is_null() {
            unsafe { libc::close(fd) };
            return Err(PmError::io(path, std::io::Error::last_os_error()));
        }
        struct Directory(*mut libc::DIR);
        impl Drop for Directory {
            fn drop(&mut self) {
                unsafe { libc::closedir(self.0) };
            }
        }
        let guard = Directory(ptr);
        let mut names = Vec::new();
        loop {
            // errno must distinguish EOF from a failed readdir.
            #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
            unsafe {
                *libc::__error() = 0;
            }
            #[cfg(any(target_os = "linux", target_os = "android"))]
            unsafe {
                *libc::__errno_location() = 0;
            }
            let entry = unsafe { libc::readdir(guard.0) };
            if entry.is_null() {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error().unwrap_or(0) != 0 {
                    return Err(PmError::io(path, error));
                }
                break;
            }
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }
                .to_str()
                .map_err(|_| unsafe_path(path))?;
            if matches!(name, "." | "..") {
                continue;
            }
            crate::commands::validation::leaf(name).map_err(|_| unsafe_path(path))?;
            names.push(name.to_string());
            if names.len() > max_entries {
                return Err(limit(path));
            }
        }
        names.sort();
        Ok(names)
    }
    pub(super) fn hash(mut file: File, path: &Path, max: u64) -> Result<HashedFile> {
        let before = file.metadata().map_err(|e| PmError::io(path, e))?;
        if !before.is_file() {
            return Err(unsafe_path(path));
        }
        if before.len() > max {
            return Err(limit(path));
        }
        let mut hash = Sha256::new();
        let mut size = 0u64;
        let mut buf = [0u8; 64 * 1024];
        loop {
            let count = file.read(&mut buf).map_err(|e| PmError::io(path, e))?;
            if count == 0 {
                break;
            }
            size += count as u64;
            if size > max {
                return Err(limit(path));
            }
            hash.update(&buf[..count]);
        }
        let after = file.metadata().map_err(|e| PmError::io(path, e))?;
        if identity(&before) != identity(&after)
            || before.len() != size
            || after.len() != size
            || before.modified().ok() != after.modified().ok()
        {
            return Err(stale().at(path));
        }
        Ok(HashedFile {
            content: format!("{:x}", hash.finalize()).parse()?,
            size,
            executable: before.mode() & 0o111 != 0,
            identity: identity(&before),
        })
    }
    struct Scan<'a> {
        root: &'a File,
        limits: &'a InputLimits,
        entries: BTreeMap<PathBuf, InputEntry>,
        identities: BTreeMap<PathBuf, FileIdentity>,
        total: u64,
    }
    impl Scan<'_> {
        fn visit(&mut self, path: &Path, tree: bool, optional: bool) -> Result<()> {
            if self.entries.contains_key(path) {
                return Ok(());
            }
            if excluded(path) {
                return Err(PmError::new(
                    ErrorCode::InvalidInput,
                    "explicit selection cannot name excluded Git or Workdeck engine output",
                )
                .at(path));
            }
            if self.entries.len() >= self.limits.max_entries {
                return Err(limit(path));
            }
            let file = match open(self.root, path) {
                Ok(file) => file,
                Err(e) if optional && e.kind() == std::io::ErrorKind::NotFound => {
                    self.entries
                        .insert(path.into(), InputEntry::Absent { path: path.into() });
                    return Ok(());
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Err(PmError::new(
                        ErrorCode::NotFound,
                        "selected execution input is missing",
                    )
                    .at(path));
                }
                Err(_) => return Err(unsafe_path(path)),
            };
            let metadata = file.metadata().map_err(|e| PmError::io(path, e))?;
            self.identities.insert(path.into(), identity(&metadata));
            if metadata.is_dir() {
                if !tree {
                    return Err(unsafe_path(path));
                }
                let names = names(&file, path, self.limits.max_entries)?;
                let mut members = Vec::new();
                let mut children = Vec::new();
                for name in names {
                    let child = if path == Path::new(".") {
                        PathBuf::from(&name)
                    } else {
                        path.join(&name)
                    };
                    if excluded(&child) {
                        continue;
                    }
                    let child_file = raw_open(&file, std::ffi::OsStr::new(&name), false)
                        .map_err(|_| unsafe_path(&child))?;
                    let meta = child_file.metadata().map_err(|e| PmError::io(&child, e))?;
                    let kind = if meta.is_file() {
                        "file"
                    } else if meta.is_dir() {
                        "directory"
                    } else {
                        return Err(unsafe_path(&child));
                    };
                    members.push((name, kind));
                    children.push(child);
                }
                let membership = crate::transactions::canonical_hash(&serde_json::json!(members))?;
                self.entries.insert(
                    path.into(),
                    InputEntry::Directory {
                        path: path.into(),
                        membership,
                    },
                );
                for child in children {
                    self.visit(&child, true, false)?
                }
            } else {
                let value = hash(
                    file,
                    path,
                    self.limits
                        .max_file_bytes
                        .min(self.limits.max_total_bytes.saturating_sub(self.total)),
                )?;
                self.total += value.size;
                self.entries.insert(
                    path.into(),
                    InputEntry::File {
                        path: path.into(),
                        content: value.content,
                        size: value.size,
                        executable: value.executable,
                    },
                );
            }
            Ok(())
        }
    }
    pub(super) fn capture(
        root_path: &Path,
        selection: &InputSelection,
        limits: &InputLimits,
    ) -> Result<InputFiles> {
        let root = root(root_path)?;
        let root_identity = identity(&root.metadata().map_err(|e| PmError::io(root_path, e))?);
        let mut scan = Scan {
            root: &root,
            limits,
            entries: BTreeMap::new(),
            identities: BTreeMap::new(),
            total: 0,
        };
        for path in selection
            .files
            .iter()
            .chain(&selection.dependency_files)
            .chain(&selection.toolchain_files)
        {
            scan.visit(path, false, false)?
        }
        for path in &selection.optional_files {
            scan.visit(path, false, true)?
        }
        for path in &selection.trees {
            if excluded(path) {
                return Err(PmError::new(
                    ErrorCode::InvalidInput,
                    "explicit selection cannot name excluded Git or Workdeck engine output",
                )
                .at(path));
            }
            let directory = open(&root, path).map_err(|_| unsafe_path(path))?;
            if !directory
                .metadata()
                .map_err(|e| PmError::io(path, e))?
                .is_dir()
            {
                return Err(unsafe_path(path));
            }
            scan.visit(path, true, false)?
        }
        Ok(InputFiles {
            entries: scan.entries.into_values().collect(),
            identities: scan.identities,
            root_identity,
            total_bytes: scan.total,
        })
    }
}
pub(crate) fn capture(
    root: &Path,
    selection: &InputSelection,
    limits: &InputLimits,
) -> Result<InputFiles> {
    selection.validate()?;
    let maximum = InputLimits::default();
    if limits.max_entries == 0
        || limits.max_entries > maximum.max_entries
        || limits.max_file_bytes == 0
        || limits.max_file_bytes > maximum.max_file_bytes
        || limits.max_total_bytes == 0
        || limits.max_total_bytes > maximum.max_total_bytes
    {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "execution capture limits exceed supported bounds",
        ));
    }
    #[cfg(unix)]
    {
        unix::capture(root, selection, limits)
    }
    #[cfg(not(unix))]
    {
        let _ = (root, selection, limits);
        Err(PmError::new(
            ErrorCode::Unsupported,
            "descriptor-safe execution inputs are unavailable on this platform",
        ))
    }
}
pub(crate) fn hash_absolute(path: &Path, max: u64) -> Result<HashedFile> {
    #[cfg(unix)]
    {
        let root = unix::root(Path::new("/"))?;
        let file = unix::open(
            &root,
            path.strip_prefix("/").map_err(|_| unsafe_path(path))?,
        )
        .map_err(|_| unsafe_path(path))?;
        unix::hash(file, path, max)
    }
    #[cfg(not(unix))]
    {
        let _ = (path, max);
        Err(PmError::new(
            ErrorCode::Unsupported,
            "descriptor-safe tool capture is unavailable on this platform",
        ))
    }
}
pub(crate) fn directory_identity(root: &Path, relative: &Path) -> Result<FileIdentity> {
    crate::commands::validation::relative(relative, true)?;
    #[cfg(unix)]
    {
        let root = unix::root(root)?;
        let file = unix::open(&root, relative).map_err(|_| unsafe_path(relative))?;
        let metadata = file.metadata().map_err(|e| PmError::io(relative, e))?;
        if !metadata.is_dir() {
            return Err(unsafe_path(relative));
        }
        Ok(unix::identity(&metadata))
    }
    #[cfg(not(unix))]
    {
        let _ = (root, relative);
        Err(PmError::new(
            ErrorCode::Unsupported,
            "descriptor-safe execution cwd is unavailable on this platform",
        ))
    }
}
