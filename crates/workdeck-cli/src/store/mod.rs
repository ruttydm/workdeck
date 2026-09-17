use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

mod legacy_transfer;
pub use legacy_transfer::LegacyImportSummary;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Ord, PartialOrd)]
#[serde(rename_all = "kebab-case")]
pub enum IssueStatus {
    Inbox,
    Backlog,
    Todo,
    InProgress,
    InReview,
    Done,
}

impl IssueStatus {
    pub const ALL: [IssueStatus; 6] = [
        IssueStatus::Inbox,
        IssueStatus::Backlog,
        IssueStatus::Todo,
        IssueStatus::InProgress,
        IssueStatus::InReview,
        IssueStatus::Done,
    ];

    pub fn label(self) -> &'static str {
        match self {
            IssueStatus::Inbox => "Inbox",
            IssueStatus::Backlog => "Backlog",
            IssueStatus::Todo => "Todo",
            IssueStatus::InProgress => "In Progress",
            IssueStatus::InReview => "In Review",
            IssueStatus::Done => "Done",
        }
    }

    pub fn next(self) -> Self {
        match self {
            IssueStatus::Inbox => IssueStatus::Backlog,
            IssueStatus::Backlog => IssueStatus::Todo,
            IssueStatus::Todo => IssueStatus::InProgress,
            IssueStatus::InProgress => IssueStatus::InReview,
            IssueStatus::InReview => IssueStatus::Done,
            IssueStatus::Done => IssueStatus::Inbox,
        }
    }
}

impl FromStr for IssueStatus {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match normalize_token(value).as_str() {
            "inbox" => Ok(IssueStatus::Inbox),
            "backlog" => Ok(IssueStatus::Backlog),
            "todo" => Ok(IssueStatus::Todo),
            "inprogress" | "progress" => Ok(IssueStatus::InProgress),
            "inreview" | "review" => Ok(IssueStatus::InReview),
            "done" | "closed" => Ok(IssueStatus::Done),
            _ => Err(format!("unknown status {value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Ord, PartialOrd)]
#[serde(rename_all = "kebab-case")]
pub enum Priority {
    None,
    Low,
    Medium,
    High,
    Urgent,
}

impl Priority {
    pub fn label(self) -> &'static str {
        match self {
            Priority::None => "none",
            Priority::Low => "low",
            Priority::Medium => "medium",
            Priority::High => "high",
            Priority::Urgent => "urgent",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Priority::None => Priority::Low,
            Priority::Low => Priority::Medium,
            Priority::Medium => Priority::High,
            Priority::High => Priority::Urgent,
            Priority::Urgent => Priority::None,
        }
    }
}

impl FromStr for Priority {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match normalize_token(value).as_str() {
            "none" | "no" => Ok(Priority::None),
            "low" => Ok(Priority::Low),
            "medium" | "med" => Ok(Priority::Medium),
            "high" => Ok(Priority::High),
            "urgent" | "critical" => Ok(Priority::Urgent),
            _ => Err(format!("unknown priority {value}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Issue {
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_status")]
    pub status: IssueStatus,
    #[serde(default = "default_priority")]
    pub priority: Priority,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub cycle: String,
    #[serde(default)]
    pub assignee: String,
    #[serde(default = "now")]
    pub created_at: String,
    #[serde(default = "now")]
    pub updated_at: String,
    #[serde(default)]
    pub due_at: String,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub linked_files: Vec<String>,
    #[serde(default)]
    pub linked_commits: Vec<String>,
    #[serde(default, flatten, skip_serializing_if = "toml::Table::is_empty")]
    pub extra: toml::Table,
}

impl Issue {
    pub fn new(key: String, title: String) -> Self {
        let now = now();
        Self {
            key,
            title,
            description: String::new(),
            status: IssueStatus::Todo,
            priority: Priority::Medium,
            project: String::new(),
            cycle: String::new(),
            assignee: String::new(),
            created_at: now.clone(),
            updated_at: now,
            due_at: String::new(),
            labels: Vec::new(),
            linked_files: Vec::new(),
            linked_commits: Vec::new(),
            extra: toml::Table::new(),
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = now();
    }
}

impl Project {
    pub fn new(id: String, name: String) -> Self {
        let now = now();
        Self {
            id,
            name,
            description: String::new(),
            status: "active".to_string(),
            created_at: now.clone(),
            updated_at: now,
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = now();
    }
}

impl Cycle {
    pub fn new(id: String, name: String) -> Self {
        Self {
            id,
            name,
            starts_at: String::new(),
            ends_at: String::new(),
            status: "active".to_string(),
        }
    }
}

impl Label {
    pub fn new(id: String, name: String) -> Self {
        Self {
            id,
            name,
            color: String::new(),
        }
    }
}

impl AgentSession {
    pub fn new(title: String) -> Self {
        let id = format!("{}-{}", Utc::now().format("%Y%m%d%H%M%S"), slug(&title));
        Self {
            id,
            title,
            agent: String::new(),
            cwd: String::new(),
            status: "active".to_string(),
            started_at: now(),
            ended_at: String::new(),
            goal: String::new(),
            summary: String::new(),
            plan: Vec::new(),
            commands_run: Vec::new(),
            tests_run: Vec::new(),
            handoff_notes: Vec::new(),
            touched_files: Vec::new(),
            extra: toml::Table::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cycle {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub starts_at: String,
    #[serde(default)]
    pub ends_at: String,
    #[serde(default)]
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Label {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub color: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceData {
    #[serde(default)]
    pub projects: Vec<Project>,
    #[serde(default)]
    pub cycles: Vec<Cycle>,
    #[serde(default)]
    pub labels: Vec<Label>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentSession {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub agent: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub started_at: String,
    #[serde(default)]
    pub ended_at: String,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub plan: Vec<String>,
    #[serde(default)]
    pub commands_run: Vec<String>,
    #[serde(default)]
    pub tests_run: Vec<String>,
    #[serde(default)]
    pub handoff_notes: Vec<String>,
    #[serde(default)]
    pub touched_files: Vec<AgentTouchedFile>,
    #[serde(default, flatten, skip_serializing_if = "toml::Table::is_empty")]
    pub extra: toml::Table,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTouchedFile {
    pub path: String,
    #[serde(default)]
    pub change_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreEvent {
    pub kind: String,
    #[serde(default)]
    pub payload: serde_json::Value,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct WorkdeckStore {
    root: PathBuf,
}

impl WorkdeckStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn ensure_legacy_format(&self) -> Result<()> {
        if fs::symlink_metadata(&self.root)
            .is_ok_and(|metadata| metadata.file_type().is_symlink() || !metadata.is_dir())
        {
            bail!(
                "legacy data root must be an ordinary directory: {}",
                self.root.display()
            );
        }
        if [
            "config.yml",
            "migration.yml",
            "restore.yml",
            "schema.yml",
            "labels.yml",
            "operations",
            "tombstones",
        ]
        .iter()
        .any(|name| fs::symlink_metadata(self.root.join(name)).is_ok())
        {
            bail!(
                "native project-management authority cannot be opened by the legacy store: {}",
                self.root.display()
            );
        }
        Ok(())
    }

    fn entries(&self, directory: &str) -> Result<Vec<crate::bounded_files::Entry>> {
        self.ensure_legacy_format()?;
        let path = self.root.join(directory);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
        let (entries, truncated) =
            crate::bounded_files::list(&self.root, Path::new(directory), 10_000)?;
        if truncated {
            bail!("legacy {directory} exceeds the 10,000-entry read limit");
        }
        Ok(entries)
    }

    pub fn load_issues(&self) -> Result<Vec<Issue>> {
        let mut issues = Vec::new();
        let mut total_bytes = 0usize;
        for entry in self.entries("issues")? {
            let path = self.issues_dir().join(entry.name);
            if path.extension().and_then(|value| value.to_str()) != Some("toml") {
                continue;
            }

            let raw = read_legacy_text(&path, 2 * 1024 * 1024)?;
            total_bytes += raw.len();
            if total_bytes > 64 * 1024 * 1024 {
                bail!("legacy issues exceed the 64 MiB read budget");
            }
            let issue: Issue = toml::from_str(&raw)
                .with_context(|| format!("failed to parse issue {}", path.display()))?;
            issues.push(issue);
        }

        issues.sort_by_key(|issue| issue_key_number(&issue.key).unwrap_or(u64::MAX));
        Ok(issues)
    }

    pub fn load_reference_data(&self) -> Result<ReferenceData> {
        self.ensure_legacy_format()?;
        Ok(ReferenceData {
            projects: read_toml_list::<ProjectsFile>(&self.root.join("projects.toml"))?.projects,
            cycles: read_toml_list::<CyclesFile>(&self.root.join("cycles.toml"))?.cycles,
            labels: read_toml_list::<LabelsFile>(&self.root.join("labels.toml"))?.labels,
        })
    }

    pub fn load_agent_sessions(&self) -> Result<Vec<AgentSession>> {
        let mut sessions = Vec::new();
        let mut total_bytes = 0usize;
        for entry in self.entries("agents")? {
            let path = self.agents_dir().join(entry.name);
            if path.extension().and_then(|value| value.to_str()) != Some("toml") {
                continue;
            }

            let raw = read_legacy_text(&path, 2 * 1024 * 1024)?;
            total_bytes += raw.len();
            if total_bytes > 64 * 1024 * 1024 {
                bail!("legacy sessions exceed the 64 MiB read budget");
            }
            let session: AgentSession = toml::from_str(&raw)
                .with_context(|| format!("failed to parse agent session {}", path.display()))?;
            sessions.push(session);
        }

        sessions.sort_by(|a, b| b.started_at.cmp(&a.started_at).then(a.id.cmp(&b.id)));
        Ok(sessions)
    }

    pub fn load_events(&self) -> Result<Vec<StoreEvent>> {
        self.ensure_legacy_format()?;
        let path = self.root.join("events.jsonl");
        if missing(&path)? {
            return Ok(Vec::new());
        }

        let raw = read_legacy_text(&path, 64 * 1024 * 1024)?;
        let mut events = Vec::new();
        for (index, line) in raw.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let event: StoreEvent = serde_json::from_str(line)
                .with_context(|| format!("failed to parse event line {}", index + 1))?;
            events.push(event);
        }
        Ok(events)
    }

    fn issues_dir(&self) -> PathBuf {
        self.root.join("issues")
    }

    fn agents_dir(&self) -> PathBuf {
        self.root.join("agents")
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ProjectsFile {
    #[serde(default)]
    projects: Vec<Project>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct CyclesFile {
    #[serde(default)]
    cycles: Vec<Cycle>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct LabelsFile {
    #[serde(default)]
    labels: Vec<Label>,
}

fn read_toml_list<T>(path: &Path) -> Result<T>
where
    T: Default + for<'de> Deserialize<'de>,
{
    if missing(path)? {
        return Ok(T::default());
    }

    let raw = read_legacy_text(path, 2 * 1024 * 1024)?;
    if raw.trim().is_empty() {
        return Ok(T::default());
    }

    toml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

fn missing(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error.into()),
    }
}

fn read_legacy_text(path: &Path, limit: usize) -> Result<String> {
    let parent = path.parent().context("legacy file has no parent")?;
    let name = path.file_name().context("legacy file has no filename")?;
    let read = crate::bounded_files::read(parent, Path::new(name), limit)
        .with_context(|| format!("failed to read legacy file {}", path.display()))?;
    if read.truncated {
        bail!(
            "legacy file exceeds its {}-byte read limit: {}",
            limit,
            path.display()
        );
    }
    String::from_utf8(read.bytes)
        .with_context(|| format!("legacy file is not UTF-8: {}", path.display()))
}

fn issue_key_number(key: &str) -> Option<u64> {
    key.strip_prefix("WD-")?.parse().ok()
}

fn valid_issue_key(key: &str) -> bool {
    issue_key_number(key).is_some_and(|number| number > 0)
}

fn normalized_reference_id(id: Option<String>, name: &str) -> Result<String> {
    let id = id.unwrap_or_else(|| slug(name));
    if id.trim().is_empty() {
        bail!("id cannot be empty");
    }
    if !id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        bail!("id may only contain ASCII letters, numbers, dashes, and underscores");
    }
    Ok(id)
}

fn sanitize_key(key: &str) -> String {
    key.chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
        .collect()
}

fn slug(value: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "session".to_string()
    } else {
        slug
    }
}

fn normalize_token(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn default_status() -> IssueStatus {
    IssueStatus::Todo
}

fn default_priority() -> Priority {
    Priority::Medium
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parses_status_and_priority_aliases() {
        assert_eq!(
            "in-progress".parse::<IssueStatus>().unwrap(),
            IssueStatus::InProgress
        );
        assert_eq!("critical".parse::<Priority>().unwrap(), Priority::Urgent);
    }

    #[test]
    fn legacy_issue_reads_preserve_unknown_fields_and_file_bytes() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("issues")).unwrap();
        let path = dir.path().join("issues/WD-1.toml");
        let raw = "key='WD-1'\ntitle='External metadata'\nexternal_id='lin-123'\nlinked_files=['src/main.rs']\n[agent_context]\nmodel='codex'\n";
        fs::write(&path, raw).unwrap();
        let store = WorkdeckStore::new(dir.path());
        let issue = store.load_issues().unwrap().remove(0);
        assert_eq!(issue.extra["external_id"].as_str(), Some("lin-123"));
        assert_eq!(
            issue.extra["agent_context"]["model"].as_str(),
            Some("codex")
        );
        assert_eq!(issue.linked_files, ["src/main.rs"]);
        assert_eq!(fs::read_to_string(path).unwrap(), raw);
    }

    #[test]
    fn legacy_reference_reads_keep_project_cycle_and_label_fields() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("projects.toml"), "[[projects]]\nid='workdeck-mvp'\nname='Workdeck MVP'\ncreated_at='2026-09-01T00:00:00Z'\nupdated_at='2026-09-02T00:00:00Z'\n").unwrap();
        fs::write(
            dir.path().join("cycles.toml"),
            "[[cycles]]\nid='mvp'\nname='MVP'\nstarts_at='2026-05-24'\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("labels.toml"),
            "[[labels]]\nid='git'\nname='Git'\ncolor='green'\n",
        )
        .unwrap();
        let refs = WorkdeckStore::new(dir.path())
            .load_reference_data()
            .unwrap();
        assert_eq!(refs.projects[0].name, "Workdeck MVP");
        assert_eq!(refs.projects[0].updated_at, "2026-09-02T00:00:00Z");
        assert_eq!(refs.cycles[0].starts_at, "2026-05-24");
        assert_eq!(refs.labels[0].color, "green");
    }

    #[test]
    fn legacy_session_reads_preserve_unknown_fields_and_touched_files() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("agents")).unwrap();
        let path = dir.path().join("agents/session-1.toml");
        let raw = "id='session-1'\ntitle='Imported'\nexternal_trace_id='trace-123'\n[[touched_files]]\npath='src/main.rs'\nchange_type='modified'\n[runner]\nhost='local'\n";
        fs::write(&path, raw).unwrap();
        let session = WorkdeckStore::new(dir.path())
            .load_agent_sessions()
            .unwrap()
            .remove(0);
        assert_eq!(
            session.extra["external_trace_id"].as_str(),
            Some("trace-123")
        );
        assert_eq!(session.extra["runner"]["host"].as_str(), Some("local"));
        assert_eq!(session.touched_files[0].path, "src/main.rs");
        assert_eq!(fs::read_to_string(path).unwrap(), raw);
    }

    #[test]
    fn loads_events_without_initializing_store() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("legacy");
        let store = WorkdeckStore::new(&root);
        assert!(store.load_events().unwrap().is_empty());
        assert!(!root.exists());
        fs::create_dir(&root).unwrap();
        let raw = "{\"kind\":\"test_event\",\"payload\":{\"key\":\"value\"}}\n";
        fs::write(root.join("events.jsonl"), raw).unwrap();
        let events = store.load_events().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, "test_event");
        assert_eq!(events[0].payload["key"], "value");
        assert_eq!(fs::read_to_string(root.join("events.jsonl")).unwrap(), raw);
    }
}
