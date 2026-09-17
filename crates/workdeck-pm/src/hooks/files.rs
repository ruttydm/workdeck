use crate::{ContentHash, ErrorCode, PmError, Result, sources::fs as source_fs};
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

pub(super) const MAX_HOOK: usize = 64 * 1024;
pub(super) const MAX_PROOF: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DirectoryPin {
    path: PathBuf,
    device: u64,
    inode: u64,
}
pub(super) fn unsafe_path(path: &Path) -> PmError {
    PmError::new(
        ErrorCode::UnsafePath,
        "hook paths require ordinary directories and regular files",
    )
    .at(path)
}
pub(super) fn parents(path: &Path) -> Result<Vec<DirectoryPin>> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::RootDir | Component::Normal(_)))
    {
        return Err(unsafe_path(path));
    }
    let mut current = PathBuf::from("/");
    let mut pins = Vec::new();
    for part in path
        .parent()
        .ok_or_else(|| unsafe_path(path))?
        .components()
        .filter(|p| !matches!(p, Component::RootDir))
    {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                let source_fs::Identity(device, inode) = source_fs::directory(&current)?;
                pins.push(DirectoryPin {
                    path: current.clone(),
                    device,
                    inode,
                });
            }
            Ok(_) => return Err(unsafe_path(&current)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(PmError::io(&current, error)),
        }
    }
    Ok(pins)
}
pub(super) fn verify_parents(pins: &[DirectoryPin]) -> Result<()> {
    for pin in pins {
        if source_fs::directory(&pin.path)? != source_fs::Identity(pin.device, pin.inode) {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "hook parent directory was replaced after inspection",
            )
            .at(&pin.path));
        }
    }
    Ok(())
}
pub(super) fn parent_hash(pins: &[DirectoryPin]) -> Result<ContentHash> {
    serde_json::to_vec(pins)
        .map(|bytes| ContentHash::of(&bytes))
        .map_err(|error| PmError::new(ErrorCode::InvalidInput, error.to_string()))
}
pub(super) fn read(path: &Path, limit: usize) -> Result<Option<Vec<u8>>> {
    parents(path)?;
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(PmError::io(path, error)),
        Ok(_) => source_fs::read(path, limit).map(|(bytes, _)| Some(bytes)),
    }
}
pub(super) fn mode(path: &Path) -> Result<Option<u32>> {
    parents(path)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            #[cfg(unix)]
            {
                Ok(Some(metadata.permissions().mode() & 0o777))
            }
            #[cfg(not(unix))]
            {
                Err(PmError::new(
                    ErrorCode::Unsupported,
                    "Git hook publication requires qualified Unix file modes",
                ))
            }
        }
        Ok(_) => Err(unsafe_path(path)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(PmError::io(path, error)),
    }
}
pub(super) fn ensure_directory(path: &Path) -> Result<()> {
    parents(&path.join("placeholder"))?;
    fs::create_dir_all(path).map_err(|error| PmError::io(path, error))?;
    source_fs::directory(path)?;
    Ok(())
}
pub(super) fn sync(path: &Path) -> Result<()> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|error| PmError::io(path, error))
}
pub(super) fn write(path: &Path, bytes: &[u8], mode: u32, replace: bool) -> Result<()> {
    let pins = parents(path)?;
    let parent = path.parent().ok_or_else(|| unsafe_path(path))?;
    source_fs::directory(parent)?;
    let temporary = parent.join(format!(".workdeck-hook-{}.tmp", ulid::Ulid::new()));
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut file = options
        .open(&temporary)
        .map_err(|error| PmError::io(&temporary, error))?;
    let _cleanup = Cleanup(temporary.clone());
    file.write_all(bytes)
        .map_err(|error| PmError::io(&temporary, error))?;
    #[cfg(unix)]
    file.set_permissions(fs::Permissions::from_mode(mode))
        .map_err(|error| PmError::io(&temporary, error))?;
    #[cfg(not(unix))]
    let _ = mode;
    file.sync_all()
        .map_err(|error| PmError::io(&temporary, error))?;
    verify_parents(&pins)?;
    if replace {
        fs::rename(&temporary, path).map_err(|error| PmError::io(path, error))?;
    } else {
        fs::hard_link(&temporary, path).map_err(|error| PmError::io(path, error))?;
    }
    sync(parent)
}
pub(super) fn ensure_local(path: &Path) -> Result<()> {
    ensure_directory(path)?;
    let ignore = path.join(".gitignore");
    if let Some(bytes) = read(&ignore, 4096)? {
        let text = std::str::from_utf8(&bytes).map_err(|_| {
            PmError::new(ErrorCode::Conflict, "local hook ignore file must be UTF-8").at(&ignore)
        })?;
        if text
            .lines()
            .rfind(|line| !line.trim().is_empty() && !line.trim().starts_with('#'))
            .map(str::trim)
            != Some("*")
        {
            return Err(PmError::new(ErrorCode::Conflict, "preserved local hook ignore rules must end with '*' before writing private recovery state").at(&ignore));
        }
    } else {
        write(
            &ignore,
            b"# Machine-local hook recovery and retry records.\n*\n",
            0o600,
            false,
        )?;
    }
    ensure_directory(&path.join("requests"))?;
    sync(path)
}
pub(super) struct Lock {
    file: File,
    path: PathBuf,
    parents: Vec<DirectoryPin>,
}
impl Lock {
    pub(super) fn verify(&self) -> Result<()> {
        verify_parents(&self.parents)?;
        let (bytes, current) = source_fs::read(&self.path, 4096)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = self
                .file
                .metadata()
                .map_err(|error| PmError::io(&self.path, error))?;
            if current != source_fs::Identity(metadata.dev(), metadata.ino()) || !bytes.is_empty() {
                return Err(PmError::new(
                    ErrorCode::StaleSource,
                    "local hook writer lock was replaced after acquisition",
                )
                .at(&self.path));
            }
        }
        #[cfg(not(unix))]
        let _ = (bytes, current);
        Ok(())
    }
}
pub(super) fn lock(local: &Path) -> Result<Lock> {
    let path = local.join("writer.lock");
    if read(&path, 4096)?.is_none() {
        match write(&path, b"", 0o600, false) {
            Ok(()) => {}
            Err(error) if fs::symlink_metadata(&path).is_ok() => {
                read(&path, 4096)?;
                let _ = error;
            }
            Err(error) => return Err(error),
        }
    }
    let pins = parents(&path)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    let file = options
        .open(&path)
        .map_err(|error| PmError::io(&path, error))?;
    if !file
        .metadata()
        .map_err(|error| PmError::io(&path, error))?
        .is_file()
    {
        return Err(unsafe_path(&path));
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match file.try_lock() {
            Ok(()) => {
                let guard = Lock {
                    file,
                    path,
                    parents: pins,
                };
                guard.verify()?;
                return Ok(guard);
            }
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(PmError::new(
                    ErrorCode::Locked,
                    "another hook publication holds the local writer lock",
                ));
            }
            Err(std::fs::TryLockError::Error(error)) => return Err(PmError::io(&path, error)),
        }
    }
}
