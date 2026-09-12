//! Repository-bound, read-only Git panels. libgit2 reads objects/index/worktree
//! directly: no Git command, external diff, textconv, hook or pager is launched.
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    path::{Path, PathBuf},
};

use git2::{
    Diff, DiffFindOptions, DiffFormat, DiffOptions, ErrorCode, ObjectType, Oid, Patch, Reference,
    Repository, RepositoryOpenFlags, Sort, Tree,
};
use workdeck_tui::workbench::*;

use super::{MAX_ENTRIES, MAX_PREVIEW_BYTES, error, files, limit};

const MAX_DIFF_FILE_BYTES: i64 = 2 * 1024 * 1024;

/// libgit2 reopens attribute files internally. This bounded descriptor preflight
/// rejects existing unsafe sources, but cannot prevent a hostile concurrent
/// swap between this check and the library's read. No external helper executes.
fn preflight_attributes<'a>(
    repository: &Repository,
    paths: impl IntoIterator<Item = &'a Path>,
) -> Result<(), PanelError> {
    const FILE_LIMIT: usize = 2 * 1024 * 1024;
    const TOTAL_LIMIT: usize = 8 * 1024 * 1024;
    let root = repository.workdir().expect("bound working directory");
    let mut directories = BTreeSet::from([PathBuf::new()]);
    for (count, path) in paths.into_iter().enumerate() {
        if count >= 50_000 {
            return Err(PanelError::new(
                "Git attribute inspection exceeds 50,000 paths",
            ));
        }
        let path = files::relative(
            path.to_str()
                .ok_or_else(|| PanelError::new("Git path must be UTF-8"))?,
            false,
        )?;
        for (depth, parent) in path.ancestors().skip(1).enumerate() {
            if depth >= 128 || directories.len() > 2048 {
                return Err(PanelError::new(
                    "Git attribute inspection exceeds its directory limit",
                ));
            }
            directories.insert(parent.to_owned());
        }
    }
    let mut total = 0usize;
    let mut inspect = |base: Option<&Path>, path: &Path| -> Result<(), PanelError> {
        let read = match base {
            Some(base) => crate::bounded_files::read(base, path, FILE_LIMIT),
            None => {
                let absolute = if path.is_absolute() {
                    path.to_owned()
                } else {
                    std::env::current_dir().map_err(error)?.join(path)
                };
                crate::bounded_files::read_absolute(&absolute, FILE_LIMIT)
            }
        };
        let read = match read {
            Ok(read) => read,
            Err(failure)
                if failure
                    .chain()
                    .filter_map(|cause| cause.downcast_ref::<std::io::Error>())
                    .any(|cause| cause.kind() == std::io::ErrorKind::NotFound) =>
            {
                return Ok(());
            }
            Err(failure) => {
                return Err(PanelError::new(format!(
                    "Unsafe or unreadable Git attribute source {}: {failure:#}",
                    path.display()
                )));
            }
        };
        total += read.bytes.len();
        if read.truncated || total > TOTAL_LIMIT {
            return Err(PanelError::new(
                "Git attributes exceed the 2 MiB file or 8 MiB aggregate limit",
            ));
        }
        Ok(())
    };
    inspect(Some(repository.commondir()), Path::new("info/attributes"))?;
    for directory in directories {
        inspect(Some(root), &directory.join(".gitattributes"))?;
    }
    let config = repository.config().map_err(error)?;
    match config.get_path("core.attributesfile") {
        Ok(path) if !path.as_os_str().is_empty() => inspect(None, &path)?,
        Ok(_) => {}
        Err(failure) if failure.code() == ErrorCode::NotFound => {
            let xdg = std::env::var_os("XDG_CONFIG_HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"))
                });
            if let Some(xdg) = xdg {
                inspect(None, &xdg.join("git/attributes"))?;
            }
        }
        Err(failure) => return Err(error(failure)),
    }
    // This repository does not alter libgit2's global search paths; the linked
    // Unix library's system attribute directory is /etc (sysdir.c).
    #[cfg(unix)]
    inspect(None, Path::new("/etc/gitattributes"))?;
    #[cfg(not(unix))]
    return Err(PanelError::new(
        "Git attribute source preflight is not qualified on this platform",
    ));
    Ok(())
}

fn preflight_diff_attributes(repository: &Repository, diff: &Diff<'_>) -> Result<(), PanelError> {
    let paths = diff
        .deltas()
        .flat_map(|delta| {
            [
                delta.old_file().path().map(Path::to_owned),
                delta.new_file().path().map(Path::to_owned),
            ]
        })
        .flatten()
        .collect::<Vec<_>>();
    preflight_attributes(repository, paths.iter().map(PathBuf::as_path))
}

pub(super) fn open(root: &Path) -> Result<Repository, PanelError> {
    // FROM_ENV is deliberately absent. In particular GIT_DIR, GIT_WORK_TREE,
    // GIT_INDEX_FILE and GIT_NAMESPACE must not redirect the bound provider.
    let repository = Repository::open_ext(root, RepositoryOpenFlags::NO_SEARCH, &[] as &[&OsStr])
        .map_err(error)?;
    let workdir = repository
        .workdir()
        .ok_or_else(|| PanelError::new("Git panels require a repository working directory"))?
        .canonicalize()
        .map_err(error)?;
    if workdir != root.canonicalize().map_err(error)? {
        return Err(PanelError::new(
            "Git working directory differs from the bound repository root",
        ));
    }
    Ok(repository)
}

fn head_tree(repository: &Repository) -> Result<Option<Tree<'_>>, PanelError> {
    match repository.head() {
        Ok(head) => head.peel_to_tree().map(Some).map_err(error),
        Err(failure)
            if matches!(
                failure.code(),
                ErrorCode::UnbornBranch | ErrorCode::NotFound
            ) =>
        {
            Ok(None)
        }
        Err(failure) => Err(error(failure)),
    }
}

fn diff_options(path: Option<&str>) -> Result<DiffOptions, PanelError> {
    let mut options = DiffOptions::new();
    options
        .include_typechange(true)
        .disable_pathspec_match(true)
        .max_size(MAX_DIFF_FILE_BYTES)
        .context_lines(3);
    if let Some(path) = path {
        files::relative(path, false)?;
        options.pathspec(path);
    }
    Ok(options)
}

fn preflight_worktree_ignores(repository: &Repository) -> Result<bool, PanelError> {
    let config = repository.config().map_err(error)?;
    let global_exclude = match config.get_path("core.excludesfile") {
        Ok(path) => (!path.as_os_str().is_empty()).then_some(path),
        Err(failure) if failure.code() == ErrorCode::NotFound => {
            std::env::var_os("XDG_CONFIG_HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"))
                })
                .map(|xdg| xdg.join("git/ignore"))
        }
        Err(failure) => return Err(error(failure)),
    };
    files::preflight_git_ignores(
        repository.workdir().expect("bound worktree"),
        global_exclude.as_deref(),
    )
}

fn changes_diff<'a>(
    repository: &'a Repository,
    staged: bool,
    path: Option<&str>,
) -> Result<Diff<'a>, PanelError> {
    let mut options = diff_options(path)?;
    if !staged {
        if path.is_none() && !preflight_worktree_ignores(repository)? {
            return Err(PanelError::new(
                "Git ignore inspection exceeded its traversal limit; narrow the repository scope",
            ));
        }
        // Worktree stat comparisons can hash filtered content before yielding
        // deltas, so inspect tracked ancestors before asking libgit2 to scan.
        let index = repository.index().map_err(error)?;
        let mut paths = Vec::new();
        for entry in index.iter().take(50_001) {
            let path = std::str::from_utf8(&entry.path).map_err(error)?;
            paths.push(PathBuf::from(path));
        }
        preflight_attributes(repository, paths.iter().map(PathBuf::as_path))?;
    }
    let mut diff = if staged {
        let tree = head_tree(repository)?;
        repository.diff_tree_to_index(tree.as_ref(), None, Some(&mut options))
    } else {
        options
            .include_untracked(true)
            .recurse_untracked_dirs(true)
            .show_untracked_content(true);
        repository.diff_index_to_workdir(None, Some(&mut options))
    }
    .map_err(error)?;
    preflight_diff_attributes(repository, &diff)?;
    // Bound rename pairing; its quadratic candidate search is unnecessary on
    // large changesets. A path-scoped preview remains a literal-path diff.
    if path.is_none() && diff.deltas().len() <= 200 {
        diff.find_similar(Some(DiffFindOptions::new().renames(true).rename_limit(100)))
            .map_err(error)?;
    }
    Ok(diff)
}

fn row(
    id: String,
    label: impl Into<String>,
    detail: impl Into<String>,
    section: impl Into<String>,
    target: PanelTarget,
) -> PanelEntry {
    PanelEntry {
        id,
        label: label.into(),
        detail: detail.into(),
        section: section.into(),
        target,
        changes: None,
    }
}

struct ChangeRecord {
    path: PathBuf,
    old_path: Option<PathBuf>,
    kind: crate::git::ChangeKind,
    staged: bool,
    additions: usize,
    deletions: usize,
    binary: bool,
    statistics_limited: bool,
}

fn read_changes(
    root: &Path,
    request: &PanelRequest,
) -> Result<(Vec<ChangeRecord>, bool), PanelError> {
    let repository = open(root)?;
    let directory = files::relative(&request.directory, true)?;
    if !preflight_worktree_ignores(&repository)? {
        return Ok((Vec::new(), true));
    }
    let query = request.query.to_lowercase();
    let mut entries = Vec::new();
    let mut truncated = false;
    'sides: for staged in [true, false] {
        let diff = changes_diff(&repository, staged, None)?;
        for (index, delta) in diff.deltas().enumerate() {
            let path = delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())
                .ok_or_else(|| PanelError::new("Git change has no path"))?;
            let path_text = path
                .to_str()
                .ok_or_else(|| PanelError::new("Git change path must be UTF-8"))?;
            files::relative(path_text, false)?;
            if !path.starts_with(directory) || !path_text.to_lowercase().contains(&query) {
                continue;
            }
            if entries.len() == limit(request) {
                truncated = true;
                break 'sides;
            }
            let patch = Patch::from_diff(&diff, index).map_err(error)?;
            let (_, additions, deletions) = patch
                .as_ref()
                .map(Patch::line_stats)
                .transpose()
                .map_err(error)?
                .unwrap_or_default();
            let delta = patch.as_ref().map(Patch::delta).unwrap_or(delta);
            let binary = delta.old_file().is_binary() || delta.new_file().is_binary();
            let statistics_limited = delta.old_file().size() > MAX_DIFF_FILE_BYTES as u64
                || delta.new_file().size() > MAX_DIFF_FILE_BYTES as u64;
            let kind = match delta.status() {
                git2::Delta::Added => crate::git::ChangeKind::Added,
                git2::Delta::Deleted => crate::git::ChangeKind::Deleted,
                git2::Delta::Renamed => crate::git::ChangeKind::Renamed,
                git2::Delta::Typechange => crate::git::ChangeKind::Typechange,
                git2::Delta::Untracked => crate::git::ChangeKind::Untracked,
                git2::Delta::Conflicted => crate::git::ChangeKind::Conflicted,
                _ => crate::git::ChangeKind::Modified,
            };
            let old_path = (delta.status() == git2::Delta::Renamed)
                .then(|| delta.old_file().path().map(Path::to_owned))
                .flatten();
            entries.push(ChangeRecord {
                path: path.to_owned(),
                old_path,
                kind,
                staged,
                additions,
                deletions,
                binary,
                statistics_limited,
            });
        }
    }
    Ok((entries, truncated))
}

pub(super) fn changes(root: &Path, request: &PanelRequest) -> Result<PanelSnapshot, PanelError> {
    let (records, truncated) = read_changes(root, request)?;
    let entries = records
        .into_iter()
        .map(|record| {
            let path = record.path.to_str().expect("validated UTF-8 Git path");
            let side = if record.staged { "Staged" } else { "Unstaged" };
            let group = record
                .path
                .parent()
                .filter(|value| !value.as_os_str().is_empty())
                .and_then(Path::to_str)
                .unwrap_or(".");
            let detail = format!(
                "{:?} · +{} −{}{}",
                record.kind,
                record.additions,
                record.deletions,
                if record.binary {
                    " · binary or larger than 2 MiB"
                } else {
                    ""
                }
            );
            let mut entry = row(
                format!("change:{}:{path}", record.staged),
                path,
                detail,
                format!("{side} · {group}"),
                PanelTarget::Change {
                    path: path.into(),
                    staged: record.staged,
                },
            );
            entry.changes = Some(PanelChangeStats {
                additions: record.additions,
                deletions: record.deletions,
                staged: record.staged,
                unstaged: !record.staged,
            });
            entry
        })
        .collect::<Vec<_>>();
    Ok(PanelSnapshot {
        page: request.page,
        title: "Changes".into(),
        summary: format!(
            "{} staged/worktree entries{}",
            entries.len(),
            if truncated { " (limited)" } else { "" }
        ),
        entries,
        truncated,
    })
}

pub(super) fn snapshot(root: &Path) -> Result<super::ChangeSnapshot, PanelError> {
    let request = PanelRequest {
        page: PanelPage::Changes,
        directory: String::new(),
        query: String::new(),
        limit: MAX_ENTRIES,
    };
    let (records, mut truncated) = read_changes(root, &request)?;
    let mut changes = BTreeMap::<PathBuf, crate::git::ChangeEntry>::new();
    let mut old_paths = BTreeMap::new();
    let rank = |kind| match kind {
        crate::git::ChangeKind::Conflicted => 7,
        crate::git::ChangeKind::Untracked => 6,
        crate::git::ChangeKind::Renamed => 5,
        crate::git::ChangeKind::Deleted => 4,
        crate::git::ChangeKind::Added => 3,
        crate::git::ChangeKind::Typechange => 2,
        crate::git::ChangeKind::Modified => 1,
    };
    for record in records {
        truncated |= record.statistics_limited;
        if let Some(old) = record.old_path {
            old_paths.insert(record.path.clone(), old);
        }
        let change = changes
            .entry(record.path.clone())
            .or_insert(crate::git::ChangeEntry {
                path: record.path,
                kind: record.kind,
                staged: false,
                unstaged: false,
                additions: 0,
                deletions: 0,
            });
        if rank(record.kind) > rank(change.kind) {
            change.kind = record.kind;
        }
        change.staged |= record.staged;
        change.unstaged |= !record.staged;
        change.additions += record.additions;
        change.deletions += record.deletions;
    }
    let changes = changes.into_values().collect::<Vec<_>>();
    let groups = crate::git::group_by_directory(&changes);
    Ok(super::ChangeSnapshot {
        snapshot: crate::git::RepoSnapshot {
            root: root.to_owned(),
            changes,
            groups,
        },
        truncated,
        old_paths,
    })
}

fn oid(reference: &str) -> Result<Oid, PanelError> {
    if reference.len() != 40 || !reference.bytes().all(|value| value.is_ascii_hexdigit()) {
        return Err(PanelError::new(
            "Git commit and stash targets require a full object ID",
        ));
    }
    Oid::from_str(reference).map_err(error)
}

fn named_ref(reference: &str, prefixes: &[&str]) -> Result<(), PanelError> {
    if reference.len() > 1024
        || reference.chars().any(char::is_control)
        || !prefixes.iter().any(|prefix| reference.starts_with(prefix))
        || !Reference::is_valid_name(reference)
    {
        return Err(PanelError::new(
            "Git target must be a valid full branch or tag reference",
        ));
    }
    Ok(())
}

fn resolve_base(repository: &Repository, reference: &str) -> Result<Oid, PanelError> {
    if reference == "HEAD" {
        return repository
            .head()
            .and_then(|head| head.peel_to_commit())
            .map(|commit| commit.id())
            .map_err(error);
    }
    if let Ok(id) = oid(reference) {
        return repository
            .find_commit(id)
            .map(|commit| commit.id())
            .map_err(error);
    }
    if reference.starts_with("refs/") {
        named_ref(reference, &["refs/heads/", "refs/remotes/", "refs/tags/"])?;
        return repository
            .find_reference(reference)
            .and_then(|value| value.peel_to_commit())
            .map(|value| value.id())
            .map_err(error);
    }
    if reference.starts_with('-')
        || reference.len() > 1000
        || reference.chars().any(char::is_control)
    {
        return Err(PanelError::new("Invalid Git comparison base"));
    }
    let mut selected = None;
    for prefix in ["refs/heads/", "refs/remotes/", "refs/tags/"] {
        let full = format!("{prefix}{reference}");
        named_ref(&full, &[prefix])?;
        match repository.find_reference(&full) {
            Ok(value) => {
                let candidate = value.peel_to_commit().map_err(error)?.id();
                if selected.is_some_and(|selected| selected != candidate) {
                    return Err(PanelError::new(
                        "Git comparison base is ambiguous; use a full reference",
                    ));
                }
                selected = Some(candidate);
            }
            Err(failure) if failure.code() == ErrorCode::NotFound => {}
            Err(failure) => return Err(error(failure)),
        }
    }
    selected.ok_or_else(|| PanelError::new("Git comparison base was not found"))
}

fn summary(repository: &Repository, base: Option<&str>) -> Result<String, PanelError> {
    let base_id = base
        .map(|reference| resolve_base(repository, reference))
        .transpose()?;
    let mut text = format!("Repository state: {:?}\n", repository.state());
    match repository.head() {
        Ok(head) => {
            let commit = head.peel_to_commit().map_err(error)?;
            text.push_str(&format!(
                "HEAD: {}\nCommit: {}\n",
                head.shorthand().unwrap_or("detached"),
                commit.id()
            ));
            if let Some(base_id) = base_id {
                let (ahead, behind) = repository
                    .graph_ahead_behind(commit.id(), base_id)
                    .map_err(error)?;
                text.push_str(&format!(
                    "Base: {} ({base_id})\nAhead: {ahead}; behind: {behind}\n",
                    base.unwrap_or_default()
                ));
            }
        }
        Err(failure)
            if matches!(
                failure.code(),
                ErrorCode::UnbornBranch | ErrorCode::NotFound
            ) =>
        {
            text.push_str("HEAD: unborn (no commits)\n")
        }
        Err(failure) => return Err(error(failure)),
    }
    if repository.is_shallow() {
        text.push_str("History is shallow\n");
    }
    Ok(text)
}

struct Entries<'a> {
    request: &'a PanelRequest,
    values: Vec<PanelEntry>,
    truncated: bool,
}
impl Entries<'_> {
    fn push(&mut self, entry: PanelEntry) -> bool {
        let query = self.request.query.to_lowercase();
        if !format!("{} {} {}", entry.label, entry.detail, entry.section)
            .to_lowercase()
            .contains(&query)
        {
            return true;
        }
        if self.values.len() == limit(self.request) {
            self.truncated = true;
            return false;
        }
        self.values.push(entry);
        true
    }
}

pub(super) fn overview(
    root: &Path,
    base: Option<&str>,
    recent_limit: usize,
    request: &PanelRequest,
) -> Result<PanelSnapshot, PanelError> {
    let mut repository = open(root)?;
    let summary = summary(&repository, base)?;
    let mut rows = Entries {
        request,
        values: Vec::new(),
        truncated: false,
    };
    rows.push(row(
        "git:summary".into(),
        "Repository summary",
        summary.lines().take(3).collect::<Vec<_>>().join(" · "),
        "Summary",
        PanelTarget::GitSummary,
    ));
    for (count, branch) in repository.branches(None).map_err(error)?.enumerate() {
        if count == MAX_ENTRIES {
            rows.truncated = true;
            break;
        }
        let (branch, kind) = branch.map_err(error)?;
        let reference = branch.get().name().map_err(error)?.to_owned();
        let label = branch
            .name()
            .map_err(error)?
            .unwrap_or(&reference)
            .to_owned();
        if !rows.push(row(
            format!("branch:{reference}"),
            label,
            format!(
                "{kind:?}{}",
                if branch.is_head() { " · current" } else { "" }
            ),
            "Branches",
            PanelTarget::Branch { reference },
        )) {
            break;
        }
    }
    match repository.head() {
        Ok(head) => {
            let mut walk = repository.revwalk().map_err(error)?;
            walk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME)
                .map_err(error)?;
            walk.push(head.peel_to_commit().map_err(error)?.id())
                .map_err(error)?;
            for id in walk.take(recent_limit.min(MAX_ENTRIES)) {
                let commit = repository.find_commit(id.map_err(error)?).map_err(error)?;
                if !rows.push(row(
                    format!("commit:{}", commit.id()),
                    commit
                        .summary()
                        .map_err(error)?
                        .unwrap_or("(no commit message)"),
                    format!("{} · {}", &commit.id().to_string()[..8], commit.author()),
                    "Recent commits",
                    PanelTarget::Commit {
                        reference: commit.id().to_string(),
                    },
                )) {
                    break;
                }
            }
        }
        Err(failure)
            if matches!(
                failure.code(),
                ErrorCode::UnbornBranch | ErrorCode::NotFound
            ) => {}
        Err(failure) => return Err(error(failure)),
    }
    let mut stashes = Vec::new();
    repository
        .stash_foreach(|_, message, id| {
            if stashes.len() == MAX_ENTRIES {
                rows.truncated = true;
                return false;
            }
            stashes.push((message.to_owned(), *id));
            true
        })
        .map_err(error)?;
    for (message, id) in stashes {
        if !rows.push(row(
            format!("stash:{id}"),
            message,
            id.to_string(),
            "Stashes",
            PanelTarget::Stash {
                reference: id.to_string(),
            },
        )) {
            break;
        }
    }
    for (count, reference) in repository
        .references_glob("refs/tags/*")
        .map_err(error)?
        .enumerate()
    {
        if count == MAX_ENTRIES {
            rows.truncated = true;
            break;
        }
        let reference = reference.map_err(error)?;
        let name = reference.name().map_err(error)?.to_owned();
        let label = name.strip_prefix("refs/tags/").unwrap_or(&name).to_owned();
        if !rows.push(row(
            format!("tag:{name}"),
            label,
            reference
                .target()
                .map(|id| id.to_string())
                .unwrap_or_default(),
            "Tags",
            PanelTarget::Tag { reference: name },
        )) {
            break;
        }
    }
    for (count, name) in repository.remotes().map_err(error)?.iter().enumerate() {
        if count == MAX_ENTRIES {
            rows.truncated = true;
            break;
        }
        let name = name
            .map_err(error)?
            .ok_or_else(|| PanelError::new("Git remote name must be UTF-8"))?;
        let remote = repository.find_remote(name).map_err(error)?;
        if !rows.push(row(
            format!("remote:{name}"),
            name,
            display_url(remote.url().unwrap_or_default()),
            "Remotes",
            PanelTarget::Remote { name: name.into() },
        )) {
            break;
        }
    }
    Ok(PanelSnapshot {
        page: request.page,
        title: "Git".into(),
        summary: summary.lines().nth(1).unwrap_or("Git repository").into(),
        entries: rows.values,
        truncated: rows.truncated,
    })
}

// Hide URL credentials and query tokens without resolving a remote or invoking
// credential helpers. Ordinary SCP-style git@host:path remains readable.
fn display_url(url: &str) -> String {
    let url = url.split(['?', '#']).next().unwrap_or_default();
    if let Some((scheme, rest)) = url.split_once("://") {
        let (authority, tail) = rest.split_once('/').unwrap_or((rest, ""));
        let authority = authority.rsplit('@').next().unwrap_or(authority);
        return format!(
            "{scheme}://{authority}{}{tail}",
            if tail.is_empty() { "" } else { "/" }
        );
    }
    url.to_owned()
}

struct Output {
    bytes: Vec<u8>,
    truncated: bool,
    binary: bool,
}
impl Output {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            truncated: false,
            binary: false,
        }
    }
    fn push(&mut self, bytes: &[u8]) -> bool {
        let available = MAX_PREVIEW_BYTES.saturating_sub(self.bytes.len());
        self.bytes
            .extend_from_slice(&bytes[..bytes.len().min(available)]);
        if bytes.len() > available {
            self.truncated = true;
            return false;
        }
        true
    }
    fn finish(mut self, title: String, kind: PanelPreviewKind) -> PanelPreview {
        let text = String::from_utf8_lossy(&self.bytes);
        let mut body =
            workdeck_diff::sanitize_terminal_text(&text, workdeck_diff::SanitizeOptions::default());
        if body.len() > MAX_PREVIEW_BYTES {
            let mut boundary = MAX_PREVIEW_BYTES;
            while !body.is_char_boundary(boundary) {
                boundary -= 1;
            }
            body.truncate(boundary);
            self.truncated = true;
        }
        PanelPreview {
            title: workdeck_diff::sanitize_terminal_line(&title),
            body,
            kind,
            truncated: self.truncated,
            binary: self.binary,
        }
    }
}

fn append_commit(output: &mut Output, commit: &git2::Commit<'_>) {
    output.push(
        format!(
            "Commit: {}\nAuthor: {}\nTime: {}\n\n",
            commit.id(),
            commit.author(),
            commit.time().seconds()
        )
        .as_bytes(),
    );
    output.push(commit.message_bytes());
    output.push(b"\n\n");
}

fn append_diff(output: &mut Output, diff: &Diff<'_>) -> Result<(), PanelError> {
    if output.truncated {
        return Ok(());
    }
    let result = diff.print(DiffFormat::Patch, |delta, _, line| {
        output.binary |= delta.old_file().is_binary() || delta.new_file().is_binary();
        if matches!(line.origin(), '+' | '-' | ' ') && !output.push(&[line.origin() as u8]) {
            return false;
        }
        output.push(line.content())
    });
    if let Err(failure) = result
        && !output.truncated
    {
        return Err(error(failure));
    }
    Ok(())
}

fn commit_diff<'a>(
    repository: &'a Repository,
    commit: &git2::Commit<'_>,
    base: Option<Oid>,
) -> Result<Diff<'a>, PanelError> {
    let old = if let Some(id) = base {
        Some(
            repository
                .find_commit(id)
                .and_then(|value| value.tree())
                .map_err(error)?,
        )
    } else if commit.parent_count() > 0 {
        Some(
            commit
                .parent(0)
                .and_then(|value| value.tree())
                .map_err(error)?,
        )
    } else {
        None
    };
    let diff = repository
        .diff_tree_to_tree(
            old.as_ref(),
            Some(&commit.tree().map_err(error)?),
            Some(&mut diff_options(None)?),
        )
        .map_err(error)?;
    preflight_diff_attributes(repository, &diff)?;
    Ok(diff)
}

pub(super) fn preview(
    root: &Path,
    base: Option<&str>,
    target: &PanelTarget,
) -> Result<PanelPreview, PanelError> {
    let repository = open(root)?;
    let mut output = Output::new();
    let title = match target {
        PanelTarget::GitSummary => {
            output.push(summary(&repository, base)?.as_bytes());
            return Ok(output.finish("Repository summary".into(), PanelPreviewKind::Text));
        }
        PanelTarget::Change { path, staged } => {
            return change_preview(&repository, root, path, *staged)?.ok_or_else(|| {
                PanelError::new("This Git change no longer exists; refresh the panel")
            });
        }
        PanelTarget::Commit { reference } | PanelTarget::Stash { reference } => {
            let commit = repository.find_commit(oid(reference)?).map_err(error)?;
            append_commit(&mut output, &commit);
            append_diff(&mut output, &commit_diff(&repository, &commit, None)?)?;
            if matches!(target, PanelTarget::Stash { .. }) && commit.parent_count() > 2 {
                let untracked = commit
                    .parent(2)
                    .and_then(|parent| parent.tree())
                    .map_err(error)?;
                output.push(b"\nUntracked stash content:\n");
                let diff = repository
                    .diff_tree_to_tree(None, Some(&untracked), Some(&mut diff_options(None)?))
                    .map_err(error)?;
                preflight_diff_attributes(&repository, &diff)?;
                append_diff(&mut output, &diff)?;
            }
            reference.clone()
        }
        PanelTarget::Branch { reference } | PanelTarget::Tag { reference } => {
            let prefixes: &[&str] = if matches!(target, PanelTarget::Branch { .. }) {
                &["refs/heads/", "refs/remotes/"]
            } else {
                &["refs/tags/"]
            };
            named_ref(reference, prefixes)?;
            let value = repository.find_reference(reference).map_err(error)?;
            output.push(format!("Reference: {reference}\n\n").as_bytes());
            if matches!(target, PanelTarget::Tag { .. })
                && let Ok(tag) = value.peel(ObjectType::Tag)
                && let Some(tag) = tag.as_tag()
            {
                output.push(tag.message_bytes().unwrap_or_default());
                output.push(b"\n\n");
            }
            let commit = value.peel_to_commit().map_err(error)?;
            append_commit(&mut output, &commit);
            let base = if matches!(target, PanelTarget::Branch { .. }) {
                base.map(|reference| resolve_base(&repository, reference))
                    .transpose()?
            } else {
                None
            };
            append_diff(&mut output, &commit_diff(&repository, &commit, base)?)?;
            reference.clone()
        }
        PanelTarget::Remote { name } => {
            if name.len() > 1024
                || name.chars().any(char::is_control)
                || !git2::Remote::is_valid_name(name)
            {
                return Err(PanelError::new("Invalid Git remote name"));
            }
            let remote = repository.find_remote(name).map_err(error)?;
            output.push(
                format!(
                    "Remote: {name}\nFetch: {}\nPush: {}\n",
                    display_url(remote.url().unwrap_or_default()),
                    display_url(
                        remote
                            .pushurl()
                            .map_err(error)?
                            .unwrap_or(remote.url().map_err(error)?)
                    )
                )
                .as_bytes(),
            );
            for refspec in remote.refspecs() {
                if !output.push(
                    format!(
                        "Refspec: {}\n",
                        refspec.str().unwrap_or("(non-UTF-8 refspec)")
                    )
                    .as_bytes(),
                ) {
                    break;
                }
            }
            return Ok(output.finish(format!("Remote · {name}"), PanelPreviewKind::Text));
        }
        _ => return Err(PanelError::new("Target is not a Git panel entry")),
    };
    Ok(output.finish(title, PanelPreviewKind::Diff))
}

fn change_preview(
    repository: &Repository,
    root: &Path,
    path: &str,
    staged: bool,
) -> Result<Option<PanelPreview>, PanelError> {
    let expected_path = files::relative(path, false)?;
    let diff = changes_diff(repository, staged, None)?;
    let Some((index, delta)) = diff.deltas().enumerate().find(|(_, delta)| {
        delta.new_file().path().or_else(|| delta.old_file().path()) == Some(expected_path)
    }) else {
        return Ok(None);
    };
    if !staged && delta.status() == git2::Delta::Untracked {
        return files::preview(root, path, MAX_PREVIEW_BYTES).map(Some);
    }
    let mut output = Output::new();
    append_change_patch(&mut output, repository, &diff, index, path, staged)?;
    Ok(Some(output.finish(
        format!("{} · {path}", if staged { "Staged" } else { "Unstaged" }),
        PanelPreviewKind::Diff,
    )))
}

fn append_change_patch(
    output: &mut Output,
    repository: &Repository,
    diff: &Diff<'_>,
    index: usize,
    path: &str,
    staged: bool,
) -> Result<(), PanelError> {
    let patch = Patch::from_diff(diff, index).map_err(error)?;
    let delta = patch
        .as_ref()
        .map(Patch::delta)
        .or_else(|| diff.get_delta(index))
        .ok_or_else(|| PanelError::new("Git change disappeared during inspection"))?;
    let input_limited = delta.old_file().size() > MAX_DIFF_FILE_BYTES as u64
        || delta.new_file().size() > MAX_DIFF_FILE_BYTES as u64;
    if let Some(mut patch) = patch {
        let result = patch.print(&mut |delta, _, line| {
            output.binary |= delta.old_file().is_binary() || delta.new_file().is_binary();
            if matches!(line.origin(), '+' | '-' | ' ') && !output.push(&[line.origin() as u8]) {
                return false;
            }
            output.push(line.content())
        });
        if let Err(failure) = result
            && !output.truncated
        {
            return Err(error(failure));
        }
    } else {
        append_diff(output, &changes_diff(repository, staged, Some(path))?)?;
    }
    if input_limited {
        output.push(b"\nFile content omitted: exceeds the 2 MiB diff input limit.\n");
        output.truncated = true;
    }
    Ok(())
}

pub(super) fn command_diff(root: &Path, path: &str) -> Result<crate::git::FilePreview, PanelError> {
    let scope = files::relative(path, true)?;
    let repository = open(root)?;
    let mut content = String::new();
    let mut binary = false;
    let mut truncated = false;
    for staged in [true, false] {
        let diff = changes_diff(&repository, staged, None)?;
        let mut output = Output::new();
        for (index, delta) in diff.deltas().enumerate() {
            if delta.status() == git2::Delta::Untracked {
                continue;
            }
            let current = delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())
                .ok_or_else(|| PanelError::new("Git change has no path"))?;
            let matches = current == scope
                || current.starts_with(scope)
                || delta.status() == git2::Delta::Renamed
                    && delta
                        .old_file()
                        .path()
                        .is_some_and(|old| old == scope || old.starts_with(scope));
            if !matches {
                continue;
            }
            let current = current
                .to_str()
                .ok_or_else(|| PanelError::new("Git change path must be UTF-8"))?;
            files::relative(current, false)?;
            append_change_patch(&mut output, &repository, &diff, index, current, staged)?;
            if output.truncated {
                break;
            }
        }
        if output.bytes.is_empty() {
            continue;
        }
        let preview = output.finish(format!("diff {path}"), PanelPreviewKind::Diff);
        if !content.is_empty() {
            content.push('\n');
        }
        content.push_str(if staged { "# staged\n" } else { "# unstaged\n" });
        content.push_str(&preview.body);
        if preview.truncated {
            content.push_str("\n\n... diff truncated ...");
        }
        binary |= preview.binary;
        truncated |= preview.truncated;
    }
    if content.is_empty() {
        return files::command_preview(root, path, 80_000);
    }
    Ok(crate::git::FilePreview {
        title: format!("diff {path}"),
        content,
        truncated,
        binary,
    })
}
