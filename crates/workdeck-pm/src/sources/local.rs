//! Private request journals; never planning authority or remote confirmation.
use super::fs;
use crate::{ContentHash, ErrorCode, PmError, Repository, Result};
use std::{
    fs::{File, OpenOptions, TryLockError},
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
pub(super) const MAX_JOURNAL_BYTES: usize = 64 * 1024 * 1024;
pub(super) struct LocalState {
    root: PathBuf,
    directory: PathBuf,
    _lock: File,
}
impl LocalState {
    pub(super) fn open(repository: &Repository) -> Result<Self> {
        let root = repository.root().to_owned();
        let directory = root.join(".local/sources");
        crate::execution::local::directory(&root, &directory)?;
        let lock_path = directory.join("writer.lock");
        crate::execution::local::safe(&root, &lock_path)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let lock = options
            .open(&lock_path)
            .map_err(|e| PmError::io(&lock_path, e))?;
        if !lock
            .metadata()
            .map_err(|e| PmError::io(&lock_path, e))?
            .is_file()
        {
            return Err(fs::unsafe_path(&lock_path));
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(TryLockError::WouldBlock) => {
                    return Err(PmError::new(
                        ErrorCode::Locked,
                        "another source publication owns this local coordinator",
                    ));
                }
                Err(TryLockError::Error(e)) => return Err(PmError::io(&lock_path, e)),
            }
        }
        let state = Self {
            root,
            directory,
            _lock: lock,
        };
        match state.read(".gitignore")? {
            Some(bytes) if bytes == b"*\n" => {}
            Some(_) => {
                return Err(PmError::new(
                    ErrorCode::Conflict,
                    "preserve existing source-local ignore policy",
                ));
            }
            None => state.publish(".gitignore", b"*\n", None)?,
        }
        Ok(state)
    }
    pub(super) fn path(&self, name: &str) -> Result<PathBuf> {
        let path = self.directory.join(name);
        crate::execution::local::safe(&self.root, &path)?;
        Ok(path)
    }
    pub(super) fn read(&self, name: &str) -> Result<Option<Vec<u8>>> {
        let path = self.path(name)?;
        match std::fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(PmError::io(&path, e)),
            Ok(_) => fs::read(&path, MAX_JOURNAL_BYTES).map(|(bytes, _)| Some(bytes)),
        }
    }
    pub(super) fn publish(
        &self,
        name: &str,
        bytes: &[u8],
        expected: Option<&ContentHash>,
    ) -> Result<()> {
        if bytes.len() > MAX_JOURNAL_BYTES {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "local publication journal exceeds 64 MiB",
            ));
        }
        let path = self.path(name)?;
        let parent = path.parent().ok_or_else(|| fs::unsafe_path(&path))?;
        crate::execution::local::directory(&self.root, parent)?;
        let identity = fs::directory(parent)?;
        if self.read(name)?.as_deref().map(ContentHash::of).as_ref() != expected {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "source-local journal changed",
            ));
        }
        let temporary = parent.join(format!(".write-{}", crate::OperationId::new()));
        let result = (|| {
            let mut file = crate::execution::local::create_file(&self.root, &temporary)?;
            file.write_all(bytes)
                .and_then(|_| file.sync_all())
                .map_err(|e| PmError::io(&temporary, e))?;
            if fs::directory(parent)? != identity
                || self.read(name)?.as_deref().map(ContentHash::of).as_ref() != expected
            {
                return Err(PmError::new(
                    ErrorCode::StaleSource,
                    "source-local publication precondition changed",
                ));
            }
            if expected.is_none() {
                std::fs::hard_link(&temporary, &path).map_err(|e| PmError::io(&path, e))?;
            } else {
                std::fs::rename(&temporary, &path).map_err(|e| PmError::io(&path, e))?;
            }
            File::open(parent)
                .and_then(|f| f.sync_all())
                .map_err(|e| PmError::io(parent, e))?;
            Ok(())
        })();
        if crate::execution::local::safe(&self.root, &temporary).is_ok() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }
    pub(super) fn save<T: serde::Serialize>(&self, name: &str, value: &T) -> Result<()> {
        let previous = self.read(name)?.as_deref().map(ContentHash::of);
        let bytes = serde_json::to_vec(value)
            .map_err(|e| PmError::new(ErrorCode::InvalidInput, e.to_string()))?;
        self.publish(name, &bytes, previous.as_ref())
    }
}
pub(super) fn request_name(request: &crate::RequestId) -> String {
    format!(
        "requests/{}.json",
        ContentHash::of(request.as_str().as_bytes())
    )
}
pub(super) fn read_at(root: &Path, name: &str) -> Result<Option<Vec<u8>>> {
    let path = root.join(".local/sources").join(name);
    crate::execution::local::safe(root, &path)?;
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(PmError::io(&path, e)),
        Ok(_) => fs::read(&path, MAX_JOURNAL_BYTES).map(|(bytes, _)| Some(bytes)),
    }
}
