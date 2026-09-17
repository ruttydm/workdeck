use super::*;
use crate::{ContentHash, ErrorCode, PmError, Result};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

pub(crate) fn safe(root: &Path, path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(root).map_err(|e| PmError::io(root, e))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(unsafe_path(root));
    }
    let relative = path.strip_prefix(root).map_err(|_| unsafe_path(path))?;
    let mut current = root.to_owned();
    for component in relative.components() {
        let Component::Normal(value) = component else {
            return Err(unsafe_path(path));
        };
        current.push(value);
        match fs::symlink_metadata(&current) {
            Ok(metadata)
                if metadata.file_type().is_symlink() || (current != path && !metadata.is_dir()) =>
            {
                return Err(unsafe_path(&current));
            }
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(PmError::io(&current, e)),
        }
    }
    Ok(())
}
fn unsafe_path(path: &Path) -> PmError {
    PmError::new(
        ErrorCode::UnsafePath,
        "execution paths must remain within real directories and cannot follow symlinks",
    )
    .at(path)
}
fn options(options: &mut OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000);
    }
}
pub(crate) fn read(root: &Path, path: &Path, max: usize) -> Result<Option<Vec<u8>>> {
    safe(root, path)?;
    match fs::symlink_metadata(path) {
        Ok(m) if !m.is_file() => return Err(unsafe_path(path)),
        Ok(m) if m.len() > max as u64 => {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "local execution artifact exceeds size bound",
            )
            .at(path));
        }
        Ok(_) => (),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(PmError::io(path, e)),
    }
    let mut opts = OpenOptions::new();
    opts.read(true);
    options(&mut opts);
    let file = opts.open(path).map_err(|e| PmError::io(path, e))?;
    if !file.metadata().map_err(|e| PmError::io(path, e))?.is_file() {
        return Err(unsafe_path(path));
    }
    let mut bytes = Vec::new();
    file.take((max + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| PmError::io(path, e))?;
    if bytes.len() > max {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "local execution artifact exceeds size bound",
        )
        .at(path));
    }
    Ok(Some(bytes))
}
pub(crate) fn directory(root: &Path, path: &Path) -> Result<()> {
    safe(root, path)?;
    fs::create_dir_all(path).map_err(|e| PmError::io(path, e))?;
    safe(root, path)?;
    if !fs::symlink_metadata(path)
        .map_err(|e| PmError::io(path, e))?
        .is_dir()
    {
        return Err(unsafe_path(path));
    }
    Ok(())
}
fn sync(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)
        .and_then(|f| f.sync_all())
        .map_err(|e| PmError::io(path, e))?;
    let _ = path;
    Ok(())
}
pub(crate) fn create_file(root: &Path, path: &Path) -> Result<File> {
    safe(root, path)?;
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    options(&mut opts);
    opts.open(path).map_err(|e| PmError::io(path, e))
}
pub(crate) fn publish(
    root: &Path,
    path: &Path,
    bytes: &[u8],
    expected: Option<&ContentHash>,
) -> Result<()> {
    let before = read(root, path, MAX_RUN_RECORD_BYTES)?;
    if before.as_deref().map(ContentHash::of).as_ref() != expected {
        return Err(
            PmError::new(ErrorCode::StaleSource, "local execution journal changed").at(path),
        );
    }
    let parent = path.parent().ok_or_else(|| unsafe_path(path))?;
    let temporary = parent.join(format!(".write-{}", ulid::Ulid::new()));
    let result = (|| {
        let mut file = create_file(root, &temporary)?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| PmError::io(&temporary, e))?;
        if read(root, path, MAX_RUN_RECORD_BYTES)?
            .as_deref()
            .map(ContentHash::of)
            .as_ref()
            != expected
        {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "local execution journal changed before publication",
            )
            .at(path));
        }
        safe(root, path)?;
        if expected.is_none() {
            fs::hard_link(&temporary, path).map_err(|e| PmError::io(path, e))?;
        } else {
            fs::rename(&temporary, path).map_err(|e| PmError::io(path, e))?;
        }
        sync(parent)
    })();
    if safe(root, &temporary).is_ok() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
pub(crate) fn root(planning: &Path, id: &LocalRunId) -> PathBuf {
    planning.join(".local/runs").join(id.as_str())
}
pub(crate) fn prepare(planning: &Path, id: &LocalRunId) -> Result<PathBuf> {
    let base = planning.join(".local/runs");
    directory(planning, &base)?;
    let ignore = base.join(".gitignore");
    match read(planning, &ignore, 4096)? {
        Some(bytes) if bytes == b"*\n" => (),
        Some(_) => return Err(PmError::new(ErrorCode::Conflict,"preserve existing execution ignore policy; it must contain '*' before retaining local records").at(ignore)),
        None => {
            if let Err(error) = publish(planning, &ignore, b"*\n", None)
                && read(planning, &ignore, 4096)?.as_deref() != Some(b"*\n") {
                return Err(error);
            }
        }
    }
    let root = root(planning, id);
    directory(planning, &root)?;
    sync(&base)?;
    Ok(root)
}
pub(super) struct RunLock {
    file: File,
}
pub(super) fn locked(planning: &Path, id: &LocalRunId) -> Result<bool> {
    let path = root(planning, id).join("run.lock");
    safe(planning, &path)?;
    match fs::symlink_metadata(&path) {
        Ok(m) if !m.is_file() => return Err(unsafe_path(&path)),
        Ok(_) => (),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(PmError::io(&path, e)),
    }
    let mut opts = OpenOptions::new();
    opts.read(true);
    options(&mut opts);
    let file = opts.open(&path).map_err(|e| PmError::io(&path, e))?;
    if !file
        .metadata()
        .map_err(|e| PmError::io(&path, e))?
        .is_file()
    {
        return Err(unsafe_path(&path));
    }
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Ok(true);
        }
        let _ = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
        Ok(false)
    }
    #[cfg(not(unix))]
    {
        let _ = file;
        Ok(false)
    }
}
impl RunLock {
    pub(crate) fn acquire(planning: &Path, id: &LocalRunId) -> Result<Self> {
        let path = root(planning, id).join("run.lock");
        safe(planning, &path)?;
        if fs::symlink_metadata(&path).is_ok_and(|m| !m.is_file()) {
            return Err(unsafe_path(&path));
        }
        let mut opts = OpenOptions::new();
        opts.read(true).write(true).create(true).truncate(false);
        options(&mut opts);
        let file = opts.open(&path).map_err(|e| PmError::io(&path, e))?;
        if !file
            .metadata()
            .map_err(|e| PmError::io(&path, e))?
            .is_file()
        {
            return Err(unsafe_path(&path));
        }
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return Err(PmError::new(
                    ErrorCode::Locked,
                    "execution is owned by another foreground caller",
                )
                .at(path));
            }
        }
        if !cfg!(unix) {
            return Err(PmError::new(
                ErrorCode::Unsupported,
                "foreground execution requires supported process-group and local-lock semantics",
            ));
        }
        Ok(Self { file })
    }
}
impl Drop for RunLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let _ = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
        }
    }
}
