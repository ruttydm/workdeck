//! Native, dependency-audited port of Hunk's GitHub pull-request CLI extension.

use serde_json::{Value, json};
use std::collections::BTreeMap;
#[cfg(unix)]
use std::fs;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::{Builder as TempBuilder, TempDir};
use url::Url;
use workdeck_extension_api::{
    API_VERSION, Capability, CliCommandExecution, CliCommandInvocation, CliCommandRegistration,
    CliCommandResult, CliOutputNotification, CliOutputStream, HandshakeResponse, JsonRpcError,
    JsonRpcRequest, JsonRpcResponse, Registration,
};

const COMMAND_NAME: &str = "gh";
const MAX_DIFF_BYTES: usize = 64 * 1024 * 1024;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const GITHUB_API_ORIGIN: &str = "https://api.github.com";
const ORIGIN_TIMEOUT: Duration = Duration::from_secs(5);
const NETWORK_TIMEOUT: Duration = Duration::from_secs(25);

pub const GITHUB_PR_HELP: &str = "Usage: workdeck gh <pull-request> [--repo <owner/repo>] [-- <patch-options...>]\n\nReview a GitHub pull request without requiring the gh CLI.\n\nPull request forms:\n  123                                  infer owner/repo from the local origin\n  123 --repo modem-dev/hunk            use an explicit repository\n  'modem-dev/hunk#123'                 name the repository and pull request\n  https://github.com/modem-dev/hunk/pull/123\n\nAuthentication:\n  GH_TOKEN, then GITHUB_TOKEN           optional for public repositories\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubPrUserError {
    pub message: String,
    pub suggestions: Vec<String>,
}

impl GitHubPrUserError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            suggestions: Vec::new(),
        }
    }

    fn with_suggestions(message: impl Into<String>, suggestions: &[&str]) -> Self {
        Self {
            message: message.into(),
            suggestions: suggestions.iter().map(|value| (*value).into()).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubPullRequestLocator {
    pub owner: Option<String>,
    pub repo: Option<String>,
    pub number: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubPrInvocation {
    pub locator: GitHubPullRequestLocator,
    pub explicit_repository: Option<String>,
    pub patch_args: Vec<String>,
    pub help: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedGitHubPullRequest {
    pub owner: String,
    pub repo: String,
    pub number: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubHttpRequest {
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub redirect_manual: bool,
}

pub struct GitHubHttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Option<Box<dyn Read + Send>>,
}

pub type GitHubFetcher =
    dyn Fn(&GitHubHttpRequest) -> Result<GitHubHttpResponse, String> + Send + Sync;
pub type OriginResolver =
    dyn Fn(&Path, &AtomicBool) -> Result<String, GitHubPrUserError> + Send + Sync;

#[derive(Clone)]
pub struct GitHubPrRuntime {
    pub fetch: Arc<GitHubFetcher>,
    pub environment: BTreeMap<String, String>,
    pub resolve_origin: Arc<OriginResolver>,
    pub temporary_root: PathBuf,
}

impl Default for GitHubPrRuntime {
    fn default() -> Self {
        let environment = ["GH_TOKEN", "GITHUB_TOKEN"]
            .into_iter()
            .filter_map(|name| std::env::var(name).ok().map(|value| (name.into(), value)))
            .collect();
        Self {
            fetch: Arc::new(fetch_with_ureq),
            environment,
            resolve_origin: Arc::new(read_git_origin),
            temporary_root: std::env::temp_dir(),
        }
    }
}

struct RetainedPatch {
    _directory: TempDir,
    path: PathBuf,
}

struct SharedState {
    runtime: GitHubPrRuntime,
    retained: Mutex<Vec<RetainedPatch>>,
    active_registries: AtomicUsize,
}

#[derive(Clone)]
pub struct GitHubPrExtension {
    shared: Arc<SharedState>,
}

impl GitHubPrExtension {
    #[must_use]
    pub fn new(runtime: GitHubPrRuntime) -> Self {
        Self {
            shared: Arc::new(SharedState {
                runtime,
                retained: Mutex::new(Vec::new()),
                active_registries: AtomicUsize::new(0),
            }),
        }
    }

    #[must_use]
    pub fn register(&self) -> GitHubPrRegistration {
        self.shared.active_registries.fetch_add(1, Ordering::AcqRel);
        GitHubPrRegistration {
            extension: self.clone(),
            retired: false,
        }
    }

    pub fn execute(
        &self,
        invocation: &CliCommandInvocation,
        cancelled: &Arc<AtomicBool>,
        mut emit: impl FnMut(CliOutputStream, &[u8]) -> io::Result<()>,
    ) -> Result<CliCommandExecution, GitHubPrUserError> {
        let parsed = parse_github_pr_invocation(&invocation.args)?;
        if parsed.help {
            emit(CliOutputStream::Stdout, GITHUB_PR_HELP.as_bytes()).map_err(output_error)?;
            return Ok(exit_success());
        }

        let target = resolve_github_pull_request(
            &parsed,
            &invocation.cwd,
            cancelled.as_ref(),
            self.shared.runtime.resolve_origin.as_ref(),
        )?;
        emit(
            CliOutputStream::Stderr,
            format!(
                "Fetching GitHub pull request {}/{}#{}…\n",
                target.owner, target.repo, target.number
            )
            .as_bytes(),
        )
        .map_err(output_error)?;
        let diff = fetch_github_pull_request_diff(
            &target,
            cancelled,
            &self.shared.runtime.environment,
            Arc::clone(&self.shared.runtime.fetch),
        )?;
        require_not_cancelled(cancelled)?;
        let patch_path = self.write_temporary_patch(&target, &diff)?;
        if let Err(error) = require_not_cancelled(cancelled) {
            self.discard_patch(&patch_path);
            return Err(error);
        }
        emit(
            CliOutputStream::Stderr,
            format!(
                "Opening {} bytes in Workdeck…\n",
                format_decimal(diff.len())
            )
            .as_bytes(),
        )
        .map_err(output_error)?;
        if let Err(error) = require_not_cancelled(cancelled) {
            self.discard_patch(&patch_path);
            return Err(error);
        }
        Ok(CliCommandExecution {
            result: CliCommandResult::Delegate {
                argv: std::iter::once("patch".into())
                    .chain(std::iter::once(patch_path.to_string_lossy().into_owned()))
                    .chain(parsed.patch_args)
                    .collect(),
            },
            stdin_read_started: false,
            stdin_consumed: false,
        })
    }

    fn write_temporary_patch(
        &self,
        target: &ResolvedGitHubPullRequest,
        bytes: &[u8],
    ) -> Result<PathBuf, GitHubPrUserError> {
        let directory = TempBuilder::new()
            .prefix("workdeck-github-pr-")
            .tempdir_in(&self.shared.runtime.temporary_root)
            .map_err(temporary_error)?;
        set_private_directory_permissions(directory.path()).map_err(temporary_error)?;
        let safe_repo = target
            .repo
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '-') {
                    character
                } else {
                    '-'
                }
            })
            .collect::<String>();
        let path = directory
            .path()
            .join(format!("{safe_repo}-pr-{}.diff", target.number));
        let mut file = private_new_file(&path).map_err(temporary_error)?;
        file.write_all(bytes).map_err(temporary_error)?;
        file.flush().map_err(temporary_error)?;
        self.shared
            .retained
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(RetainedPatch {
                _directory: directory,
                path: path.clone(),
            });
        Ok(path)
    }

    fn discard_patch(&self, path: &Path) {
        let mut retained = self
            .shared
            .retained
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(index) = retained.iter().position(|entry| entry.path == path) {
            retained.swap_remove(index);
        }
    }

    #[must_use]
    pub fn retained_paths(&self) -> Vec<PathBuf> {
        self.shared
            .retained
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .map(|entry| entry.path.clone())
            .collect()
    }

    fn retire_registry(&self) {
        let prior = self.shared.active_registries.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(prior > 0, "registry retirement remains balanced");
        if prior == 1 {
            self.shared
                .retained
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clear();
        }
    }
}

pub struct GitHubPrRegistration {
    extension: GitHubPrExtension,
    retired: bool,
}

impl GitHubPrRegistration {
    pub fn retire(&mut self) {
        if !self.retired {
            self.retired = true;
            self.extension.retire_registry();
        }
    }
}

impl Drop for GitHubPrRegistration {
    fn drop(&mut self) {
        self.retire();
    }
}

fn exit_success() -> CliCommandExecution {
    CliCommandExecution {
        result: CliCommandResult::Exit { code: 0 },
        stdin_read_started: false,
        stdin_consumed: false,
    }
}

fn invocation_error(message: impl Into<String>) -> GitHubPrUserError {
    GitHubPrUserError::with_suggestions(
        message,
        &[
            "Run `workdeck gh --help` for accepted pull-request forms.",
            "Use `workdeck gh 123 --repo owner/repo` outside a GitHub checkout.",
        ],
    )
}

fn parse_pull_request_number(value: &str) -> Result<String, GitHubPrUserError> {
    if value.is_empty()
        || !matches!(value.as_bytes().first(), Some(b'1'..=b'9'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(invocation_error(format!(
            "Invalid pull-request number: {value}"
        )));
    }
    let number = value
        .parse::<u64>()
        .map_err(|_| invocation_error(format!("Pull-request number is too large: {value}")))?;
    if number == 0 {
        return Err(invocation_error(format!(
            "Invalid pull-request number: {value}"
        )));
    }
    if number > MAX_SAFE_INTEGER {
        return Err(invocation_error(format!(
            "Pull-request number is too large: {value}"
        )));
    }
    Ok(number.to_string())
}

pub fn parse_github_repository(value: &str) -> Result<(String, String), GitHubPrUserError> {
    let parts = value.split('/').collect::<Vec<_>>();
    let valid_part = |part: &str| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
    };
    if parts.len() != 2 || !valid_part(parts[0]) || !valid_part(parts[1]) {
        return Err(invocation_error(format!(
            "Invalid GitHub repository: {value}"
        )));
    }
    Ok((parts[0].into(), parts[1].into()))
}

pub fn parse_github_pull_request_locator(
    value: &str,
) -> Result<GitHubPullRequestLocator, GitHubPrUserError> {
    let numeric = value.strip_prefix('#').unwrap_or(value);
    if !numeric.is_empty() && numeric.bytes().all(|byte| byte.is_ascii_digit()) {
        return Ok(GitHubPullRequestLocator {
            owner: None,
            repo: None,
            number: parse_pull_request_number(numeric)?,
        });
    }

    if value.matches('#').count() == 1 {
        let (repository, number) = value.split_once('#').expect("one hash is present");
        if repository.matches('/').count() == 1 && !number.is_empty() {
            let (owner, repo) = parse_github_repository(repository)?;
            return Ok(GitHubPullRequestLocator {
                owner: Some(owner),
                repo: Some(repo),
                number: parse_pull_request_number(number)?,
            });
        }
    }

    let url = Url::parse(value)
        .map_err(|_| invocation_error(format!("Invalid GitHub pull-request locator: {value}")))?;
    if url.scheme() != "https"
        || url
            .host_str()
            .is_none_or(|host| !host.eq_ignore_ascii_case("github.com"))
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invocation_error(
            "Pull-request URLs must be unmodified https://github.com URLs.",
        ));
    }
    let parts = url
        .path()
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.len() != 4 || parts[2] != "pull" {
        return Err(invocation_error(
            "GitHub pull-request URLs must end with /owner/repo/pull/number.",
        ));
    }
    let (owner, repo) = parse_github_repository(&format!("{}/{}", parts[0], parts[1]))?;
    Ok(GitHubPullRequestLocator {
        owner: Some(owner),
        repo: Some(repo),
        number: parse_pull_request_number(parts[3])?,
    })
}

pub fn parse_github_pr_invocation(
    args: &[String],
) -> Result<GitHubPrInvocation, GitHubPrUserError> {
    let separator = args.iter().position(|argument| argument == "--");
    let owned = &args[..separator.unwrap_or(args.len())];
    let patch_args = separator
        .map(|index| args[index + 1..].to_vec())
        .unwrap_or_default();
    if owned
        .iter()
        .any(|argument| argument == "--help" || argument == "-h")
    {
        return Ok(GitHubPrInvocation {
            locator: GitHubPullRequestLocator {
                owner: None,
                repo: None,
                number: "1".into(),
            },
            explicit_repository: None,
            patch_args,
            help: true,
        });
    }

    let mut target = None;
    let mut explicit_repository = None;
    let mut index = 0;
    while index < owned.len() {
        let token = &owned[index];
        if token == "--repo" {
            if explicit_repository.is_some() {
                return Err(invocation_error("Specify --repo only once."));
            }
            let value = owned
                .get(index + 1)
                .filter(|value| !value.starts_with("--"));
            let Some(value) = value else {
                return Err(invocation_error("`--repo` requires an owner/repo value."));
            };
            explicit_repository = Some(value.clone());
            index += 2;
            continue;
        }
        if let Some(value) = token.strip_prefix("--repo=") {
            if explicit_repository.is_some() {
                return Err(invocation_error("Specify --repo only once."));
            }
            if value.is_empty() {
                return Err(invocation_error("`--repo` requires an owner/repo value."));
            }
            explicit_repository = Some(value.into());
            index += 1;
            continue;
        }
        if token.starts_with('-') {
            return Err(invocation_error(format!("Unknown gh option: {token}")));
        }
        if target.is_some() {
            return Err(invocation_error("Specify exactly one pull request."));
        }
        target = Some(token.clone());
        index += 1;
    }

    let target = target.ok_or_else(|| invocation_error("Specify one GitHub pull request."))?;
    let locator = parse_github_pull_request_locator(&target)?;
    if let Some(repository) = &explicit_repository {
        parse_github_repository(repository)?;
        if locator.owner.is_some() || locator.repo.is_some() {
            return Err(invocation_error(
                "Do not combine --repo with a locator that already names a repository.",
            ));
        }
    }
    Ok(GitHubPrInvocation {
        locator,
        explicit_repository,
        patch_args,
        help: false,
    })
}

pub fn parse_github_remote_repository(value: &str) -> Option<(String, String)> {
    let repository_path = parse_scp_github_remote(value).or_else(|| {
        let url = Url::parse(value).ok()?;
        if !matches!(url.scheme(), "https" | "ssh" | "git")
            || url
                .host_str()
                .is_none_or(|host| !host.eq_ignore_ascii_case("github.com"))
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return None;
        }
        Some(url.path().trim_start_matches('/').to_owned())
    })?;
    let suffix_start = repository_path.len().saturating_sub(4);
    let without_suffix = if repository_path
        .get(suffix_start..)
        .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".git"))
    {
        repository_path
            .get(..suffix_start)
            .expect("an ASCII suffix starts at a UTF-8 boundary")
    } else {
        &repository_path
    };
    parse_github_repository(without_suffix).ok()
}

fn parse_scp_github_remote(value: &str) -> Option<String> {
    let (authority, path) = value.split_once(':')?;
    if path.is_empty() || path.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return None;
    }
    if authority.matches('@').count() > 1 {
        return None;
    }
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    if !host.eq_ignore_ascii_case("github.com") {
        return None;
    }
    if authority.contains('@') {
        let user = authority.split_once('@')?.0;
        if user.is_empty() || user.bytes().any(|byte| byte.is_ascii_whitespace()) {
            return None;
        }
    }
    Some(path.into())
}

pub fn read_git_origin(cwd: &Path, cancelled: &AtomicBool) -> Result<String, GitHubPrUserError> {
    require_not_cancelled(cancelled)?;
    let mut command = Command::new("git");
    command
        .args(["remote", "get-url", "origin"])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            GitHubPrUserError::with_suggestions(
                "Git is unavailable for local origin inference.",
                &["Pass `--repo owner/repo` or use an owner/repo#number locator."],
            )
        } else {
            origin_error()
        }
    })?;
    let output_reader = child.stdout.take().map(|stdout| {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            stdout.take(16 * 1024 + 1).read_to_end(&mut bytes)?;
            Ok::<_, io::Error>(bytes)
        })
    });
    let deadline = Instant::now() + ORIGIN_TIMEOUT;
    let status = loop {
        if cancelled.load(Ordering::Acquire) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(cancelled_error());
        }
        if let Some(status) = child.try_wait().map_err(|_| origin_error())? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(origin_error());
        }
        thread::sleep(Duration::from_millis(5));
    };
    let bytes = output_reader
        .ok_or_else(origin_error)?
        .join()
        .map_err(|_| origin_error())?
        .map_err(|_| origin_error())?;
    let value = String::from_utf8_lossy(&bytes).trim().to_owned();
    if !status.success() || bytes.len() > 16 * 1024 || value.is_empty() {
        return Err(origin_error());
    }
    Ok(value)
}

fn origin_error() -> GitHubPrUserError {
    GitHubPrUserError::with_suggestions(
        "The current directory has no small, readable Git origin.",
        &["Pass `--repo owner/repo` or use an owner/repo#number locator."],
    )
}

pub fn resolve_github_pull_request(
    invocation: &GitHubPrInvocation,
    cwd: &Path,
    cancelled: &AtomicBool,
    resolve_origin: &OriginResolver,
) -> Result<ResolvedGitHubPullRequest, GitHubPrUserError> {
    if let (Some(owner), Some(repo)) = (&invocation.locator.owner, &invocation.locator.repo) {
        return Ok(ResolvedGitHubPullRequest {
            owner: owner.clone(),
            repo: repo.clone(),
            number: invocation.locator.number.clone(),
        });
    }
    if let Some(repository) = &invocation.explicit_repository {
        let (owner, repo) = parse_github_repository(repository)?;
        return Ok(ResolvedGitHubPullRequest {
            owner,
            repo,
            number: invocation.locator.number.clone(),
        });
    }
    let origin = resolve_origin(cwd, cancelled)?;
    let Some((owner, repo)) = parse_github_remote_repository(&origin) else {
        return Err(GitHubPrUserError::with_suggestions(
            "The local origin is not a supported github.com repository.",
            &["Pass `--repo owner/repo` or use an owner/repo#number locator."],
        ));
    };
    Ok(ResolvedGitHubPullRequest {
        owner,
        repo,
        number: invocation.locator.number.clone(),
    })
}

fn build_github_request(
    target: &ResolvedGitHubPullRequest,
    environment: &BTreeMap<String, String>,
) -> Result<GitHubHttpRequest, GitHubPrUserError> {
    let mut headers = BTreeMap::from([
        ("Accept".into(), "application/vnd.github.v3.diff".into()),
        ("User-Agent".into(), "workdeck-github-pr-extension".into()),
        ("X-GitHub-Api-Version".into(), "2022-11-28".into()),
    ]);
    let token = environment
        .get("GH_TOKEN")
        .filter(|value| !value.is_empty())
        .or_else(|| {
            environment
                .get("GITHUB_TOKEN")
                .filter(|value| !value.is_empty())
        });
    if let Some(token) = token {
        let value = format!("Bearer {token}");
        ureq::http::HeaderValue::from_str(&value).map_err(|_| {
            GitHubPrUserError::with_suggestions(
                "The configured GitHub token contains characters that cannot be sent in an HTTP header.",
                &["Set GH_TOKEN or GITHUB_TOKEN to the token value without line breaks."],
            )
        })?;
        headers.insert("Authorization".into(), value);
    }
    Ok(GitHubHttpRequest {
        url: format!(
            "{GITHUB_API_ORIGIN}/repos/{}/{}/pulls/{}",
            target.owner, target.repo, target.number
        ),
        headers,
        redirect_manual: true,
    })
}

fn fetch_with_ureq(request: &GitHubHttpRequest) -> Result<GitHubHttpResponse, String> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(NETWORK_TIMEOUT))
        .max_redirects(if request.redirect_manual { 0 } else { 10 })
        .http_status_as_error(false)
        .build();
    let agent: ureq::Agent = config.into();
    let mut builder = agent.get(&request.url);
    for (name, value) in &request.headers {
        builder = builder.header(name, value);
    }
    let response = builder
        .call()
        .map_err(|_| "GitHub request failed".to_owned())?;
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_ascii_lowercase(), value.to_owned()))
        })
        .collect();
    let (_, body) = response.into_parts();
    Ok(GitHubHttpResponse {
        status,
        headers,
        body: Some(Box::new(body.into_reader())),
    })
}

pub fn fetch_github_pull_request_diff(
    target: &ResolvedGitHubPullRequest,
    cancelled: &Arc<AtomicBool>,
    environment: &BTreeMap<String, String>,
    fetch: Arc<GitHubFetcher>,
) -> Result<Vec<u8>, GitHubPrUserError> {
    require_not_cancelled(cancelled.as_ref())?;
    let request = build_github_request(target, environment)?;
    let worker_cancelled = Arc::clone(cancelled);
    let worker_target = target.clone();
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let result = fetch(&request)
            .map_err(|_| {
                if worker_cancelled.load(Ordering::Acquire) {
                    cancelled_error()
                } else {
                    GitHubPrUserError::with_suggestions(
                        "GitHub could not be reached while loading the pull request.",
                        &["Check network access and retry."],
                    )
                }
            })
            .and_then(|response| {
                if !(200..300).contains(&response.status) {
                    Err(github_response_error(&response, &worker_target))
                } else {
                    read_bounded_response(response, worker_cancelled.as_ref(), MAX_DIFF_BYTES)
                }
            });
        let _ = sender.send(result);
    });
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err(cancelled_error());
        }
        match receiver.recv_timeout(Duration::from_millis(5)) {
            Ok(result) => return result,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(GitHubPrUserError::with_suggestions(
                    "GitHub could not be reached while loading the pull request.",
                    &["Check network access and retry."],
                ));
            }
        }
    }
}

fn read_bounded_response(
    mut response: GitHubHttpResponse,
    cancelled: &AtomicBool,
    limit: usize,
) -> Result<Vec<u8>, GitHubPrUserError> {
    if response_header(&response.headers, "content-length")
        .and_then(|value| value.parse::<f64>().ok())
        .is_some_and(|length| length.is_finite() && length > limit as f64)
    {
        return Err(diff_limit_error(limit));
    }
    let Some(mut body) = response.body.take() else {
        return Err(GitHubPrUserError::new(
            "GitHub returned an empty pull-request response.",
        ));
    };
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 32 * 1024];
    loop {
        require_not_cancelled(cancelled)?;
        let read = body.read(&mut buffer).map_err(|_| {
            if cancelled.load(Ordering::Acquire) {
                cancelled_error()
            } else {
                GitHubPrUserError::with_suggestions(
                    "GitHub could not be reached while loading the pull request.",
                    &["Check network access and retry."],
                )
            }
        })?;
        if read == 0 {
            break;
        }
        if bytes.len().saturating_add(read) > limit {
            return Err(diff_limit_error(limit));
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
    if bytes.is_empty() {
        return Err(GitHubPrUserError::new(
            "GitHub returned an empty pull-request diff.",
        ));
    }
    Ok(bytes)
}

fn diff_limit_error(limit: usize) -> GitHubPrUserError {
    let mebibytes = limit / (1024 * 1024);
    GitHubPrUserError::new(format!(
        "The pull-request diff exceeds the {mebibytes} MiB safety limit."
    ))
}

fn github_response_error(
    response: &GitHubHttpResponse,
    target: &ResolvedGitHubPullRequest,
) -> GitHubPrUserError {
    let name = format!("{}/{}#{}", target.owner, target.repo, target.number);
    match response.status {
        401 => GitHubPrUserError::with_suggestions(
            format!("GitHub rejected the configured token for {name}."),
            &["Refresh GH_TOKEN or GITHUB_TOKEN and retry."],
        ),
        403 if response_header(&response.headers, "x-ratelimit-remaining") == Some("0") => {
            GitHubPrUserError::with_suggestions(
                format!("GitHub API rate limiting blocked {name}."),
                &[
                    "Authenticate with GH_TOKEN or GITHUB_TOKEN, or retry after the rate limit resets.",
                ],
            )
        }
        403 => GitHubPrUserError::with_suggestions(
            format!("GitHub denied access to {name}."),
            &["Check token repository permissions and organization SSO authorization."],
        ),
        404 => GitHubPrUserError::with_suggestions(
            format!("GitHub could not find an accessible pull request at {name}."),
            &["Check the repository and PR number; private repositories require token access."],
        ),
        300..=399 => GitHubPrUserError::new(
            "GitHub redirected the pull-request request; refusing to forward credentials.",
        ),
        status => GitHubPrUserError::new(format!("GitHub returned HTTP {status} for {name}.")),
    }
}

fn response_header<'a>(headers: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn require_not_cancelled(cancelled: &AtomicBool) -> Result<(), GitHubPrUserError> {
    if cancelled.load(Ordering::Acquire) {
        Err(cancelled_error())
    } else {
        Ok(())
    }
}

fn cancelled_error() -> GitHubPrUserError {
    GitHubPrUserError::new("GitHub pull-request loading was cancelled.")
}

fn output_error(error: io::Error) -> GitHubPrUserError {
    GitHubPrUserError::new(format!("CLI output failed: {error}"))
}

fn temporary_error(_error: io::Error) -> GitHubPrUserError {
    GitHubPrUserError::new("Could not create a private temporary pull-request patch.")
}

fn format_decimal(value: usize) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            formatted.push(',');
        }
        formatted.push(character);
    }
    formatted
}

#[cfg(unix)]
fn set_private_directory_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn set_private_directory_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn private_new_file(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn private_new_file(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

type SharedWriter<W> = Arc<Mutex<W>>;
type ActiveRequest = Arc<Mutex<Option<(u64, Arc<AtomicBool>)>>>;

pub fn serve<R, W>(input: R, output: W) -> io::Result<()>
where
    R: BufRead,
    W: Write + Send + 'static,
{
    serve_with_extension(
        input,
        output,
        GitHubPrExtension::new(GitHubPrRuntime::default()),
    )
}

pub fn serve_with_extension<R, W>(
    mut input: R,
    output: W,
    extension: GitHubPrExtension,
) -> io::Result<()>
where
    R: BufRead,
    W: Write + Send + 'static,
{
    let output = Arc::new(Mutex::new(output));
    let active: ActiveRequest = Arc::new(Mutex::new(None));
    let mut registry = extension.register();
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let value: Value = serde_json::from_str(&line).map_err(io::Error::other)?;
        if value.get("id").is_none() {
            let method = value.get("method").and_then(Value::as_str);
            if method == Some("workdeck/shutdown") {
                if let Some((_, cancelled)) = active
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .as_ref()
                {
                    cancelled.store(true, Ordering::Release);
                }
                registry.retire();
                return Ok(());
            }
            handle_cancellation_notification(&value, &active);
            continue;
        }
        let request: JsonRpcRequest = serde_json::from_value(value).map_err(io::Error::other)?;
        match request.method.as_str() {
            "workdeck/handshake" => write_result(
                &output,
                request.id,
                &HandshakeResponse {
                    extension_api_version: API_VERSION,
                    extension_version: env!("CARGO_PKG_VERSION").into(),
                    registrations: vec![
                        Registration::CliCommand(CliCommandRegistration {
                            name: COMMAND_NAME.into(),
                            summary: "Review a GitHub pull request".into(),
                            usage: Some(
                                "<number|owner/repo#number|pull-request-url> [--repo <owner/repo>]"
                                    .into(),
                            ),
                        }),
                        Registration::EventSubscription {
                            names: vec!["shutdown".into()],
                        },
                    ],
                },
            )?,
            "workdeck/cli/invoke" => {
                let invocation: CliCommandInvocation =
                    serde_json::from_value(request.params).map_err(io::Error::other)?;
                if invocation.command_name != COMMAND_NAME {
                    write_error(
                        &output,
                        request.id,
                        -32601,
                        format!("Unknown CLI command: {}", invocation.command_name),
                        None,
                    )?;
                    continue;
                }
                let cancelled = Arc::new(AtomicBool::new(false));
                *active
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                    Some((request.id, Arc::clone(&cancelled)));
                let output = Arc::clone(&output);
                let active = Arc::clone(&active);
                let extension = extension.clone();
                thread::spawn(move || {
                    let result = extension.execute(&invocation, &cancelled, |stream, bytes| {
                        write_cli_output(&output, request.id, stream, bytes)
                    });
                    match result {
                        Ok(execution) => {
                            let _ = write_result(&output, request.id, &execution);
                        }
                        Err(error) => {
                            let data = (!error.suggestions.is_empty())
                                .then(|| json!({ "suggestions": error.suggestions }));
                            let _ = write_error(&output, request.id, -32000, error.message, data);
                        }
                    }
                    let mut current = active
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    if current.as_ref().is_some_and(|(id, _)| *id == request.id) {
                        *current = None;
                    }
                });
            }
            _ => write_error(
                &output,
                request.id,
                -32601,
                format!("Unknown method: {}", request.method),
                None,
            )?,
        }
    }
}

fn handle_cancellation_notification(value: &Value, active: &ActiveRequest) {
    if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || value.get("method").and_then(Value::as_str) != Some("$/cancelRequest")
    {
        return;
    }
    let Some(id) = value
        .get("params")
        .and_then(|params| params.get("id"))
        .and_then(Value::as_u64)
    else {
        return;
    };
    if let Some((active_id, cancelled)) = active
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        && *active_id == id
    {
        cancelled.store(true, Ordering::Release);
    }
}

fn write_cli_output<W: Write>(
    output: &SharedWriter<W>,
    request_id: u64,
    stream: CliOutputStream,
    bytes: &[u8],
) -> io::Result<()> {
    write_line(
        output,
        &json!({
            "jsonrpc": "2.0",
            "method": "workdeck/cli/output",
            "params": CliOutputNotification { request_id, stream, bytes: bytes.to_vec() },
        }),
    )
}

fn write_result<W: Write>(
    output: &SharedWriter<W>,
    id: u64,
    result: &impl serde::Serialize,
) -> io::Result<()> {
    write_line(
        output,
        &JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id,
            result: Some(serde_json::to_value(result).map_err(io::Error::other)?),
            error: None,
        },
    )
}

fn write_error<W: Write>(
    output: &SharedWriter<W>,
    id: u64,
    code: i32,
    message: String,
    data: Option<Value>,
) -> io::Result<()> {
    write_line(
        output,
        &JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message,
                data,
            }),
        },
    )
}

fn write_line<W: Write>(output: &SharedWriter<W>, value: &impl serde::Serialize) -> io::Result<()> {
    let mut output = output
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    serde_json::to_writer(&mut *output, value).map_err(io::Error::other)?;
    output.write_all(b"\n")?;
    output.flush()
}

#[must_use]
pub fn required_capabilities() -> Vec<Capability> {
    vec![Capability::CliCommands, Capability::Events]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn response(status: u16, body: impl Into<Vec<u8>>) -> GitHubHttpResponse {
        GitHubHttpResponse {
            status,
            headers: BTreeMap::new(),
            body: Some(Box::new(Cursor::new(body.into()))),
        }
    }

    #[test]
    fn bounded_reader_rejects_declared_streamed_empty_and_absent_bodies() {
        let cancelled = AtomicBool::new(false);
        let mut declared = response(200, b"small".to_vec());
        declared.headers.insert("content-length".into(), "9".into());
        assert!(
            read_bounded_response(declared, &cancelled, 8)
                .unwrap_err()
                .message
                .contains("safety limit")
        );
        assert!(
            read_bounded_response(response(200, vec![b'x'; 9]), &cancelled, 8)
                .unwrap_err()
                .message
                .contains("safety limit")
        );
        assert!(
            read_bounded_response(response(200, Vec::new()), &cancelled, 8)
                .unwrap_err()
                .message
                .contains("empty pull-request diff")
        );
        assert!(
            read_bounded_response(
                GitHubHttpResponse {
                    status: 200,
                    headers: BTreeMap::new(),
                    body: None,
                },
                &cancelled,
                8,
            )
            .unwrap_err()
            .message
            .contains("empty pull-request response")
        );
    }

    #[test]
    fn decimal_formatter_matches_javascript_grouping_for_byte_counts() {
        assert_eq!(format_decimal(0), "0");
        assert_eq!(format_decimal(999), "999");
        assert_eq!(format_decimal(1_000), "1,000");
        assert_eq!(format_decimal(64 * 1024 * 1024), "67,108,864");
    }
}
