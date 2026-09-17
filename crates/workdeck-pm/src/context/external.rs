//! Inert, scoped source capture. Unix uses no-follow descriptor traversal and
//! nonblocking leaf opens. No external commands, ignore files or global config.
use super::*;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[cfg(unix)]
use std::{fs::File, io::Read, path::Component};

const MAX_FILE: usize = 256 * 1024;
const MAX_TOTAL: usize = 4 * 1024 * 1024;
const MAX_FILES: usize = 256;
#[derive(Debug, Clone, PartialEq, Eq)]
enum Observed {
    Text {
        text: String,
        hash: ContentHash,
        identity: (u64, u64),
    },
    Absent,
    Unavailable {
        reason: String,
        identity: Option<(u64, u64)>,
    },
}
pub(super) struct External {
    root: Option<PathBuf>,
    root_identity: Option<(u64, u64)>,
    observed: BTreeMap<PathBuf, (usize, Observed)>,
    total: usize,
}
impl External {
    pub fn new(root: &Path) -> Result<Self> {
        let parent = root.parent();
        let root = match parent {
            Some(parent)
                if root.file_name().is_some_and(|name| name == ".workdeck")
                    && crate::repository::project_root(parent)? == parent =>
            {
                Some(parent.to_owned())
            }
            _ => None,
        };
        let root_identity = root
            .as_ref()
            .map(|path| {
                std::fs::symlink_metadata(path)
                    .map(|m| identity(&m))
                    .map_err(|e| PmError::io(path, e))
            })
            .transpose()?;
        Ok(Self {
            root,
            root_identity,
            observed: BTreeMap::new(),
            total: 0,
        })
    }
    fn observe(&mut self, path: &Path) -> Result<Observed> {
        if let Some((_, value)) = self.observed.get(path) {
            return Ok(value.clone());
        }
        let Some(root) = &self.root else {
            return Ok(unavailable("worktree_unbound"));
        };
        if self.observed.len() >= MAX_FILES || self.total >= MAX_TOTAL {
            return Ok(unavailable("capture_limit"));
        }
        let limit = MAX_FILE.min(MAX_TOTAL - self.total);
        let value = observe(root, path, limit)?;
        if let Observed::Text { text, .. } = &value {
            self.total += text.len();
        }
        self.observed
            .insert(path.to_owned(), (limit, value.clone()));
        Ok(value)
    }
    pub fn sources(&mut self, links: &[SourceLink]) -> Result<Vec<ContextEntry>> {
        let mut entries = Vec::new();
        for link in links {
            link.validate()?;
            let path = Path::new(&link.path);
            let value = if forbidden(path) {
                unavailable("private_or_configuration_source")
            } else {
                self.observe(path)?
            };
            let (excerpt, source, reason_code) = match value {
                Observed::Text { text, hash, .. } => {
                    let (text, reason) = excerpt(&text, link);
                    (Some(text), Some(hash), reason)
                }
                Observed::Absent => (None, None, Some("source_missing".into())),
                Observed::Unavailable { reason, .. } => (None, None, Some(reason)),
            };
            entries.push(ContextEntry {
                content: ContextContent::Source {
                    link: link.clone(),
                    excerpt,
                    reason_code,
                },
                citations: vec![ContextCitation {
                    target: ContextTarget::WorktreeSource { link: link.clone() },
                    source,
                }],
            });
        }
        Ok(entries)
    }
    pub fn instructions(&mut self, links: &[SourceLink]) -> Result<Vec<ContextEntry>> {
        if self.root.is_none() {
            return Ok(vec![notice(
                "worktree_unbound",
                "Scoped instructions require a verified canonical repository worktree.",
            )]);
        }
        let mut paths = std::collections::BTreeSet::from([PathBuf::from("AGENTS.md")]);
        for link in links {
            if forbidden(Path::new(&link.path)) {
                continue;
            }
            if let Some(parent) = Path::new(&link.path).parent() {
                for ancestor in parent.ancestors() {
                    paths.insert(ancestor.join("AGENTS.md"));
                }
            }
        }
        let mut entries = Vec::new();
        for path in paths {
            match self.observe(&path)? {
                Observed::Text { text, hash, .. } => entries.push(ContextEntry {
                    content: ContextContent::Instruction {
                        path: path.clone(),
                        text,
                    },
                    citations: vec![ContextCitation {
                        target: ContextTarget::WorktreeSource {
                            link: SourceLink {
                                path: path.to_string_lossy().into_owned(),
                                line: None,
                                end_line: None,
                            },
                        },
                        source: Some(hash),
                    }],
                }),
                Observed::Absent => (),
                Observed::Unavailable { reason, .. } => entries.push(ContextEntry {
                    content: ContextContent::Notice {
                        reason_code: reason,
                        message: format!(
                            "Scoped instruction {} could not be captured",
                            path.display()
                        ),
                    },
                    citations: vec![ContextCitation {
                        target: ContextTarget::WorktreeSource {
                            link: SourceLink {
                                path: path.to_string_lossy().into_owned(),
                                line: None,
                                end_line: None,
                            },
                        },
                        source: None,
                    }],
                }),
            }
        }
        Ok(entries)
    }
    pub fn verify(&self) -> Result<()> {
        let Some(root) = &self.root else {
            return Ok(());
        };
        let metadata = std::fs::symlink_metadata(root).map_err(|_| stale())?;
        if metadata.file_type().is_symlink() || Some(identity(&metadata)) != self.root_identity {
            return Err(stale());
        }
        for (path, (limit, previous)) in &self.observed {
            // A changed source is rejected even when the corresponding section did
            // not fit the output budget: its capture still influenced the packet.
            if observe(root, path, *limit).map_err(|_| stale())? != *previous {
                return Err(stale().at(root.join(path)));
            }
        }
        Ok(())
    }
    pub fn fingerprint(&self) -> Result<ContentHash> {
        // Directory/file identities are deliberately retained only in `observed`
        // for within-capture verification. Durable context follows the relative
        // source and its content/availability, so identical clones and atomic
        // saves remain equivalent. Older inode-bound anchors remain readable but
        // conservatively stale; no historical handoff is rewritten or upgraded.
        let sources = self
            .observed
            .iter()
            .map(|(path, (limit, observed))| {
                let observation = match observed {
                    Observed::Text { hash, .. } => {
                        serde_json::json!({"kind":"text","content":hash})
                    }
                    Observed::Absent => serde_json::json!({"kind":"absent"}),
                    Observed::Unavailable { reason, .. } => {
                        serde_json::json!({"kind":"unavailable","reason":reason})
                    }
                };
                (
                    path,
                    serde_json::json!({"limit_bytes":limit,"observation":observation}),
                )
            })
            .collect::<BTreeMap<_, _>>();
        crate::transactions::canonical_hash(
            &serde_json::json!({"bound":self.root.is_some(),"sources":sources}),
        )
    }
}
fn stale() -> PmError {
    PmError::new(
        ErrorCode::StaleSource,
        "repository instructions or linked sources changed during context capture",
    )
}
fn unavailable(reason: &str) -> Observed {
    Observed::Unavailable {
        reason: reason.into(),
        identity: None,
    }
}
fn forbidden(path: &Path) -> bool {
    path.components().any(|c| {
        matches!(
            c.as_os_str().to_str(),
            Some(".git" | ".agents" | ".workdeck" | ".ssh" | ".codex" | ".claude")
        )
    }) || path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with(".env") || n.ends_with(".pem") || n.ends_with(".key"))
}
fn excerpt(text: &str, link: &SourceLink) -> (String, Option<String>) {
    let start = link.line.unwrap_or(1) as usize;
    let lines = text.lines().collect::<Vec<_>>();
    if start > lines.len() && !text.is_empty() {
        return (String::new(), Some("range_outside_source".into()));
    }
    let requested_end = link
        .end_line
        .map(|n| n as usize)
        .unwrap_or(lines.len())
        .min(lines.len());
    let end = requested_end.min(start.saturating_add(39));
    let mut truncated = end < requested_end;
    let mut output = String::new();
    for line in lines
        .iter()
        .skip(start - 1)
        .take(end.saturating_add(1).saturating_sub(start))
    {
        if output.len() + line.len() + 1 > 16 * 1024 {
            truncated = true;
            break;
        }
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(line);
    }
    (output, truncated.then(|| "excerpt_truncated".into()))
}
#[cfg(unix)]
fn identity(metadata: &std::fs::Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (metadata.dev(), metadata.ino())
}
#[cfg(not(unix))]
fn identity(_metadata: &std::fs::Metadata) -> (u64, u64) {
    (0, 0)
}
#[cfg(unix)]
fn observe(root: &Path, path: &Path, limit: usize) -> Result<Observed> {
    SourceLink {
        path: path
            .to_str()
            .ok_or_else(|| PmError::new(ErrorCode::UnsafePath, "source paths require UTF-8"))?
            .into(),
        line: None,
        end_line: None,
    }
    .validate()?;
    let file = match open(root, path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Observed::Absent),
        Err(_) => return Ok(unavailable("unsafe_or_unreadable_source")),
    };
    let metadata = file.metadata().map_err(|e| PmError::io(path, e))?;
    let id = identity(&metadata);
    if !metadata.is_file() {
        return Ok(Observed::Unavailable {
            reason: "non_regular_source".into(),
            identity: Some(id),
        });
    }
    if metadata.len() > limit as u64 {
        return Ok(Observed::Unavailable {
            reason: "source_size_limit".into(),
            identity: Some(id),
        });
    }
    let mut bytes = Vec::new();
    file.take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| PmError::io(path, e))?;
    if bytes.len() > limit {
        return Ok(Observed::Unavailable {
            reason: "source_size_limit".into(),
            identity: Some(id),
        });
    }
    let hash = ContentHash::of(&bytes);
    if bytes.contains(&0) {
        return Ok(Observed::Unavailable {
            reason: "binary_source".into(),
            identity: Some(id),
        });
    }
    let Ok(text) = String::from_utf8(bytes) else {
        return Ok(Observed::Unavailable {
            reason: "binary_source".into(),
            identity: Some(id),
        });
    };
    Ok(Observed::Text {
        text,
        hash,
        identity: id,
    })
}
#[cfg(not(unix))]
fn observe(_root: &Path, _path: &Path, _limit: usize) -> Result<Observed> {
    Ok(unavailable("descriptor_reads_unsupported"))
}
#[cfg(unix)]
fn open(root: &Path, relative: &Path) -> std::io::Result<File> {
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
        .open(root)?;
    let parts = relative.components().collect::<Vec<_>>();
    for (index, part) in parts.iter().enumerate() {
        if !matches!(part, Component::Normal(_)) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "noncanonical source path",
            ));
        }
        let name = CString::new(part.as_os_str().as_bytes())?;
        let flags = libc::O_RDONLY
            | libc::O_NOFOLLOW
            | libc::O_CLOEXEC
            | if index + 1 < parts.len() {
                libc::O_DIRECTORY
            } else {
                libc::O_NONBLOCK
            };
        // SAFETY: owned live parent descriptor and NUL-terminated leaf name; a
        // successful descriptor is transferred exactly once to File ownership.
        let descriptor = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
        if descriptor < 0 {
            return Err(std::io::Error::last_os_error());
        }
        directory = unsafe { File::from_raw_fd(descriptor) };
        if index + 1 < parts.len() {
            // Inspect the nested boundary itself without opening it (a .git
            // marker may be a directory, file, symlink or special file).
            let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
            let found = unsafe {
                libc::fstatat(
                    directory.as_raw_fd(),
                    c".git".as_ptr(),
                    metadata.as_mut_ptr(),
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            };
            if found == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "nested repository boundary",
                ));
            }
            if std::io::Error::last_os_error().kind() != std::io::ErrorKind::NotFound {
                return Err(std::io::Error::last_os_error());
            }
        }
    }
    Ok(directory)
}
