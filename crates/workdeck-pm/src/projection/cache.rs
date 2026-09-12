//! SQLite sees only memory. This module owns the complete binary checkpoint.
use crate::{ContentHash, ErrorCode, PmError, Result, sources::fs};
use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Debug)]
pub(super) struct Cache {
    pub worktree: PathBuf,
    pub planning: PathBuf,
    pub slot: ContentHash,
    worktree_id: fs::Identity,
    planning_id: fs::Identity,
    base: PathBuf,
    base_id: fs::Identity,
    directory: PathBuf,
    directory_id: fs::Identity,
    handle: File,
    limit: usize,
}
impl Cache {
    pub fn open(worktree: &Path, selector: &crate::SourceSelector, limit: usize) -> Result<Self> {
        Self::open_mode(worktree, selector, limit, true)
    }
    pub fn open_cached(
        worktree: &Path,
        selector: &crate::SourceSelector,
        limit: usize,
    ) -> Result<Self> {
        Self::open_mode(worktree, selector, limit, false)
    }
    #[cfg(unix)]
    fn open_mode(
        worktree: &Path,
        selector: &crate::SourceSelector,
        limit: usize,
        create: bool,
    ) -> Result<Self> {
        // Resolve the caller's root spelling once, then retain no-follow identities.
        let worktree = worktree
            .canonicalize()
            .map_err(|e| PmError::io(worktree, e))?;
        let worktree_id = fs::directory(&worktree)?;
        let planning = worktree.join(".workdeck");
        let planning_id = fs::directory(&planning)?;
        let slot = slot_hash(&worktree, worktree_id, planning_id, selector)?;
        let base = planning.join(".index");
        if create {
            crate::execution::local::directory(&planning, &base)?;
        }
        let base_id = fs::directory(&base)?;
        let ignore = base.join(".gitignore");
        match read_optional(&ignore, 4096)? {
            Some(bytes) if bytes == b"*\n" => (),
            Some(_) => {
                return Err(PmError::new(
                    ErrorCode::Conflict,
                    "preserve existing projection ignore policy",
                )
                .at(ignore));
            }
            None if create => {
                publish_ignore(&base)?;
            }
            None => return Err(PmError::new(
                ErrorCode::NotFound,
                "Cached projection ignore policy is missing; explicit index refresh is required",
            )
            .at(ignore)),
        }
        let directory = base.join(slot.as_str());
        if create {
            crate::execution::local::directory(&planning, &directory)?;
        }
        let handle = fs::open(&directory, true)?;
        let directory_id = fs::directory(&directory)?;
        let result = Self {
            worktree,
            planning,
            slot,
            worktree_id,
            planning_id,
            base,
            base_id,
            directory,
            directory_id,
            handle,
            limit,
        };
        result.verify()?;
        Ok(result)
    }
    #[cfg(not(unix))]
    fn open_mode(_: &Path, _: &crate::SourceSelector, _: usize, _: bool) -> Result<Self> {
        Err(PmError::new(
            ErrorCode::Unsupported,
            "projection checkpoints require qualified Unix descriptor paths",
        ))
    }
    pub fn verify(&self) -> Result<()> {
        if fs::directory(&self.worktree)? != self.worktree_id
            || fs::directory(&self.planning)? != self.planning_id
            || fs::directory(&self.base)? != self.base_id
            || fs::directory(&self.directory)? != self.directory_id
        {
            return Err(stale());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = self
                .handle
                .metadata()
                .map_err(|e| PmError::io(&self.directory, e))?;
            if fs::Identity(metadata.dev(), metadata.ino()) != self.directory_id {
                return Err(stale());
            }
        }
        Ok(())
    }
    pub fn read(&self) -> Result<Option<Vec<u8>>> {
        self.verify()?;
        let bytes = read_optional(&self.directory.join("checkpoint.bin"), self.limit)?;
        self.verify()?;
        Ok(bytes)
    }
    #[cfg(unix)]
    fn file(&self, name: &str, flags: i32) -> Result<File> {
        use std::{
            ffi::CString,
            os::fd::{AsRawFd, FromRawFd},
        };
        self.verify()?;
        let name = CString::new(name).map_err(|_| stale())?;
        let descriptor = unsafe {
            libc::openat(
                self.handle.as_raw_fd(),
                name.as_ptr(),
                flags | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
                0o600,
            )
        };
        if descriptor < 0 {
            return Err(PmError::io(
                &self.directory,
                std::io::Error::last_os_error(),
            ));
        }
        let file = unsafe { File::from_raw_fd(descriptor) };
        if !file
            .metadata()
            .map_err(|e| PmError::io(&self.directory, e))?
            .is_file()
        {
            return Err(PmError::new(
                ErrorCode::UnsafePath,
                "projection checkpoint paths must be regular files",
            )
            .at(&self.directory));
        }
        Ok(file)
    }
    /// Compare-and-publish under one coordinator. Returning false never overwrites a winner.
    #[cfg(unix)]
    pub fn publish(
        &self,
        bytes: &[u8],
        expected: Option<&ContentHash>,
        before: impl FnOnce() -> Result<()>,
    ) -> Result<bool> {
        use std::{ffi::CString, fs::TryLockError, os::fd::AsRawFd};
        if bytes.len() > self.limit {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "projection checkpoint exceeds byte limit",
            ));
        }
        let lock = self.file("writer.lock", libc::O_RDWR | libc::O_CREAT)?;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(TryLockError::WouldBlock) => {
                    return Err(PmError::new(
                        ErrorCode::Locked,
                        "another projection refresh owns checkpoint publication",
                    ));
                }
                Err(TryLockError::Error(e)) => return Err(PmError::io(&self.directory, e)),
            }
        }
        // A replaced lock path cannot create a second cooperating writer.
        let (_, lock_id) = fs::read(&self.directory.join("writer.lock"), 0)?;
        use std::os::unix::fs::MetadataExt;
        let metadata = lock
            .metadata()
            .map_err(|e| PmError::io(&self.directory, e))?;
        if lock_id != fs::Identity(metadata.dev(), metadata.ino()) {
            return Err(stale());
        }
        if self.read()?.as_deref().map(ContentHash::of).as_ref() != expected {
            return Ok(false);
        }
        let name = format!(".candidate-{}", crate::OperationId::new());
        let c_name = CString::new(name.as_str()).map_err(|_| stale())?;
        struct Candidate<'a> {
            directory: &'a File,
            name: &'a std::ffi::CStr,
        }
        impl Drop for Candidate<'_> {
            fn drop(&mut self) {
                unsafe { libc::unlinkat(self.directory.as_raw_fd(), self.name.as_ptr(), 0) };
            }
        }
        let _candidate = Candidate {
            directory: &self.handle,
            name: &c_name,
        };
        let target = c"checkpoint.bin";
        (|| {
            let mut file = self.file(&name, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL)?;
            file.write_all(bytes)
                .and_then(|_| file.sync_all())
                .map_err(|e| PmError::io(&self.directory, e))?;
            before()?;
            self.verify()?;
            if fs::read(&self.directory.join("writer.lock"), 0)?.1 != lock_id {
                return Err(stale());
            }
            if self.read()?.as_deref().map(ContentHash::of).as_ref() != expected {
                return Ok(false);
            }
            let result = unsafe {
                libc::renameat(
                    self.handle.as_raw_fd(),
                    c_name.as_ptr(),
                    self.handle.as_raw_fd(),
                    target.as_ptr(),
                )
            };
            if result != 0 {
                return Err(PmError::io(
                    &self.directory,
                    std::io::Error::last_os_error(),
                ));
            }
            self.handle
                .sync_all()
                .map_err(|e| PmError::io(&self.directory, e))?;
            Ok(true)
        })()
    }
    #[cfg(not(unix))]
    pub fn publish(
        &self,
        _: &[u8],
        _: Option<&ContentHash>,
        _: impl FnOnce() -> Result<()>,
    ) -> Result<bool> {
        Err(PmError::new(
            ErrorCode::Unsupported,
            "projection publication requires Unix",
        ))
    }
}
fn read_optional(path: &Path, limit: usize) -> Result<Option<Vec<u8>>> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(PmError::io(path, e)),
        Ok(_) => fs::read(path, limit).map(|(bytes, _)| Some(bytes)),
    }
}
fn stale() -> PmError {
    PmError::new(
        ErrorCode::StaleSource,
        "projection checkout or cache directory changed",
    )
}

pub(super) fn slot_hash(
    worktree: &Path,
    worktree_id: fs::Identity,
    planning_id: fs::Identity,
    selector: &crate::SourceSelector,
) -> Result<ContentHash> {
    crate::transactions::canonical_hash(&serde_json::json!({
        "schema":1,"worktree":worktree,"directory":[worktree_id.0,worktree_id.1],
        "planning":[planning_id.0,planning_id.1],"selector":selector
    }))
}

// Publish complete bytes without replacing another reader's ignore policy.
// A direct create/write exposes an empty file to concurrent cold readers.
#[cfg(unix)]
fn publish_ignore(base: &Path) -> Result<()> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let directory = fs::open(base, true)?;
    let name = std::ffi::CString::new(format!(".ignore-{}", crate::RequestId::new().as_str()))
        .expect("generated identifier");
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(PmError::io(base, std::io::Error::last_os_error()));
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    let result = (|| {
        file.write_all(b"*\n")
            .and_then(|_| file.sync_all())
            .map_err(|error| PmError::io(base, error))?;
        let result = unsafe {
            libc::linkat(
                directory.as_raw_fd(),
                name.as_ptr(),
                directory.as_raw_fd(),
                c".gitignore".as_ptr(),
                0,
            )
        };
        if result != 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::AlreadyExists {
                return Err(PmError::io(base, error));
            }
        }
        directory
            .sync_all()
            .map_err(|error| PmError::io(base, error))?;
        let ignore = base.join(".gitignore");
        if read_optional(&ignore, 4096)?.as_deref() != Some(b"*\n") {
            return Err(PmError::new(
                ErrorCode::Conflict,
                "preserve existing projection ignore policy",
            )
            .at(ignore));
        }
        Ok(())
    })();
    unsafe {
        libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0);
    }
    result
}
