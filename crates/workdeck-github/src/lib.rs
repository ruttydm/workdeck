use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

const JSON_RESPONSE_LIMIT: usize = 16 * 1024 * 1024;
const LOG_RESPONSE_LIMIT: usize = 50 * 1024 * 1024;
const ARTIFACT_RESPONSE_LIMIT: u64 = 512 * 1024 * 1024;
const STDERR_LIMIT: usize = 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);
const MAX_PAGES: usize = 20;
const PAGE_SIZE: usize = 100;

#[derive(Debug, Clone)]
pub struct GitHubClient {
    executable: String,
}

#[derive(Debug, Clone, Default)]
pub struct GitHubCancellation(Arc<AtomicBool>);

impl GitHubCancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

impl Default for GitHubClient {
    fn default() -> Self {
        Self::new("gh")
    }
}

impl GitHubClient {
    pub fn new(executable: impl Into<String>) -> Self {
        Self {
            executable: executable.into(),
        }
    }

    pub fn status(&self) -> GitHubStatus {
        let cancellation = GitHubCancellation::default();
        match self.run_once(
            &["auth", "status", "--hostname", "github.com"],
            STDERR_LIMIT,
            &cancellation,
        ) {
            Ok(output) => GitHubStatus {
                installed: true,
                authenticated: output.status.success(),
                detail: sanitized_status(&output),
            },
            Err(error) => GitHubStatus {
                installed: false,
                authenticated: false,
                detail: error.to_string(),
            },
        }
    }

    pub fn pull_request(&self, repository: &str, number: u64) -> Result<PullRequestSnapshot> {
        self.pull_request_with_cancellation(repository, number, &GitHubCancellation::default())
    }

    pub fn pull_requests(&self, repository: &str) -> Result<Vec<PullRequestListItem>> {
        validate_repository(repository)?;
        let pulls: Vec<PullRequestListWire> = self.api_paginated_with_cancellation(
            &format!("repos/{repository}/pulls?state=all&per_page={PAGE_SIZE}"),
            &GitHubCancellation::default(),
        )?;
        Ok(pulls
            .into_iter()
            .map(|pull| PullRequestListItem {
                number: pull.number,
                title: pull.title,
                url: pull.html_url,
                state: pull.state,
                draft: pull.draft,
                author: pull.user.login,
                base_ref: pull.base.ref_name,
                head_ref: pull.head.ref_name,
                updated_at: pull.updated_at,
            })
            .collect())
    }

    pub fn pull_request_with_cancellation(
        &self,
        repository: &str,
        number: u64,
        cancellation: &GitHubCancellation,
    ) -> Result<PullRequestSnapshot> {
        validate_repository(repository)?;
        let pull: PullRequest = self.api_json_with_cancellation(
            &format!("repos/{repository}/pulls/{number}"),
            &[],
            cancellation,
        )?;
        let files: Vec<PullRequestFile> = self.api_paginated_with_cancellation(
            &format!("repos/{repository}/pulls/{number}/files?per_page=100"),
            cancellation,
        )?;
        let commits: Vec<PullRequestCommit> = self.api_paginated_with_cancellation(
            &format!("repos/{repository}/pulls/{number}/commits?per_page=100"),
            cancellation,
        )?;
        let checks: CheckRuns = self.api_json_with_cancellation(
            &format!(
                "repos/{repository}/commits/{}/check-runs?per_page=100",
                pull.head.sha
            ),
            &["-H", "Accept: application/vnd.github+json"],
            cancellation,
        )?;
        Ok(PullRequestSnapshot {
            repository: repository.to_string(),
            number,
            title: pull.title,
            url: pull.html_url,
            state: pull.state,
            draft: pull.draft,
            author: pull.user.login,
            base_ref: pull.base.ref_name,
            base_sha: pull.base.sha,
            head_ref: pull.head.ref_name,
            head_sha: pull.head.sha,
            additions: pull.additions,
            deletions: pull.deletions,
            changed_files: pull.changed_files,
            files,
            commits,
            checks: checks.check_runs,
        })
    }

    pub fn file_at_revision(
        &self,
        repository: &str,
        revision: &str,
        path: &Path,
    ) -> Result<Vec<u8>> {
        self.file_at_revision_with_cancellation(
            repository,
            revision,
            path,
            &GitHubCancellation::default(),
        )
    }

    pub fn file_at_revision_with_cancellation(
        &self,
        repository: &str,
        revision: &str,
        path: &Path,
        cancellation: &GitHubCancellation,
    ) -> Result<Vec<u8>> {
        validate_repository(repository)?;
        if revision.is_empty() {
            bail!("GitHub revision cannot be empty");
        }
        let path = path
            .to_str()
            .context("GitHub paths must be valid UTF-8")?
            .split('/')
            .map(percent_encode_component)
            .collect::<Vec<_>>()
            .join("/");
        let endpoint = format!("repos/{repository}/contents/{path}");
        let revision_argument = format!("ref={revision}");
        let arguments = [
            "api",
            "--allow-escape-sequences",
            "-X",
            "GET",
            "-H",
            "Accept: application/vnd.github.raw+json",
            "-f",
            revision_argument.as_str(),
            endpoint.as_str(),
        ];
        let output =
            self.run_limited_with_cancellation(&arguments, JSON_RESPONSE_LIMIT, cancellation)?;
        Ok(output.stdout)
    }

    pub fn workflow_run(&self, repository: &str, run_id: &str) -> Result<WorkflowRunSnapshot> {
        self.workflow_run_with_cancellation(repository, run_id, &GitHubCancellation::default())
    }

    pub fn workflow_runs(&self, repository: &str) -> Result<Vec<WorkflowRunListItem>> {
        validate_repository(repository)?;
        let runs: WorkflowRuns = self.api_json_with_cancellation(
            &format!("repos/{repository}/actions/runs?per_page={PAGE_SIZE}"),
            &[],
            &GitHubCancellation::default(),
        )?;
        Ok(runs.workflow_runs.into_iter().map(Into::into).collect())
    }

    pub fn workflow_run_with_cancellation(
        &self,
        repository: &str,
        run_id: &str,
        cancellation: &GitHubCancellation,
    ) -> Result<WorkflowRunSnapshot> {
        validate_repository(repository)?;
        validate_numeric_id(run_id, "workflow run")?;
        let run: WorkflowRun = self.api_json_with_cancellation(
            &format!("repos/{repository}/actions/runs/{run_id}"),
            &[],
            cancellation,
        )?;
        let jobs = self.workflow_jobs_with_cancellation(repository, run_id, cancellation)?;
        let artifacts =
            self.workflow_artifacts_with_cancellation(repository, run_id, cancellation)?;
        Ok(WorkflowRunSnapshot {
            id: run.id,
            name: run.name,
            display_title: run.display_title,
            status: run.status,
            conclusion: run.conclusion,
            html_url: run.html_url,
            head_sha: run.head_sha,
            head_branch: run.head_branch,
            event: run.event,
            run_attempt: run.run_attempt.max(1),
            actor: run.actor.map(|actor| actor.login),
            created_at: run.created_at,
            updated_at: run.updated_at,
            jobs,
            artifacts,
        })
    }

    pub fn job_log(&self, repository: &str, job_id: u64) -> Result<Vec<u8>> {
        self.job_log_with_cancellation(repository, job_id, &GitHubCancellation::default())
    }

    pub fn job_log_with_cancellation(
        &self,
        repository: &str,
        job_id: u64,
        cancellation: &GitHubCancellation,
    ) -> Result<Vec<u8>> {
        validate_repository(repository)?;
        let endpoint = format!("repos/{repository}/actions/jobs/{job_id}/logs");
        let arguments = raw_download_arguments(&endpoint);
        let output =
            self.run_limited_with_cancellation(&arguments, LOG_RESPONSE_LIMIT, cancellation)?;
        Ok(output.stdout)
    }

    pub fn download_artifact(
        &self,
        repository: &str,
        artifact_id: u64,
        destination: &Path,
    ) -> Result<PathBuf> {
        self.download_artifact_with_cancellation(
            repository,
            artifact_id,
            destination,
            &GitHubCancellation::default(),
        )
    }

    pub fn download_artifact_with_cancellation(
        &self,
        repository: &str,
        artifact_id: u64,
        destination: &Path,
        cancellation: &GitHubCancellation,
    ) -> Result<PathBuf> {
        validate_repository(repository)?;
        if cancellation.is_cancelled() {
            bail!("GitHub request was cancelled");
        }
        fs::create_dir_all(destination)?;
        let name = self.artifact_name_with_cancellation(repository, artifact_id, cancellation)?;
        let endpoint = format!("repos/{repository}/actions/artifacts/{artifact_id}/zip");
        let arguments = raw_download_arguments(&endpoint);
        let path = destination.join(format!(
            "{}-{artifact_id}.zip",
            safe_artifact_filename(&name)
        ));
        self.run_to_file_with_cancellation(
            &arguments,
            &path,
            ARTIFACT_RESPONSE_LIMIT,
            cancellation,
        )?;
        Ok(path)
    }

    fn artifact_name_with_cancellation(
        &self,
        repository: &str,
        artifact_id: u64,
        cancellation: &GitHubCancellation,
    ) -> Result<String> {
        let artifact: WorkflowArtifact = self.api_json_with_cancellation(
            &format!("repos/{repository}/actions/artifacts/{artifact_id}"),
            &[],
            cancellation,
        )?;
        Ok(artifact.name)
    }

    fn api_json_with_cancellation<T: serde::de::DeserializeOwned>(
        &self,
        endpoint: &str,
        extra: &[&str],
        cancellation: &GitHubCancellation,
    ) -> Result<T> {
        let mut arguments = vec!["api"];
        arguments.extend_from_slice(extra);
        arguments.push(endpoint);
        let output =
            self.run_limited_with_cancellation(&arguments, JSON_RESPONSE_LIMIT, cancellation)?;
        serde_json::from_slice(&output.stdout)
            .with_context(|| format!("GitHub returned invalid JSON for {endpoint}"))
    }

    fn api_paginated_with_cancellation<T: serde::de::DeserializeOwned>(
        &self,
        endpoint: &str,
        cancellation: &GitHubCancellation,
    ) -> Result<Vec<T>> {
        let mut values = Vec::new();
        for page in 1..=MAX_PAGES {
            let separator = if endpoint.contains('?') { '&' } else { '?' };
            let page_endpoint = format!("{endpoint}{separator}page={page}");
            let mut page_values: Vec<T> =
                self.api_json_with_cancellation(&page_endpoint, &[], cancellation)?;
            let count = page_values.len();
            values.append(&mut page_values);
            if count < PAGE_SIZE {
                return Ok(values);
            }
        }
        bail!("GitHub pagination exceeded the {MAX_PAGES}-page review limit for {endpoint}")
    }

    fn run_limited_with_cancellation(
        &self,
        arguments: &[&str],
        stdout_limit: usize,
        cancellation: &GitHubCancellation,
    ) -> Result<Output> {
        let mut last_error = String::new();
        for attempt in 0..3 {
            if cancellation.is_cancelled() {
                bail!("GitHub request was cancelled");
            }
            let output = self.run_once(arguments, stdout_limit, cancellation)?;
            if output.status.success() {
                return Ok(output);
            }
            last_error = sanitize_cli_error(&String::from_utf8_lossy(&output.stderr));
            if attempt == 2 || !transient_provider_error(&last_error) {
                break;
            }
            let delay = Duration::from_millis(150 * (1 << attempt));
            let retry_deadline = Instant::now() + delay;
            while Instant::now() < retry_deadline {
                if cancellation.is_cancelled() {
                    bail!("GitHub request was cancelled");
                }
                thread::sleep(Duration::from_millis(10));
            }
        }
        bail!("GitHub request failed: {last_error}")
    }

    fn run_once(
        &self,
        arguments: &[&str],
        stdout_limit: usize,
        cancellation: &GitHubCancellation,
    ) -> Result<Output> {
        let mut child = Command::new(&self.executable)
            .args(arguments)
            .env("GH_PAGER", "cat")
            .env("NO_COLOR", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to run {}", self.executable))?;
        let stdout = child
            .stdout
            .take()
            .context("GitHub stdout was unavailable")?;
        let stderr = child
            .stderr
            .take()
            .context("GitHub stderr was unavailable")?;
        let stdout_reader = thread::spawn(move || read_bounded(stdout, stdout_limit));
        let stderr_reader = thread::spawn(move || read_bounded(stderr, STDERR_LIMIT));
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if cancellation.is_cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                bail!("GitHub request was cancelled");
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                bail!("GitHub request exceeded the 45 second deadline");
            }
            thread::sleep(Duration::from_millis(10));
        };
        let (stdout, stdout_exceeded) = stdout_reader
            .join()
            .map_err(|_| anyhow::anyhow!("GitHub stdout reader panicked"))??;
        let (stderr, _) = stderr_reader
            .join()
            .map_err(|_| anyhow::anyhow!("GitHub stderr reader panicked"))??;
        if stdout_exceeded {
            bail!(
                "GitHub response exceeded the {} MiB limit",
                stdout_limit.div_ceil(1024 * 1024)
            );
        }
        Ok(Output {
            status,
            stdout,
            stderr,
        })
    }

    fn run_to_file_with_cancellation(
        &self,
        arguments: &[&str],
        destination: &Path,
        limit: u64,
        cancellation: &GitHubCancellation,
    ) -> Result<()> {
        let parent = destination
            .parent()
            .context("artifact destination has no parent")?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        let output_file = temporary.reopen()?;
        let mut child = Command::new(&self.executable)
            .args(arguments)
            .env("GH_PAGER", "cat")
            .env("NO_COLOR", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdout(Stdio::from(output_file))
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to run {}", self.executable))?;
        let stderr = child
            .stderr
            .take()
            .context("GitHub stderr was unavailable")?;
        let stderr_reader = thread::spawn(move || read_bounded(stderr, STDERR_LIMIT));
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if cancellation.is_cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                bail!("GitHub artifact download was cancelled");
            }
            if temporary.as_file().metadata()?.len() > limit {
                let _ = child.kill();
                let _ = child.wait();
                bail!("GitHub artifact exceeded the 512 MiB limit");
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                bail!("GitHub artifact download exceeded the 45 second deadline");
            }
            thread::sleep(Duration::from_millis(20));
        };
        let (stderr, _) = stderr_reader
            .join()
            .map_err(|_| anyhow::anyhow!("GitHub stderr reader panicked"))??;
        if !status.success() {
            bail!(
                "GitHub request failed: {}",
                sanitize_cli_error(&String::from_utf8_lossy(&stderr))
            );
        }
        if temporary.as_file().metadata()?.len() > limit {
            bail!("GitHub artifact exceeded the 512 MiB limit");
        }
        temporary.as_file_mut().flush()?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(destination)
            .map_err(|error| anyhow::anyhow!(error.error))?;
        Ok(())
    }

    fn workflow_jobs_with_cancellation(
        &self,
        repository: &str,
        run_id: &str,
        cancellation: &GitHubCancellation,
    ) -> Result<Vec<WorkflowJob>> {
        let mut values = Vec::new();
        for page in 1..=MAX_PAGES {
            let page_data: Jobs = self.api_json_with_cancellation(
                &format!(
                    "repos/{repository}/actions/runs/{run_id}/jobs?per_page={PAGE_SIZE}&page={page}"
                ),
                &[],
                cancellation,
            )?;
            let count = page_data.jobs.len();
            values.extend(page_data.jobs);
            if count < PAGE_SIZE {
                return Ok(values);
            }
        }
        bail!("GitHub workflow jobs exceeded the {MAX_PAGES}-page review limit")
    }

    fn workflow_artifacts_with_cancellation(
        &self,
        repository: &str,
        run_id: &str,
        cancellation: &GitHubCancellation,
    ) -> Result<Vec<WorkflowArtifact>> {
        let mut values = Vec::new();
        for page in 1..=MAX_PAGES {
            let page_data: Artifacts = self.api_json_with_cancellation(
                &format!(
                    "repos/{repository}/actions/runs/{run_id}/artifacts?per_page={PAGE_SIZE}&page={page}"
                ),
                &[],
                cancellation,
            )?;
            let count = page_data.artifacts.len();
            values.extend(page_data.artifacts);
            if count < PAGE_SIZE {
                return Ok(values);
            }
        }
        bail!("GitHub workflow artifacts exceeded the {MAX_PAGES}-page review limit")
    }
}

fn read_bounded(mut reader: impl Read, limit: usize) -> std::io::Result<(Vec<u8>, bool)> {
    let mut retained = Vec::with_capacity(limit.min(64 * 1024));
    let mut exceeded = false;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let remaining = limit.saturating_sub(retained.len());
        let keep = remaining.min(count);
        retained.extend_from_slice(&buffer[..keep]);
        exceeded |= keep < count;
    }
    Ok((retained, exceeded))
}

fn transient_provider_error(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    [
        "rate limit",
        "http 429",
        "http 502",
        "http 503",
        "timeout",
        "temporarily unavailable",
    ]
    .iter()
    .any(|needle| value.contains(needle))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubStatus {
    pub installed: bool,
    pub authenticated: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequestListItem {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub state: String,
    pub draft: bool,
    pub author: String,
    pub base_ref: String,
    pub head_ref: String,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRunListItem {
    pub id: u64,
    pub name: String,
    pub url: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub head_branch: Option<String>,
    pub head_sha: String,
    pub run_started_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequestSnapshot {
    pub repository: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub state: String,
    pub draft: bool,
    pub author: String,
    pub base_ref: String,
    pub base_sha: String,
    pub head_ref: String,
    pub head_sha: String,
    pub additions: u64,
    pub deletions: u64,
    pub changed_files: u64,
    pub files: Vec<PullRequestFile>,
    pub commits: Vec<PullRequestCommit>,
    pub checks: Vec<CheckRun>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequestFile {
    pub sha: String,
    pub filename: String,
    pub status: String,
    pub additions: u64,
    pub deletions: u64,
    pub changes: u64,
    pub previous_filename: Option<String>,
    pub patch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequestCommit {
    pub sha: String,
    pub commit: CommitDetails,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitDetails {
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckRun {
    pub id: u64,
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub html_url: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub output: CheckOutput,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckOutput {
    pub title: Option<String>,
    pub summary: Option<String>,
    pub text: Option<String>,
    pub annotations_count: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct PullRequest {
    title: String,
    html_url: String,
    state: String,
    #[serde(default)]
    draft: bool,
    user: User,
    base: PullRef,
    head: PullRef,
    additions: u64,
    deletions: u64,
    changed_files: u64,
}

#[derive(Debug, Deserialize)]
struct PullRequestListWire {
    number: u64,
    title: String,
    html_url: String,
    state: String,
    #[serde(default)]
    draft: bool,
    user: User,
    base: PullRef,
    head: PullRef,
    updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize)]
struct User {
    login: String,
}

#[derive(Debug, Deserialize)]
struct PullRef {
    #[serde(rename = "ref")]
    ref_name: String,
    sha: String,
}

#[derive(Debug, Deserialize)]
struct CheckRuns {
    #[serde(default)]
    check_runs: Vec<CheckRun>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRunSnapshot {
    pub id: u64,
    pub name: Option<String>,
    pub display_title: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub html_url: String,
    pub head_sha: String,
    #[serde(default)]
    pub head_branch: Option<String>,
    pub event: String,
    #[serde(default = "default_run_attempt")]
    pub run_attempt: u64,
    #[serde(default)]
    pub actor: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    pub jobs: Vec<WorkflowJob>,
    pub artifacts: Vec<WorkflowArtifact>,
}

fn default_run_attempt() -> u64 {
    1
}

#[derive(Debug, Deserialize)]
struct WorkflowRun {
    id: u64,
    name: Option<String>,
    display_title: String,
    status: String,
    conclusion: Option<String>,
    html_url: String,
    head_sha: String,
    #[serde(default)]
    head_branch: Option<String>,
    event: String,
    #[serde(default = "default_run_attempt")]
    run_attempt: u64,
    #[serde(default)]
    actor: Option<User>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WorkflowRuns {
    #[serde(default)]
    workflow_runs: Vec<WorkflowRunListItemWire>,
}

#[derive(Debug, Deserialize)]
struct WorkflowRunListItemWire {
    id: u64,
    name: Option<String>,
    display_title: String,
    html_url: String,
    status: String,
    conclusion: Option<String>,
    head_branch: Option<String>,
    head_sha: String,
    run_started_at: chrono::DateTime<chrono::Utc>,
}

impl From<WorkflowRunListItemWire> for WorkflowRunListItem {
    fn from(run: WorkflowRunListItemWire) -> Self {
        Self {
            id: run.id,
            name: run.name.unwrap_or(run.display_title),
            url: run.html_url,
            status: run.status,
            conclusion: run.conclusion,
            head_branch: run.head_branch,
            head_sha: run.head_sha,
            run_started_at: run.run_started_at,
        }
    }
}

#[derive(Debug, Deserialize)]
struct Jobs {
    #[serde(default)]
    jobs: Vec<WorkflowJob>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowJob {
    pub id: u64,
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    #[serde(default)]
    pub html_url: Option<String>,
    #[serde(default)]
    pub runner_name: Option<String>,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub completed_at: Option<String>,
    #[serde(default)]
    pub steps: Vec<WorkflowStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub number: u64,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub completed_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Artifacts {
    #[serde(default)]
    artifacts: Vec<WorkflowArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowArtifact {
    pub id: u64,
    pub name: String,
    pub size_in_bytes: u64,
    pub expired: bool,
    pub archive_download_url: String,
}

fn validate_repository(repository: &str) -> Result<()> {
    let mut parts = repository.split('/');
    let owner = parts.next().unwrap_or_default();
    let name = parts.next().unwrap_or_default();
    if owner.is_empty()
        || name.is_empty()
        || parts.next().is_some()
        || !owner.chars().all(safe_repository_character)
        || !name.chars().all(safe_repository_character)
    {
        bail!("invalid GitHub repository {repository}; expected owner/name");
    }
    Ok(())
}

fn safe_repository_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
}

fn safe_artifact_filename(value: &str) -> String {
    let normalized = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let normalized = normalized
        .trim_matches(|character| character == '.' || character == '-')
        .chars()
        .take(96)
        .collect::<String>();
    if normalized.is_empty() {
        "artifact".into()
    } else {
        normalized
    }
}

fn raw_download_arguments(endpoint: &str) -> [&str; 3] {
    ["api", "--allow-escape-sequences", endpoint]
}

fn validate_numeric_id(value: &str, label: &str) -> Result<()> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        bail!("invalid {label} ID {value}");
    }
    Ok(())
}

fn percent_encode_component(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                vec![byte as char]
            } else {
                format!("%{byte:02X}").chars().collect()
            }
        })
        .collect()
}

fn sanitized_status(output: &Output) -> String {
    let source = if output.status.success() {
        &output.stdout
    } else {
        &output.stderr
    };
    sanitize_cli_error(&String::from_utf8_lossy(source))
}

fn sanitize_cli_error(value: &str) -> String {
    value
        .lines()
        .filter(|line| !line.to_ascii_lowercase().contains("token"))
        .take(4)
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_repository_coordinates() {
        assert!(validate_repository("openai/codex").is_ok());
        assert!(validate_repository("openai").is_err());
        assert!(validate_repository("../secret/repo").is_err());
    }

    #[test]
    fn artifact_names_cannot_escape_the_download_directory() {
        assert_eq!(
            safe_artifact_filename("../../reports/nightly"),
            "reports-nightly"
        );
        assert_eq!(safe_artifact_filename(".."), "artifact");
        assert_eq!(safe_artifact_filename("quality report"), "quality-report");
    }

    #[test]
    fn encodes_content_path_segments() {
        assert_eq!(
            percent_encode_component("hello world.rs"),
            "hello%20world.rs"
        );
        assert_eq!(percent_encode_component("lib.rs"), "lib.rs");
    }

    #[test]
    fn raw_downloads_allow_escape_sequences_without_printing_them() {
        assert_eq!(
            raw_download_arguments("repos/openai/codex/actions/jobs/42/logs"),
            [
                "api",
                "--allow-escape-sequences",
                "repos/openai/codex/actions/jobs/42/logs"
            ]
        );
    }

    #[test]
    fn status_sanitizer_drops_token_lines() {
        assert_eq!(
            sanitize_cli_error("not logged in\nToken: secret\nrun gh auth login"),
            "not logged in run gh auth login"
        );
    }

    #[test]
    fn parses_pull_request_files_without_patch() {
        let file: PullRequestFile = serde_json::from_value(serde_json::json!({
            "sha": "abc",
            "filename": "large.bin",
            "status": "modified",
            "additions": 0,
            "deletions": 0,
            "changes": 0,
            "previous_filename": null
        }))
        .unwrap();
        assert!(file.patch.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn provider_contract_covers_pr_run_logs_and_artifact_downloads() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt as _;

        let fixture = tempfile::tempdir().unwrap();
        let executable = fixture.path().join("fake-gh");
        fs::write(
            &executable,
            r##"#!/bin/sh
args="$*"
case "$args" in
  *"pulls/7/files"*) printf '%s\n' '[{"sha":"file","filename":"src/lib.rs","status":"modified","additions":2,"deletions":1,"changes":3,"previous_filename":null,"patch":"@@"}]' ;;
  *"pulls/7/commits"*) printf '%s\n' '[{"sha":"commit","commit":{"message":"Ship it"}}]' ;;
  *"commits/head/check-runs"*) printf '%s\n' '{"check_runs":[{"id":8,"name":"test","status":"completed","conclusion":"success","html_url":null,"started_at":null,"completed_at":null,"output":{"title":null,"summary":null,"text":null,"annotations_count":0}}]}' ;;
  *"pulls/7"*) printf '%s\n' '{"title":"Agent burst","html_url":"https://example.test/pr/7","state":"open","draft":false,"user":{"login":"agent"},"base":{"ref":"main","sha":"base"},"head":{"ref":"burst","sha":"head"},"additions":2,"deletions":1,"changed_files":1}' ;;
  *"actions/runs/42/jobs"*) printf '%s\n' '{"jobs":[{"id":99,"name":"quality","status":"completed","conclusion":"success","html_url":"https://github.com/owner/repository/actions/runs/42/job/99","runner_name":"runner-1","started_at":"2026-08-26T09:00:00Z","completed_at":"2026-08-26T09:01:00Z","steps":[{"name":"test","status":"completed","conclusion":"success","number":1,"started_at":"2026-08-26T09:00:10Z","completed_at":"2026-08-26T09:00:50Z"}]}]}' ;;
  *"actions/runs/42/artifacts"*) printf '%s\n' '{"artifacts":[{"id":77,"name":"report","size_in_bytes":4,"expired":false,"archive_download_url":"https://example.test/report"}]}' ;;
  *"actions/runs/42"*) printf '%s\n' '{"id":42,"name":"CI","display_title":"Quality","status":"completed","conclusion":"success","html_url":"https://github.com/owner/repository/actions/runs/42","head_sha":"head","head_branch":"main","event":"push","run_attempt":2,"actor":{"login":"agent"},"created_at":"2026-08-26T09:00:00Z","updated_at":"2026-08-26T09:01:00Z"}' ;;
  *"actions/jobs/99/logs"*) printf '\033[32mjob passed\033[0m\n' ;;
  *"actions/artifacts/77/zip"*) printf 'PK\003\004' ;;
  *"actions/artifacts/77"*) printf '%s\n' '{"id":77,"name":"report","size_in_bytes":4,"expired":false,"archive_download_url":"https://example.test/report"}' ;;
  *) printf '%s\n' "unexpected fake gh arguments: $args" >&2; exit 1 ;;
esac
"##,
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        let client = GitHubClient::new(executable.to_string_lossy());

        let pull = client.pull_request("owner/repository", 7).unwrap();
        assert_eq!(pull.title, "Agent burst");
        assert_eq!(pull.files.len(), 1);
        assert_eq!(pull.commits.len(), 1);
        assert_eq!(pull.checks.len(), 1);

        let run = client.workflow_run("owner/repository", "42").unwrap();
        assert_eq!(run.jobs[0].steps.len(), 1);
        assert_eq!(run.run_attempt, 2);
        assert_eq!(run.head_branch.as_deref(), Some("main"));
        assert_eq!(run.actor.as_deref(), Some("agent"));
        assert_eq!(run.jobs[0].runner_name.as_deref(), Some("runner-1"));
        assert_eq!(
            run.jobs[0].started_at.as_deref(),
            Some("2026-08-26T09:00:00Z")
        );
        assert_eq!(run.artifacts[0].id, 77);
        let log = client.job_log("owner/repository", 99).unwrap();
        assert!(log.starts_with(b"\x1b[32mjob passed"));

        let downloads = fixture.path().join("downloads");
        let downloaded = client
            .download_artifact("owner/repository", 77, &downloads)
            .unwrap();
        assert_eq!(downloaded, downloads.join("report-77.zip"));
        assert_eq!(
            fs::read(downloads.join("report-77.zip")).unwrap(),
            b"PK\x03\x04"
        );
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_terminates_an_inflight_provider_process() {
        use std::os::unix::fs::PermissionsExt as _;

        let fixture = tempfile::tempdir().unwrap();
        let executable = fixture.path().join("slow-gh");
        fs::write(&executable, "#!/bin/sh\nexec sleep 5\n").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        let client = GitHubClient::new(executable.to_string_lossy());
        let cancellation = GitHubCancellation::default();
        let worker_cancellation = cancellation.clone();
        let started = Instant::now();
        let worker = thread::spawn(move || {
            client.job_log_with_cancellation("owner/repository", 99, &worker_cancellation)
        });
        thread::sleep(Duration::from_millis(40));
        cancellation.cancel();
        let error = worker.join().unwrap().unwrap_err().to_string();
        assert!(error.contains("cancelled"));
        assert!(started.elapsed() < Duration::from_millis(150));
    }

    #[cfg(unix)]
    #[test]
    fn artifact_metadata_lookup_is_cancellable_before_download() {
        use std::os::unix::fs::PermissionsExt as _;

        let fixture = tempfile::tempdir().unwrap();
        let executable = fixture.path().join("slow-gh");
        fs::write(&executable, "#!/bin/sh\nexec sleep 5\n").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        let client = GitHubClient::new(executable.to_string_lossy());
        let cancellation = GitHubCancellation::default();
        let worker_cancellation = cancellation.clone();
        let downloads = fixture.path().join("downloads");
        let started = Instant::now();
        let worker = thread::spawn(move || {
            client.download_artifact_with_cancellation(
                "owner/repository",
                77,
                &downloads,
                &worker_cancellation,
            )
        });
        thread::sleep(Duration::from_millis(40));
        cancellation.cancel();
        let error = worker.join().unwrap().unwrap_err().to_string();
        assert!(error.contains("cancelled"));
        assert!(started.elapsed() < Duration::from_millis(150));
    }
}
