//! Repository-bound read adapters for the single native review/workbench loop.
//! Native planning is queried through workdeck-pm; historical session annotations
//! remain inert. A panel failure does not acquire terminal or process ownership.
mod files;
mod git;
mod history;
mod registered;

use crate::{
    config::Config,
    search::{SearchIndex, SearchRecord, SearchResult, SearchTarget},
    store::WorkdeckStore,
};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};
use workdeck_pm::{ErrorCode, PlanningKind, Repository};
use workdeck_tui::workbench::*;

const MAX_ENTRIES: usize = 10_000;
const MAX_PREVIEW_BYTES: usize = 512 * 1024;

#[derive(Debug)]
pub struct RepositoryPanels {
    root: PathBuf,
    identity: String,
    planning_binding: PlanningBinding,
    base: Mutex<Option<String>>,
    base_key: Option<String>,
    recent_limit: usize,
}

impl Clone for RepositoryPanels {
    fn clone(&self) -> Self {
        Self {
            root: self.root.clone(),
            identity: self.identity.clone(),
            planning_binding: self.planning_binding.clone(),
            base: Mutex::new(self.base()),
            base_key: self.base_key.clone(),
            recent_limit: self.recent_limit,
        }
    }
}

/// Typed compatibility projection of the same read-only Git panel data.
#[derive(Debug)]
pub struct ChangeSnapshot {
    pub snapshot: crate::git::RepoSnapshot,
    pub truncated: bool,
    pub old_paths: std::collections::BTreeMap<PathBuf, PathBuf>,
}

pub(super) enum PlanningSource {
    Native(Repository),
    Legacy(PathBuf),
    Empty,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PlanningBinding {
    Native {
        repository: workdeck_pm::RepositoryId,
        directory: String,
    },
    Legacy {
        directory: String,
    },
    Empty,
    Unavailable,
}

impl PlanningBinding {
    fn from_source(source: &PlanningSource) -> Result<Self, PanelError> {
        match source {
            PlanningSource::Native(repository) => Ok(Self::Native {
                repository: repository.identity().clone(),
                directory: root_identity(repository.root())?,
            }),
            PlanningSource::Legacy(root) => Ok(Self::Legacy {
                directory: root_identity(root)?,
            }),
            PlanningSource::Empty => Ok(Self::Empty),
        }
    }

    fn identity(&self) -> String {
        match self {
            Self::Native {
                repository,
                directory,
            } => format!("native:{repository}:{directory}"),
            Self::Legacy { directory } => format!("legacy:{directory}"),
            Self::Empty => "empty".into(),
            Self::Unavailable => "unavailable".into(),
        }
    }
}

impl RepositoryPanels {
    /// Bounded preview for headless clients; the caller's limit is capped at
    /// the panel limit. Existing metadata-only file categories are retained.
    pub fn file_preview(
        &self,
        path: &Path,
        max_bytes: usize,
    ) -> Result<crate::git::FilePreview, PanelError> {
        self.verify_root()?;
        let relative = if path.is_absolute() {
            path.strip_prefix(&self.root).map_err(error)?
        } else {
            path
        };
        let relative = relative.strip_prefix(".").unwrap_or(relative);
        let mut result = files::command_preview(
            &self.root,
            relative
                .to_str()
                .ok_or_else(|| PanelError::new("File path must be UTF-8"))?,
            max_bytes.min(MAX_PREVIEW_BYTES),
        )?;
        result.title = path.display().to_string();
        self.verify_root()?;
        Ok(result)
    }

    /// Merged staged/unstaged status, bounded to 10,000 deltas. Truncation also
    /// reports omitted statistics for files over the 2 MiB diff input limit.
    pub fn change_snapshot(&self) -> Result<ChangeSnapshot, PanelError> {
        self.verify_root()?;
        let snapshot = git::snapshot(&self.root)?;
        self.verify_root()?;
        Ok(snapshot)
    }

    /// Literal file/directory diffs, capped at 512 KiB per stage. Unchanged or
    /// untracked paths fall back to an 80,000-byte file preview. Truncation flags
    /// report omitted content; labels and small truncation markers are extra.
    pub fn change_preview(&self, path: &Path) -> Result<crate::git::FilePreview, PanelError> {
        self.verify_root()?;
        let relative = if path.is_absolute() {
            path.strip_prefix(&self.root).map_err(error)?
        } else {
            path
        };
        let relative = relative.strip_prefix(".").unwrap_or(relative);
        let result = git::command_diff(
            &self.root,
            relative
                .to_str()
                .ok_or_else(|| PanelError::new("Git path must be UTF-8"))?,
        )?;
        self.verify_root()?;
        Ok(result)
    }

    /// Immutable source bytes for workbench file navigation. File comparison
    /// consumes this snapshot without reopening a possibly replaced path.
    pub fn read_review_file(root: &Path, path: &Path) -> Result<Vec<u8>, PanelError> {
        Self::new(root, None, 1)?.read_file_for_navigation(path)
    }

    pub fn read_file_for_navigation(&self, path: &Path) -> Result<Vec<u8>, PanelError> {
        self.verify_root()?;
        let path = if path.is_absolute() {
            path.strip_prefix(&self.root).map_err(error)?
        } else {
            path
        };
        let path = path
            .to_str()
            .ok_or_else(|| PanelError::new("File path must be UTF-8"))?;
        let read = files::read(&self.root, path, 16 * 1024 * 1024)?;
        if read.truncated {
            return Err(PanelError::new(
                "Workbench file navigation is limited to 16 MiB per source file",
            ));
        }
        self.verify_root()?;
        Ok(read.bytes)
    }

    pub fn new(
        root: impl AsRef<Path>,
        base: Option<String>,
        recent_limit: usize,
    ) -> Result<Self, PanelError> {
        let root = root.as_ref().canonicalize().map_err(error)?;
        if !root.is_dir() {
            return Err(PanelError::new("Repository panel root must be a directory"));
        }
        let identity = root_identity(&root)?;
        // Capture source identity without making planning diagnostics fatal to
        // Files/Review. A repaired or newly initialized source needs a new
        // provider, just like a replaced existing authority.
        let planning_binding = resolve_planning(&root)
            .and_then(|source| PlanningBinding::from_source(&source))
            .unwrap_or(PlanningBinding::Unavailable);
        Ok(Self {
            root,
            identity,
            planning_binding,
            base: Mutex::new(base),
            base_key: None,
            recent_limit: recent_limit.clamp(1, MAX_ENTRIES),
        })
    }

    /// Session-local comparison base shared by Git panel loads and previews.
    fn base(&self) -> Option<String> {
        self.base
            .lock()
            .unwrap_or_else(|failure| failure.into_inner())
            .clone()
    }

    /// Bind the validated `keys.base` setting used by the workbench Git page.
    /// A binding that cannot be normalized, or that the panel matcher could
    /// never serve (a chord such as shift-tab, or an uppercase letter that
    /// only arrives modified), disables cycling for this provider.
    pub fn with_git_base_key(mut self, key: Option<String>) -> Self {
        self.base_key = key.and_then(|binding| {
            let normalized = crate::config::normalize_key(&binding).ok()?;
            let served = matches!(normalized.as_str(), "tab" | "enter" | "esc" | "space")
                || (normalized.chars().count() == 1
                    && !normalized.contains(|character: char| character.is_ascii_uppercase()));
            served.then_some(normalized)
        });
        self
    }

    /// Rotate the Git panel's comparison base through the repository's
    /// branches. The setting is session-local, so a read-only workbench never
    /// writes repository configuration; subsequent loads and previews still
    /// use the selected value for this provider.
    fn rotate_base_branch(&self) -> Result<String, PanelError> {
        self.verify_root()?;
        let repository = git::open(&self.root)?;
        let current_branch = repository
            .head()
            .ok()
            .and_then(|head| head.shorthand().ok().map(str::to_owned))
            .unwrap_or_default();
        let mut branches = Vec::<String>::new();
        for (count, branch) in repository.branches(None).map_err(error)?.enumerate() {
            if count == MAX_ENTRIES {
                break;
            }
            let (branch, _) = branch.map_err(error)?;
            let reference = branch.get().name().map_err(error)?.to_owned();
            branches.push(
                branch
                    .name()
                    .map_err(error)?
                    .unwrap_or(&reference)
                    .to_owned(),
            );
        }
        let selected = self.base();
        let mut candidates = Vec::<String>::new();
        let mut seen = std::collections::HashSet::new();
        if let Some(base) = selected
            .as_deref()
            .map(str::trim)
            .filter(|base| !base.is_empty() && *base != current_branch)
        {
            candidates.push(base.to_owned());
            seen.insert(base.to_owned());
        }
        for branch in branches {
            if branch == current_branch || !seen.insert(branch.clone()) {
                continue;
            }
            candidates.push(branch);
        }
        let Some(first) = candidates.first() else {
            return Err(PanelError::new("no alternate base branches available"));
        };
        let next = selected
            .as_ref()
            .and_then(|base| candidates.iter().position(|candidate| candidate == base))
            .map_or_else(
                || first.clone(),
                |index| candidates[(index + 1) % candidates.len()].clone(),
            );
        // Recheck the root before publishing so a failed cycle never mutates
        // the session-local selection.
        self.verify_root()?;
        *self
            .base
            .lock()
            .unwrap_or_else(|failure| failure.into_inner()) = Some(next.clone());
        Ok(next)
    }

    fn verify_root(&self) -> Result<(), PanelError> {
        if root_identity(&self.root)? != self.identity {
            return Err(PanelError::new(
                "The workbench repository directory changed; reopen Workdeck for the new source",
            ));
        }
        Ok(())
    }

    fn planning(&self) -> Result<PlanningSource, PanelError> {
        let source = resolve_planning(&self.root)?;
        if PlanningBinding::from_source(&source)? != self.planning_binding {
            return Err(PanelError::new(
                "The planning authority changed; reopen Workdeck to select the new source",
            ));
        }
        Ok(source)
    }

    /// Shared neutral search output used by terminal and headless adapters.
    pub fn search(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<(Vec<SearchResult>, bool), PanelError> {
        self.search_matching(query, max_results, |_| true)
    }

    /// Restrict eligible targets before ranking and result truncation. Scan
    /// coverage remains explicit even when a target group has few matches.
    pub fn search_matching(
        &self,
        query: &str,
        max_results: usize,
        matches_target: impl Fn(&SearchTarget) -> bool,
    ) -> Result<(Vec<SearchResult>, bool), PanelError> {
        self.verify_root()?;
        let (paths, mut truncated) = files::inventory(&self.root)?;
        let mut records = paths
            .iter()
            .map(|path| {
                let label = path.to_string_lossy().into_owned();
                SearchRecord {
                    haystack: label.clone(),
                    label,
                    detail: "file".into(),
                    target: SearchTarget::File(path.clone()),
                }
            })
            .collect::<Vec<_>>();
        let request = PanelRequest {
            page: PanelPage::Changes,
            directory: String::new(),
            query: String::new(),
            limit: MAX_ENTRIES,
        };
        match git::changes(&self.root, &request) {
            Ok(snapshot) => {
                truncated |= snapshot.truncated;
                records.extend(snapshot.entries.into_iter().filter_map(search_record));
            }
            Err(failure) if self.root.join(".git").exists() => return Err(failure),
            Err(_) => {} // Non-Git repositories still have file/planning search.
        }
        let request = PanelRequest {
            page: PanelPage::Git,
            ..request
        };
        match git::overview(
            &self.root,
            self.base().as_deref(),
            self.recent_limit,
            &request,
        ) {
            Ok(snapshot) => {
                truncated |= snapshot.truncated;
                records.extend(snapshot.entries.into_iter().filter_map(search_record));
            }
            Err(failure) if self.root.join(".git").exists() => return Err(failure),
            Err(_) => {}
        }
        let source = self.planning()?;
        match &source {
            PlanningSource::Native(repository) => {
                for issue in repository
                    .query_issues(&workdeck_pm::IssueQuery::default())
                    .map_err(pm_error)?
                {
                    let label = format!("{} {}", issue.metadata.id, issue.metadata.title);
                    let detail = format!("issue {}", issue.metadata.status);
                    records.push(SearchRecord {
                        haystack: format!(
                            "{label} {detail} {} {}",
                            issue.body,
                            serde_json::to_string(&issue.metadata).map_err(error)?
                        ),
                        label,
                        detail,
                        target: SearchTarget::Issue(issue.metadata.id.to_string()),
                    });
                }
                for kind in [
                    PlanningKind::Initiative,
                    PlanningKind::Project,
                    PlanningKind::Milestone,
                    PlanningKind::Cycle,
                    PlanningKind::Target,
                    PlanningKind::Label,
                ] {
                    for record in repository.list_planning(kind).map_err(pm_error)? {
                        if record.metadata.archived {
                            continue;
                        }
                        let id = record.metadata.id.clone();
                        let target = match kind {
                            PlanningKind::Initiative => SearchTarget::Initiative(id),
                            PlanningKind::Project => SearchTarget::Project(id),
                            PlanningKind::Milestone => SearchTarget::Milestone(id),
                            PlanningKind::Cycle => SearchTarget::Cycle(id),
                            PlanningKind::Target => SearchTarget::Target(id),
                            PlanningKind::Label => SearchTarget::Label(id),
                        };
                        let label = format!("{} {}", record.metadata.id, record.metadata.name);
                        let detail = format!("{kind:?}").to_lowercase();
                        records.push(SearchRecord {
                            haystack: format!(
                                "{label} {detail} {} {}",
                                record.body,
                                serde_json::to_string(&record.metadata).map_err(error)?
                            ),
                            label,
                            detail,
                            target,
                        });
                    }
                }
            }
            PlanningSource::Legacy(path) => {
                let store = WorkdeckStore::new(path);
                // The old index builder is retained only for actual legacy
                // models; native records above never lose configured states.
                records.extend(
                    SearchIndex::rebuild(
                        &[],
                        &[],
                        &store.load_issues().map_err(error)?,
                        &[],
                        &store.load_reference_data().map_err(error)?,
                        &[],
                        None,
                    )
                    .query("", usize::MAX)
                    .into_iter()
                    .map(|result| result.record),
                );
            }
            PlanningSource::Empty => {}
        }
        let sessions = history::load(&source)?;
        for session in sessions {
            let label = format!("{} {}", session.id, session.title);
            let detail = format!("recorded agent {} {}", session.agent, session.status);
            records.push(SearchRecord {
                haystack: format!(
                    "{label} {detail} {}",
                    serde_json::to_string(&session).map_err(error)?
                ),
                label,
                detail,
                target: SearchTarget::AgentSession(session.id),
            });
        }
        // The extractor consumes bounded descriptor reads, never follows a
        // symlink, and reports the intentionally bounded symbol coverage.
        let symbol_paths = paths
            .iter()
            .filter(|path| crate::search::is_symbol_source(path))
            .collect::<Vec<_>>();
        truncated |= symbol_paths.len() > 1000;
        for path in symbol_paths.into_iter().take(1000) {
            let text = files::read(
                &self.root,
                path.to_str().expect("inventory paths are UTF-8"),
                64 * 1024,
            )?;
            truncated |= text.truncated;
            if text.bytes.contains(&0) {
                continue;
            }
            let Ok(text) = std::str::from_utf8(&text.bytes) else {
                continue;
            };
            let symbols = crate::search::symbols_in_content(path, text, 33);
            truncated |= symbols.len() > 32;
            for symbol in symbols.into_iter().take(32) {
                let label = format!("{}:{} {}", path.display(), symbol.line, symbol.name);
                records.push(SearchRecord {
                    haystack: format!("{label} {}", symbol.name.replace(['_', '-'], " ")),
                    label,
                    detail: format!("symbol {}", symbol.kind),
                    target: SearchTarget::Symbol {
                        path: path.clone(),
                        line: symbol.line,
                        name: symbol.name,
                    },
                });
            }
        }
        let max_results = max_results.clamp(1, MAX_ENTRIES);
        let mut seen = std::collections::BTreeSet::new();
        records.retain(|record| {
            matches_target(&record.target) && seen.insert(format!("{:?}", record.target))
        });
        let mut results = SearchIndex::from_records(records).query(query, max_results + 1);
        truncated |= results.len() > max_results;
        results.truncate(max_results);
        self.planning()?;
        self.verify_root()?;
        Ok((results, truncated))
    }
}

impl RepositoryPanelProvider for RepositoryPanels {
    fn source(&self) -> RepositoryPanelSource {
        RepositoryPanelSource {
            root: self.root.clone(),
            identity: format!(
                "{}:planning={}",
                self.identity,
                self.planning_binding.identity()
            ),
        }
    }
    fn git_base_key(&self) -> Option<String> {
        self.base_key.clone()
    }
    fn cycle_git_base_branch(&self) -> Result<String, PanelError> {
        self.rotate_base_branch()
    }
    fn load(&self, request: &PanelRequest) -> Result<PanelSnapshot, PanelError> {
        self.verify_root()?;
        let snapshot = match request.page {
            PanelPage::Changes => git::changes(&self.root, request)?,
            PanelPage::Git => git::overview(
                &self.root,
                self.base().as_deref(),
                self.recent_limit,
                request,
            )?,
            PanelPage::Files => files::list(&self.root, request)?,
            PanelPage::Agents => history::list(&self.planning()?, request)?,
            PanelPage::Search => {
                let (results, truncated) = self.search(&request.query, limit(request))?;
                let entries = results
                    .into_iter()
                    .map(|result| {
                        let target = panel_target(result.record.target);
                        entry(
                            format!("{target:?}"),
                            result.record.label,
                            result.record.detail,
                            "",
                            target,
                        )
                    })
                    .collect::<Vec<_>>();
                PanelSnapshot {
                    page: PanelPage::Search,
                    title: "Search".into(),
                    summary: format!("{} results", entries.len()),
                    entries,
                    truncated,
                }
            }
        };
        if matches!(request.page, PanelPage::Agents | PanelPage::Search) {
            self.planning()?;
        }
        self.verify_root()?;
        Ok(snapshot)
    }
    fn preview(&self, target: &PanelTarget) -> Result<PanelPreview, PanelError> {
        self.verify_root()?;
        let preview = match target {
            PanelTarget::File { path, .. } => files::preview(&self.root, path, MAX_PREVIEW_BYTES)?,
            PanelTarget::Directory { path } => {
                let snapshot = files::list(
                    &self.root,
                    &PanelRequest {
                        page: PanelPage::Files,
                        directory: path.clone(),
                        query: String::new(),
                        limit: 200,
                    },
                )?;
                PanelPreview {
                    title: path.clone(),
                    body: snapshot
                        .entries
                        .iter()
                        .map(|row| format!("{}  {}", row.label, row.detail))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    truncated: snapshot.truncated,
                    binary: false,
                    kind: PanelPreviewKind::Text,
                }
            }
            PanelTarget::AgentSession { id } => history::preview(&self.planning()?, id)?,
            PanelTarget::Issue { id } => match self.planning()? {
                PlanningSource::Native(repository) => {
                    let record = repository.show_issue(id).map_err(pm_error)?;
                    text_preview(
                        format!("{} {}", record.metadata.id, record.metadata.title),
                        format!(
                            "{}\n\n{}",
                            serde_json::to_string_pretty(&record.metadata).map_err(error)?,
                            record.body
                        ),
                        PanelPreviewKind::Markdown,
                    )
                }
                PlanningSource::Legacy(path) => {
                    let issue = WorkdeckStore::new(path)
                        .load_issues()
                        .map_err(error)?
                        .into_iter()
                        .find(|issue| issue.key == *id)
                        .ok_or_else(|| PanelError::new("Issue no longer exists"))?;
                    text_preview(
                        issue.title.clone(),
                        serde_json::to_string_pretty(&issue).map_err(error)?,
                        PanelPreviewKind::Text,
                    )
                }
                PlanningSource::Empty => {
                    return Err(PanelError::new("Project management is not initialized"));
                }
            },
            PanelTarget::Initiative { id }
            | PanelTarget::Project { id }
            | PanelTarget::Milestone { id }
            | PanelTarget::Cycle { id }
            | PanelTarget::Target { id }
            | PanelTarget::Label { id } => match self.planning()? {
                PlanningSource::Native(repository) => {
                    let kind = match target {
                        PanelTarget::Initiative { .. } => PlanningKind::Initiative,
                        PanelTarget::Project { .. } => PlanningKind::Project,
                        PanelTarget::Milestone { .. } => PlanningKind::Milestone,
                        PanelTarget::Cycle { .. } => PlanningKind::Cycle,
                        PanelTarget::Target { .. } => PlanningKind::Target,
                        PanelTarget::Label { .. } => PlanningKind::Label,
                        _ => unreachable!("planning target was matched above"),
                    };
                    let record = repository.planning_record(kind, id).map_err(pm_error)?;
                    text_preview(
                        record.metadata.name.clone(),
                        format!(
                            "{}\n\n{}",
                            serde_json::to_string_pretty(&record.metadata).map_err(error)?,
                            record.body
                        ),
                        PanelPreviewKind::Markdown,
                    )
                }
                source => {
                    if matches!(
                        target,
                        PanelTarget::Initiative { .. }
                            | PanelTarget::Milestone { .. }
                            | PanelTarget::Target { .. }
                    ) {
                        return Err(PanelError::new(
                            "Initiatives, milestones and targets require native project management",
                        ));
                    }
                    history::legacy_reference_preview(&source, target)?
                }
            },
            _ => git::preview(&self.root, self.base().as_deref(), target)?,
        };
        if matches!(
            target,
            PanelTarget::AgentSession { .. }
                | PanelTarget::Issue { .. }
                | PanelTarget::Initiative { .. }
                | PanelTarget::Project { .. }
                | PanelTarget::Milestone { .. }
                | PanelTarget::Cycle { .. }
                | PanelTarget::Target { .. }
                | PanelTarget::Label { .. }
        ) {
            self.planning()?;
        }
        self.verify_root()?;
        Ok(preview)
    }
}

fn resolve_planning(root: &Path) -> Result<PlanningSource, PanelError> {
    let config = Config::load(root).map_err(error)?;
    match Repository::discover(root) {
        Ok(repository) => {
            if repository.root().parent() != Some(root) {
                return Err(PanelError::new(
                    "The planning authority belongs to a different repository root",
                ));
            }
            Ok(PlanningSource::Native(repository))
        }
        Err(failure)
            if matches!(
                failure.code,
                ErrorCode::NotInitialized | ErrorCode::LegacyStore
            ) =>
        {
            if config
                .startup_notices
                .iter()
                .any(|notice| notice.key == "planning:source-unavailable")
            {
                return Err(pm_error(failure));
            }
            let path = config.data_dir(root);
            if path != root.join(".workdeck")
                && [
                    "issues",
                    "projects.toml",
                    "cycles.toml",
                    "labels.toml",
                    "agents",
                    "events.jsonl",
                ]
                .iter()
                .any(|name| std::fs::symlink_metadata(path.join(name)).is_ok())
            {
                // Repository-local paths may not gain an outside authority by
                // replacing a parent component with a symlink. Explicit custom
                // roots remain supported and are pinned to their canonical
                // directory identity after rejecting a symlink root itself.
                root_identity(&path)?;
                if let Ok(relative) = path.strip_prefix(root) {
                    // An explicitly configured relative custom root may use
                    // ../ to select a sibling store. Ordinary repository-local
                    // paths still validate every directory component in place.
                    if !config.explicit_data_dir
                        || relative
                            .components()
                            .all(|part| matches!(part, std::path::Component::Normal(_)))
                    {
                        files::directory(
                            root,
                            relative.to_str().ok_or_else(|| {
                                PanelError::new("Legacy source path must be UTF-8")
                            })?,
                        )?;
                    }
                }
                return Ok(PlanningSource::Legacy(path.canonicalize().map_err(error)?));
            }
            if failure.code == ErrorCode::LegacyStore {
                return Err(pm_error(failure));
            }
            Ok(PlanningSource::Empty)
        }
        Err(failure) => Err(pm_error(failure)),
    }
}

fn root_identity(root: &Path) -> Result<String, PanelError> {
    let metadata = std::fs::symlink_metadata(root).map_err(error)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(PanelError::new("The repository directory was replaced"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(format!(
            "{}:{}:{}",
            root.display(),
            metadata.dev(),
            metadata.ino()
        ))
    }
    #[cfg(not(unix))]
    {
        Ok(root.to_string_lossy().into_owned())
    }
}
fn error(value: impl std::fmt::Display) -> PanelError {
    PanelError::new(value.to_string())
}
fn pm_error(value: workdeck_pm::PmError) -> PanelError {
    PanelError {
        message: value.message,
        hint: value.hint,
    }
}
fn limit(request: &PanelRequest) -> usize {
    request.limit.clamp(1, MAX_ENTRIES)
}
fn entry(
    id: String,
    label: String,
    detail: String,
    section: &str,
    target: PanelTarget,
) -> PanelEntry {
    PanelEntry {
        id,
        label,
        detail,
        section: section.into(),
        target,
        changes: None,
    }
}
fn text_preview(title: String, mut body: String, kind: PanelPreviewKind) -> PanelPreview {
    let truncated = body.len() > MAX_PREVIEW_BYTES;
    if truncated {
        let mut boundary = MAX_PREVIEW_BYTES;
        while !body.is_char_boundary(boundary) {
            boundary -= 1;
        }
        body.truncate(boundary);
    }
    PanelPreview {
        title,
        body,
        kind,
        truncated,
        binary: false,
    }
}
fn search_record(entry: PanelEntry) -> Option<SearchRecord> {
    let target = match entry.target {
        PanelTarget::Change { path, staged: true } => SearchTarget::StagedChange(path.into()),
        PanelTarget::Change {
            path,
            staged: false,
        } => SearchTarget::Change(path.into()),
        PanelTarget::Commit { reference } => SearchTarget::GitCommit(reference),
        PanelTarget::Branch { reference } => SearchTarget::GitBranch(reference),
        PanelTarget::Stash { reference } => SearchTarget::GitStash(reference),
        PanelTarget::Tag { reference } => SearchTarget::GitTag(reference),
        _ => return None,
    };
    Some(SearchRecord {
        haystack: format!("{} {}", entry.label, entry.detail),
        label: entry.label,
        detail: entry.detail,
        target,
    })
}
fn panel_target(target: SearchTarget) -> PanelTarget {
    match target {
        SearchTarget::File(path) => PanelTarget::File {
            path: path.to_string_lossy().into_owned(),
            line: None,
        },
        SearchTarget::Change(path) => PanelTarget::Change {
            path: path.to_string_lossy().into_owned(),
            staged: false,
        },
        SearchTarget::StagedChange(path) => PanelTarget::Change {
            path: path.to_string_lossy().into_owned(),
            staged: true,
        },
        SearchTarget::Issue(id) => PanelTarget::Issue { id },
        SearchTarget::AgentSession(id) => PanelTarget::AgentSession { id },
        SearchTarget::GitCommit(reference) => PanelTarget::Commit { reference },
        SearchTarget::GitBranch(reference) => PanelTarget::Branch { reference },
        SearchTarget::GitStash(reference) => PanelTarget::Stash { reference },
        SearchTarget::GitTag(reference) => PanelTarget::Tag { reference },
        SearchTarget::Project(id) => PanelTarget::Project { id },
        SearchTarget::Initiative(id) => PanelTarget::Initiative { id },
        SearchTarget::Milestone(id) => PanelTarget::Milestone { id },
        SearchTarget::Cycle(id) => PanelTarget::Cycle { id },
        SearchTarget::Target(id) => PanelTarget::Target { id },
        SearchTarget::Label(id) => PanelTarget::Label { id },
        SearchTarget::Symbol { path, line, .. } => PanelTarget::File {
            path: path.to_string_lossy().into_owned(),
            line: u32::try_from(line).ok(),
        },
    }
}
