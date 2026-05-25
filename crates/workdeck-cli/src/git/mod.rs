use anyhow::{Context, Result, bail};
use git2::{Repository, Status, StatusOptions};
use ignore::WalkBuilder;
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const DIFF_SECTION_MAX_BYTES: usize = 70_000;
const DIFF_TOTAL_MAX_BYTES: usize = 160_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Typechange,
    Untracked,
    Conflicted,
}

impl ChangeKind {
    pub fn marker(self) -> &'static str {
        match self {
            ChangeKind::Added => "A",
            ChangeKind::Modified => "M",
            ChangeKind::Deleted => "D",
            ChangeKind::Renamed => "R",
            ChangeKind::Typechange => "T",
            ChangeKind::Untracked => "?",
            ChangeKind::Conflicted => "!",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ChangeKind::Added => "added",
            ChangeKind::Modified => "modified",
            ChangeKind::Deleted => "deleted",
            ChangeKind::Renamed => "renamed",
            ChangeKind::Typechange => "typechange",
            ChangeKind::Untracked => "untracked",
            ChangeKind::Conflicted => "conflicted",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeEntry {
    pub path: PathBuf,
    pub kind: ChangeKind,
    pub staged: bool,
    pub unstaged: bool,
    pub additions: usize,
    pub deletions: usize,
}

impl ChangeEntry {
    pub fn path_display(&self) -> String {
        self.path.to_string_lossy().to_string()
    }

    pub fn stage_label(&self) -> &'static str {
        match (self.staged, self.unstaged) {
            (true, true) => "staged+unstaged",
            (true, false) => "staged",
            (false, true) => "unstaged",
            (false, false) => "none",
        }
    }

    pub fn stage_marker(&self) -> &'static str {
        match (self.staged, self.unstaged) {
            (true, true) => "[SU]",
            (true, false) => "[S ]",
            (false, true) => "[ U]",
            (false, false) => "[  ]",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryGroup {
    pub path: PathBuf,
    pub files: Vec<ChangeEntry>,
    pub total_additions: usize,
    pub total_deletions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoSnapshot {
    pub root: PathBuf,
    pub changes: Vec<ChangeEntry>,
    pub groups: Vec<DirectoryGroup>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePreview {
    pub title: String,
    pub content: String,
    pub truncated: bool,
    pub binary: bool,
}

pub fn discover_repo_root(cwd: &Path) -> Result<PathBuf> {
    let repo = Repository::discover(cwd)
        .with_context(|| format!("failed to discover git repo from {}", cwd.display()))?;
    Ok(repo
        .workdir()
        .context("bare repositories are not supported")?
        .to_path_buf())
}

pub fn scan_repo(cwd: &Path) -> Result<RepoSnapshot> {
    let root = discover_repo_root(cwd)?;
    let repo = Repository::open(&root)
        .with_context(|| format!("failed to open git repo {}", root.display()))?;
    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .renames_head_to_index(true)
        .renames_index_to_workdir(true);

    let statuses = repo.statuses(Some(&mut options))?;
    let churn = collect_churn(&root);
    let mut changes = Vec::new();
    for entry in statuses.iter() {
        let Some(path) = entry.path() else {
            continue;
        };
        let status = entry.status();
        let kind = change_kind(status);
        let (additions, deletions) = if kind == ChangeKind::Untracked {
            untracked_line_count(&root, Path::new(path))
        } else {
            churn.get(Path::new(path)).copied().unwrap_or((0, 0))
        };
        changes.push(ChangeEntry {
            path: PathBuf::from(path),
            kind,
            staged: is_staged(status),
            unstaged: is_unstaged(status),
            additions,
            deletions,
        });
    }

    changes.sort_by(|a, b| a.path.cmp(&b.path));
    let groups = group_by_directory(&changes);
    Ok(RepoSnapshot {
        root,
        changes,
        groups,
    })
}

pub fn group_by_directory(changes: &[ChangeEntry]) -> Vec<DirectoryGroup> {
    let mut map: BTreeMap<PathBuf, Vec<ChangeEntry>> = BTreeMap::new();
    for change in changes {
        let dir = change
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        map.entry(dir).or_default().push(change.clone());
    }

    map.into_iter()
        .map(|(path, files)| DirectoryGroup {
            total_additions: files.iter().map(|file| file.additions).sum(),
            total_deletions: files.iter().map(|file| file.deletions).sum(),
            path,
            files,
        })
        .collect()
}

pub fn diff_for_path(repo_root: &Path, path: &Path) -> Result<FilePreview> {
    let (unstaged, unstaged_truncated) =
        git_diff_preview(repo_root, path, false, DIFF_SECTION_MAX_BYTES)?;
    let (staged, staged_truncated) =
        git_diff_preview(repo_root, path, true, DIFF_SECTION_MAX_BYTES)?;

    let mut content = String::new();
    if !staged.is_empty() {
        content.push_str("# staged\n");
        content.push_str(&staged);
        if staged_truncated {
            content.push_str("\n\n... staged diff truncated ...");
        }
    }
    if !unstaged.is_empty() {
        if !content.is_empty() {
            content.push('\n');
        }
        content.push_str("# unstaged\n");
        content.push_str(&unstaged);
        if unstaged_truncated {
            content.push_str("\n\n... unstaged diff truncated ...");
        }
    }

    if content.trim().is_empty() {
        read_file_preview(repo_root, path, 80_000)
    } else {
        let (content, final_truncated) = truncate_string(content, DIFF_TOTAL_MAX_BYTES);
        Ok(FilePreview {
            title: format!("diff {}", path.display()),
            content,
            truncated: final_truncated || staged_truncated || unstaged_truncated,
            binary: false,
        })
    }
}

fn git_diff_preview(
    repo_root: &Path,
    path: &Path,
    cached: bool,
    max_bytes: usize,
) -> Result<(String, bool)> {
    let rel = path.to_string_lossy().to_string();
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(repo_root)
        .arg("diff")
        .arg("--no-ext-diff")
        .arg("--color=never");
    if cached {
        command.arg("--cached");
    }
    command
        .arg("--")
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mode = if cached {
        "git diff --cached"
    } else {
        "git diff"
    };
    let mut child = command
        .spawn()
        .with_context(|| format!("failed to run {mode} for {rel}"))?;
    let stdout = child
        .stdout
        .take()
        .with_context(|| format!("failed to capture {mode} stdout for {rel}"))?;
    let mut bytes = Vec::with_capacity(max_bytes.min(64 * 1024));
    stdout
        .take(max_bytes.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read {mode} output for {rel}"))?;

    let truncated = bytes.len() > max_bytes;
    if truncated {
        bytes.truncate(max_bytes);
        let _ = child.kill();
        let _ = child.wait();
        return Ok((String::from_utf8_lossy(&bytes).to_string(), true));
    }

    let output = child
        .wait_with_output()
        .with_context(|| format!("failed to wait for {mode} for {rel}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("{mode} failed for {rel}: {stderr}");
    }

    Ok((String::from_utf8_lossy(&bytes).to_string(), false))
}

fn truncate_string(mut content: String, max_bytes: usize) -> (String, bool) {
    if content.len() <= max_bytes {
        return (content, false);
    }

    let mut boundary = max_bytes;
    while !content.is_char_boundary(boundary) {
        boundary -= 1;
    }
    content.truncate(boundary);
    content.push_str("\n\n... truncated ...");
    (content, true)
}

pub fn read_file_preview(repo_root: &Path, path: &Path, max_bytes: usize) -> Result<FilePreview> {
    let full = repo_root.join(path);
    let meta = fs::metadata(&full)
        .with_context(|| format!("failed to read metadata for {}", full.display()))?;
    if meta.is_dir() {
        return Ok(FilePreview {
            title: path.display().to_string(),
            content: "directory".to_string(),
            truncated: false,
            binary: false,
        });
    }

    if let Some(kind) = metadata_preview_kind(path) {
        return Ok(metadata_preview(path, meta.len(), kind));
    }

    let bytes = read_preview_bytes(&full, max_bytes)
        .with_context(|| format!("failed to read {}", full.display()))?;
    if looks_binary(&bytes) {
        return Ok(metadata_preview(path, meta.len(), "binary file"));
    }

    let truncated = meta.len() > max_bytes as u64 || bytes.len() > max_bytes;
    let slice = if truncated {
        &bytes[..max_bytes.min(bytes.len())]
    } else {
        &bytes
    };
    let mut content = String::from_utf8_lossy(slice).to_string();
    if truncated {
        content.push_str("\n\n... truncated ...");
    }

    Ok(FilePreview {
        title: path.display().to_string(),
        content,
        truncated,
        binary: false,
    })
}

fn read_preview_bytes(path: &Path, max_bytes: usize) -> Result<Vec<u8>> {
    let read_limit = max_bytes.saturating_add(1).max(4096);
    let mut bytes = Vec::with_capacity(read_limit.min(64 * 1024));
    let file = fs::File::open(path)?;
    file.take(read_limit as u64).read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn metadata_preview(path: &Path, size: u64, kind: &'static str) -> FilePreview {
    FilePreview {
        title: path.display().to_string(),
        content: format!("{kind}\nsize: {size} bytes"),
        truncated: false,
        binary: true,
    }
}

fn metadata_preview_kind(path: &Path) -> Option<&'static str> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())?
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "bmp" | "tiff" | "ico" => {
            Some("image file")
        }
        "zip" | "gz" | "tgz" | "bz2" | "xz" | "7z" | "rar" | "tar" => Some("archive file"),
        "pdf" => Some("pdf file"),
        "woff" | "woff2" | "ttf" | "otf" => Some("font file"),
        "mp3" | "wav" | "flac" | "m4a" | "ogg" => Some("audio file"),
        "mp4" | "mov" | "webm" | "mkv" | "avi" => Some("video file"),
        "wasm" | "class" | "o" | "a" | "so" | "dylib" | "dll" | "exe" => Some("binary file"),
        _ => None,
    }
}

fn looks_binary(bytes: &[u8]) -> bool {
    let sample = bytes.iter().take(4096).copied().collect::<Vec<_>>();
    if sample.is_empty() {
        return false;
    }
    if sample.contains(&0) {
        return true;
    }

    let control_count = sample
        .iter()
        .filter(|byte| byte.is_ascii_control() && !matches!(byte, b'\n' | b'\r' | b'\t'))
        .count();
    control_count * 100 / sample.len() > 5
}

pub fn list_repo_files(repo_root: &Path, limit: usize) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in WalkBuilder::new(repo_root)
        .standard_filters(true)
        .hidden(false)
        .build()
    {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path
            .components()
            .any(|component| component.as_os_str() == ".git")
        {
            continue;
        }
        let rel = path.strip_prefix(repo_root).unwrap_or(path).to_path_buf();
        files.push(rel);
        if files.len() >= limit {
            break;
        }
    }
    files.sort();
    Ok(files)
}

fn change_kind(status: Status) -> ChangeKind {
    if status.is_conflicted() {
        ChangeKind::Conflicted
    } else if status.contains(Status::WT_NEW) {
        ChangeKind::Untracked
    } else if status.contains(Status::INDEX_RENAMED) || status.contains(Status::WT_RENAMED) {
        ChangeKind::Renamed
    } else if status.contains(Status::INDEX_DELETED) || status.contains(Status::WT_DELETED) {
        ChangeKind::Deleted
    } else if status.contains(Status::INDEX_NEW) {
        ChangeKind::Added
    } else if status.contains(Status::INDEX_TYPECHANGE) || status.contains(Status::WT_TYPECHANGE) {
        ChangeKind::Typechange
    } else {
        ChangeKind::Modified
    }
}

fn is_staged(status: Status) -> bool {
    status.intersects(
        Status::INDEX_NEW
            | Status::INDEX_MODIFIED
            | Status::INDEX_DELETED
            | Status::INDEX_RENAMED
            | Status::INDEX_TYPECHANGE,
    )
}

fn is_unstaged(status: Status) -> bool {
    status.intersects(
        Status::WT_NEW
            | Status::WT_MODIFIED
            | Status::WT_DELETED
            | Status::WT_RENAMED
            | Status::WT_TYPECHANGE,
    )
}

fn collect_churn(repo_root: &Path) -> BTreeMap<PathBuf, (usize, usize)> {
    let mut churn = BTreeMap::new();
    for args in [
        &["diff", "--numstat"][..],
        &["diff", "--cached", "--numstat"][..],
    ] {
        let Ok(output) = Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .args(args)
            .output()
        else {
            continue;
        };
        let raw = String::from_utf8_lossy(&output.stdout);
        for line in raw.lines() {
            let mut parts = line.split('\t');
            let additions = parts
                .next()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0);
            let deletions = parts
                .next()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0);
            let Some(path) = parts.next() else {
                continue;
            };
            let path = PathBuf::from(path);
            let entry = churn.entry(path).or_insert((0, 0));
            entry.0 += additions;
            entry.1 += deletions;
        }
    }
    churn
}

fn untracked_line_count(repo_root: &Path, path: &Path) -> (usize, usize) {
    let full = repo_root.join(path);
    let Ok(content) = fs::read_to_string(full) else {
        return (0, 0);
    };
    (content.lines().count(), 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(path: &str, kind: ChangeKind) -> ChangeEntry {
        ChangeEntry {
            path: PathBuf::from(path),
            kind,
            staged: false,
            unstaged: true,
            additions: 1,
            deletions: 2,
        }
    }

    #[test]
    fn groups_by_parent_directory() {
        let groups = group_by_directory(&[
            change("src/main.rs", ChangeKind::Modified),
            change("src/lib.rs", ChangeKind::Added),
            change("README.md", ChangeKind::Modified),
        ]);

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].path, PathBuf::from("."));
        assert_eq!(groups[1].path, PathBuf::from("src"));
        assert_eq!(groups[1].total_additions, 2);
        assert_eq!(groups[1].total_deletions, 4);
    }

    #[test]
    fn truncates_without_breaking_utf8() {
        let (content, truncated) = truncate_string("aébc".to_string(), 2);

        assert!(truncated);
        assert!(content.starts_with('a'));
        assert!(content.contains("truncated"));
    }

    #[test]
    fn detects_binary_preview_extensions() {
        assert_eq!(
            metadata_preview_kind(Path::new("image.png")),
            Some("image file")
        );
        assert_eq!(
            metadata_preview_kind(Path::new("assets.zip")),
            Some("archive file")
        );
        assert_eq!(
            metadata_preview_kind(Path::new("font.woff2")),
            Some("font file")
        );
        assert_eq!(metadata_preview_kind(Path::new("main.rs")), None);
    }

    #[test]
    fn detects_binary_bytes_without_nul() {
        let bytes = [1, 2, 3, 4, 5, 6, b'a', b'b'];
        assert!(looks_binary(&bytes));
        assert!(!looks_binary(b"hello\nworld\n"));
    }
}
