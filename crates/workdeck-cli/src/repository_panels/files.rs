use super::*;
use crate::bounded_files::{self, Kind};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

const MAX_IGNORE_BYTES: usize = 2 * 1024 * 1024;
const MAX_IGNORE_TOTAL: usize = 8 * 1024 * 1024;
const MAX_VISITED: usize = 50_000;
const MAX_DIRECTORIES: usize = 2048;
const MAX_DEPTH: usize = 128;

pub(super) fn relative(path: &str, allow_root: bool) -> Result<&Path, PanelError> {
    let value = Path::new(path);
    bounded_files::validate_relative(value, allow_root).map_err(error)?;
    if value.components().any(|part| part.as_os_str() == ".git") {
        return Err(PanelError::new(
            "Paths must stay outside Git's private directory",
        ));
    }
    Ok(value)
}

pub(super) fn directory(root: &Path, path: &str) -> Result<PathBuf, PanelError> {
    let relative = relative(path, true)?;
    bounded_files::list(root, relative, 0).map_err(error)?;
    Ok(root.join(relative))
}

pub(super) fn list(root: &Path, request: &PanelRequest) -> Result<PanelSnapshot, PanelError> {
    let relative = relative(&request.directory, true)?;
    let mut budget = Budget::default();
    let rules = rules_for_directory(root, relative, &mut budget)?;
    let (items, mut truncated) = bounded_files::list(root, relative, MAX_VISITED).map_err(error)?;
    let mut entries = Vec::new();
    for item in items {
        if item.name == ".git" {
            continue;
        }
        let path = relative.join(&item.name);
        if rules.ignored(&root.join(&path), item.kind == Kind::Directory) {
            continue;
        }
        let path = path.to_str().expect("validated UTF-8 path").to_owned();
        if !path
            .to_lowercase()
            .contains(&request.query.trim().to_lowercase())
        {
            continue;
        }
        let (detail, target) = if item.kind == Kind::Directory {
            ("directory".into(), PanelTarget::Directory { path })
        } else {
            (
                match item.kind {
                    Kind::Symlink => "symbolic link; target is not followed".into(),
                    Kind::Other => "special file; content cannot be opened".into(),
                    _ => format!("{} bytes", item.size),
                },
                PanelTarget::File { path, line: None },
            )
        };
        entries.push(entry(
            format!("file:{}", relative.join(&item.name).display()),
            item.name,
            detail,
            "",
            target,
        ));
        if entries.len() > MAX_ENTRIES {
            truncated = true;
            break;
        }
    }
    entries.sort_by(|a, b| {
        (!matches!(a.target, PanelTarget::Directory { .. }))
            .cmp(&!matches!(b.target, PanelTarget::Directory { .. }))
            .then(a.label.cmp(&b.label))
    });
    if entries.len() > limit(request) {
        entries.truncate(limit(request));
        truncated = true;
    }
    Ok(PanelSnapshot {
        page: PanelPage::Files,
        title: if request.directory.is_empty() {
            "Files".into()
        } else {
            request.directory.clone()
        },
        summary: format!("{} entries", entries.len()),
        entries,
        truncated,
    })
}

pub(super) fn inventory(root: &Path) -> Result<(Vec<PathBuf>, bool), PanelError> {
    let mut budget = Budget::default();
    let rules = rules_for_directory(root, Path::new(""), &mut budget)?;
    let mut paths = Vec::new();
    let mut stack = vec![(PathBuf::new(), rules, 0usize)];
    let mut visited = 0usize;
    let mut directories = 0usize;
    let mut truncated = false;
    while let Some((directory, mut rules, depth)) = stack.pop() {
        if directories == MAX_DIRECTORIES || visited >= MAX_VISITED {
            truncated = true;
            break;
        }
        directories += 1;
        if rules.pending.take().is_some() {
            add_directory_rules(root, &directory, &mut rules, &mut budget)?;
        }
        let (entries, limited) =
            bounded_files::list(root, &directory, MAX_VISITED - visited).map_err(error)?;
        truncated |= limited;
        visited += entries.len();
        let mut children = Vec::new();
        for item in entries {
            if item.name == ".git" {
                continue;
            }
            let path = directory.join(&item.name);
            if rules.ignored(&root.join(&path), item.kind == Kind::Directory) {
                continue;
            }
            match item.kind {
                Kind::Directory => {
                    if depth >= MAX_DEPTH {
                        truncated = true;
                        continue;
                    }
                    children.push(path);
                }
                Kind::File => {
                    if paths.len() == MAX_ENTRIES {
                        truncated = true;
                        break;
                    }
                    paths.push(path);
                }
                _ => {}
            }
        }
        if paths.len() == MAX_ENTRIES {
            truncated |= !children.is_empty() || !stack.is_empty();
            break;
        }
        // Delay loading child rules until that directory is actually visited;
        // skipped subtrees must not introduce ignored sources or consume reads.
        for child in children.into_iter().rev() {
            if stack.len() + directories >= MAX_DIRECTORIES {
                truncated = true;
                continue;
            }
            let mut child_rules = rules.clone();
            child_rules.pending = Some(child.clone());
            stack.push((child, child_rules, depth + 1));
        }
        if truncated && visited >= MAX_VISITED {
            break;
        }
    }
    paths.sort();
    Ok((paths, truncated))
}

/// Qualify Git's ignore sources before libgit2 traverses untracked directories.
/// Only Git ignore rules apply here: a .ignore file must never hide a source
/// that libgit2 will still read. Ignored build trees and nested repos are pruned.
/// Returns false when traversal coverage is incomplete; unsafe sources error.
pub(super) fn preflight_git_ignores(
    root: &Path,
    global_exclude: Option<&Path>,
) -> Result<bool, PanelError> {
    let mut budget = Budget::default();
    let rules = Rules {
        git_active: true,
        exclude: repository_exclude(root, &mut budget)?.map(Arc::new),
        global: global_exclude
            .map(|path| {
                read_ignore(None, path, &mut budget)?
                    .map(|bytes| matcher(root, path, &bytes).map(Arc::new))
                    .transpose()
            })
            .transpose()?
            .flatten(),
        ..Rules::default()
    };
    let mut stack = vec![(PathBuf::new(), rules, 0usize)];
    let mut visited = 0usize;
    let mut directories = 0usize;
    while let Some((directory, mut rules, depth)) = stack.pop() {
        if directories == MAX_DIRECTORIES || visited >= MAX_VISITED {
            return Ok(false);
        }
        directories += 1;
        let (entries, truncated) =
            bounded_files::list(root, &directory, MAX_VISITED - visited).map_err(error)?;
        if truncated {
            return Ok(false);
        }
        visited += entries.len();
        if !directory.as_os_str().is_empty() && entries.iter().any(|entry| entry.name == ".git") {
            continue;
        }
        let source = directory.join(".gitignore");
        if let Some(bytes) = read_ignore(Some(root), &source, &mut budget)? {
            rules
                .git
                .push(Arc::new(matcher(&root.join(&directory), &source, &bytes)?));
        }
        for entry in entries.into_iter().rev() {
            if entry.name == ".git" || entry.kind != Kind::Directory {
                continue;
            }
            let path = directory.join(entry.name);
            if rules.ignored(&root.join(&path), true) {
                continue;
            }
            if depth >= MAX_DEPTH || stack.len() + directories >= MAX_DIRECTORIES {
                return Ok(false);
            }
            stack.push((path, rules.clone(), depth + 1));
        }
    }
    Ok(true)
}

pub(super) use crate::bounded_files::ReadFile;
pub(super) fn read(root: &Path, path: &str, max_bytes: usize) -> Result<ReadFile, PanelError> {
    bounded_files::read(root, relative(path, false)?, max_bytes).map_err(error)
}

pub(super) fn preview(
    root: &Path,
    path: &str,
    max_bytes: usize,
) -> Result<PanelPreview, PanelError> {
    let read = read(root, path, max_bytes)?;
    let text = match std::str::from_utf8(&read.bytes) {
        Ok(text) => Some(text),
        Err(failure) if read.truncated && failure.error_len().is_none() => {
            std::str::from_utf8(&read.bytes[..failure.valid_up_to()]).ok()
        }
        Err(_) => None,
    };
    let binary = read.bytes.contains(&0) || text.is_none();
    let mut body = if binary {
        format!("Binary file\n{} bytes", read.size)
    } else {
        text.expect("checked UTF-8").to_owned()
    };
    let mut truncated = read.truncated;
    if body.len() > max_bytes {
        let mut boundary = max_bytes;
        while !body.is_char_boundary(boundary) {
            boundary -= 1;
        }
        body.truncate(boundary);
        truncated = true;
    }
    Ok(PanelPreview {
        title: path.into(),
        body,
        kind: if path.ends_with(".md") {
            PanelPreviewKind::Markdown
        } else {
            PanelPreviewKind::Source
        },
        truncated,
        binary,
    })
}

#[derive(Default)]
struct Budget {
    bytes: usize,
}
#[derive(Clone, Default)]
struct Rules {
    plain: Vec<Arc<Gitignore>>,
    git: Vec<Arc<Gitignore>>,
    exclude: Option<Arc<Gitignore>>,
    global: Option<Arc<Gitignore>>,
    git_active: bool,
    pending: Option<PathBuf>,
}
impl Rules {
    fn ignored(&self, path: &Path, is_directory: bool) -> bool {
        for matcher in self
            .plain
            .iter()
            .rev()
            .chain(self.git.iter().rev())
            .chain(self.exclude.iter())
            .chain(self.global.iter())
        {
            let result = matcher.matched(path, is_directory);
            if result.is_ignore() {
                return true;
            }
            if result.is_whitelist() {
                return false;
            }
        }
        false
    }
}

fn missing(failure: &anyhow::Error) -> bool {
    failure
        .chain()
        .filter_map(|cause| cause.downcast_ref::<std::io::Error>())
        .any(|failure| failure.kind() == std::io::ErrorKind::NotFound)
}
fn read_ignore(
    root: Option<&Path>,
    path: &Path,
    budget: &mut Budget,
) -> Result<Option<Vec<u8>>, PanelError> {
    let read = match root {
        Some(root) => bounded_files::read(root, path, MAX_IGNORE_BYTES),
        None => {
            let absolute = if path.is_absolute() {
                path.to_owned()
            } else {
                std::env::current_dir().map_err(error)?.join(path)
            };
            bounded_files::read_absolute(&absolute, MAX_IGNORE_BYTES)
        }
    };
    let read = match read {
        Ok(read) => read,
        Err(failure) if missing(&failure) => return Ok(None),
        Err(failure) => {
            return Err(PanelError::new(format!(
                "Unsafe or unreadable active ignore source {}: {failure:#}",
                path.display()
            )));
        }
    };
    budget.bytes += read.bytes.len();
    if read.truncated || budget.bytes > MAX_IGNORE_TOTAL {
        return Err(PanelError::new(format!(
            "Active ignore sources exceed the bounded read budget at {}",
            path.display()
        )));
    }
    Ok(Some(read.bytes))
}
fn matcher(scope: &Path, source: &Path, bytes: &[u8]) -> Result<Gitignore, PanelError> {
    let text = std::str::from_utf8(bytes).map_err(error)?;
    let mut builder = GitignoreBuilder::new(scope);
    for (index, line) in text.lines().enumerate() {
        let line = if index == 0 {
            line.trim_start_matches('\u{feff}')
        } else {
            line
        };
        builder
            .add_line(Some(source.to_owned()), line)
            .map_err(error)?;
    }
    builder.build().map_err(error)
}
fn add_directory_rules(
    root: &Path,
    relative: &Path,
    rules: &mut Rules,
    budget: &mut Budget,
) -> Result<(), PanelError> {
    let scope = root.join(relative);
    if !relative.as_os_str().is_empty() && fs::symlink_metadata(scope.join(".git")).is_ok() {
        rules.git.clear();
        rules.git_active = true;
        rules.exclude = repository_exclude(&scope, budget)?.map(Arc::new);
        rules.global = global_ignore(&scope, budget)?.map(Arc::new);
    }
    for (name, git) in [(".ignore", false), (".gitignore", true)] {
        if git && !rules.git_active {
            continue;
        }
        let source = relative.join(name);
        if let Some(bytes) = read_ignore(Some(root), &source, budget)? {
            let parsed = matcher(&scope, &root.join(source), &bytes)?;
            if git {
                rules.git.push(Arc::new(parsed))
            } else {
                rules.plain.push(Arc::new(parsed))
            }
        }
    }
    Ok(())
}

fn rules_for_directory(
    root: &Path,
    relative: &Path,
    budget: &mut Budget,
) -> Result<Rules, PanelError> {
    let ancestors = root.ancestors().take(MAX_DEPTH + 1).collect::<Vec<_>>();
    if ancestors.len() > MAX_DEPTH {
        return Err(PanelError::new(
            "Repository ancestor depth exceeds the traversal limit",
        ));
    }
    let git_root = ancestors
        .iter()
        .copied()
        .find(|path| fs::symlink_metadata(path.join(".git")).is_ok());
    let mut rules = Rules {
        git_active: git_root.is_some(),
        ..Rules::default()
    };
    // Ancestor .ignore files apply to nested navigation. Git's own ignore files
    // stop at the closest repository boundary, matching the former walker.
    for ancestor in ancestors.iter().skip(1).rev() {
        for (name, git) in [(".ignore", false), (".gitignore", true)] {
            if git && !git_root.is_some_and(|boundary| ancestor.starts_with(boundary)) {
                continue;
            }
            let path = ancestor.join(name);
            if let Some(bytes) = read_ignore(None, &path, budget)? {
                let parsed = matcher(ancestor, &path, &bytes)?;
                if git {
                    rules.git.push(Arc::new(parsed))
                } else {
                    rules.plain.push(Arc::new(parsed))
                }
            }
        }
    }
    if let Some(git_root) = git_root {
        rules.exclude = repository_exclude(git_root, budget)?.map(Arc::new);
        rules.global = global_ignore(root, budget)?.map(Arc::new);
    }
    let mut scope = PathBuf::new();
    add_directory_rules(root, &scope, &mut rules, budget)?;
    for (depth, component) in relative.components().enumerate() {
        if depth >= MAX_DEPTH {
            return Err(PanelError::new(
                "Directory depth exceeds the traversal limit",
            ));
        }
        scope.push(component);
        bounded_files::list(root, &scope, 0).map_err(error)?;
        add_directory_rules(root, &scope, &mut rules, budget)?;
    }
    Ok(rules)
}

fn repository_exclude(root: &Path, budget: &mut Budget) -> Result<Option<Gitignore>, PanelError> {
    let path = root.join(".git");
    let metadata = fs::symlink_metadata(&path).map_err(error)?;
    if metadata.file_type().is_symlink() {
        return Err(PanelError::new(
            "Git metadata cannot be a symbolic link during repository file discovery",
        ));
    }
    let git_directory = if metadata.is_dir() {
        path
    } else {
        let bytes = read_ignore(Some(root), Path::new(".git"), budget)?
            .ok_or_else(|| PanelError::new("Git metadata changed during discovery"))?;
        let text = std::str::from_utf8(&bytes).map_err(error)?;
        let path = text
            .trim()
            .strip_prefix("gitdir:")
            .ok_or_else(|| PanelError::new("Invalid Git worktree metadata"))?
            .trim();
        if path.is_empty() || path.chars().any(char::is_control) {
            return Err(PanelError::new("Invalid Git metadata directory"));
        }
        root.join(path).canonicalize().map_err(error)?
    };
    let common =
        if let Some(bytes) = read_ignore(Some(&git_directory), Path::new("commondir"), budget)? {
            let value = std::str::from_utf8(&bytes).map_err(error)?.trim();
            if value.is_empty() || value.chars().any(char::is_control) {
                return Err(PanelError::new("Invalid common Git metadata directory"));
            }
            git_directory.join(value).canonicalize().map_err(error)?
        } else {
            git_directory
        };
    let source = common.join("info/exclude");
    read_ignore(Some(&common), Path::new("info/exclude"), budget)?
        .map(|bytes| matcher(root, &source, &bytes))
        .transpose()
}

fn global_ignore(root: &Path, budget: &mut Budget) -> Result<Option<Gitignore>, PanelError> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let xdg = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|home| home.join(".config")));
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("GIT_CONFIG_GLOBAL").filter(|value| !value.is_empty()) {
        candidates.push(PathBuf::from(path));
    }
    if let Some(home) = &home {
        candidates.push(home.join(".gitconfig"));
    }
    if let Some(xdg) = &xdg {
        candidates.push(xdg.join("git/config"));
    }
    candidates.push(
        std::env::var_os("GIT_CONFIG_SYSTEM")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/etc/gitconfig")),
    );
    let mut selected = None;
    for candidate in candidates {
        // GIT_CONFIG_GLOBAL=/dev/null is the standard hermetic-test isolation
        // idiom and behaves as an empty configuration for Git, so it can never
        // carry an excludesfile directive.
        if candidate == Path::new(if cfg!(windows) { "NUL" } else { "/dev/null" }) {
            continue;
        }
        let Some(bytes) = read_ignore(None, &candidate, budget)? else {
            continue;
        };
        let text = std::str::from_utf8(&bytes).map_err(error)?;
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            if !key.trim().eq_ignore_ascii_case("excludesfile") {
                continue;
            }
            let value = value.trim().trim_matches('"').trim();
            if value.is_empty() {
                return Err(PanelError::new("Global excludesFile must identify a file"));
            }
            let path = if let Some(value) = value.strip_prefix("~/") {
                home.as_ref()
                    .ok_or_else(|| {
                        PanelError::new(
                            "Cannot resolve ~/ in global excludesFile without a home directory",
                        )
                    })?
                    .join(value)
            } else {
                PathBuf::from(value)
            };
            selected = Some(if path.is_absolute() {
                path
            } else {
                std::env::current_dir().map_err(error)?.join(path)
            });
            break;
        }
        if selected.is_some() {
            break;
        }
    }
    let Some(source) = selected.or_else(|| xdg.map(|xdg| xdg.join("git/ignore"))) else {
        return Ok(None);
    };
    read_ignore(None, &source, budget)?
        .map(|bytes| matcher(root, &source, &bytes))
        .transpose()
}

pub(super) fn command_preview(
    root: &Path,
    path: &str,
    max_bytes: usize,
) -> Result<crate::git::FilePreview, PanelError> {
    let relative = relative(path, true)?;
    if bounded_files::list(root, relative, 0).is_ok() {
        return Ok(crate::git::FilePreview {
            title: path.into(),
            content: "directory".into(),
            truncated: false,
            binary: false,
        });
    }
    let read = read(root, path, max_bytes)?;
    let category = match relative
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
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
    };
    let text = match std::str::from_utf8(&read.bytes) {
        Ok(text) => Some(text),
        Err(failure) if read.truncated && failure.error_len().is_none() => {
            std::str::from_utf8(&read.bytes[..failure.valid_up_to()]).ok()
        }
        Err(_) => None,
    };
    if category.is_some() || text.is_none() || read.bytes.contains(&0) {
        return Ok(crate::git::FilePreview {
            title: path.into(),
            content: format!(
                "{}\nsize: {} bytes",
                category.unwrap_or("binary file"),
                read.size
            ),
            truncated: false,
            binary: true,
        });
    }
    let mut content = text.expect("checked UTF-8").to_owned();
    if read.truncated {
        content.push_str("\n\n... truncated ...");
    }
    Ok(crate::git::FilePreview {
        title: path.into(),
        content,
        truncated: read.truncated,
        binary: false,
    })
}
