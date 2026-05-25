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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitOverview {
    pub current_branch: String,
    pub upstream: Option<String>,
    pub ahead: usize,
    pub behind: usize,
    pub base_branch: Option<String>,
    pub remotes: Vec<GitRemote>,
    pub branches: Vec<GitBranch>,
    pub recent_commits: Vec<GitCommit>,
    pub stashes: Vec<GitStash>,
    pub tags: Vec<GitTag>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRemote {
    pub name: String,
    pub fetch_url: String,
    pub push_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitBranch {
    pub name: String,
    pub is_current: bool,
    pub is_remote: bool,
    pub upstream: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommit {
    pub sha: String,
    pub short_sha: String,
    pub summary: String,
    pub author: String,
    pub date: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitStash {
    pub name: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitTag {
    pub name: String,
}

pub fn discover_repo_root(cwd: &Path) -> Result<PathBuf> {
    let repo = Repository::discover(cwd).with_context(|| {
        format!(
            "Workdeck must be run inside a Git repository; {} is not in one. Run `workdeck --cwd <repo-path>` or initialize this directory with `git init`.",
            cwd.display()
        )
    })?;
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

pub fn scan_git_overview(
    repo_root: &Path,
    base_config: Option<&str>,
    recent_limit: usize,
) -> Result<GitOverview> {
    let current_branch = current_branch(repo_root)?;
    let upstream = optional_git_output(
        repo_root,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    )?
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty());
    let (ahead, behind) = if upstream.is_some() {
        parse_ahead_behind(&git_output(
            repo_root,
            &["rev-list", "--left-right", "--count", "HEAD...@{u}"],
        )?)
    } else {
        (0, 0)
    };
    let remotes = parse_remote_output(&git_output(repo_root, &["remote", "-v"])?);
    let branches = parse_branch_output(
        &git_output(
            repo_root,
            &[
                "branch",
                "--all",
                "--format=%(refname:short)%09%(HEAD)%09%(upstream:short)",
            ],
        )?,
        &remotes,
    );
    let recent_commits = parse_commit_output(
        &optional_git_output(
            repo_root,
            &[
                "log",
                "--date=short",
                "--pretty=format:%H%x09%h%x09%ad%x09%an%x09%s",
                "-n",
                &recent_limit.max(1).to_string(),
            ],
        )?
        .unwrap_or_default(),
    );
    let stashes = parse_stash_output(&git_output(
        repo_root,
        &["stash", "list", "--format=%gd%x09%s"],
    )?);
    let tags = parse_tag_output(&git_output(
        repo_root,
        &["tag", "--sort=-creatordate", "--list"],
    )?);
    let base_branch = infer_base_branch(base_config, upstream.as_deref(), &branches);

    Ok(GitOverview {
        current_branch,
        upstream,
        ahead,
        behind,
        base_branch,
        remotes,
        branches,
        recent_commits,
        stashes,
        tags,
    })
}

pub fn git_commit_preview(repo_root: &Path, sha: &str) -> Result<FilePreview> {
    let content = git_output(
        repo_root,
        &["show", "--stat", "--patch", "--color=never", sha],
    )?;
    Ok(text_preview(format!("commit {sha}"), content))
}

pub fn git_stash_preview(repo_root: &Path, stash: &str) -> Result<FilePreview> {
    let content = git_output(
        repo_root,
        &["stash", "show", "--stat", "--patch", "--color=never", stash],
    )?;
    Ok(text_preview(format!("stash {stash}"), content))
}

pub fn git_branch_preview(repo_root: &Path, branch: &str, limit: usize) -> Result<FilePreview> {
    let content = git_output(
        repo_root,
        &[
            "log",
            "--date=short",
            "--pretty=format:%h %ad %an %s",
            "-n",
            &limit.max(1).to_string(),
            branch,
        ],
    )?;
    Ok(text_preview(format!("branch {branch}"), content))
}

pub fn git_summary_preview(repo_root: &Path, base: Option<&str>) -> Result<FilePreview> {
    let Some(base) = base.filter(|base| !base.trim().is_empty()) else {
        return Ok(text_preview(
            "git summary".to_string(),
            "No base branch configured or detected.\n\nSet [git].base_branch in Workdeck config to enable ahead summary previews.".to_string(),
        ));
    };

    let commits = git_output(repo_root, &["log", "--oneline", &format!("{base}..HEAD")])?;
    let stat = git_output(repo_root, &["diff", "--stat", &format!("{base}..HEAD")])?;
    let mut content = format!("# Commits since {base}\n");
    if commits.trim().is_empty() {
        content.push_str("No commits ahead of base.\n");
    } else {
        content.push_str(&commits);
        if !content.ends_with('\n') {
            content.push('\n');
        }
    }
    content.push_str("\n# Diff stat\n");
    if stat.trim().is_empty() {
        content.push_str("No diff against base.\n");
    } else {
        content.push_str(&stat);
    }
    Ok(text_preview(format!("summary {base}..HEAD"), content))
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

fn current_branch(repo_root: &Path) -> Result<String> {
    let branch = git_output(repo_root, &["branch", "--show-current"])?;
    let branch = branch.trim();
    if !branch.is_empty() {
        return Ok(branch.to_string());
    }

    let short_sha = optional_git_output(repo_root, &["rev-parse", "--short", "HEAD"])?
        .unwrap_or_default()
        .trim()
        .to_string();
    if short_sha.is_empty() {
        Ok("HEAD".to_string())
    } else {
        Ok(format!("HEAD {short_sha}"))
    }
}

fn git_output(repo_root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("-c")
        .arg("color.ui=never")
        .args(args)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git {} failed: {}", args.join(" "), stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn optional_git_output(repo_root: &Path, args: &[&str]) -> Result<Option<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("-c")
        .arg("color.ui=never")
        .args(args)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    if output.status.success() {
        Ok(Some(String::from_utf8_lossy(&output.stdout).to_string()))
    } else {
        Ok(None)
    }
}

fn parse_ahead_behind(output: &str) -> (usize, usize) {
    let mut parts = output.split_whitespace();
    let ahead = parts
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let behind = parts
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    (ahead, behind)
}

pub fn parse_branch_output(output: &str, remotes: &[GitRemote]) -> Vec<GitBranch> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let name = parts.next()?.trim();
            if name.is_empty() {
                return None;
            }
            let head = parts.next().unwrap_or_default().trim();
            let upstream = parts
                .next()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            let is_remote = remotes.iter().any(|remote| {
                name == format!("{}/HEAD", remote.name)
                    || name.starts_with(&format!("{}/", remote.name))
            });
            Some(GitBranch {
                name: name.to_string(),
                is_current: head == "*",
                is_remote,
                upstream,
            })
        })
        .collect()
}

pub fn parse_remote_output(output: &str) -> Vec<GitRemote> {
    let mut remotes = BTreeMap::<String, GitRemote>::new();
    for line in output.lines() {
        let Some((name, rest)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let rest = rest.trim();
        let Some((url, kind)) = rest.rsplit_once(' ') else {
            continue;
        };
        let remote = remotes
            .entry(name.to_string())
            .or_insert_with(|| GitRemote {
                name: name.to_string(),
                fetch_url: String::new(),
                push_url: String::new(),
            });
        match kind {
            "(fetch)" => remote.fetch_url = url.to_string(),
            "(push)" => remote.push_url = url.to_string(),
            _ => {}
        }
    }
    remotes.into_values().collect()
}

pub fn parse_commit_output(output: &str) -> Vec<GitCommit> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(5, '\t');
            Some(GitCommit {
                sha: parts.next()?.to_string(),
                short_sha: parts.next()?.to_string(),
                date: parts.next()?.to_string(),
                author: parts.next()?.to_string(),
                summary: parts.next().unwrap_or_default().to_string(),
            })
        })
        .collect()
}

pub fn parse_stash_output(output: &str) -> Vec<GitStash> {
    output
        .lines()
        .filter_map(|line| {
            let (name, summary) = line.split_once('\t')?;
            Some(GitStash {
                name: name.to_string(),
                summary: summary.to_string(),
            })
        })
        .collect()
}

pub fn parse_tag_output(output: &str) -> Vec<GitTag> {
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(20)
        .map(|line| GitTag {
            name: line.trim().to_string(),
        })
        .collect()
}

pub fn infer_base_branch(
    base_config: Option<&str>,
    upstream: Option<&str>,
    branches: &[GitBranch],
) -> Option<String> {
    base_config
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| upstream.map(str::to_string))
        .or_else(|| branch_exists(branches, "origin/main").then(|| "origin/main".to_string()))
        .or_else(|| branch_exists(branches, "origin/master").then(|| "origin/master".to_string()))
}

fn branch_exists(branches: &[GitBranch], name: &str) -> bool {
    branches.iter().any(|branch| branch.name == name)
}

fn text_preview(title: String, content: String) -> FilePreview {
    FilePreview {
        title,
        content,
        truncated: false,
        binary: false,
    }
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

    #[test]
    fn parses_git_branch_output() {
        let remotes = vec![GitRemote {
            name: "origin".to_string(),
            fetch_url: "https://example.test/repo.git".to_string(),
            push_url: "https://example.test/repo.git".to_string(),
        }];
        let branches = parse_branch_output(
            "main\t*\torigin/main\nfeature/foo\t\t\norigin/main\t\t\n",
            &remotes,
        );

        assert_eq!(branches.len(), 3);
        assert!(branches[0].is_current);
        assert_eq!(branches[0].upstream.as_deref(), Some("origin/main"));
        assert!(!branches[1].is_remote);
        assert!(branches[2].is_remote);
    }

    #[test]
    fn parses_git_remote_output() {
        let remotes = parse_remote_output(
            "origin\thttps://example.test/fetch.git (fetch)\norigin\tssh://example.test/push.git (push)\nupstream\thttps://example.test/up.git (fetch)\n",
        );

        assert_eq!(remotes.len(), 2);
        assert_eq!(remotes[0].name, "origin");
        assert_eq!(remotes[0].fetch_url, "https://example.test/fetch.git");
        assert_eq!(remotes[0].push_url, "ssh://example.test/push.git");
    }

    #[test]
    fn parses_git_stash_output() {
        let stashes = parse_stash_output("stash@{0}\tWIP on main: abc123 work\n");

        assert_eq!(stashes[0].name, "stash@{0}");
        assert_eq!(stashes[0].summary, "WIP on main: abc123 work");
    }

    #[test]
    fn parses_git_commit_output() {
        let commits = parse_commit_output("abc123456\tabc1234\t2026-05-25\tRutger\tAdd Git tab\n");

        assert_eq!(commits[0].sha, "abc123456");
        assert_eq!(commits[0].short_sha, "abc1234");
        assert_eq!(commits[0].date, "2026-05-25");
        assert_eq!(commits[0].author, "Rutger");
        assert_eq!(commits[0].summary, "Add Git tab");
    }

    #[test]
    fn infers_base_branch_order() {
        let branches = vec![
            GitBranch {
                name: "origin/main".to_string(),
                is_current: false,
                is_remote: true,
                upstream: None,
            },
            GitBranch {
                name: "origin/master".to_string(),
                is_current: false,
                is_remote: true,
                upstream: None,
            },
        ];

        assert_eq!(
            infer_base_branch(Some("develop"), Some("origin/main"), &branches).as_deref(),
            Some("develop")
        );
        assert_eq!(
            infer_base_branch(Some(""), Some("origin/main"), &branches).as_deref(),
            Some("origin/main")
        );
        assert_eq!(
            infer_base_branch(None, None, &branches).as_deref(),
            Some("origin/main")
        );
    }
}
