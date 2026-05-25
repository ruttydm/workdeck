use crate::config::Config;
use crate::git::{self, ChangeEntry, FilePreview, RepoSnapshot};
use crate::search::{SearchIndex, SearchResult, SearchTarget, SymbolRecord};
use crate::store::{AgentSession, Issue, ReferenceData, WorkdeckStore};
use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Changes,
    Files,
    Tasks,
    Agents,
    Search,
}

impl Tab {
    pub const ALL: [Tab; 5] = [
        Tab::Changes,
        Tab::Files,
        Tab::Tasks,
        Tab::Agents,
        Tab::Search,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Changes => "Changes",
            Tab::Files => "Files",
            Tab::Tasks => "Tasks",
            Tab::Agents => "Agents",
            Tab::Search => "Search",
        }
    }

    pub fn next(self) -> Self {
        let index = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    pub fn previous(self) -> Self {
        let index = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0);
        Self::ALL[(index + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    Narrow,
    Medium,
    Wide,
}

impl LayoutMode {
    pub fn for_width(width: u16) -> Self {
        if width < 70 {
            Self::Narrow
        } else if width < 110 {
            Self::Medium
        } else {
            Self::Wide
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPane {
    Tree,
    Preview,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRow {
    pub path: PathBuf,
    pub depth: usize,
    pub kind: TreeRowKind,
    pub file_index: Option<usize>,
    pub collapsed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeRowKind {
    Directory,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileBrowserEntry {
    pub path: PathBuf,
    pub name: String,
    pub kind: FileBrowserEntryKind,
    pub file_index: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileBrowserEntryKind {
    Parent,
    Directory,
    File,
}

#[derive(Debug)]
pub struct App {
    pub cwd: PathBuf,
    pub repo_root: PathBuf,
    pub config: Config,
    pub store: WorkdeckStore,
    pub active_tab: Tab,
    pub preview_visible: bool,
    pub focus: FocusPane,
    pub preview_scroll: usize,
    pub collapsed_change_dirs: BTreeSet<PathBuf>,
    pub collapsed_file_dirs: BTreeSet<PathBuf>,
    pub change_grouping: ChangeGrouping,
    pub dirstat_visible: bool,
    pub help_visible: bool,
    pub search_query: String,
    pub status_message: String,
    pub loading: bool,
    pub refresh_generation: u64,
    pub changes: Vec<ChangeEntry>,
    pub snapshot: Option<RepoSnapshot>,
    pub files: Vec<PathBuf>,
    pub issues: Vec<Issue>,
    pub sessions: Vec<AgentSession>,
    pub reference_data: ReferenceData,
    pub symbols: Vec<SymbolRecord>,
    pub search_index: SearchIndex,
    pub search_results: Vec<SearchResult>,
    pub preview_cache: Option<PreviewCache>,
    pub preview_loading: Option<PreviewTarget>,
    pub selected_change: usize,
    pub selected_change_row: usize,
    pub selected_file: usize,
    pub selected_file_row: usize,
    pub files_cwd: PathBuf,
    pub selected_file_entry: usize,
    pub file_browser_scroll: usize,
    pub selected_issue: usize,
    pub selected_session: usize,
    pub selected_search: usize,
}

#[derive(Debug, Clone)]
pub struct PreviewCache {
    pub target: PreviewTarget,
    pub preview: FilePreview,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewTarget {
    pub tab: Tab,
    pub path: PathBuf,
    pub kind: PreviewKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewKind {
    File,
    Diff,
    Issue,
    Agent,
}

#[derive(Debug, Clone)]
pub struct PreviewData {
    pub target: PreviewTarget,
    pub preview: FilePreview,
}

#[derive(Debug)]
pub struct RefreshData {
    pub generation: u64,
    pub snapshot: RepoSnapshot,
    pub files: Vec<PathBuf>,
    pub issues: Vec<Issue>,
    pub sessions: Vec<AgentSession>,
    pub reference_data: ReferenceData,
    pub symbols: Vec<SymbolRecord>,
}

impl App {
    pub fn new(cwd: impl AsRef<Path>) -> Result<Self> {
        let cwd = cwd.as_ref().to_path_buf();
        let repo_root = git::discover_repo_root(&cwd)?;
        let config = Config::load(&repo_root)?;
        let preview_visible = config.ui.preview;
        let store = WorkdeckStore::new(config.data_dir(&repo_root));
        Ok(Self {
            cwd,
            repo_root,
            config,
            store,
            active_tab: Tab::Changes,
            preview_visible,
            focus: FocusPane::Tree,
            preview_scroll: 0,
            collapsed_change_dirs: BTreeSet::new(),
            collapsed_file_dirs: BTreeSet::new(),
            change_grouping: ChangeGrouping::Directory,
            dirstat_visible: true,
            help_visible: false,
            search_query: String::new(),
            status_message: "loading".to_string(),
            loading: false,
            refresh_generation: 0,
            changes: Vec::new(),
            snapshot: None,
            files: Vec::new(),
            issues: Vec::new(),
            sessions: Vec::new(),
            reference_data: ReferenceData::default(),
            symbols: Vec::new(),
            search_index: SearchIndex::default(),
            search_results: Vec::new(),
            preview_cache: None,
            preview_loading: None,
            selected_change: 0,
            selected_change_row: 0,
            selected_file: 0,
            selected_file_row: 0,
            files_cwd: PathBuf::new(),
            selected_file_entry: 0,
            file_browser_scroll: 0,
            selected_issue: 0,
            selected_session: 0,
            selected_search: 0,
        })
    }

    pub fn load(cwd: impl AsRef<Path>) -> Result<Self> {
        let mut app = Self::new(cwd)?;
        app.refresh()?;
        Ok(app)
    }

    pub fn refresh(&mut self) -> Result<()> {
        let generation = self.begin_refresh();
        let data = self.load_refresh_data(generation)?;
        self.apply_refresh_data(data);
        Ok(())
    }

    pub fn begin_refresh(&mut self) -> u64 {
        self.refresh_generation = self.refresh_generation.saturating_add(1);
        self.loading = true;
        self.status_message = "loading".to_string();
        self.refresh_generation
    }

    pub fn load_refresh_data(&self, generation: u64) -> Result<RefreshData> {
        let snapshot = git::scan_repo(&self.repo_root)?;
        let files = git::list_repo_files(&self.repo_root, 20_000)?;
        let issues = self.store.load_issues()?;
        let sessions = self.store.load_agent_sessions()?;
        let reference_data = self.store.load_reference_data()?;
        let symbols = crate::search::extract_symbols(&self.repo_root, &files);
        Ok(RefreshData {
            generation,
            snapshot,
            files,
            issues,
            sessions,
            reference_data,
            symbols,
        })
    }

    pub fn apply_refresh_data(&mut self, data: RefreshData) -> bool {
        if data.generation != self.refresh_generation {
            return false;
        }
        self.changes = data.snapshot.changes.clone();
        self.snapshot = Some(data.snapshot);
        self.files = data.files;
        self.issues = data.issues;
        self.sessions = data.sessions;
        self.reference_data = data.reference_data;
        self.symbols = data.symbols;
        self.rebuild_search();
        self.clamp_selections();
        self.sync_rows_from_selected_files();
        self.preview_cache = None;
        self.preview_loading = None;
        self.preview_scroll = 0;
        self.status_message = "refreshed".to_string();
        self.loading = false;
        true
    }

    pub fn apply_refresh_error(&mut self, generation: u64, error: String) -> bool {
        if generation != self.refresh_generation {
            return false;
        }
        self.loading = false;
        self.status_message = error;
        true
    }

    pub fn rebuild_search(&mut self) {
        self.search_index = SearchIndex::rebuild(
            &self.files,
            &self.changes,
            &self.issues,
            &self.sessions,
            &self.reference_data,
            &self.symbols,
        );
        self.search_results = self.search_index.query(&self.search_query, 100);
        if self.selected_search >= self.search_results.len() {
            self.selected_search = self.search_results.len().saturating_sub(1);
        }
    }

    pub fn selected_path(&self) -> Option<PathBuf> {
        match self.active_tab {
            Tab::Changes => self.selected_change_row_data().map(|row| row.path),
            Tab::Files => self.selected_file_browser_entry().map(|entry| entry.path),
            Tab::Tasks => self
                .issues
                .get(self.selected_issue)
                .and_then(|issue| issue.linked_files.first())
                .map(PathBuf::from),
            Tab::Agents => self
                .sessions
                .get(self.selected_session)
                .and_then(|session| session.touched_files.first())
                .map(|file| PathBuf::from(&file.path)),
            Tab::Search => self
                .search_results
                .get(self.selected_search)
                .and_then(|result| match &result.record.target {
                    SearchTarget::File(path) | SearchTarget::Change(path) => Some(path.clone()),
                    SearchTarget::Symbol { path, .. } => Some(path.clone()),
                    SearchTarget::Issue(key) => self
                        .issues
                        .iter()
                        .find(|issue| &issue.key == key)
                        .and_then(|issue| issue.linked_files.first())
                        .map(PathBuf::from),
                    SearchTarget::AgentSession(id) => self
                        .sessions
                        .iter()
                        .find(|session| &session.id == id)
                        .and_then(|session| session.touched_files.first())
                        .map(|file| PathBuf::from(&file.path)),
                    SearchTarget::Project(id) => self
                        .issues
                        .iter()
                        .find(|issue| &issue.project == id)
                        .and_then(|issue| issue.linked_files.first())
                        .map(PathBuf::from),
                    SearchTarget::Cycle(id) => self
                        .issues
                        .iter()
                        .find(|issue| &issue.cycle == id)
                        .and_then(|issue| issue.linked_files.first())
                        .map(PathBuf::from),
                    SearchTarget::Label(id) => self
                        .issues
                        .iter()
                        .find(|issue| issue.labels.iter().any(|label| label == id))
                        .and_then(|issue| issue.linked_files.first())
                        .map(PathBuf::from),
                }),
        }
    }

    pub fn selected_preview(&self) -> Option<FilePreview> {
        let target = self.preview_target()?;
        match target.kind {
            PreviewKind::Issue => {
                let key = target.path.to_string_lossy();
                return self
                    .issues
                    .iter()
                    .find(|issue| issue.key == key)
                    .map(issue_preview);
            }
            PreviewKind::Agent => {
                let id = target.path.to_string_lossy();
                return self
                    .sessions
                    .iter()
                    .find(|session| session.id == id)
                    .map(agent_preview);
            }
            PreviewKind::File | PreviewKind::Diff => {}
        }
        self.preview_cache.as_ref().and_then(|cache| {
            if cache.target == target {
                Some(cache.preview.clone())
            } else {
                None
            }
        })
    }

    pub fn preview_target(&self) -> Option<PreviewTarget> {
        match self.active_tab {
            Tab::Changes => Some(PreviewTarget {
                tab: self.active_tab,
                path: self.selected_change_file_path()?,
                kind: PreviewKind::Diff,
            }),
            Tab::Files => Some(PreviewTarget {
                tab: self.active_tab,
                path: self.selected_file_browser_file_path()?,
                kind: PreviewKind::File,
            }),
            Tab::Tasks => {
                let issue = self.issues.get(self.selected_issue)?;
                Some(PreviewTarget {
                    tab: self.active_tab,
                    path: PathBuf::from(&issue.key),
                    kind: PreviewKind::Issue,
                })
            }
            Tab::Agents => {
                let session = self.sessions.get(self.selected_session)?;
                Some(PreviewTarget {
                    tab: self.active_tab,
                    path: PathBuf::from(&session.id),
                    kind: PreviewKind::Agent,
                })
            }
            Tab::Search => {
                let result = self.search_results.get(self.selected_search)?;
                match &result.record.target {
                    SearchTarget::Change(path) => Some(PreviewTarget {
                        tab: self.active_tab,
                        path: path.clone(),
                        kind: PreviewKind::Diff,
                    }),
                    SearchTarget::File(path) | SearchTarget::Symbol { path, .. } => {
                        Some(PreviewTarget {
                            tab: self.active_tab,
                            path: path.clone(),
                            kind: PreviewKind::File,
                        })
                    }
                    SearchTarget::Issue(key) => Some(PreviewTarget {
                        tab: self.active_tab,
                        path: PathBuf::from(key),
                        kind: PreviewKind::Issue,
                    }),
                    SearchTarget::AgentSession(id) => Some(PreviewTarget {
                        tab: self.active_tab,
                        path: PathBuf::from(id),
                        kind: PreviewKind::Agent,
                    }),
                    SearchTarget::Project(_) | SearchTarget::Cycle(_) | SearchTarget::Label(_) => {
                        self.selected_path().map(|path| PreviewTarget {
                            tab: self.active_tab,
                            path,
                            kind: PreviewKind::File,
                        })
                    }
                }
            }
        }
    }

    pub fn missing_preview_target(&self) -> Option<PreviewTarget> {
        if !self.preview_visible {
            return None;
        }

        let target = self.preview_target()?;
        if matches!(target.kind, PreviewKind::Issue | PreviewKind::Agent) {
            return None;
        }
        if self
            .preview_cache
            .as_ref()
            .is_some_and(|cache| cache.target == target)
            || self.preview_loading.as_ref() == Some(&target)
        {
            None
        } else {
            Some(target)
        }
    }

    pub fn mark_preview_loading(&mut self, target: PreviewTarget) {
        self.preview_loading = Some(target);
    }

    pub fn apply_preview_data(&mut self, data: PreviewData) {
        if self.preview_loading.as_ref() == Some(&data.target) {
            self.preview_loading = None;
        }

        if self.preview_target().as_ref() == Some(&data.target) {
            self.preview_cache = Some(PreviewCache {
                target: data.target,
                preview: data.preview,
            });
            self.clamp_preview_scroll(usize::MAX);
        }
    }

    pub fn apply_preview_error(&mut self, target: PreviewTarget, error: String) {
        if self.preview_loading.as_ref() == Some(&target) {
            self.preview_loading = None;
        }
        if self.preview_target().as_ref() == Some(&target) {
            self.status_message = format!("preview failed: {error}");
        }
    }

    pub fn move_down(&mut self) {
        if self.focus == FocusPane::Preview {
            self.scroll_preview_down(1, usize::MAX);
            return;
        }
        match self.active_tab {
            Tab::Changes => {
                let rows = self.change_tree_rows();
                increment(&mut self.selected_change_row, rows.len());
                self.sync_selected_change_from_row();
            }
            Tab::Files => {
                let rows = self.file_tree_rows();
                increment(&mut self.selected_file_row, rows.len());
                self.sync_selected_file_from_row();
            }
            Tab::Tasks => self.move_issue_selection(1),
            Tab::Agents => increment(&mut self.selected_session, self.sessions.len()),
            Tab::Search => increment(&mut self.selected_search, self.search_results.len()),
        }
    }

    pub fn move_up(&mut self) {
        if self.focus == FocusPane::Preview {
            self.scroll_preview_up(1);
            return;
        }
        match self.active_tab {
            Tab::Changes => {
                decrement(&mut self.selected_change_row);
                self.sync_selected_change_from_row();
            }
            Tab::Files => {
                decrement(&mut self.selected_file_row);
                self.sync_selected_file_from_row();
            }
            Tab::Tasks => self.move_issue_selection(-1),
            Tab::Agents => decrement(&mut self.selected_session),
            Tab::Search => decrement(&mut self.selected_search),
        }
    }

    pub fn move_file_browser_down(&mut self) {
        if self.focus == FocusPane::Preview {
            self.scroll_preview_down(1, usize::MAX);
            return;
        }
        let len = self.file_browser_entries().len();
        increment(&mut self.selected_file_entry, len);
        self.sync_file_row_from_browser();
    }

    pub fn move_file_browser_up(&mut self) {
        if self.focus == FocusPane::Preview {
            self.scroll_preview_up(1);
            return;
        }
        decrement(&mut self.selected_file_entry);
        self.sync_file_row_from_browser();
    }

    pub fn file_browser_top(&mut self) {
        self.selected_file_entry = 0;
        self.sync_file_row_from_browser();
    }

    pub fn file_browser_bottom(&mut self) {
        let len = self.file_browser_entries().len();
        self.selected_file_entry = len.saturating_sub(1);
        self.sync_file_row_from_browser();
    }

    pub fn activate_selected_file_browser_entry(&mut self) {
        let Some(entry) = self.selected_file_browser_entry() else {
            self.status_message = "no file selected".to_string();
            return;
        };
        match entry.kind {
            FileBrowserEntryKind::Parent => {
                self.move_file_browser_parent();
            }
            FileBrowserEntryKind::Directory => {
                self.files_cwd = entry.path.clone();
                self.selected_file_entry = 0;
                self.file_browser_scroll = 0;
                self.status_message = format!("folder {}", self.files_cwd.display());
                self.sync_file_row_from_browser();
            }
            FileBrowserEntryKind::File => {
                self.selected_file = entry.file_index.unwrap_or(self.selected_file);
                self.sync_rows_from_selected_files();
                self.focus_preview();
            }
        }
    }

    pub fn move_file_browser_parent(&mut self) -> bool {
        if self.files_cwd.as_os_str().is_empty() {
            return false;
        }
        let exited = self.files_cwd.clone();
        let parent = exited.parent().unwrap_or(Path::new("")).to_path_buf();
        self.files_cwd = parent;
        self.selected_file_entry = self
            .file_browser_entries()
            .iter()
            .position(|entry| entry.path == exited)
            .unwrap_or(0);
        self.file_browser_scroll = 0;
        self.status_message = if self.files_cwd.as_os_str().is_empty() {
            "folder /".to_string()
        } else {
            format!("folder {}", self.files_cwd.display())
        };
        self.sync_file_row_from_browser();
        true
    }

    pub fn create_issue_from_selection(&mut self) -> Result<()> {
        let selected_path = self.selected_path();
        let title = if let Some(path) = &selected_path {
            format!("Follow up {}", path.display())
        } else {
            "New issue".to_string()
        };
        let issue = self.store.create_issue(title)?;
        if let Some(path) = selected_path {
            let path = path.to_string_lossy().to_string();
            self.store.link_issue_file(&issue.key, &path)?;
        }
        self.status_message = format!("created {}", issue.key);
        self.issues = self.store.load_issues()?;
        self.rebuild_search();
        self.selected_issue = self
            .issues
            .iter()
            .position(|candidate| candidate.key == issue.key)
            .unwrap_or(0);
        self.active_tab = Tab::Tasks;
        Ok(())
    }

    pub fn cycle_selected_issue_status(&mut self) -> Result<()> {
        let Some(issue) = self.issues.get_mut(self.selected_issue) else {
            return Ok(());
        };
        let key = issue.key.clone();
        issue.status = issue.status.next();
        issue.touch();
        self.store.save_issue(issue)?;
        self.status_message = format!("{} status {}", issue.key, issue.status.label());
        self.issues = self.store.load_issues()?;
        self.selected_issue = self
            .issues
            .iter()
            .position(|issue| issue.key == key)
            .unwrap_or(0);
        self.rebuild_search();
        Ok(())
    }

    pub fn cycle_selected_issue_priority(&mut self) -> Result<()> {
        let Some(issue) = self.issues.get_mut(self.selected_issue) else {
            return Ok(());
        };
        let key = issue.key.clone();
        issue.priority = issue.priority.next();
        issue.touch();
        self.store.save_issue(issue)?;
        self.status_message = format!("{} priority {}", issue.key, issue.priority.label());
        self.issues = self.store.load_issues()?;
        self.selected_issue = self
            .issues
            .iter()
            .position(|issue| issue.key == key)
            .unwrap_or(0);
        self.rebuild_search();
        Ok(())
    }

    pub fn cycle_change_grouping(&mut self) {
        self.change_grouping = self.change_grouping.next();
        self.status_message = format!("changes grouped by {}", self.change_grouping.label());
    }

    pub fn toggle_dirstat(&mut self) {
        self.dirstat_visible = !self.dirstat_visible;
        self.status_message = if self.dirstat_visible {
            "dirstat weight visible".to_string()
        } else {
            "dirstat weight hidden".to_string()
        };
    }

    pub fn toggle_selected_issue_label(&mut self) -> Result<()> {
        let Some(issue) = self.issues.get_mut(self.selected_issue) else {
            return Ok(());
        };
        let Some(label) = next_label_for_issue(&self.reference_data, issue) else {
            self.status_message = "no labels configured".to_string();
            return Ok(());
        };

        let key = issue.key.clone();
        if let Some(position) = issue.labels.iter().position(|existing| existing == &label) {
            issue.labels.remove(position);
            self.status_message = format!("{} removed label {label}", issue.key);
        } else {
            issue.labels.push(label.clone());
            issue.labels.sort();
            issue.labels.dedup();
            self.status_message = format!("{} added label {label}", issue.key);
        }
        issue.touch();
        self.store.save_issue(issue)?;
        self.reload_issue_selection(&key)?;
        Ok(())
    }

    pub fn toggle_selected_issue_assignee(&mut self) -> Result<()> {
        self.toggle_selected_issue_assignee_to(current_assignee())
    }

    fn toggle_selected_issue_assignee_to(&mut self, assignee: String) -> Result<()> {
        let Some(issue) = self.issues.get_mut(self.selected_issue) else {
            return Ok(());
        };
        if assignee.is_empty() {
            self.status_message = "no USER configured".to_string();
            return Ok(());
        }

        let key = issue.key.clone();
        if issue.assignee == assignee {
            issue.assignee.clear();
            self.status_message = format!("{} unassigned", issue.key);
        } else {
            issue.assignee = assignee.clone();
            self.status_message = format!("{} assigned {assignee}", issue.key);
        }
        issue.touch();
        self.store.save_issue(issue)?;
        self.reload_issue_selection(&key)?;
        Ok(())
    }

    pub fn link_selected_file_to_issue(&mut self) -> Result<()> {
        let Some(path) = self.selected_path() else {
            return Ok(());
        };
        let Some(issue) = self.issues.get(self.selected_issue) else {
            self.status_message = "no issue selected".to_string();
            return Ok(());
        };
        let path = path.to_string_lossy().to_string();
        let key = issue.key.clone();
        self.store.link_issue_file(&key, &path)?;
        self.status_message = format!("linked {path} to {key}");
        self.issues = self.store.load_issues()?;
        self.selected_issue = self
            .issues
            .iter()
            .position(|issue| issue.key == key)
            .unwrap_or(0);
        self.rebuild_search();
        Ok(())
    }

    pub fn jump_between_issue_and_file(&mut self) {
        match self.active_tab {
            Tab::Tasks => self.jump_from_issue_to_file(),
            Tab::Changes | Tab::Files | Tab::Agents => self.jump_from_file_to_issue_or_file(),
            Tab::Search => {}
        }
    }

    fn jump_from_issue_to_file(&mut self) {
        let Some(path) = self.selected_path() else {
            self.status_message = "issue has no linked file".to_string();
            return;
        };
        self.active_tab = Tab::Files;
        if let Some(index) = self.files.iter().position(|file| file == &path) {
            self.selected_file = index;
            self.reveal_file_in_browser(&path);
        }
        self.preview_visible = true;
        self.status_message = format!("file {}", path.display());
    }

    fn jump_from_file_to_issue_or_file(&mut self) {
        let Some(path) = self.selected_path() else {
            self.status_message = "nothing selected".to_string();
            return;
        };

        if matches!(self.active_tab, Tab::Agents) {
            self.active_tab = Tab::Files;
            if let Some(index) = self.files.iter().position(|file| file == &path) {
                self.selected_file = index;
                self.reveal_file_in_browser(&path);
            }
            self.preview_visible = true;
            self.status_message = format!("file {}", path.display());
            return;
        }

        let path_text = path.to_string_lossy();
        let Some(index) = self
            .issues
            .iter()
            .position(|issue| issue.linked_files.iter().any(|linked| linked == &path_text))
        else {
            self.status_message = format!("no issue linked to {}", path.display());
            return;
        };
        self.selected_issue = index;
        self.active_tab = Tab::Tasks;
        self.status_message = format!("issue {}", self.issues[index].key);
    }

    fn reload_issue_selection(&mut self, key: &str) -> Result<()> {
        self.issues = self.store.load_issues()?;
        self.selected_issue = self
            .issues
            .iter()
            .position(|issue| issue.key == key)
            .unwrap_or(0);
        self.rebuild_search();
        Ok(())
    }

    pub fn reveal_selected_context(&mut self) {
        match self.preview_target() {
            Some(target) => {
                self.preview_visible = true;
                self.focus = FocusPane::Preview;
                self.status_message = format!("preview {}", target.path.display());
            }
            None => {
                self.status_message = "nothing selected".to_string();
            }
        }
    }

    pub fn open_selected_in_editor(&mut self) -> Result<()> {
        let Some(path) = self.selected_path() else {
            self.status_message = "no file selected".to_string();
            return Ok(());
        };
        open_path_in_editor(&self.repo_root.join(&path))
            .with_context(|| format!("failed to open {}", path.display()))?;
        Ok(())
    }

    pub fn open_selected_issue_in_editor(&mut self) -> Result<()> {
        let Some(issue) = self.issues.get(self.selected_issue) else {
            self.status_message = "no issue selected".to_string();
            return Ok(());
        };
        open_path_in_editor(&self.store.issue_file_path(&issue.key))
            .with_context(|| format!("failed to open {}", issue.key))?;
        self.refresh()?;
        Ok(())
    }

    pub fn copy_selected_reference(&mut self) {
        let text = match self.active_tab {
            Tab::Tasks => self
                .issues
                .get(self.selected_issue)
                .map(|issue| issue.key.clone()),
            _ => self
                .selected_path()
                .map(|path| path.to_string_lossy().to_string()),
        };

        let Some(text) = text else {
            self.status_message = "nothing to copy".to_string();
            return;
        };

        match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text.clone())) {
            Ok(()) => self.status_message = format!("copied {text}"),
            Err(error) => self.status_message = format!("copy failed: {error}"),
        }
    }

    pub fn accept_search_result(&mut self) {
        let Some(result) = self.search_results.get(self.selected_search) else {
            return;
        };
        match result.record.target.clone() {
            SearchTarget::File(path) => {
                self.active_tab = Tab::Files;
                if let Some(index) = self.files.iter().position(|file| file == &path) {
                    self.selected_file = index;
                    self.reveal_file_in_browser(&path);
                }
            }
            SearchTarget::Change(path) => {
                self.active_tab = Tab::Changes;
                if let Some(index) = self.changes.iter().position(|change| change.path == path) {
                    self.selected_change = index;
                    self.sync_rows_from_selected_files();
                }
            }
            SearchTarget::Issue(key) => {
                self.active_tab = Tab::Tasks;
                if let Some(index) = self.issues.iter().position(|issue| issue.key == key) {
                    self.selected_issue = index;
                }
            }
            SearchTarget::AgentSession(id) => {
                self.active_tab = Tab::Agents;
                if let Some(index) = self.sessions.iter().position(|session| session.id == id) {
                    self.selected_session = index;
                }
            }
            SearchTarget::Project(id) => {
                self.active_tab = Tab::Tasks;
                self.select_first_matching_issue(|issue| issue.project == id.as_str());
                self.status_message = format!("project {id}");
            }
            SearchTarget::Cycle(id) => {
                self.active_tab = Tab::Tasks;
                self.select_first_matching_issue(|issue| issue.cycle == id.as_str());
                self.status_message = format!("cycle {id}");
            }
            SearchTarget::Label(id) => {
                self.active_tab = Tab::Tasks;
                self.select_first_matching_issue(|issue| {
                    issue.labels.iter().any(|label| label == id.as_str())
                });
                self.status_message = format!("label {id}");
            }
            SearchTarget::Symbol { path, line, name } => {
                self.active_tab = Tab::Files;
                if let Some(index) = self.files.iter().position(|file| file == &path) {
                    self.selected_file = index;
                    self.reveal_file_in_browser(&path);
                }
                self.status_message = format!("{name} line {line}");
            }
        }
    }

    pub fn focus_preview(&mut self) {
        if self.preview_target().is_some() {
            self.preview_visible = true;
            self.focus = FocusPane::Preview;
            self.status_message = "preview focus".to_string();
        }
    }

    pub fn focus_tree(&mut self) {
        self.focus = FocusPane::Tree;
        self.status_message = "tree focus".to_string();
    }

    pub fn collapse_selected_tree_row(&mut self) -> bool {
        let Some(path) = self.selected_path() else {
            return false;
        };
        let collapse_path = match self.active_tab {
            Tab::Changes => {
                let Some(row) = self.selected_change_row_data() else {
                    return false;
                };
                if row.kind == TreeRowKind::Directory {
                    row.path
                } else {
                    row.path.parent().unwrap_or(Path::new("")).to_path_buf()
                }
            }
            Tab::Files => {
                let Some(row) = self.selected_file_row_data() else {
                    return false;
                };
                if row.kind == TreeRowKind::Directory {
                    row.path
                } else {
                    row.path.parent().unwrap_or(Path::new("")).to_path_buf()
                }
            }
            _ => return false,
        };
        if collapse_path.as_os_str().is_empty() {
            return false;
        }

        match self.active_tab {
            Tab::Changes => {
                self.collapsed_change_dirs.insert(collapse_path.clone());
                self.selected_change_row = self
                    .change_tree_rows()
                    .iter()
                    .position(|row| row.path == collapse_path)
                    .unwrap_or(self.selected_change_row);
                self.sync_selected_change_from_row();
            }
            Tab::Files => {
                self.collapsed_file_dirs.insert(collapse_path.clone());
                self.selected_file_row = self
                    .file_tree_rows()
                    .iter()
                    .position(|row| row.path == collapse_path)
                    .unwrap_or(self.selected_file_row);
                self.sync_selected_file_from_row();
            }
            _ => {}
        }
        self.status_message = format!("collapsed {}", collapse_path.display());
        let _ = path;
        true
    }

    pub fn expand_selected_tree_row(&mut self) -> bool {
        let expanded = match self.active_tab {
            Tab::Changes => {
                let Some(row) = self.selected_change_row_data() else {
                    return false;
                };
                row.kind == TreeRowKind::Directory && self.collapsed_change_dirs.remove(&row.path)
            }
            Tab::Files => {
                let Some(row) = self.selected_file_row_data() else {
                    return false;
                };
                row.kind == TreeRowKind::Directory && self.collapsed_file_dirs.remove(&row.path)
            }
            _ => false,
        };
        if expanded {
            self.status_message = "expanded".to_string();
        }
        expanded
    }

    pub fn scroll_preview_down(&mut self, amount: usize, viewport_height: usize) {
        self.preview_scroll = self.preview_scroll.saturating_add(amount);
        if viewport_height != usize::MAX {
            self.clamp_preview_scroll(viewport_height);
        }
    }

    pub fn scroll_preview_up(&mut self, amount: usize) {
        self.preview_scroll = self.preview_scroll.saturating_sub(amount);
    }

    pub fn preview_top(&mut self) {
        self.preview_scroll = 0;
    }

    pub fn preview_bottom(&mut self, viewport_height: usize) {
        self.preview_scroll = if viewport_height == usize::MAX {
            usize::MAX
        } else {
            self.max_preview_scroll(viewport_height)
        };
    }

    pub fn clamp_preview_scroll(&mut self, viewport_height: usize) {
        self.preview_scroll = self
            .preview_scroll
            .min(self.max_preview_scroll(viewport_height));
    }

    fn max_preview_scroll(&self, viewport_height: usize) -> usize {
        let Some(preview) = self.selected_preview() else {
            return 0;
        };
        let lines = preview.content.lines().count();
        lines.saturating_sub(viewport_height.max(1))
    }

    pub fn set_search_query(&mut self, query: String) {
        self.search_query = query;
        self.selected_search = 0;
        self.rebuild_search();
    }

    fn clamp_selections(&mut self) {
        clamp(&mut self.selected_change, self.changes.len());
        clamp(&mut self.selected_file, self.files.len());
        let change_rows_len = self.change_tree_rows().len();
        let file_rows_len = self.file_tree_rows().len();
        clamp(&mut self.selected_change_row, change_rows_len);
        clamp(&mut self.selected_file_row, file_rows_len);
        self.clamp_file_browser_selection();
        clamp(&mut self.selected_issue, self.issues.len());
        clamp(&mut self.selected_session, self.sessions.len());
        clamp(&mut self.selected_search, self.search_results.len());
        if !self.preview_visible {
            self.focus = FocusPane::Tree;
        }
    }

    pub fn change_tree_rows(&self) -> Vec<TreeRow> {
        if self.change_grouping != ChangeGrouping::Directory {
            return self
                .changes
                .iter()
                .enumerate()
                .map(|(file_index, change)| TreeRow {
                    path: change.path.clone(),
                    depth: 0,
                    kind: TreeRowKind::File,
                    file_index: Some(file_index),
                    collapsed: false,
                })
                .collect();
        }
        tree_rows(
            self.changes.iter().map(|change| change.path.as_path()),
            &self.collapsed_change_dirs,
        )
    }

    pub fn file_tree_rows(&self) -> Vec<TreeRow> {
        tree_rows(
            self.files.iter().map(|path| path.as_path()),
            &self.collapsed_file_dirs,
        )
    }

    pub fn file_browser_entries(&self) -> Vec<FileBrowserEntry> {
        let mut dirs = BTreeSet::<PathBuf>::new();
        let mut files = Vec::<FileBrowserEntry>::new();

        for (index, path) in self.files.iter().enumerate() {
            let relative = if self.files_cwd.as_os_str().is_empty() {
                path.as_path()
            } else {
                match path.strip_prefix(&self.files_cwd) {
                    Ok(relative) if !relative.as_os_str().is_empty() => relative,
                    _ => continue,
                }
            };
            let mut components = relative.components();
            let Some(first) = components.next() else {
                continue;
            };
            let child_name = first.as_os_str().to_string_lossy().to_string();
            let child_path = self.files_cwd.join(&child_name);
            if components.next().is_some() {
                dirs.insert(child_path);
            } else {
                files.push(FileBrowserEntry {
                    path: child_path,
                    name: child_name,
                    kind: FileBrowserEntryKind::File,
                    file_index: Some(index),
                });
            }
        }

        let mut entries = Vec::new();
        if !self.files_cwd.as_os_str().is_empty() {
            entries.push(FileBrowserEntry {
                path: self
                    .files_cwd
                    .parent()
                    .unwrap_or(Path::new(""))
                    .to_path_buf(),
                name: "../".to_string(),
                kind: FileBrowserEntryKind::Parent,
                file_index: None,
            });
        }
        entries.extend(dirs.into_iter().map(|path| FileBrowserEntry {
            name: format!(
                "{}/",
                path.file_name().unwrap_or_default().to_string_lossy()
            ),
            path,
            kind: FileBrowserEntryKind::Directory,
            file_index: None,
        }));
        files.sort_by(|left, right| left.name.cmp(&right.name));
        entries.extend(files);
        entries
    }

    fn selected_change_row_data(&self) -> Option<TreeRow> {
        self.change_tree_rows()
            .get(self.selected_change_row)
            .cloned()
    }

    fn selected_file_row_data(&self) -> Option<TreeRow> {
        self.file_tree_rows().get(self.selected_file_row).cloned()
    }

    pub fn selected_file_browser_entry(&self) -> Option<FileBrowserEntry> {
        self.file_browser_entries()
            .get(self.selected_file_entry)
            .cloned()
    }

    fn selected_change_file_path(&self) -> Option<PathBuf> {
        self.selected_change_row_data().and_then(|row| {
            row.file_index
                .and_then(|index| self.changes.get(index).map(|change| change.path.clone()))
        })
    }

    fn selected_file_browser_file_path(&self) -> Option<PathBuf> {
        self.selected_file_browser_entry()
            .filter(|entry| entry.kind == FileBrowserEntryKind::File)
            .map(|entry| entry.path)
    }

    fn sync_selected_change_from_row(&mut self) {
        if let Some(index) = self
            .selected_change_row_data()
            .and_then(|row| row.file_index)
        {
            self.selected_change = index;
        }
    }

    fn sync_selected_file_from_row(&mut self) {
        if let Some(row) = self.selected_file_row_data() {
            match row.kind {
                TreeRowKind::Directory => self.select_file_browser_path(&row.path),
                TreeRowKind::File => {
                    if let Some(index) = row.file_index {
                        self.selected_file = index;
                    }
                    self.select_file_browser_path(&row.path);
                }
            }
        }
    }

    fn sync_rows_from_selected_files(&mut self) {
        if let Some(index) = self
            .change_tree_rows()
            .iter()
            .position(|row| row.file_index == Some(self.selected_change))
        {
            self.selected_change_row = index;
        }
        if let Some(index) = self
            .file_tree_rows()
            .iter()
            .position(|row| row.file_index == Some(self.selected_file))
        {
            self.selected_file_row = index;
        }
        if let Some(path) = self.files.get(self.selected_file).cloned() {
            self.select_file_browser_path(&path);
        }
    }

    pub fn reveal_file_in_browser(&mut self, path: &Path) {
        self.select_file_browser_path(path);
        self.sync_file_row_from_browser();
    }

    fn select_file_browser_path(&mut self, path: &Path) {
        self.files_cwd = path.parent().unwrap_or(Path::new("")).to_path_buf();
        self.selected_file_entry = self
            .file_browser_entries()
            .iter()
            .position(|entry| entry.path == path)
            .unwrap_or(0);
        self.clamp_file_browser_selection();
    }

    fn sync_file_row_from_browser(&mut self) {
        let Some(entry) = self.selected_file_browser_entry() else {
            return;
        };
        if let Some(index) = entry.file_index {
            self.selected_file = index;
        }
        if let Some(index) = self
            .file_tree_rows()
            .iter()
            .position(|row| row.path == entry.path)
        {
            self.selected_file_row = index;
        }
    }

    fn clamp_file_browser_selection(&mut self) {
        while !self.files_cwd.as_os_str().is_empty() && self.file_browser_entries().is_empty() {
            self.files_cwd = self
                .files_cwd
                .parent()
                .unwrap_or(Path::new(""))
                .to_path_buf();
        }
        let len = self.file_browser_entries().len();
        clamp(&mut self.selected_file_entry, len);
    }

    fn move_issue_selection(&mut self, delta: isize) {
        let order = self.visual_issue_order();
        if order.is_empty() {
            self.selected_issue = 0;
            return;
        }

        let current = order
            .iter()
            .position(|index| *index == self.selected_issue)
            .unwrap_or(0);
        let next = (current as isize + delta).clamp(0, order.len() as isize - 1) as usize;
        self.selected_issue = order[next];
    }

    fn visual_issue_order(&self) -> Vec<usize> {
        crate::store::IssueStatus::ALL
            .iter()
            .flat_map(|status| {
                self.issues
                    .iter()
                    .enumerate()
                    .filter_map(|(index, issue)| (issue.status == *status).then_some(index))
            })
            .collect()
    }

    fn select_first_matching_issue(&mut self, predicate: impl Fn(&Issue) -> bool) {
        if let Some(index) = self.issues.iter().position(predicate) {
            self.selected_issue = index;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeGrouping {
    Directory,
    Status,
}

impl ChangeGrouping {
    pub fn label(self) -> &'static str {
        match self {
            ChangeGrouping::Directory => "directory",
            ChangeGrouping::Status => "status",
        }
    }

    fn next(self) -> Self {
        match self {
            ChangeGrouping::Directory => ChangeGrouping::Status,
            ChangeGrouping::Status => ChangeGrouping::Directory,
        }
    }
}

fn increment(index: &mut usize, len: usize) {
    if len == 0 {
        *index = 0;
    } else {
        *index = (*index + 1).min(len - 1);
    }
}

fn decrement(index: &mut usize) {
    *index = index.saturating_sub(1);
}

fn clamp(index: &mut usize, len: usize) {
    if len == 0 {
        *index = 0;
    } else if *index >= len {
        *index = len - 1;
    }
}

fn next_label_for_issue(reference_data: &ReferenceData, issue: &Issue) -> Option<String> {
    if reference_data.labels.is_empty() {
        return None;
    }

    let label_ids = reference_data
        .labels
        .iter()
        .map(|label| label.id.as_str())
        .collect::<Vec<_>>();
    let next = issue
        .labels
        .iter()
        .filter_map(|label| label_ids.iter().position(|candidate| candidate == label))
        .max()
        .map(|index| (index + 1) % label_ids.len())
        .unwrap_or(0);
    Some(label_ids[next].to_string())
}

fn tree_rows<'a>(
    paths: impl Iterator<Item = &'a Path>,
    collapsed_dirs: &BTreeSet<PathBuf>,
) -> Vec<TreeRow> {
    let mut rows = Vec::new();
    let mut seen_dirs = BTreeSet::<PathBuf>::new();

    for (file_index, path) in paths.enumerate() {
        let dirs = cumulative_dirs(path);
        let mut hidden_by_collapse = false;
        for (depth, dir) in dirs.iter().enumerate() {
            if seen_dirs.insert(dir.clone()) {
                rows.push(TreeRow {
                    path: dir.clone(),
                    depth,
                    kind: TreeRowKind::Directory,
                    file_index: None,
                    collapsed: collapsed_dirs.contains(dir),
                });
            }
            if collapsed_dirs.contains(dir) {
                hidden_by_collapse = true;
                break;
            }
        }
        if !hidden_by_collapse {
            rows.push(TreeRow {
                path: path.to_path_buf(),
                depth: dirs.len(),
                kind: TreeRowKind::File,
                file_index: Some(file_index),
                collapsed: false,
            });
        }
    }

    rows
}

fn cumulative_dirs(path: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return dirs;
    };
    let mut current = PathBuf::new();
    for component in parent.components() {
        current.push(component.as_os_str());
        dirs.push(current.clone());
    }
    dirs
}

fn current_assignee() -> String {
    std::env::var("WORKDECK_ASSIGNEE")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn issue_preview(issue: &Issue) -> FilePreview {
    let mut lines = vec![
        format!("# {} {}", issue.key, issue.title),
        String::new(),
        format!("Status: `{}`", issue.status.label()),
        format!("Priority: `{}`", issue.priority.label()),
    ];

    if !issue.project.is_empty() {
        lines.push(format!("Project: `{}`", issue.project));
    }
    if !issue.cycle.is_empty() {
        lines.push(format!("Cycle: `{}`", issue.cycle));
    }
    if !issue.assignee.is_empty() {
        lines.push(format!("Assignee: `{}`", issue.assignee));
    }
    if !issue.due_at.is_empty() {
        lines.push(format!("Due: `{}`", issue.due_at));
    }
    if !issue.labels.is_empty() {
        lines.push(format!(
            "Labels: {}",
            issue
                .labels
                .iter()
                .map(|label| format!("`{label}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !issue.linked_files.is_empty() {
        lines.push(String::new());
        lines.push("## Linked files".to_string());
        lines.extend(issue.linked_files.iter().map(|path| format!("- `{path}`")));
    }
    if !issue.linked_commits.is_empty() {
        lines.push(String::new());
        lines.push("## Linked commits".to_string());
        lines.extend(
            issue
                .linked_commits
                .iter()
                .map(|commit| format!("- `{commit}`")),
        );
    }
    if !issue.description.trim().is_empty() {
        lines.push(String::new());
        lines.push("## Description".to_string());
        lines.push(issue.description.clone());
    }

    FilePreview {
        title: format!("{} {}", issue.key, issue.title),
        content: lines.join("\n"),
        truncated: false,
        binary: false,
    }
}

fn agent_preview(session: &AgentSession) -> FilePreview {
    let mut lines = vec![
        format!("# {}", session.title),
        String::new(),
        format!("ID: `{}`", session.id),
    ];

    if !session.agent.is_empty() {
        lines.push(format!("Agent: `{}`", session.agent));
    }
    if !session.status.is_empty() {
        lines.push(format!("Status: `{}`", session.status));
    }
    if !session.cwd.is_empty() {
        lines.push(format!("CWD: `{}`", session.cwd));
    }
    if !session.started_at.is_empty() {
        lines.push(format!("Started: `{}`", session.started_at));
    }
    if !session.ended_at.is_empty() {
        lines.push(format!("Ended: `{}`", session.ended_at));
    }
    if !session.goal.trim().is_empty() {
        lines.push(String::new());
        lines.push("## Goal".to_string());
        lines.push(session.goal.clone());
    }
    if !session.summary.trim().is_empty() {
        lines.push(String::new());
        lines.push("## Summary".to_string());
        lines.push(session.summary.clone());
    }
    push_markdown_list(&mut lines, "Plan", &session.plan);
    if !session.touched_files.is_empty() {
        lines.push(String::new());
        lines.push("## Touched files".to_string());
        lines.extend(session.touched_files.iter().map(|file| {
            if file.change_type.is_empty() {
                format!("- `{}`", file.path)
            } else {
                format!("- `{}` {}", file.path, file.change_type)
            }
        }));
    }
    push_markdown_list(&mut lines, "Commands run", &session.commands_run);
    push_markdown_list(&mut lines, "Tests run", &session.tests_run);
    push_markdown_list(&mut lines, "Handoff notes", &session.handoff_notes);

    FilePreview {
        title: format!("agent {}", session.id),
        content: lines.join("\n"),
        truncated: false,
        binary: false,
    }
}

fn push_markdown_list(lines: &mut Vec<String>, heading: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push(format!("## {heading}"));
    lines.extend(items.iter().map(|item| format!("- {item}")));
}

fn open_path_in_editor(path: &Path) -> Result<()> {
    let editor = editor_command();
    let status = if editor_needs_shell(&editor) {
        Command::new("sh")
            .arg("-c")
            .arg(format!("{editor} \"$1\""))
            .arg("workdeck-editor")
            .arg(path)
            .status()
    } else {
        Command::new(editor).arg(path).status()
    }?;

    if !status.success() {
        anyhow::bail!("editor exited with {status}");
    }
    Ok(())
}

fn editor_needs_shell(editor: &str) -> bool {
    editor.contains(char::is_whitespace)
}

fn editor_command() -> String {
    std::env::var("EDITOR")
        .ok()
        .filter(|editor| !editor.trim().is_empty())
        .unwrap_or_else(|| "vi".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::tempdir;

    #[test]
    fn layout_modes_match_widths() {
        assert_eq!(LayoutMode::for_width(44), LayoutMode::Narrow);
        assert_eq!(LayoutMode::for_width(80), LayoutMode::Medium);
        assert_eq!(LayoutMode::for_width(120), LayoutMode::Wide);
    }

    #[test]
    fn tabs_wrap() {
        assert_eq!(Tab::Changes.previous(), Tab::Search);
        assert_eq!(Tab::Search.next(), Tab::Changes);
    }

    #[test]
    fn toggles_change_grouping_and_dirstat_visibility() {
        let mut app = new_with_parts_for_test(Vec::new());

        app.cycle_change_grouping();
        assert_eq!(app.change_grouping, ChangeGrouping::Status);
        assert_eq!(app.status_message, "changes grouped by status");

        app.cycle_change_grouping();
        assert_eq!(app.change_grouping, ChangeGrouping::Directory);

        app.toggle_dirstat();
        assert!(!app.dirstat_visible);
        assert_eq!(app.status_message, "dirstat weight hidden");

        app.toggle_dirstat();
        assert!(app.dirstat_visible);
        assert_eq!(app.status_message, "dirstat weight visible");
    }

    #[test]
    fn editor_commands_with_arguments_use_shell_launch() {
        assert!(!editor_needs_shell("vi"));
        assert!(editor_needs_shell("code -w"));
        assert!(editor_needs_shell("vim -n"));
    }

    #[test]
    fn new_does_not_create_workdeck_store() {
        let dir = tempdir().unwrap();
        let status = Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .arg("init")
            .status()
            .unwrap();
        assert!(status.success());

        let app = App::new(dir.path()).unwrap();

        assert_eq!(app.active_tab, Tab::Changes);
        assert!(!dir.path().join(".agents/workdeck").exists());
    }

    #[test]
    fn new_honors_preview_config() {
        let dir = tempdir().unwrap();
        let status = Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .arg("init")
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::create_dir_all(dir.path().join(".agents/workdeck")).unwrap();
        std::fs::write(
            dir.path().join(".agents/workdeck/config.toml"),
            r#"
            [ui]
            preview = false
            "#,
        )
        .unwrap();

        let app = App::new(dir.path()).unwrap();

        assert!(!app.preview_visible);
    }

    #[test]
    fn task_movement_follows_visual_status_order() {
        let mut app = new_with_parts_for_test(vec![
            Issue::new("WD-1".to_string(), "Done issue".to_string()),
            Issue::new("WD-2".to_string(), "Inbox issue".to_string()),
        ]);
        app.issues[0].status = crate::store::IssueStatus::Done;
        app.issues[1].status = crate::store::IssueStatus::Inbox;
        app.active_tab = Tab::Tasks;
        app.selected_issue = 1;

        app.move_down();

        assert_eq!(app.issues[app.selected_issue].key, "WD-1");
    }

    #[test]
    fn preview_state_only_returns_matching_cached_target() {
        let mut app = new_with_parts_for_test(Vec::new());
        app.files = vec![PathBuf::from("src/main.rs"), PathBuf::from("src/lib.rs")];
        app.active_tab = Tab::Files;
        app.selected_file = 0;
        app.selected_file_row = 1;
        app.reveal_file_in_browser(Path::new("src/main.rs"));

        let target = app.preview_target().unwrap();
        assert_eq!(target.path, PathBuf::from("src/main.rs"));
        assert_eq!(app.missing_preview_target(), Some(target.clone()));

        app.mark_preview_loading(target.clone());
        assert_eq!(app.missing_preview_target(), None);
        app.apply_preview_data(PreviewData {
            target,
            preview: FilePreview {
                title: "src/main.rs".to_string(),
                content: "fn main() {}".to_string(),
                truncated: false,
                binary: false,
            },
        });

        assert!(app.selected_preview().is_some());
        app.move_down();
        assert!(app.selected_preview().is_none());
        assert_eq!(
            app.missing_preview_target().unwrap().path,
            PathBuf::from("src/lib.rs")
        );
    }

    #[test]
    fn preview_focus_scrolls_independently_from_tree_selection() {
        let mut app = new_with_parts_for_test(Vec::new());
        app.files = vec![PathBuf::from("src/main.rs"), PathBuf::from("src/lib.rs")];
        app.active_tab = Tab::Files;
        app.selected_file = 0;
        app.selected_file_row = 1;
        app.reveal_file_in_browser(Path::new("src/main.rs"));

        app.focus_preview();
        assert_eq!(app.focus, FocusPane::Preview);

        app.scroll_preview_down(4, usize::MAX);
        assert_eq!(app.preview_scroll, 4);
        assert_eq!(app.selected_file, 0);

        app.scroll_preview_up(2);
        assert_eq!(app.preview_scroll, 2);

        app.focus_tree();
        app.move_down();
        assert_eq!(app.focus, FocusPane::Tree);
        assert_eq!(app.selected_file, 1);
    }

    #[test]
    fn collapses_and_expands_selected_tree_directory() {
        let mut app = new_with_parts_for_test(Vec::new());
        app.files = vec![
            PathBuf::from("src/main.rs"),
            PathBuf::from("src/lib.rs"),
            PathBuf::from("tests/main.rs"),
        ];
        app.active_tab = Tab::Files;
        app.selected_file = 0;
        app.selected_file_row = 1;
        app.reveal_file_in_browser(Path::new("src/main.rs"));

        assert!(app.collapse_selected_tree_row());
        assert!(app.collapsed_file_dirs.contains(Path::new("src")));
        assert_eq!(
            app.file_tree_rows()
                .iter()
                .filter(|row| row.kind == TreeRowKind::File)
                .count(),
            1
        );

        assert!(app.expand_selected_tree_row());
        assert!(!app.collapsed_file_dirs.contains(Path::new("src")));
        assert_eq!(
            app.file_tree_rows()
                .iter()
                .filter(|row| row.kind == TreeRowKind::File)
                .count(),
            3
        );
    }

    #[test]
    fn file_browser_drills_into_folders_and_returns_to_parent() {
        let mut app = new_with_parts_for_test(Vec::new());
        app.active_tab = Tab::Files;
        app.files = vec![
            PathBuf::from("crates/workdeck-cli/src/app.rs"),
            PathBuf::from("crates/workdeck-cli/src/views/mod.rs"),
            PathBuf::from("README.md"),
        ];

        let root_entries = app.file_browser_entries();
        assert_eq!(root_entries[0].name, "crates/");
        assert_eq!(root_entries[1].name, "README.md");

        app.activate_selected_file_browser_entry();
        assert_eq!(app.files_cwd, PathBuf::from("crates"));
        assert_eq!(app.file_browser_entries()[0].name, "../");
        assert_eq!(app.file_browser_entries()[1].name, "workdeck-cli/");

        app.move_file_browser_down();
        app.activate_selected_file_browser_entry();
        assert_eq!(app.files_cwd, PathBuf::from("crates/workdeck-cli"));

        assert!(app.move_file_browser_parent());
        assert_eq!(app.files_cwd, PathBuf::from("crates"));
        assert_eq!(
            app.selected_file_browser_entry().unwrap().path,
            PathBuf::from("crates/workdeck-cli")
        );
    }

    #[test]
    fn file_browser_enter_on_file_focuses_preview() {
        let mut app = new_with_parts_for_test(Vec::new());
        app.active_tab = Tab::Files;
        app.files = vec![PathBuf::from("README.md")];

        app.activate_selected_file_browser_entry();

        assert_eq!(app.focus, FocusPane::Preview);
        assert!(app.preview_visible);
        assert_eq!(
            app.preview_target().unwrap().path,
            PathBuf::from("README.md")
        );
    }

    #[test]
    fn search_preview_kind_tracks_selected_result_type() {
        let mut issue = Issue::new("WD-1".to_string(), "Search issue".to_string());
        issue.description = "Search issue body".to_string();
        let mut session = AgentSession::new("Search agent".to_string());
        session.id = "agent-1".to_string();
        session.summary = "Search agent body".to_string();
        let mut app = new_with_parts_for_test(vec![issue]);
        app.sessions = vec![session];
        app.active_tab = Tab::Search;
        app.search_results = vec![
            search_result(
                "src/main.rs",
                SearchTarget::File(PathBuf::from("src/main.rs")),
            ),
            search_result(
                "src/lib.rs",
                SearchTarget::Change(PathBuf::from("src/lib.rs")),
            ),
            search_result("WD-1", SearchTarget::Issue("WD-1".to_string())),
            search_result("agent-1", SearchTarget::AgentSession("agent-1".to_string())),
        ];

        app.selected_search = 0;
        let file_target = app.preview_target().unwrap();
        assert_eq!(file_target.path, PathBuf::from("src/main.rs"));
        assert_eq!(file_target.kind, PreviewKind::File);

        app.selected_search = 1;
        let diff_target = app.preview_target().unwrap();
        assert_eq!(diff_target.path, PathBuf::from("src/lib.rs"));
        assert_eq!(diff_target.kind, PreviewKind::Diff);

        app.selected_search = 2;
        let issue_target = app.preview_target().unwrap();
        assert_eq!(issue_target.path, PathBuf::from("WD-1"));
        assert_eq!(issue_target.kind, PreviewKind::Issue);
        assert!(
            app.selected_preview()
                .unwrap()
                .content
                .contains("Search issue body")
        );

        app.selected_search = 3;
        let agent_target = app.preview_target().unwrap();
        assert_eq!(agent_target.path, PathBuf::from("agent-1"));
        assert_eq!(agent_target.kind, PreviewKind::Agent);
        assert!(
            app.selected_preview()
                .unwrap()
                .content
                .contains("Search agent body")
        );
    }

    #[test]
    fn reveal_selected_context_enables_preview() {
        let mut app = new_with_parts_for_test(Vec::new());
        app.active_tab = Tab::Files;
        app.preview_visible = false;
        app.files = vec![PathBuf::from("src/main.rs")];
        app.selected_file_row = 1;
        app.reveal_file_in_browser(Path::new("src/main.rs"));

        app.reveal_selected_context();

        assert!(app.preview_visible);
        assert_eq!(app.status_message, "preview src/main.rs");
    }

    #[test]
    fn accepting_symbol_and_reference_search_results_jumps_to_context() {
        let mut issue = Issue::new("WD-1".to_string(), "Project issue".to_string());
        issue.project = "workdeck-mvp".to_string();
        let mut app = new_with_parts_for_test(vec![issue]);
        app.files = vec![PathBuf::from("src/app.rs")];
        app.active_tab = Tab::Search;
        app.search_results = vec![
            search_result(
                "refresh_workspace",
                SearchTarget::Symbol {
                    path: PathBuf::from("src/app.rs"),
                    line: 12,
                    name: "refresh_workspace".to_string(),
                },
            ),
            search_result(
                "workdeck-mvp",
                SearchTarget::Project("workdeck-mvp".to_string()),
            ),
        ];

        app.accept_search_result();
        assert_eq!(app.active_tab, Tab::Files);
        assert_eq!(app.selected_file, 0);
        assert_eq!(app.files_cwd, PathBuf::from("src"));
        assert_eq!(
            app.selected_file_browser_entry().unwrap().path,
            PathBuf::from("src/app.rs")
        );
        assert!(app.status_message.contains("line 12"));

        app.active_tab = Tab::Search;
        app.selected_search = 1;
        app.accept_search_result();
        assert_eq!(app.active_tab, Tab::Tasks);
        assert_eq!(app.selected_issue, 0);
        assert_eq!(app.status_message, "project workdeck-mvp");
    }

    #[test]
    fn toggles_selected_issue_labels_from_reference_data() {
        let dir = tempdir().unwrap();
        let store = WorkdeckStore::new(dir.path().join(".agents/workdeck"));
        let issue = store.create_issue("Label me".to_string()).unwrap();
        let mut app = new_with_parts_for_test(vec![issue]);
        app.store = store;
        app.reference_data.labels = vec![
            crate::store::Label::new("git".to_string(), "Git".to_string()),
            crate::store::Label::new("preview".to_string(), "Preview".to_string()),
        ];
        app.active_tab = Tab::Tasks;

        app.toggle_selected_issue_label().unwrap();
        assert_eq!(app.issues[0].labels, vec!["git"]);
        assert_eq!(app.status_message, "WD-1 added label git");

        app.toggle_selected_issue_label().unwrap();
        assert_eq!(app.issues[0].labels, vec!["git", "preview"]);
        assert_eq!(app.status_message, "WD-1 added label preview");

        app.toggle_selected_issue_label().unwrap();
        assert_eq!(app.issues[0].labels, vec!["preview"]);
        assert_eq!(app.status_message, "WD-1 removed label git");
    }

    #[test]
    fn toggles_selected_issue_assignee() {
        let dir = tempdir().unwrap();
        let store = WorkdeckStore::new(dir.path().join(".agents/workdeck"));
        let issue = store.create_issue("Assign me".to_string()).unwrap();
        let mut app = new_with_parts_for_test(vec![issue]);
        app.store = store;
        app.active_tab = Tab::Tasks;

        app.toggle_selected_issue_assignee_to("rutger".to_string())
            .unwrap();
        assert_eq!(app.issues[0].assignee, "rutger");
        assert_eq!(app.status_message, "WD-1 assigned rutger");

        app.toggle_selected_issue_assignee_to("rutger".to_string())
            .unwrap();
        assert_eq!(app.issues[0].assignee, "");
        assert_eq!(app.status_message, "WD-1 unassigned");
    }

    #[test]
    fn jumps_between_issue_and_linked_file() {
        let mut issue = Issue::new("WD-1".to_string(), "Linked".to_string());
        issue.linked_files = vec!["src/main.rs".to_string()];
        let mut app = new_with_parts_for_test(vec![issue]);
        app.files = vec![PathBuf::from("src/main.rs")];
        app.active_tab = Tab::Tasks;

        app.jump_between_issue_and_file();
        assert_eq!(app.active_tab, Tab::Files);
        assert_eq!(app.selected_file, 0);
        assert!(app.preview_visible);
        assert_eq!(app.status_message, "file src/main.rs");

        app.jump_between_issue_and_file();
        assert_eq!(app.active_tab, Tab::Tasks);
        assert_eq!(app.selected_issue, 0);
        assert_eq!(app.status_message, "issue WD-1");
    }

    #[test]
    fn issue_preview_is_markdown_shaped_for_syntax_highlighting() {
        let mut issue = Issue::new("WD-1".to_string(), "Highlight task".to_string());
        issue.status = crate::store::IssueStatus::InProgress;
        issue.priority = crate::store::Priority::High;
        issue.labels = vec!["ux".to_string(), "preview".to_string()];
        issue.linked_files = vec!["src/app.rs".to_string()];
        issue.linked_commits = vec!["abc123".to_string()];
        issue.description = "Review task preview colors.".to_string();
        let mut app = new_with_parts_for_test(vec![issue]);
        app.active_tab = Tab::Tasks;

        let preview = app.selected_preview().unwrap();

        assert_eq!(preview.title, "WD-1 Highlight task");
        assert!(preview.content.contains("# WD-1 Highlight task"));
        assert!(preview.content.contains("Status: `In Progress`"));
        assert!(preview.content.contains("Labels: `ux`, `preview`"));
        assert!(preview.content.contains("## Linked files"));
        assert!(preview.content.contains("- `src/app.rs`"));
        assert!(preview.content.contains("## Description"));
    }

    #[test]
    fn agent_preview_is_markdown_shaped_for_syntax_highlighting() {
        let mut session = AgentSession::new("Highlight agent".to_string());
        session.id = "agent-1".to_string();
        session.agent = "codex".to_string();
        session.status = "active".to_string();
        session.cwd = "/tmp/workdeck".to_string();
        session.goal = "Keep session state readable.".to_string();
        session.summary = "Agent summary body.".to_string();
        session.plan = vec!["Inspect previews.".to_string()];
        session.commands_run = vec!["cargo test".to_string()];
        session.tests_run = vec!["cargo test".to_string()];
        session.handoff_notes = vec!["Review light mode.".to_string()];
        session.touched_files = vec![crate::store::AgentTouchedFile {
            path: "src/app.rs".to_string(),
            change_type: "modified".to_string(),
        }];
        let mut app = new_with_parts_for_test(Vec::new());
        app.active_tab = Tab::Agents;
        app.sessions = vec![session];

        let target = app.preview_target().unwrap();
        let preview = app.selected_preview().unwrap();

        assert_eq!(target.kind, PreviewKind::Agent);
        assert_eq!(target.path, PathBuf::from("agent-1"));
        assert_eq!(preview.title, "agent agent-1");
        assert!(preview.content.contains("# Highlight agent"));
        assert!(preview.content.contains("Status: `active`"));
        assert!(preview.content.contains("## Touched files"));
        assert!(preview.content.contains("- `src/app.rs` modified"));
        assert!(preview.content.contains("## Handoff notes"));
    }

    #[test]
    fn jump_reports_missing_file_issue_link() {
        let mut app = new_with_parts_for_test(Vec::new());
        app.files = vec![PathBuf::from("src/main.rs")];
        app.active_tab = Tab::Files;
        app.selected_file_row = 1;
        app.reveal_file_in_browser(Path::new("src/main.rs"));

        app.jump_between_issue_and_file();

        assert_eq!(app.active_tab, Tab::Files);
        assert_eq!(app.status_message, "no issue linked to src/main.rs");
    }

    #[test]
    fn newer_refresh_generation_ignores_stale_results_and_errors() {
        let mut app = new_with_parts_for_test(Vec::new());
        let first = app.begin_refresh();
        let second = app.begin_refresh();
        assert!(app.loading);
        assert_eq!(first + 1, second);

        let mut stale_data = refresh_data_for_test(first, "src/stale.rs");
        stale_data.files = vec![PathBuf::from("src/stale.rs")];
        assert!(!app.apply_refresh_data(stale_data));
        assert!(app.files.is_empty());
        assert!(app.loading);

        assert!(!app.apply_refresh_error(first, "old failure".to_string()));
        assert!(app.loading);
        assert_ne!(app.status_message, "old failure");

        let fresh_data = refresh_data_for_test(second, "src/fresh.rs");
        assert!(app.apply_refresh_data(fresh_data));
        assert_eq!(app.files, vec![PathBuf::from("src/fresh.rs")]);
        assert!(!app.loading);
        assert_eq!(app.status_message, "refreshed");
    }

    fn new_with_parts_for_test(issues: Vec<Issue>) -> App {
        App {
            cwd: PathBuf::from("/tmp/workdeck"),
            repo_root: PathBuf::from("/tmp/workdeck"),
            config: Config::default(),
            store: WorkdeckStore::new("/tmp/workdeck/.agents/workdeck"),
            active_tab: Tab::Changes,
            preview_visible: true,
            focus: FocusPane::Tree,
            preview_scroll: 0,
            collapsed_change_dirs: BTreeSet::new(),
            collapsed_file_dirs: BTreeSet::new(),
            change_grouping: ChangeGrouping::Directory,
            dirstat_visible: true,
            help_visible: false,
            search_query: String::new(),
            status_message: String::new(),
            loading: false,
            refresh_generation: 0,
            changes: Vec::new(),
            snapshot: None,
            files: Vec::new(),
            issues,
            sessions: Vec::new(),
            reference_data: ReferenceData::default(),
            symbols: Vec::new(),
            search_index: SearchIndex::default(),
            search_results: Vec::new(),
            preview_cache: None,
            preview_loading: None,
            selected_change: 0,
            selected_change_row: 0,
            selected_file: 0,
            selected_file_row: 0,
            files_cwd: PathBuf::new(),
            selected_file_entry: 0,
            file_browser_scroll: 0,
            selected_issue: 0,
            selected_session: 0,
            selected_search: 0,
        }
    }

    fn search_result(label: &str, target: SearchTarget) -> SearchResult {
        SearchResult {
            score: 1,
            record: crate::search::SearchRecord {
                label: label.to_string(),
                detail: String::new(),
                haystack: label.to_string(),
                target,
            },
        }
    }

    fn refresh_data_for_test(generation: u64, path: &str) -> RefreshData {
        let changes = vec![ChangeEntry {
            path: PathBuf::from(path),
            kind: crate::git::ChangeKind::Modified,
            staged: false,
            unstaged: true,
            additions: 1,
            deletions: 0,
        }];
        RefreshData {
            generation,
            snapshot: RepoSnapshot {
                root: PathBuf::from("/tmp/workdeck"),
                changes: changes.clone(),
                groups: crate::git::group_by_directory(&changes),
            },
            files: vec![PathBuf::from(path)],
            issues: Vec::new(),
            sessions: Vec::new(),
            reference_data: ReferenceData::default(),
            symbols: Vec::new(),
        }
    }
}
