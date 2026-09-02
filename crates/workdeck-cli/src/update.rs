//! Install-source detection and channel-correct native self-updates.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use serde_json::Value;
use thiserror::Error;
use workdeck_core::{SelfUpdateCommandInput, WorkdeckInstallSource};

use crate::version::{
    UNKNOWN_CLI_VERSION, is_comparable_version, is_newer_version, is_prerelease_version,
    is_stable_version,
};

pub const INSTALL_SOURCE_ENV: &str = "WORKDECK_INSTALL_SOURCE";
pub const INSTALL_DIR_ENV: &str = "WORKDECK_INSTALL_DIR";
pub const INSTALL_VERSION_ENV: &str = "WORKDECK_VERSION";

pub const CARGO_CRATE_URL: &str = "https://crates.io/api/v1/crates/workdeck-cli";
pub const HOMEBREW_FORMULA_URL: &str = "https://formulae.brew.sh/api/formula/workdeck.json";
pub const GITHUB_LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/ruttydm/workdeck/releases/latest";
pub const CURL_INSTALL_SCRIPT_URL: &str = "https://workdeck.dev/install.sh";
pub const POWERSHELL_INSTALL_SCRIPT_URL: &str = "https://workdeck.dev/install.ps1";

const DEFAULT_RELEASE_FETCH_TIMEOUT: Duration = Duration::from_secs(5);
const HOMEBREW_PATH_SEGMENTS: &[&str] = &["cellar", "homebrew", "linuxbrew"];
type ResolveExecutablePath = dyn Fn(&str) -> Result<String, String>;

pub const UPDATE_METHOD_VALUES: &[&str] = &["cargo", "brew", "nix", "curl", "powershell", "direct"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdatePlatform {
    Macos,
    Linux,
    Windows,
}

impl UpdatePlatform {
    #[must_use]
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else {
            Self::Linux
        }
    }
}

pub struct InstallSourceFacts<'a> {
    pub env: &'a BTreeMap<String, String>,
    pub executable_path: &'a str,
    pub version: &'a str,
    pub home_dir: Option<&'a str>,
    pub platform: UpdatePlatform,
    pub realpath: Option<&'a ResolveExecutablePath>,
}

impl<'a> InstallSourceFacts<'a> {
    #[must_use]
    pub fn new(
        env: &'a BTreeMap<String, String>,
        executable_path: &'a str,
        version: &'a str,
    ) -> Self {
        Self {
            env,
            executable_path,
            version,
            home_dir: env
                .get("HOME")
                .or_else(|| env.get("USERPROFILE"))
                .map(String::as_str),
            platform: UpdatePlatform::current(),
            realpath: None,
        }
    }
}

fn split_path_segments(path: &str) -> Vec<&str> {
    path.split(['/', '\\'])
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn resolved_executable_path(facts: &InstallSourceFacts<'_>) -> String {
    if let Some(realpath) = facts.realpath
        && let Ok(path) = realpath(facts.executable_path)
    {
        return path;
    }
    std::fs::canonicalize(facts.executable_path)
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| facts.executable_path.to_owned())
}

fn declared_install_source(env: &BTreeMap<String, String>) -> Option<WorkdeckInstallSource> {
    match env.get(INSTALL_SOURCE_ENV)?.to_ascii_lowercase().as_str() {
        "cargo" => Some(WorkdeckInstallSource::Cargo),
        "brew" | "homebrew" => Some(WorkdeckInstallSource::Homebrew),
        "nix" => Some(WorkdeckInstallSource::Nix),
        "curl" => Some(WorkdeckInstallSource::Curl),
        "powershell" | "pwsh" => Some(WorkdeckInstallSource::PowerShell),
        "direct" | "github" => Some(WorkdeckInstallSource::Direct),
        "dev" => Some(WorkdeckInstallSource::Dev),
        _ => None,
    }
}

fn is_homebrew_executable(path: &str) -> bool {
    let segments = split_path_segments(path)
        .into_iter()
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    if segments.iter().any(|segment| segment == "node_modules") {
        return false;
    }
    let executable = segments
        .last()
        .map(|name| name.strip_suffix(".exe").unwrap_or(name));
    executable == Some("workdeck")
        && segments
            .iter()
            .any(|segment| HOMEBREW_PATH_SEGMENTS.contains(&segment.as_str()))
}

fn adjacent_segments(path: &str, first: &str, second: &str) -> bool {
    split_path_segments(path)
        .windows(2)
        .any(|segments| segments[0].eq_ignore_ascii_case(first) && segments[1] == second)
}

fn is_rust_build_executable(path: &str) -> bool {
    split_path_segments(path).windows(2).any(|segments| {
        segments[0].eq_ignore_ascii_case("target") && matches!(segments[1], "debug" | "release")
    })
}

fn is_inside_directory(path: &str, directory: Option<&str>, platform: UpdatePlatform) -> bool {
    let Some(directory) = directory else {
        return false;
    };
    let normalize = |value: &str| {
        split_path_segments(value)
            .into_iter()
            .map(|segment| {
                if platform == UpdatePlatform::Windows {
                    segment.to_ascii_lowercase()
                } else {
                    segment.to_owned()
                }
            })
            .collect::<Vec<_>>()
    };
    let directory = normalize(directory);
    !directory.is_empty()
        && normalize(path)
            .get(..directory.len())
            .is_some_and(|prefix| prefix == directory)
}

#[must_use]
pub fn resolve_dev_install_dir(
    env: &BTreeMap<String, String>,
    home_dir: Option<&str>,
    platform: UpdatePlatform,
) -> Option<String> {
    if let Some(configured) = env.get(INSTALL_DIR_ENV).filter(|value| !value.is_empty()) {
        return Some(configured.clone());
    }
    match platform {
        UpdatePlatform::Windows => env
            .get("LOCALAPPDATA")
            .cloned()
            .or_else(|| home_dir.map(|home| format!(r"{home}\AppData\Local")))
            .map(|base| format!(r"{base}\Programs\workdeck")),
        UpdatePlatform::Macos | UpdatePlatform::Linux => {
            home_dir.map(|home| format!("{home}/.local/bin"))
        }
    }
}

/// Resolve which installation channel owns the running executable.
#[must_use]
pub fn detect_install_source(facts: &InstallSourceFacts<'_>) -> WorkdeckInstallSource {
    if let Some(source) = declared_install_source(facts.env) {
        return source;
    }
    let path = resolved_executable_path(facts);
    if path.starts_with("/nix/store/") {
        return WorkdeckInstallSource::Nix;
    }
    if is_homebrew_executable(&path) {
        return WorkdeckInstallSource::Homebrew;
    }
    if adjacent_segments(&path, ".cargo", "bin") {
        return WorkdeckInstallSource::Cargo;
    }
    if adjacent_segments(&path, ".workdeck", "bin") {
        return match facts.platform {
            UpdatePlatform::Macos | UpdatePlatform::Linux => WorkdeckInstallSource::Curl,
            UpdatePlatform::Windows => WorkdeckInstallSource::PowerShell,
        };
    }
    if is_rust_build_executable(&path) {
        return WorkdeckInstallSource::Dev;
    }
    let dev_dir = resolve_dev_install_dir(facts.env, facts.home_dir, facts.platform);
    if is_inside_directory(&path, dev_dir.as_deref(), facts.platform)
        || facts.version == UNKNOWN_CLI_VERSION
    {
        return WorkdeckInstallSource::Dev;
    }
    WorkdeckInstallSource::Direct
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseRequest {
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseResponse {
    pub status: u16,
    pub body: String,
}

pub trait ReleaseFetcher: Send + Sync {
    fn fetch(
        &self,
        request: &ReleaseRequest,
        cancelled: &AtomicBool,
    ) -> Result<ReleaseResponse, String>;
}

impl<F> ReleaseFetcher for F
where
    F: Fn(&ReleaseRequest, &AtomicBool) -> Result<ReleaseResponse, String> + Send + Sync,
{
    fn fetch(
        &self,
        request: &ReleaseRequest,
        cancelled: &AtomicBool,
    ) -> Result<ReleaseResponse, String> {
        self(request, cancelled)
    }
}

#[derive(Clone)]
pub struct ReleaseLookup {
    pub fetcher: Arc<dyn ReleaseFetcher>,
    pub timeout: Duration,
}

impl Default for ReleaseLookup {
    fn default() -> Self {
        Self {
            fetcher: Arc::new(CommandReleaseFetcher),
            timeout: DEFAULT_RELEASE_FETCH_TIMEOUT,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChannelVersions {
    pub latest: Option<String>,
    pub beta: Option<String>,
}

fn fetch_json(
    url: &str,
    headers: BTreeMap<String, String>,
    lookup: &ReleaseLookup,
) -> Option<Value> {
    let request = ReleaseRequest {
        url: url.into(),
        headers,
        timeout: lookup.timeout,
    };
    let cancelled = Arc::new(AtomicBool::new(false));
    let thread_cancelled = Arc::clone(&cancelled);
    let fetcher = Arc::clone(&lookup.fetcher);
    let (send, receive) = mpsc::sync_channel(1);
    let _ = std::thread::Builder::new()
        .name("workdeck-release-lookup".into())
        .spawn(move || {
            let result = fetcher.fetch(&request, &thread_cancelled);
            let _ = send.send(result);
        })
        .ok()?;
    let response = match receive.recv_timeout(lookup.timeout) {
        Ok(Ok(response)) => response,
        Ok(Err(_)) => return None,
        Err(_) => {
            cancelled.store(true, Ordering::Release);
            return None;
        }
    };
    if !(200..300).contains(&response.status) {
        return None;
    }
    serde_json::from_str(&response.body).ok()
}

fn string_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.as_object()?.get(key)?.as_str()
}

#[must_use]
pub fn has_published_releases(source: WorkdeckInstallSource) -> bool {
    !matches!(
        source,
        WorkdeckInstallSource::Nix | WorkdeckInstallSource::Dev
    )
}

#[must_use]
pub fn fetch_channel_versions(
    source: WorkdeckInstallSource,
    lookup: &ReleaseLookup,
) -> ChannelVersions {
    match source {
        WorkdeckInstallSource::Cargo => {
            let payload = fetch_json(CARGO_CRATE_URL, BTreeMap::new(), lookup);
            let package = payload.as_ref().and_then(|value| value.get("crate"));
            let latest = package
                .and_then(|value| string_field(value, "max_stable_version"))
                .filter(|version| is_stable_version(version))
                .map(str::to_owned);
            let beta = package
                .and_then(|value| string_field(value, "newest_version"))
                .filter(|version| is_prerelease_version(version))
                .map(str::to_owned);
            ChannelVersions { latest, beta }
        }
        WorkdeckInstallSource::Homebrew => {
            let payload = fetch_json(HOMEBREW_FORMULA_URL, BTreeMap::new(), lookup);
            let latest = payload
                .as_ref()
                .and_then(|value| value.get("versions"))
                .and_then(|value| string_field(value, "stable"))
                .filter(|version| is_stable_version(version))
                .map(str::to_owned);
            ChannelVersions { latest, beta: None }
        }
        WorkdeckInstallSource::Curl
        | WorkdeckInstallSource::PowerShell
        | WorkdeckInstallSource::Direct => {
            let payload = fetch_json(
                GITHUB_LATEST_RELEASE_URL,
                BTreeMap::from([("accept".into(), "application/vnd.github+json".into())]),
                lookup,
            );
            let latest = payload
                .as_ref()
                .and_then(|value| string_field(value, "tag_name"))
                .map(|version| version.strip_prefix('v').unwrap_or(version))
                .filter(|version| is_stable_version(version))
                .map(str::to_owned);
            ChannelVersions { latest, beta: None }
        }
        WorkdeckInstallSource::Nix | WorkdeckInstallSource::Dev => ChannelVersions::default(),
    }
}

struct CommandReleaseFetcher;

impl ReleaseFetcher for CommandReleaseFetcher {
    fn fetch(
        &self,
        request: &ReleaseRequest,
        cancelled: &AtomicBool,
    ) -> Result<ReleaseResponse, String> {
        if cancelled.load(Ordering::Acquire) {
            return Err("release lookup cancelled".into());
        }
        let output = if cfg!(windows) {
            let mut command = Command::new("powershell.exe");
            let headers = request
                .headers
                .iter()
                .map(|(key, value)| format!("'{key}'='{value}'"))
                .collect::<Vec<_>>()
                .join(";");
            let script = format!(
                "$h=@{{{headers}}}; (Invoke-WebRequest -UseBasicParsing -TimeoutSec {} -Headers $h -Uri '{}').Content",
                request.timeout.as_secs().max(1),
                request.url.replace('\'', "''")
            );
            command
                .args(["-NoProfile", "-NonInteractive", "-Command", &script])
                .output()
        } else {
            let mut command = Command::new("curl");
            command.args([
                "--fail",
                "--silent",
                "--show-error",
                "--location",
                "--max-time",
                &request.timeout.as_secs().max(1).to_string(),
            ]);
            for (key, value) in &request.headers {
                command.args(["--header", &format!("{key}: {value}")]);
            }
            command.arg(&request.url).output()
        }
        .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
        }
        Ok(ReleaseResponse {
            status: 200,
            body: String::from_utf8(output.stdout).map_err(|error| error.to_string())?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInvocation {
    pub command: Vec<String>,
    pub env: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateProcessResult {
    pub exit_code: i32,
    pub stderr: String,
}

pub trait UpdateCommandRunner: Send + Sync {
    fn run(&self, invocation: &UpdateInvocation) -> Result<UpdateProcessResult, String>;
}

impl<F> UpdateCommandRunner for F
where
    F: Fn(&UpdateInvocation) -> Result<UpdateProcessResult, String> + Send + Sync,
{
    fn run(&self, invocation: &UpdateInvocation) -> Result<UpdateProcessResult, String> {
        self(invocation)
    }
}

pub trait UpdateReporter: Send + Sync {
    fn stdout(&self, message: &str);
    fn stderr(&self, message: &str);
}

struct TerminalUpdateReporter;

impl UpdateReporter for TerminalUpdateReporter {
    fn stdout(&self, message: &str) {
        print!("{message}");
        let _ = std::io::stdout().flush();
    }

    fn stderr(&self, message: &str) {
        eprint!("{message}");
        let _ = std::io::stderr().flush();
    }
}

pub struct SilentUpdateReporter;

impl UpdateReporter for SilentUpdateReporter {
    fn stdout(&self, _message: &str) {}

    fn stderr(&self, _message: &str) {}
}

struct NativeUpdateCommandRunner;

impl UpdateCommandRunner for NativeUpdateCommandRunner {
    fn run(&self, invocation: &UpdateInvocation) -> Result<UpdateProcessResult, String> {
        let (program, arguments) = invocation
            .command
            .split_first()
            .ok_or_else(|| "update command was empty".to_owned())?;
        let mut command = Command::new(program);
        command
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::piped());
        if let Some(env) = &invocation.env {
            command.env_clear().envs(env);
        }
        let child = command.spawn().map_err(|error| error.to_string())?;
        let output = child
            .wait_with_output()
            .map_err(|error| error.to_string())?;
        Ok(UpdateProcessResult {
            exit_code: output.status.code().unwrap_or(1),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

#[derive(Clone)]
pub struct SelfUpdateContext {
    pub env: BTreeMap<String, String>,
    pub executable_path: String,
    pub installed_version: String,
    pub install_source: Option<WorkdeckInstallSource>,
    pub platform: UpdatePlatform,
    pub architecture: String,
    pub release_lookup: ReleaseLookup,
    pub runner: Arc<dyn UpdateCommandRunner>,
    pub reporter: Arc<dyn UpdateReporter>,
}

impl SelfUpdateContext {
    pub fn current() -> Result<Self, UpdateError> {
        Ok(Self {
            env: std::env::vars().collect(),
            executable_path: std::env::current_exe()
                .map_err(|error| {
                    UpdateError::new(
                        "Could not resolve the Workdeck executable.",
                        [error.to_string()],
                    )
                })?
                .to_string_lossy()
                .into_owned(),
            installed_version: crate::version::resolve_cli_version().into(),
            install_source: None,
            platform: UpdatePlatform::current(),
            architecture: std::env::consts::ARCH.into(),
            release_lookup: ReleaseLookup::default(),
            runner: Arc::new(NativeUpdateCommandRunner),
            reporter: Arc::new(TerminalUpdateReporter),
        })
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("{message}")]
pub struct UpdateError {
    pub message: String,
    pub suggestions: Vec<String>,
}

impl UpdateError {
    fn new(
        message: impl Into<String>,
        suggestions: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            message: message.into(),
            suggestions: suggestions.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelfUpdateResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

fn list_update_methods() -> String {
    let quoted = UPDATE_METHOD_VALUES
        .iter()
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>();
    format!(
        "{}, and {}",
        quoted[..quoted.len() - 1].join(", "),
        quoted.last().expect("update methods are not empty")
    )
}

pub fn parse_update_method(value: &str) -> Result<WorkdeckInstallSource, UpdateError> {
    let source = match value.to_ascii_lowercase().as_str() {
        "cargo" => WorkdeckInstallSource::Cargo,
        "brew" | "homebrew" => WorkdeckInstallSource::Homebrew,
        "nix" => WorkdeckInstallSource::Nix,
        "curl" => WorkdeckInstallSource::Curl,
        "powershell" | "pwsh" => WorkdeckInstallSource::PowerShell,
        "direct" | "github" => WorkdeckInstallSource::Direct,
        _ => {
            return Err(UpdateError::new(
                format!("Unknown update method: {value}"),
                [format!("Supported methods are {}.", list_update_methods())],
            ));
        }
    };
    Ok(source)
}

pub fn parse_update_version(value: &str) -> Result<String, UpdateError> {
    let version = value.strip_prefix('v').unwrap_or(value);
    if !is_comparable_version(version) {
        return Err(UpdateError::new(
            format!("Invalid version: {value}"),
            ["Pass an exact release version such as `0.1.0`."],
        ));
    }
    Ok(version.into())
}

fn describe_install_source(source: WorkdeckInstallSource) -> &'static str {
    match source {
        WorkdeckInstallSource::Cargo => "Cargo",
        WorkdeckInstallSource::Homebrew => "Homebrew",
        WorkdeckInstallSource::Nix => "Nix",
        WorkdeckInstallSource::Curl => "the curl install script",
        WorkdeckInstallSource::PowerShell => "the PowerShell install script",
        WorkdeckInstallSource::Direct => "a direct GitHub release",
        WorkdeckInstallSource::Dev => "a local source build",
    }
}

fn unmanaged_install_guidance(source: WorkdeckInstallSource) -> [&'static str; 2] {
    match source {
        WorkdeckInstallSource::Nix => [
            "Workdeck was installed with Nix.",
            "Update it through your Nix configuration, then rebuild that profile or flake.",
        ],
        WorkdeckInstallSource::Dev => [
            "Workdeck is running from a local source build.",
            "Run `cargo install --path crates/workdeck-cli --force` in the Workdeck checkout.",
        ],
        _ => unreachable!("only externally managed sources request guidance"),
    }
}

fn fetch_failure_message(source: WorkdeckInstallSource) -> &'static str {
    match source {
        WorkdeckInstallSource::Cargo => {
            "Could not read the latest Workdeck version from crates.io."
        }
        WorkdeckInstallSource::Homebrew => {
            "Could not read the latest Workdeck version from the Homebrew formula API."
        }
        WorkdeckInstallSource::Curl
        | WorkdeckInstallSource::PowerShell
        | WorkdeckInstallSource::Direct => {
            "Could not read the latest Workdeck version from the GitHub releases API."
        }
        WorkdeckInstallSource::Nix | WorkdeckInstallSource::Dev => {
            unreachable!("unmanaged sources do not fetch releases")
        }
    }
}

fn shell_install_script() -> String {
    [
        "set -e",
        "tmp=\"$(mktemp)\"",
        "trap 'rm -f \"$tmp\"' EXIT",
        &format!(
            "if command -v curl >/dev/null 2>&1; then curl -fsSL {CURL_INSTALL_SCRIPT_URL} -o \"$tmp\"; else wget -qO \"$tmp\" {CURL_INSTALL_SCRIPT_URL}; fi"
        ),
        "sh \"$tmp\"",
    ]
    .join("; ")
}

fn powershell_install_script() -> String {
    format!(
        "$ErrorActionPreference='Stop'; $tmp=[IO.Path]::GetTempFileName(); try {{ Invoke-WebRequest -UseBasicParsing -Uri '{POWERSHELL_INSTALL_SCRIPT_URL}' -OutFile $tmp; & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $tmp; if ($LASTEXITCODE) {{ exit $LASTEXITCODE }} }} finally {{ Remove-Item -Force -ErrorAction SilentlyContinue $tmp }}"
    )
}

fn direct_target(
    platform: UpdatePlatform,
    architecture: &str,
) -> Result<(&'static str, &'static str), UpdateError> {
    match (platform, architecture) {
        (UpdatePlatform::Macos, "aarch64" | "arm64") => Ok(("macos-arm64", "tar.gz")),
        (UpdatePlatform::Macos, "x86_64" | "x64") => Ok(("macos-x64", "tar.gz")),
        (UpdatePlatform::Linux, "aarch64" | "arm64") => Ok(("linux-arm64", "tar.gz")),
        (UpdatePlatform::Linux, "x86_64" | "x64") => Ok(("linux-x64", "tar.gz")),
        (UpdatePlatform::Windows, "x86_64" | "x64") => Ok(("windows-x64", "zip")),
        _ => Err(UpdateError::new(
            format!("No direct Workdeck release exists for {architecture}."),
            ["Use Cargo, Homebrew, Nix, or a platform install script instead."],
        )),
    }
}

fn direct_update_invocation(
    target_version: &str,
    context: &SelfUpdateContext,
) -> Result<UpdateInvocation, UpdateError> {
    let (target, extension) = direct_target(context.platform, &context.architecture)?;
    let archive = format!("workdeck-{target}.{extension}");
    let url = format!(
        "https://github.com/ruttydm/workdeck/releases/download/v{target_version}/{archive}"
    );
    let mut env = context.env.clone();
    env.insert("WORKDECK_DIRECT_ARCHIVE_URL".into(), url);
    env.insert(
        "WORKDECK_DIRECT_CHECKSUM_URL".into(),
        format!("https://github.com/ruttydm/workdeck/releases/download/v{target_version}/{archive}.sha256"),
    );
    env.insert("WORKDECK_DIRECT_ARCHIVE".into(), archive);
    env.insert(
        "WORKDECK_EXECUTABLE".into(),
        context.executable_path.clone(),
    );
    env.insert("WORKDECK_PARENT_PID".into(), std::process::id().to_string());
    let command = match context.platform {
        UpdatePlatform::Macos | UpdatePlatform::Linux => vec![
            "sh".into(),
            "-c".into(),
            [
                "set -eu",
                "tmp=\"$(mktemp -d)\"",
                "stage=\"$WORKDECK_EXECUTABLE.new.$$\"",
                "trap 'rm -rf \"$tmp\"; rm -f \"$stage\"' EXIT",
                "curl -fsSL \"$WORKDECK_DIRECT_ARCHIVE_URL\" -o \"$tmp/$WORKDECK_DIRECT_ARCHIVE\"",
                "curl -fsSL \"$WORKDECK_DIRECT_CHECKSUM_URL\" -o \"$tmp/$WORKDECK_DIRECT_ARCHIVE.sha256\"",
                "cd \"$tmp\"",
                "if command -v sha256sum >/dev/null 2>&1; then sha256sum -c \"$WORKDECK_DIRECT_ARCHIVE.sha256\"; else shasum -a 256 -c \"$WORKDECK_DIRECT_ARCHIVE.sha256\"; fi",
                "tar -xzf \"$WORKDECK_DIRECT_ARCHIVE\"",
                "install -m 0755 workdeck \"$stage\"",
                "mv -f \"$stage\" \"$WORKDECK_EXECUTABLE\"",
            ]
            .join("; "),
        ],
        UpdatePlatform::Windows => vec![
            "powershell.exe".into(),
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-ExecutionPolicy".into(),
            "Bypass".into(),
            "-Command".into(),
            r#"$ErrorActionPreference='Stop'; $tmp=Join-Path ([IO.Path]::GetTempPath()) ([guid]::NewGuid()); New-Item -ItemType Directory $tmp | Out-Null; try { $archive=Join-Path $tmp $env:WORKDECK_DIRECT_ARCHIVE; Invoke-WebRequest -UseBasicParsing $env:WORKDECK_DIRECT_ARCHIVE_URL -OutFile $archive; Invoke-WebRequest -UseBasicParsing $env:WORKDECK_DIRECT_CHECKSUM_URL -OutFile "$archive.sha256"; $expected=((Get-Content "$archive.sha256") -split '\s+')[0].ToLowerInvariant(); $actual=(Get-FileHash -Algorithm SHA256 $archive).Hash.ToLowerInvariant(); if ($actual -ne $expected) { throw 'Workdeck archive checksum mismatch.' }; Expand-Archive -Force $archive $tmp; $replacement="$($env:WORKDECK_EXECUTABLE).new"; Copy-Item -Force (Join-Path $tmp 'workdeck.exe') $replacement; $quotedReplacement=$replacement.Replace("'","''"); $quotedTarget=$env:WORKDECK_EXECUTABLE.Replace("'","''"); $follow="Wait-Process -Id $($env:WORKDECK_PARENT_PID) -ErrorAction SilentlyContinue; Move-Item -Force '$quotedReplacement' '$quotedTarget'"; Start-Process -WindowStyle Hidden powershell.exe -ArgumentList @('-NoProfile','-NonInteractive','-Command',$follow) } finally { Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $tmp }"#.into(),
        ],
    };
    Ok(UpdateInvocation {
        command,
        env: Some(env),
    })
}

fn build_update_invocation(
    source: WorkdeckInstallSource,
    target_version: &str,
    context: &SelfUpdateContext,
) -> Result<UpdateInvocation, UpdateError> {
    let mut env = context.env.clone();
    let invocation = match source {
        WorkdeckInstallSource::Cargo => UpdateInvocation {
            command: vec![
                "cargo".into(),
                "install".into(),
                "workdeck-cli".into(),
                "--version".into(),
                target_version.into(),
                "--locked".into(),
                "--force".into(),
            ],
            env: None,
        },
        WorkdeckInstallSource::Homebrew => UpdateInvocation {
            command: vec!["brew".into(), "upgrade".into(), "workdeck".into()],
            env: None,
        },
        WorkdeckInstallSource::Curl => {
            env.insert(INSTALL_VERSION_ENV.into(), target_version.into());
            UpdateInvocation {
                command: vec!["sh".into(), "-c".into(), shell_install_script()],
                env: Some(env),
            }
        }
        WorkdeckInstallSource::PowerShell => {
            env.insert(INSTALL_VERSION_ENV.into(), target_version.into());
            UpdateInvocation {
                command: vec![
                    "powershell.exe".into(),
                    "-NoProfile".into(),
                    "-NonInteractive".into(),
                    "-ExecutionPolicy".into(),
                    "Bypass".into(),
                    "-Command".into(),
                    powershell_install_script(),
                ],
                env: Some(env),
            }
        }
        WorkdeckInstallSource::Direct => {
            return direct_update_invocation(target_version, context);
        }
        WorkdeckInstallSource::Nix | WorkdeckInstallSource::Dev => {
            unreachable!("unmanaged sources do not build update invocations")
        }
    };
    Ok(invocation)
}

pub fn run_self_update(
    input: &SelfUpdateCommandInput,
    context: &SelfUpdateContext,
) -> Result<SelfUpdateResult, UpdateError> {
    let detected = detect_install_source(&InstallSourceFacts {
        env: &context.env,
        executable_path: &context.executable_path,
        version: &context.installed_version,
        home_dir: context
            .env
            .get("HOME")
            .or_else(|| context.env.get("USERPROFILE"))
            .map(String::as_str),
        platform: context.platform,
        realpath: None,
    });
    let source = input.method.or(context.install_source).unwrap_or(detected);
    if matches!(
        source,
        WorkdeckInstallSource::Nix | WorkdeckInstallSource::Dev
    ) {
        let guidance = unmanaged_install_guidance(source);
        if input.version.is_some() {
            return Err(UpdateError::new(
                format!(
                    "Workdeck installed with {} cannot update to a specific version from here.",
                    describe_install_source(source)
                ),
                [guidance[1]],
            ));
        }
        let result = SelfUpdateResult {
            exit_code: if input.check { 0 } else { 1 },
            stdout: format!(
                "workdeck {} (installed with {})\n{}\n{}\n",
                context.installed_version,
                describe_install_source(source),
                guidance[0],
                guidance[1]
            ),
            stderr: String::new(),
        };
        context.reporter.stdout(&result.stdout);
        return Ok(result);
    }
    if source == WorkdeckInstallSource::Homebrew && input.version.is_some() {
        return Err(UpdateError::new(
            "Homebrew installs cannot select a specific Workdeck version.",
            ["Run `workdeck update` without a version to move to the newest formula release."],
        ));
    }
    let versions = fetch_channel_versions(source, &context.release_lookup);
    let latest = versions.latest;
    if input.check {
        let Some(latest) = latest else {
            return Err(UpdateError::new(
                fetch_failure_message(source),
                ["Check your network connection."],
            ));
        };
        let requested = input
            .version
            .as_ref()
            .map(|version| format!("requested {version}\n"))
            .unwrap_or_default();
        let status = if is_newer_version(&context.installed_version, &latest) {
            "An update is available. Run `workdeck update` to install it."
        } else {
            "Workdeck is up to date."
        };
        let result = SelfUpdateResult {
            exit_code: 0,
            stdout: format!(
                "workdeck {} (installed with {})\nlatest {latest}\n{requested}{status}\n",
                context.installed_version,
                describe_install_source(source)
            ),
            stderr: String::new(),
        };
        context.reporter.stdout(&result.stdout);
        return Ok(result);
    }
    let target = input.version.clone().or(latest).ok_or_else(|| {
        UpdateError::new(
            fetch_failure_message(source),
            ["Check your network connection, or pass an explicit version."],
        )
    })?;
    let already_current = input
        .version
        .is_some()
        .then_some(())
        .is_some_and(|_| context.installed_version == target)
        || (input.version.is_none()
            && (!is_comparable_version(&context.installed_version)
                || !is_newer_version(&context.installed_version, &target)));
    if already_current {
        let result = SelfUpdateResult {
            exit_code: 0,
            stdout: format!(
                "workdeck {} is already up to date.\n",
                context.installed_version
            ),
            stderr: String::new(),
        };
        context.reporter.stdout(&result.stdout);
        return Ok(result);
    }
    let invocation = build_update_invocation(source, &target, context)?;
    let command_text = invocation.command.join(" ");
    let mut result = SelfUpdateResult {
        exit_code: 0,
        stdout: format!(
            "Updating workdeck {} -> {target} with `{command_text}`\n",
            context.installed_version
        ),
        stderr: String::new(),
    };
    context.reporter.stdout(&result.stdout);
    let process = context.runner.run(&invocation).map_err(|error| {
        let executable = invocation.command.first().map_or("updater", String::as_str);
        UpdateError::new(
            format!("Could not run {executable}: {error}"),
            [format!(
                "Updating this install needs `{executable}` on PATH."
            )],
        )
    })?;
    result.exit_code = process.exit_code;
    if process.exit_code == 0 {
        let completion = format!("Updated workdeck to {target}.\n");
        context.reporter.stdout(&completion);
        result.stdout.push_str(&completion);
    } else {
        if !process.stderr.trim().is_empty() {
            result.stderr.push_str(process.stderr.trim());
            result.stderr.push('\n');
        }
        result.stderr.push_str(&format!(
            "workdeck: `{command_text}` failed with exit code {}.\n",
            process.exit_code
        ));
        context.reporter.stderr(&result.stderr);
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
