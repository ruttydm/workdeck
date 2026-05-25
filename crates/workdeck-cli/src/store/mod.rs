use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;

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

    pub fn init(&self) -> Result<()> {
        fs::create_dir_all(self.issues_dir())
            .with_context(|| format!("failed to create {}", self.issues_dir().display()))?;
        fs::create_dir_all(self.agents_dir())
            .with_context(|| format!("failed to create {}", self.agents_dir().display()))?;
        fs::create_dir_all(self.root.join("index"))
            .with_context(|| format!("failed to create {}", self.root.join("index").display()))?;

        let config_path = self.root.join("config.toml");
        if !config_path.exists() {
            atomic_write(
                &config_path,
                r#"[ui]
theme = "auto"
preview = true

[paths]
data_dir = ".agents/workdeck"

[keys]
quit = "q"
refresh = "r"
search = "/"
help = "?"
changes = "c"
files = "f"
tasks = "i"
agents = "a"
toggle_preview = "t"
group_changes = "g"
toggle_dirstat = "w"
open_editor = "o"
copy = "y"
new_issue = "n"
edit_issue = "e"
status = "s"
priority = "p"
labels = "l"
assign = "A"
jump = "space"
link_file = "L"
"#,
            )?;
        }

        for file in ["projects.toml", "cycles.toml", "labels.toml"] {
            let path = self.root.join(file);
            if !path.exists() {
                atomic_write(&path, "")?;
            }
        }

        Ok(())
    }

    pub fn load_issues(&self) -> Result<Vec<Issue>> {
        let dir = self.issues_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut issues = Vec::new();
        for entry in fs::read_dir(&dir)
            .with_context(|| format!("failed to read issues dir {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("toml") {
                continue;
            }

            let raw = fs::read_to_string(&path)
                .with_context(|| format!("failed to read issue {}", path.display()))?;
            let issue: Issue = toml::from_str(&raw)
                .with_context(|| format!("failed to parse issue {}", path.display()))?;
            issues.push(issue);
        }

        issues.sort_by_key(|issue| issue_key_number(&issue.key).unwrap_or(u64::MAX));
        Ok(issues)
    }

    pub fn load_reference_data(&self) -> Result<ReferenceData> {
        Ok(ReferenceData {
            projects: read_toml_list::<ProjectsFile>(&self.root.join("projects.toml"))?.projects,
            cycles: read_toml_list::<CyclesFile>(&self.root.join("cycles.toml"))?.cycles,
            labels: read_toml_list::<LabelsFile>(&self.root.join("labels.toml"))?.labels,
        })
    }

    pub fn upsert_project(
        &self,
        id: Option<String>,
        name: String,
        description: Option<String>,
        status: Option<String>,
    ) -> Result<Project> {
        self.init()?;
        if name.trim().is_empty() {
            bail!("project name cannot be empty");
        }
        let id = normalized_reference_id(id, &name)?;
        let mut reference_data = self.load_reference_data()?;
        let project = if let Some(project) = reference_data
            .projects
            .iter_mut()
            .find(|project| project.id == id)
        {
            project.name = name;
            if let Some(description) = description {
                project.description = description;
            }
            if let Some(status) = status {
                project.status = status;
            }
            project.touch();
            project.clone()
        } else {
            let mut project = Project::new(id.clone(), name);
            if let Some(description) = description {
                project.description = description;
            }
            if let Some(status) = status {
                project.status = status;
            }
            reference_data.projects.push(project.clone());
            project
        };
        reference_data.projects.sort_by(|a, b| a.id.cmp(&b.id));
        self.save_projects(&reference_data.projects)?;
        self.append_event("project_saved", json!({ "id": project.id }))?;
        Ok(project)
    }

    pub fn upsert_cycle(
        &self,
        id: Option<String>,
        name: String,
        starts_at: Option<String>,
        ends_at: Option<String>,
        status: Option<String>,
    ) -> Result<Cycle> {
        self.init()?;
        if name.trim().is_empty() {
            bail!("cycle name cannot be empty");
        }
        let id = normalized_reference_id(id, &name)?;
        let mut reference_data = self.load_reference_data()?;
        let cycle = if let Some(cycle) = reference_data
            .cycles
            .iter_mut()
            .find(|cycle| cycle.id == id)
        {
            cycle.name = name;
            if let Some(starts_at) = starts_at {
                cycle.starts_at = starts_at;
            }
            if let Some(ends_at) = ends_at {
                cycle.ends_at = ends_at;
            }
            if let Some(status) = status {
                cycle.status = status;
            }
            cycle.clone()
        } else {
            let mut cycle = Cycle::new(id.clone(), name);
            if let Some(starts_at) = starts_at {
                cycle.starts_at = starts_at;
            }
            if let Some(ends_at) = ends_at {
                cycle.ends_at = ends_at;
            }
            if let Some(status) = status {
                cycle.status = status;
            }
            reference_data.cycles.push(cycle.clone());
            cycle
        };
        reference_data.cycles.sort_by(|a, b| a.id.cmp(&b.id));
        self.save_cycles(&reference_data.cycles)?;
        self.append_event("cycle_saved", json!({ "id": cycle.id }))?;
        Ok(cycle)
    }

    pub fn upsert_label(
        &self,
        id: Option<String>,
        name: String,
        color: Option<String>,
    ) -> Result<Label> {
        self.init()?;
        if name.trim().is_empty() {
            bail!("label name cannot be empty");
        }
        let id = normalized_reference_id(id, &name)?;
        let mut reference_data = self.load_reference_data()?;
        let label = if let Some(label) = reference_data
            .labels
            .iter_mut()
            .find(|label| label.id == id)
        {
            label.name = name;
            if let Some(color) = color {
                label.color = color;
            }
            label.clone()
        } else {
            let mut label = Label::new(id.clone(), name);
            if let Some(color) = color {
                label.color = color;
            }
            reference_data.labels.push(label.clone());
            label
        };
        reference_data.labels.sort_by(|a, b| a.id.cmp(&b.id));
        self.save_labels(&reference_data.labels)?;
        self.append_event("label_saved", json!({ "id": label.id }))?;
        Ok(label)
    }

    pub fn delete_project(&self, id: &str, force: bool) -> Result<Project> {
        self.init()?;
        if !force && self.load_issues()?.iter().any(|issue| issue.project == id) {
            bail!("project {id} is used by issues; pass --force to clear references");
        }
        let mut reference_data = self.load_reference_data()?;
        let index = reference_data
            .projects
            .iter()
            .position(|project| project.id == id)
            .with_context(|| format!("project {id} does not exist"))?;
        let project = reference_data.projects.remove(index);
        self.save_projects(&reference_data.projects)?;
        if force {
            for mut issue in self
                .load_issues()?
                .into_iter()
                .filter(|issue| issue.project == id)
            {
                issue.project.clear();
                issue.touch();
                self.save_issue(&issue)?;
            }
        }
        self.append_event("project_deleted", json!({ "id": id }))?;
        Ok(project)
    }

    pub fn delete_cycle(&self, id: &str, force: bool) -> Result<Cycle> {
        self.init()?;
        if !force && self.load_issues()?.iter().any(|issue| issue.cycle == id) {
            bail!("cycle {id} is used by issues; pass --force to clear references");
        }
        let mut reference_data = self.load_reference_data()?;
        let index = reference_data
            .cycles
            .iter()
            .position(|cycle| cycle.id == id)
            .with_context(|| format!("cycle {id} does not exist"))?;
        let cycle = reference_data.cycles.remove(index);
        self.save_cycles(&reference_data.cycles)?;
        if force {
            for mut issue in self
                .load_issues()?
                .into_iter()
                .filter(|issue| issue.cycle == id)
            {
                issue.cycle.clear();
                issue.touch();
                self.save_issue(&issue)?;
            }
        }
        self.append_event("cycle_deleted", json!({ "id": id }))?;
        Ok(cycle)
    }

    pub fn delete_label(&self, id: &str, force: bool) -> Result<Label> {
        self.init()?;
        if !force
            && self
                .load_issues()?
                .iter()
                .any(|issue| issue.labels.iter().any(|label| label == id))
        {
            bail!("label {id} is used by issues; pass --force to remove it from issues");
        }
        let mut reference_data = self.load_reference_data()?;
        let index = reference_data
            .labels
            .iter()
            .position(|label| label.id == id)
            .with_context(|| format!("label {id} does not exist"))?;
        let label = reference_data.labels.remove(index);
        self.save_labels(&reference_data.labels)?;
        if force {
            for mut issue in self
                .load_issues()?
                .into_iter()
                .filter(|issue| issue.labels.iter().any(|label| label == id))
            {
                issue.labels.retain(|label| label != id);
                issue.touch();
                self.save_issue(&issue)?;
            }
        }
        self.append_event("label_deleted", json!({ "id": id }))?;
        Ok(label)
    }

    pub fn load_agent_sessions(&self) -> Result<Vec<AgentSession>> {
        let dir = self.agents_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut sessions = Vec::new();
        for entry in fs::read_dir(&dir)
            .with_context(|| format!("failed to read agents dir {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("toml") {
                continue;
            }

            let raw = fs::read_to_string(&path)
                .with_context(|| format!("failed to read agent session {}", path.display()))?;
            let session: AgentSession = toml::from_str(&raw)
                .with_context(|| format!("failed to parse agent session {}", path.display()))?;
            sessions.push(session);
        }

        sessions.sort_by(|a, b| b.started_at.cmp(&a.started_at).then(a.id.cmp(&b.id)));
        Ok(sessions)
    }

    pub fn save_agent_session(&self, session: &AgentSession) -> Result<()> {
        self.init()?;
        if session.id.trim().is_empty() {
            bail!("agent session id cannot be empty");
        }
        if sanitize_key(&session.id).is_empty() {
            bail!("agent session id must contain ASCII letters, numbers, or dashes");
        }
        if session.title.trim().is_empty() {
            bail!("agent session title cannot be empty");
        }
        let path = self.agent_session_path(&session.id);
        let raw = toml::to_string_pretty(session)?;
        atomic_write(&path, &raw)?;
        self.append_event("agent_session_saved", json!({ "id": session.id }))?;
        Ok(())
    }

    pub fn create_issue(&self, title: String) -> Result<Issue> {
        self.init()?;
        let key = self.next_issue_key()?;
        let issue = Issue::new(key, title);
        self.save_issue(&issue)?;
        self.append_event("issue_created", json!({ "key": issue.key }))?;
        Ok(issue)
    }

    pub fn update_issue(&self, key: &str, update: IssueUpdate) -> Result<Issue> {
        let mut issue = self
            .load_issues()?
            .into_iter()
            .find(|issue| issue.key == key)
            .with_context(|| format!("issue {key} does not exist"))?;

        if let Some(title) = update.title {
            if title.trim().is_empty() {
                bail!("issue title cannot be empty");
            }
            issue.title = title;
        }
        if let Some(description) = update.description {
            issue.description = description;
        }
        if let Some(status) = update.status {
            issue.status = status;
        }
        if let Some(priority) = update.priority {
            issue.priority = priority;
        }
        if let Some(project) = update.project {
            issue.project = project;
        }
        if let Some(cycle) = update.cycle {
            issue.cycle = cycle;
        }
        if let Some(assignee) = update.assignee {
            issue.assignee = assignee;
        }
        if let Some(due_at) = update.due_at {
            issue.due_at = due_at;
        }
        if let Some(labels) = update.labels {
            issue.labels = labels;
            issue.labels.sort();
            issue.labels.dedup();
        }
        if let Some(linked_commits) = update.linked_commits {
            issue.linked_commits.extend(linked_commits);
            issue.linked_commits.sort();
            issue.linked_commits.dedup();
        }

        issue.touch();
        self.save_issue(&issue)?;
        self.append_event("issue_updated", json!({ "key": issue.key }))?;
        Ok(issue)
    }

    pub fn save_issue(&self, issue: &Issue) -> Result<()> {
        self.init()?;
        if !valid_issue_key(&issue.key) {
            bail!("issue key must look like WD-1");
        }

        let path = self.issue_path(&issue.key);
        let raw = toml::to_string_pretty(issue)?;
        atomic_write(&path, &raw)?;
        Ok(())
    }

    pub fn link_issue_file(&self, key: &str, file_path: &str) -> Result<Issue> {
        let mut issues = self.load_issues()?;
        let Some(issue) = issues.iter_mut().find(|issue| issue.key == key) else {
            bail!("issue {key} does not exist");
        };

        if !issue.linked_files.iter().any(|path| path == file_path) {
            issue.linked_files.push(file_path.to_string());
            issue.linked_files.sort();
            issue.touch();
            self.save_issue(issue)?;
            self.append_event(
                "issue_file_linked",
                json!({ "key": issue.key, "path": file_path }),
            )?;
        }

        Ok(issue.clone())
    }

    pub fn issue_file_path(&self, key: &str) -> PathBuf {
        self.issue_path(key)
    }

    pub fn agent_session_file_path(&self, id: &str) -> PathBuf {
        self.agent_session_path(id)
    }

    pub fn append_event(&self, kind: &str, payload: serde_json::Value) -> Result<()> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("failed to create {}", self.root.display()))?;
        let event = json!({
            "kind": kind,
            "payload": payload,
            "created_at": now(),
        });
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.root.join("events.jsonl"))
            .with_context(|| "failed to open events.jsonl")?;
        writeln!(file, "{event}")?;
        Ok(())
    }

    pub fn load_events(&self) -> Result<Vec<StoreEvent>> {
        let path = self.root.join("events.jsonl");
        if !path.exists() {
            return Ok(Vec::new());
        }

        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read events {}", path.display()))?;
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

    pub fn next_issue_key(&self) -> Result<String> {
        let max = self
            .load_issues()?
            .iter()
            .filter_map(|issue| issue_key_number(&issue.key))
            .max()
            .unwrap_or(0);
        Ok(format!("WD-{}", max + 1))
    }

    fn issues_dir(&self) -> PathBuf {
        self.root.join("issues")
    }

    fn agents_dir(&self) -> PathBuf {
        self.root.join("agents")
    }

    fn issue_path(&self, key: &str) -> PathBuf {
        self.issues_dir()
            .join(format!("{}.toml", sanitize_key(key)))
    }

    fn agent_session_path(&self, id: &str) -> PathBuf {
        self.agents_dir().join(format!("{}.toml", sanitize_key(id)))
    }

    fn save_projects(&self, projects: &[Project]) -> Result<()> {
        atomic_write(
            &self.root.join("projects.toml"),
            &toml::to_string_pretty(&ProjectsFile {
                projects: projects.to_vec(),
            })?,
        )
    }

    fn save_cycles(&self, cycles: &[Cycle]) -> Result<()> {
        atomic_write(
            &self.root.join("cycles.toml"),
            &toml::to_string_pretty(&CyclesFile {
                cycles: cycles.to_vec(),
            })?,
        )
    }

    fn save_labels(&self, labels: &[Label]) -> Result<()> {
        atomic_write(
            &self.root.join("labels.toml"),
            &toml::to_string_pretty(&LabelsFile {
                labels: labels.to_vec(),
            })?,
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct IssueUpdate {
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<IssueStatus>,
    pub priority: Option<Priority>,
    pub project: Option<String>,
    pub cycle: Option<String>,
    pub assignee: Option<String>,
    pub due_at: Option<String>,
    pub labels: Option<Vec<String>>,
    pub linked_commits: Option<Vec<String>>,
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
    if !path.exists() {
        return Ok(T::default());
    }

    let raw = fs::read_to_string(path)?;
    if raw.trim().is_empty() {
        return Ok(T::default());
    }

    toml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

fn atomic_write(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let tmp_path = path.with_extension(format!(
        "{}.{}.{}.tmp",
        path.extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("workdeck"),
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    fs::write(&tmp_path, content)
        .with_context(|| format!("failed to write temp file {}", tmp_path.display()))?;
    fs::rename(&tmp_path, path).with_context(|| {
        format!(
            "failed to atomically replace {} with {}",
            path.display(),
            tmp_path.display()
        )
    })?;
    Ok(())
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
    fn creates_sequential_issue_files() {
        let dir = tempdir().unwrap();
        let store = WorkdeckStore::new(dir.path().join(".agents/workdeck"));

        let first = store.create_issue("First issue".to_string()).unwrap();
        let second = store.create_issue("Second issue".to_string()).unwrap();

        assert_eq!(first.key, "WD-1");
        assert_eq!(second.key, "WD-2");
        assert!(
            dir.path()
                .join(".agents/workdeck/issues/WD-1.toml")
                .exists()
        );
        assert!(dir.path().join(".agents/workdeck/events.jsonl").exists());
    }

    #[test]
    fn links_issue_to_file_once() {
        let dir = tempdir().unwrap();
        let store = WorkdeckStore::new(dir.path().join(".agents/workdeck"));
        let issue = store.create_issue("Link me".to_string()).unwrap();

        store.link_issue_file(&issue.key, "src/main.rs").unwrap();
        store.link_issue_file(&issue.key, "src/main.rs").unwrap();

        let issue = store.load_issues().unwrap().remove(0);
        assert_eq!(issue.linked_files, vec!["src/main.rs"]);
    }

    #[test]
    fn rejects_invalid_issue_keys() {
        let dir = tempdir().unwrap();
        let store = WorkdeckStore::new(dir.path().join(".agents/workdeck"));
        let mut issue = Issue::new("../bad".to_string(), "Bad".to_string());
        issue.key = "../bad".to_string();

        let error = store.save_issue(&issue).unwrap_err().to_string();

        assert!(error.contains("WD-1"));
    }

    #[test]
    fn parses_status_and_priority_aliases() {
        assert_eq!(
            "in-progress".parse::<IssueStatus>().unwrap(),
            IssueStatus::InProgress
        );
        assert_eq!("critical".parse::<Priority>().unwrap(), Priority::Urgent);
    }

    #[test]
    fn updates_issue_fields_and_deduplicates_labels() {
        let dir = tempdir().unwrap();
        let store = WorkdeckStore::new(dir.path().join(".agents/workdeck"));
        let issue = store.create_issue("Original".to_string()).unwrap();

        let updated = store
            .update_issue(
                &issue.key,
                IssueUpdate {
                    title: Some("Updated".to_string()),
                    status: Some(IssueStatus::InProgress),
                    priority: Some(Priority::High),
                    due_at: Some("2026-05-31".to_string()),
                    labels: Some(vec![
                        "git".to_string(),
                        "git".to_string(),
                        "mvp".to_string(),
                    ]),
                    linked_commits: Some(vec![
                        "abc123".to_string(),
                        "abc123".to_string(),
                        "def456".to_string(),
                    ]),
                    ..IssueUpdate::default()
                },
            )
            .unwrap();

        assert_eq!(updated.title, "Updated");
        assert_eq!(updated.status, IssueStatus::InProgress);
        assert_eq!(updated.priority, Priority::High);
        assert_eq!(updated.due_at, "2026-05-31");
        assert_eq!(updated.labels, vec!["git", "mvp"]);
        assert_eq!(updated.linked_commits, vec!["abc123", "def456"]);
    }

    #[test]
    fn issue_updates_preserve_unknown_toml_fields() {
        let dir = tempdir().unwrap();
        let store = WorkdeckStore::new(dir.path().join(".agents/workdeck"));
        store.init().unwrap();
        fs::write(
            store.issue_file_path("WD-1"),
            r#"key = "WD-1"
title = "Has external metadata"
external_id = "lin-123"

[agent_context]
model = "codex"
"#,
        )
        .unwrap();

        let updated = store
            .update_issue(
                "WD-1",
                IssueUpdate {
                    status: Some(IssueStatus::InProgress),
                    ..IssueUpdate::default()
                },
            )
            .unwrap();

        assert_eq!(updated.extra["external_id"].as_str(), Some("lin-123"));
        let raw = fs::read_to_string(store.issue_file_path("WD-1")).unwrap();
        assert!(raw.contains("external_id = \"lin-123\""));
        assert!(raw.contains("[agent_context]"));
        assert!(raw.contains("model = \"codex\""));
    }

    #[test]
    fn saves_reference_data_as_reviewable_toml() {
        let dir = tempdir().unwrap();
        let store = WorkdeckStore::new(dir.path().join(".agents/workdeck"));

        let project = store
            .upsert_project(
                None,
                "Workdeck MVP".to_string(),
                Some("Initial release".to_string()),
                None,
            )
            .unwrap();
        let cycle = store
            .upsert_cycle(
                Some("mvp".to_string()),
                "MVP".to_string(),
                Some("2026-05-24".to_string()),
                None,
                None,
            )
            .unwrap();
        let label = store
            .upsert_label(None, "Git".to_string(), Some("green".to_string()))
            .unwrap();

        let reference_data = store.load_reference_data().unwrap();
        assert_eq!(project.id, "workdeck-mvp");
        assert_eq!(cycle.id, "mvp");
        assert_eq!(label.id, "git");
        assert_eq!(reference_data.projects[0].name, "Workdeck MVP");
        assert_eq!(reference_data.cycles[0].starts_at, "2026-05-24");
        assert_eq!(reference_data.labels[0].color, "green");
        assert!(
            fs::read_to_string(dir.path().join(".agents/workdeck/projects.toml"))
                .unwrap()
                .contains("[[projects]]")
        );
    }

    #[test]
    fn saves_and_loads_agent_sessions() {
        let dir = tempdir().unwrap();
        let store = WorkdeckStore::new(dir.path().join(".agents/workdeck"));
        let mut session = AgentSession::new("Build shell".to_string());
        session.agent = "codex".to_string();
        session.touched_files.push(AgentTouchedFile {
            path: "src/main.rs".to_string(),
            change_type: "modified".to_string(),
        });

        store.save_agent_session(&session).unwrap();

        let sessions = store.load_agent_sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title, "Build shell");
        assert_eq!(sessions[0].touched_files[0].path, "src/main.rs");
    }

    #[test]
    fn agent_sessions_preserve_unknown_toml_fields() {
        let dir = tempdir().unwrap();
        let store = WorkdeckStore::new(dir.path().join(".agents/workdeck"));
        store.init().unwrap();
        fs::write(
            store.agent_session_file_path("session-1"),
            r#"id = "session-1"
title = "Imported"
external_trace_id = "trace-123"

[runner]
host = "local"
"#,
        )
        .unwrap();

        let mut session = store.load_agent_sessions().unwrap().remove(0);
        session.status = "done".to_string();
        store.save_agent_session(&session).unwrap();

        let raw = fs::read_to_string(store.agent_session_file_path("session-1")).unwrap();
        assert!(raw.contains("external_trace_id = \"trace-123\""));
        assert!(raw.contains("[runner]"));
        assert!(raw.contains("host = \"local\""));
    }

    #[test]
    fn rejects_agent_sessions_without_safe_identity() {
        let dir = tempdir().unwrap();
        let store = WorkdeckStore::new(dir.path().join(".agents/workdeck"));
        let mut session = AgentSession::new("Bad".to_string());

        session.id = "///".to_string();
        assert!(store.save_agent_session(&session).is_err());

        session.id = "session-1".to_string();
        session.title.clear();
        assert!(store.save_agent_session(&session).is_err());
    }

    #[test]
    fn loads_events_without_initializing_store() {
        let dir = tempdir().unwrap();
        let store = WorkdeckStore::new(dir.path().join(".agents/workdeck"));

        assert!(store.load_events().unwrap().is_empty());
        assert!(!dir.path().join(".agents/workdeck").exists());

        store
            .append_event("test_event", json!({ "key": "value" }))
            .unwrap();
        let events = store.load_events().unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, "test_event");
        assert_eq!(events[0].payload["key"], "value");
    }
}
