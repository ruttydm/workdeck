//! Cross-process coordination and native launch of the loopback session broker daemon.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

use crate::{
    DEFAULT_SESSION_BROKER_HEALTH_PATH, ResolvedSessionBrokerConfig, request_session_daemon_http,
    workdeck_session_broker_runtime_directory,
};

pub const DEFAULT_DAEMON_LOCK_STALE: Duration = Duration::from_secs(15);
pub const DEFAULT_DAEMON_STARTUP_TIMEOUT: Duration = Duration::from_secs(3);
pub const DEFAULT_DAEMON_HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(100);
pub const MAX_DAEMON_LAUNCH_METADATA_BYTES: u64 = 16 * 1024;

const MAX_JAVASCRIPT_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonLaunchCommand {
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionBrokerRuntimePaths {
    pub runtime_dir: PathBuf,
    pub lock_path: PathBuf,
    pub metadata_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionBrokerLaunchLockFile {
    owner_pid: u32,
    host: String,
    port: u32,
    acquired_at: String,
}

/// The bounded exact launch metadata the launching process wrote beside the lock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBrokerLaunchMetadata {
    pub pid: u64,
    pub host: String,
    pub port: u64,
    pub command: String,
    pub args: Vec<String>,
    pub launched_at: String,
    pub launched_by_pid: u64,
    pub launch_cwd: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSessionBrokerHealth {
    pub ok: bool,
    pub pid: Option<u64>,
    pub sessions: Option<u64>,
    pub pending_commands: Option<u64>,
    pub started_at: Option<String>,
    pub uptime_ms: Option<u64>,
    pub session_api: Option<String>,
    pub session_capabilities: Option<String>,
    pub session_socket: Option<String>,
    pub stale_session_ttl_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonLaunchOptions {
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub argv: Vec<String>,
    pub exec_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaunchedSessionBrokerDaemon {
    pub pid: u32,
}

pub type SessionBrokerHealthProbe = Arc<dyn Fn(&ResolvedSessionBrokerConfig) -> bool + Send + Sync>;
pub type SessionBrokerPortProbe =
    Arc<dyn Fn(&ResolvedSessionBrokerConfig, Duration) -> bool + Send + Sync>;
pub type SessionBrokerDaemonLauncher = Arc<
    dyn Fn(&DaemonLaunchOptions) -> Result<LaunchedSessionBrokerDaemon, BrokerLauncherError>
        + Send
        + Sync,
>;

#[derive(Clone)]
pub struct SessionBrokerAvailabilityHooks {
    pub is_healthy: SessionBrokerHealthProbe,
    pub is_port_reachable: SessionBrokerPortProbe,
    pub launch_daemon: SessionBrokerDaemonLauncher,
}

impl std::fmt::Debug for SessionBrokerAvailabilityHooks {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionBrokerAvailabilityHooks")
            .finish_non_exhaustive()
    }
}

impl Default for SessionBrokerAvailabilityHooks {
    fn default() -> Self {
        Self {
            is_healthy: Arc::new(|config| {
                is_session_broker_healthy(config, Duration::from_millis(500))
            }),
            is_port_reachable: Arc::new(is_loopback_port_reachable),
            launch_daemon: Arc::new(launch_session_broker_daemon),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EnsureSessionBrokerAvailableOptions {
    pub config: ResolvedSessionBrokerConfig,
    pub launch: DaemonLaunchOptions,
    pub timeout: Duration,
    pub interval: Duration,
    pub lock_stale_after: Duration,
    pub timeout_message: Option<String>,
    pub hooks: SessionBrokerAvailabilityHooks,
}

impl EnsureSessionBrokerAvailableOptions {
    pub fn from_environment(env: BTreeMap<String, String>) -> Result<Self, BrokerLauncherError> {
        let config = crate::resolve_session_broker_config(&env)
            .map_err(|error| BrokerLauncherError::Configuration(error.to_string()))?;
        let cwd = std::env::current_dir()?;
        let exec_path = std::env::current_exe()?.to_string_lossy().into_owned();
        Ok(Self {
            config,
            launch: DaemonLaunchOptions {
                cwd,
                env,
                argv: std::env::args().collect(),
                exec_path,
            },
            timeout: DEFAULT_DAEMON_STARTUP_TIMEOUT,
            interval: DEFAULT_DAEMON_HEALTH_POLL_INTERVAL,
            lock_stale_after: DEFAULT_DAEMON_LOCK_STALE,
            timeout_message: None,
            hooks: SessionBrokerAvailabilityHooks::default(),
        })
    }

    pub fn from_process_environment() -> Result<Self, BrokerLauncherError> {
        Self::from_environment(std::env::vars().collect())
    }
}

#[derive(Debug, Error)]
pub enum BrokerLauncherError {
    #[error("session broker launcher I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("session broker launcher JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("session broker configuration failed: {0}")]
    Configuration(String),
    #[error(
        "Workdeck session daemon port {host}:{port} is already in use by another process. Stop the conflicting process or set WORKDECK_MCP_PORT to a different loopback port."
    )]
    PortConflict { host: String, port: u32 },
    #[error("{0}")]
    StartupTimeout(String),
}

struct SessionBrokerLaunchLock {
    path: PathBuf,
    owner_pid: u32,
}

impl Drop for SessionBrokerLaunchLock {
    fn drop(&mut self) {
        let current = read_json_file(&self.path);
        if current
            .as_ref()
            .and_then(lock_owner_pid)
            .is_some_and(|pid| pid == u64::from(self.owner_pid))
        {
            remove_file_if_present(&self.path);
        }
    }
}

/// Ownership of the per-host/port daemon launch lock; released by dropping it.
///
/// The lock serializes who may spawn a daemon: every window's reconnect loop and
/// `workdeck daemon restart` go through it, which is what stops an old window from respawning
/// the old binary while a restart is replacing it.
pub struct DaemonLaunchLockGuard {
    inner: Option<SessionBrokerLaunchLock>,
}

impl DaemonLaunchLockGuard {
    /// Release the lock immediately, ahead of drop.
    pub fn release(&mut self) {
        drop(self.inner.take());
    }
}

impl Drop for DaemonLaunchLockGuard {
    fn drop(&mut self) {
        self.release();
    }
}

/// Acquire the per-host/port daemon launch lock, or `None` while another live process holds it.
pub fn try_acquire_daemon_launch_lock(
    config: &ResolvedSessionBrokerConfig,
    env: &BTreeMap<String, String>,
) -> Result<Option<DaemonLaunchLockGuard>, BrokerLauncherError> {
    try_acquire_daemon_launch_lock_inner(config, env, DEFAULT_DAEMON_LOCK_STALE)
        .map(|lock| lock.map(|inner| DaemonLaunchLockGuard { inner: Some(inner) }))
}

/// A native Workdeck build always relaunches its sole executable with `daemon serve`.
///
/// `argv` is retained in the boundary so callers and translated fixtures can prove that obsolete
/// script-wrapper or virtual-entrypoint shapes cannot influence native process selection.
#[must_use]
pub fn resolve_daemon_launch_command(
    _argv: &[String],
    exec_path: impl Into<String>,
) -> DaemonLaunchCommand {
    DaemonLaunchCommand {
        command: exec_path.into(),
        args: vec!["daemon".into(), "serve".into()],
    }
}

/// Resolve the owner-private paths coordinating one daemon per loopback host and port.
#[must_use]
pub fn resolve_session_broker_runtime_paths(
    config: &ResolvedSessionBrokerConfig,
    env: &BTreeMap<String, String>,
) -> SessionBrokerRuntimePaths {
    let runtime_dir = workdeck_session_broker_runtime_directory(env);
    let file_stem = format!("{}-{}", safe_runtime_token(&config.host), config.port);
    SessionBrokerRuntimePaths {
        lock_path: runtime_dir.join(format!("daemon-{file_stem}.lock")),
        metadata_path: runtime_dir.join(format!("daemon-{file_stem}.json")),
        runtime_dir,
    }
}

/// Strictly parse either the minimal or the bounded legacy-rich health response.
#[must_use]
pub fn parse_session_broker_health(value: &Value) -> Option<ParsedSessionBrokerHealth> {
    const OPTIONAL: &[&str] = &[
        "pid",
        "sessions",
        "pendingCommands",
        "startedAt",
        "uptimeMs",
        "sessionApi",
        "sessionCapabilities",
        "sessionSocket",
        "staleSessionTtlMs",
        "paths",
    ];
    let object = value.as_object()?;
    if object.get("ok").and_then(Value::as_bool) != Some(true)
        || object
            .keys()
            .any(|key| key != "ok" && !OPTIONAL.contains(&key.as_str()))
    {
        return None;
    }
    if let Some(paths) = object.get("paths") {
        parse_legacy_health_paths(paths)?;
    }
    Some(ParsedSessionBrokerHealth {
        ok: true,
        pid: optional_safe_integer(object, "pid")?,
        sessions: optional_safe_integer(object, "sessions")?,
        pending_commands: optional_safe_integer(object, "pendingCommands")?,
        started_at: optional_string(object, "startedAt")?,
        uptime_ms: optional_safe_integer(object, "uptimeMs")?,
        session_api: optional_string(object, "sessionApi")?,
        session_capabilities: optional_string(object, "sessionCapabilities")?,
        session_socket: optional_string(object, "sessionSocket")?,
        stale_session_ttl_ms: optional_safe_integer(object, "staleSessionTtlMs")?,
    })
}

/// Read the bounded exact launch metadata the launching process wrote beside the lock.
///
/// This is a hint about which generation launched the daemon (its pid, command, and time), never
/// process authority: the signed hello remains the only compatibility and identity check.
#[must_use]
pub fn read_session_broker_launch_metadata(
    config: &ResolvedSessionBrokerConfig,
    env: &BTreeMap<String, String>,
) -> Option<SessionBrokerLaunchMetadata> {
    let path = resolve_session_broker_runtime_paths(config, env).metadata_path;
    let metadata = fs::metadata(&path).ok()?;
    if !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > MAX_DAEMON_LAUNCH_METADATA_BYTES
    {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    if u64::try_from(bytes.len()).ok()? != metadata.len()
        || u64::try_from(bytes.len()).ok()? > MAX_DAEMON_LAUNCH_METADATA_BYTES
    {
        return None;
    }
    let value = serde_json::from_slice(&bytes).ok()?;
    parse_launch_metadata(&value)
}

/// Read a bounded exact metadata fingerprint as a reconnect hint, never process authority.
#[must_use]
pub fn read_session_broker_launch_fingerprint(
    config: &ResolvedSessionBrokerConfig,
    env: &BTreeMap<String, String>,
) -> Option<String> {
    serde_json::to_string(&read_session_broker_launch_metadata(config, env)?).ok()
}

/// Read the daemon's exact health payload when it answers on the configured loopback port.
#[must_use]
pub fn read_session_broker_health(
    config: &ResolvedSessionBrokerConfig,
    timeout: Duration,
) -> Option<ParsedSessionBrokerHealth> {
    request_session_daemon_http(
        config,
        DEFAULT_SESSION_BROKER_HEALTH_PATH,
        "report health",
        timeout,
        |response| {
            if !(200..300).contains(&response.status) {
                return Ok(None);
            }
            let parsed = serde_json::from_slice::<Value>(&response.body)
                .ok()
                .and_then(|value| parse_session_broker_health(&value));
            Ok(parsed)
        },
    )
    .unwrap_or(None)
}

#[must_use]
pub fn is_session_broker_healthy(config: &ResolvedSessionBrokerConfig, timeout: Duration) -> bool {
    read_session_broker_health(config, timeout).is_some_and(|health| health.ok)
}

/// Check whether any local process accepts TCP connections at the broker address.
#[must_use]
pub fn is_loopback_port_reachable(config: &ResolvedSessionBrokerConfig, timeout: Duration) -> bool {
    let Ok(port) = u16::try_from(config.port) else {
        return false;
    };
    let Ok(addresses) = (config.host.as_str(), port).to_socket_addrs() else {
        return false;
    };
    addresses
        .into_iter()
        .any(|address| TcpStream::connect_timeout(&address, timeout).is_ok())
}

/// Spawn the daemon detached from the current terminal session.
pub fn launch_session_broker_daemon(
    options: &DaemonLaunchOptions,
) -> Result<LaunchedSessionBrokerDaemon, BrokerLauncherError> {
    let launch = resolve_daemon_launch_command(&options.argv, options.exec_path.clone());
    let mut command = Command::new(&launch.command);
    command
        .args(&launch.args)
        .current_dir(&options.cwd)
        .env_clear()
        .envs(&options.env)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_detached_process(&mut command);
    let child = command.spawn()?;
    Ok(LaunchedSessionBrokerDaemon { pid: child.id() })
}

/// Collaborators of one recorded launch; every field defaults to the native behavior.
#[derive(Default)]
pub struct LaunchSessionBrokerDaemonAndRecordOptions {
    pub config: Option<ResolvedSessionBrokerConfig>,
    pub launch: Option<DaemonLaunchOptions>,
    pub launch_daemon: Option<SessionBrokerDaemonLauncher>,
}

/// Spawn the daemon and record its launch metadata beside the lock. The caller must hold the
/// launch lock.
pub fn launch_session_broker_daemon_and_record(
    options: &LaunchSessionBrokerDaemonAndRecordOptions,
) -> Result<SessionBrokerLaunchMetadata, BrokerLauncherError> {
    let config = options.config.clone().unwrap_or_else(|| {
        crate::resolve_session_broker_config(&std::env::vars().collect()).unwrap_or_else(|_| {
            ResolvedSessionBrokerConfig {
                host: crate::DEFAULT_SESSION_BROKER_HOST.into(),
                port: crate::DEFAULT_SESSION_BROKER_PORT,
                http_origin: format!(
                    "http://{}:{}",
                    crate::DEFAULT_SESSION_BROKER_HOST,
                    crate::DEFAULT_SESSION_BROKER_PORT
                ),
                ws_origin: format!(
                    "ws://{}:{}",
                    crate::DEFAULT_SESSION_BROKER_HOST,
                    crate::DEFAULT_SESSION_BROKER_PORT
                ),
            }
        })
    });
    let launch = options
        .launch
        .clone()
        .unwrap_or_else(|| DaemonLaunchOptions {
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            env: std::env::vars().collect(),
            argv: std::env::args().collect(),
            exec_path: std::env::current_exe()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|_| "workdeck".into()),
        });
    let launch_daemon = options
        .launch_daemon
        .clone()
        .unwrap_or_else(|| Arc::new(launch_session_broker_daemon));
    let paths = resolve_session_broker_runtime_paths(&config, &launch.env);
    let launch_command = resolve_daemon_launch_command(&launch.argv, launch.exec_path.clone());
    let child = (launch_daemon)(&launch)?;
    let metadata = SessionBrokerLaunchMetadata {
        pid: u64::from(child.pid),
        host: config.host.clone(),
        port: u64::from(config.port),
        command: launch_command.command,
        args: launch_command.args,
        launched_at: now_iso8601(),
        launched_by_pid: u64::from(std::process::id()),
        launch_cwd: launch.cwd.to_string_lossy().into_owned(),
    };
    write_daemon_launch_metadata(&paths, &metadata)?;
    Ok(metadata)
}

/// Poll until the daemon answers health (`expected == true`) or stops answering (`false`).
pub fn wait_for_session_broker_health(
    config: &ResolvedSessionBrokerConfig,
    expected: bool,
    timeout: Duration,
) -> bool {
    let probe: SessionBrokerHealthProbe = Arc::new(move |config| {
        is_session_broker_healthy(config, Duration::from_millis(500)) == expected
    });
    wait_for_daemon_health(config, timeout, DEFAULT_DAEMON_HEALTH_POLL_INTERVAL, &probe)
}

/// Ensure one healthy local daemon exists while serializing launch attempts across processes.
pub fn ensure_session_broker_available(
    options: &EnsureSessionBrokerAvailableOptions,
) -> Result<(), BrokerLauncherError> {
    let paths = resolve_session_broker_runtime_paths(&options.config, &options.launch.env);
    clean_stale_daemon_metadata(&paths);
    if (options.hooks.is_healthy)(&options.config) {
        return Ok(());
    }

    let deadline = Instant::now() + options.timeout;
    while Instant::now() < deadline {
        let lock = try_acquire_daemon_launch_lock_inner(
            &options.config,
            &options.launch.env,
            options.lock_stale_after,
        )?;
        if let Some(lock) = lock {
            clean_stale_daemon_metadata(&paths);
            if (options.hooks.is_healthy)(&options.config) {
                drop(lock);
                return Ok(());
            }
            let launched = launch_session_broker_daemon_and_record(
                &LaunchSessionBrokerDaemonAndRecordOptions {
                    config: Some(options.config.clone()),
                    launch: Some(options.launch.clone()),
                    launch_daemon: Some(Arc::clone(&options.hooks.launch_daemon)),
                },
            );
            let _launched = launched?;
            let ready = wait_for_daemon_health(
                &options.config,
                options.timeout,
                options.interval,
                &options.hooks.is_healthy,
            );
            drop(lock);
            if ready {
                return Ok(());
            }
        }

        let now = Instant::now();
        if now >= deadline {
            break;
        }
        let remaining = deadline.saturating_duration_since(now);
        if wait_for_daemon_health(
            &options.config,
            remaining.min(options.interval),
            options.interval,
            &options.hooks.is_healthy,
        ) {
            return Ok(());
        }
        clean_stale_daemon_metadata(&paths);
    }

    if (options.hooks.is_port_reachable)(&options.config, Duration::from_millis(500)) {
        return Err(BrokerLauncherError::PortConflict {
            host: options.config.host.clone(),
            port: options.config.port,
        });
    }
    Err(BrokerLauncherError::StartupTimeout(
        options.timeout_message.clone().unwrap_or_else(|| {
            format!(
                "Timed out waiting for the Workdeck session daemon on {}:{}. The app will retry in the background.",
                options.config.host, options.config.port
            )
        }),
    ))
}

fn safe_runtime_token(value: &str) -> String {
    let mut token = String::new();
    let mut separator = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            if separator && !token.is_empty() {
                token.push('-');
            }
            token.push(character);
            separator = false;
        } else {
            separator = true;
        }
    }
    if token.is_empty() {
        "default".into()
    } else {
        token
    }
}

fn parse_launch_metadata(value: &Value) -> Option<SessionBrokerLaunchMetadata> {
    const KEYS: [&str; 8] = [
        "pid",
        "host",
        "port",
        "command",
        "args",
        "launchedAt",
        "launchedByPid",
        "launchCwd",
    ];
    let object = value.as_object()?;
    if object.len() != KEYS.len() || KEYS.iter().any(|key| !object.contains_key(*key)) {
        return None;
    }
    let args = object
        .get("args")?
        .as_array()?
        .iter()
        .map(|value| value.as_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()?;
    let pid = required_safe_integer(object, "pid")?;
    let port = required_safe_integer(object, "port")?;
    let launched_by_pid = required_safe_integer(object, "launchedByPid")?;
    if pid == 0 || launched_by_pid == 0 || !(1..=65_535).contains(&port) {
        return None;
    }
    Some(SessionBrokerLaunchMetadata {
        pid,
        host: object.get("host")?.as_str()?.into(),
        port,
        command: object.get("command")?.as_str()?.into(),
        args,
        launched_at: object.get("launchedAt")?.as_str()?.into(),
        launched_by_pid,
        launch_cwd: object.get("launchCwd")?.as_str()?.into(),
    })
}

fn parse_legacy_health_paths(value: &Value) -> Option<()> {
    let paths = value.as_object()?;
    let allowed = ["health", "socket", "api", "capabilities"];
    if !paths.contains_key("health")
        || !paths.contains_key("socket")
        || paths.keys().any(|key| !allowed.contains(&key.as_str()))
        || paths.values().any(|value| !value.is_string())
    {
        return None;
    }
    Some(())
}

fn required_safe_integer(object: &Map<String, Value>, key: &str) -> Option<u64> {
    object
        .get(key)?
        .as_u64()
        .filter(|value| *value <= MAX_JAVASCRIPT_SAFE_INTEGER)
}

fn optional_safe_integer(object: &Map<String, Value>, key: &str) -> Option<Option<u64>> {
    match object.get(key) {
        None => Some(None),
        Some(value) => value
            .as_u64()
            .filter(|value| *value <= MAX_JAVASCRIPT_SAFE_INTEGER)
            .map(Some),
    }
}

fn optional_string(object: &Map<String, Value>, key: &str) -> Option<Option<String>> {
    match object.get(key) {
        None => Some(None),
        Some(value) => value.as_str().map(|value| Some(value.to_owned())),
    }
}

fn read_json_file(path: &Path) -> Option<Value> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn js_truthy_json(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().is_some_and(|value| value != 0.0),
        Value::String(value) => !value.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

fn lock_owner_pid(value: &Value) -> Option<u64> {
    value.get("ownerPid")?.as_u64()
}

fn remove_file_if_present(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => {}
    }
}

fn clean_stale_daemon_metadata(paths: &SessionBrokerRuntimePaths) {
    let Some(metadata) = read_json_file(&paths.metadata_path) else {
        return;
    };
    if !js_truthy_json(&metadata) {
        return;
    }
    let pid = metadata.get("pid").and_then(Value::as_u64).unwrap_or(0);
    if !is_running_pid(pid) {
        remove_file_if_present(&paths.metadata_path);
    }
}

fn try_acquire_daemon_launch_lock_inner(
    config: &ResolvedSessionBrokerConfig,
    env: &BTreeMap<String, String>,
    stale_after: Duration,
) -> Result<Option<SessionBrokerLaunchLock>, BrokerLauncherError> {
    let paths = resolve_session_broker_runtime_paths(config, env);
    create_private_runtime_directory(&paths.runtime_dir)?;
    loop {
        let owner_pid = std::process::id();
        let payload = SessionBrokerLaunchLockFile {
            owner_pid,
            host: config.host.clone(),
            port: config.port,
            acquired_at: now_iso8601(),
        };
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&paths.lock_path) {
            Ok(mut file) => {
                file.write_all(&serde_json::to_vec_pretty(&payload)?)?;
                file.sync_all()?;
                return Ok(Some(SessionBrokerLaunchLock {
                    path: paths.lock_path,
                    owner_pid,
                }));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }

        let existing = read_json_file(&paths.lock_path);
        if existing.as_ref().is_none_or(|value| !js_truthy_json(value)) {
            if lock_file_is_stale(&paths.lock_path, stale_after) {
                remove_file_if_present(&paths.lock_path);
                continue;
            }
            return Ok(None);
        }
        let owner_alive = existing
            .as_ref()
            .and_then(lock_owner_pid)
            .is_some_and(is_running_pid);
        if owner_alive {
            return Ok(None);
        }
        remove_file_if_present(&paths.lock_path);
    }
}

fn lock_file_is_stale(path: &Path, stale_after: Duration) -> bool {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age > stale_after)
}

fn write_daemon_launch_metadata(
    paths: &SessionBrokerRuntimePaths,
    metadata: &SessionBrokerLaunchMetadata,
) -> Result<(), BrokerLauncherError> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&paths.metadata_path)?;
    file.write_all(&serde_json::to_vec_pretty(metadata)?)?;
    file.sync_all()?;
    Ok(())
}

fn wait_for_daemon_health(
    config: &ResolvedSessionBrokerConfig,
    timeout: Duration,
    interval: Duration,
    is_healthy: &SessionBrokerHealthProbe,
) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if is_healthy(config) {
            return true;
        }
        if interval.is_zero() {
            thread::yield_now();
        } else {
            thread::sleep(interval);
        }
    }
    false
}

fn now_iso8601() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(unix)]
fn create_private_runtime_directory(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn create_private_runtime_directory(path: &Path) -> std::io::Result<()> {
    fs::create_dir_all(path)
}

#[cfg(unix)]
fn configure_detached_process(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    // SAFETY: the callback invokes only the async-signal-safe `setsid(2)` between fork and exec.
    unsafe {
        command.pre_exec(|| {
            // SAFETY: `setsid` has no pointer arguments and its return value is checked.
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(windows)]
fn configure_detached_process(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}

#[cfg(not(any(unix, windows)))]
fn configure_detached_process(_command: &mut Command) {}

#[cfg(unix)]
fn is_running_pid(pid: u64) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    // SAFETY: signal zero performs only a process-existence/permission check.
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(windows)]
fn is_running_pid(pid: u64) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    let Ok(pid) = u32::try_from(pid) else {
        return false;
    };
    if pid == 0 {
        return false;
    }
    // SAFETY: the handle is checked before use and closed on every successful open.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut exit_code = 0;
        let success = GetExitCodeProcess(handle, &mut exit_code) != 0;
        CloseHandle(handle);
        success && exit_code == STILL_ACTIVE as u32
    }
}

#[cfg(not(any(unix, windows)))]
fn is_running_pid(pid: u64) -> bool {
    pid == u64::from(std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    fn test_config(port: u32) -> ResolvedSessionBrokerConfig {
        ResolvedSessionBrokerConfig {
            host: "127.0.0.1".into(),
            port,
            http_origin: format!("http://127.0.0.1:{port}"),
            ws_origin: format!("ws://127.0.0.1:{port}"),
        }
    }

    fn test_options(
        root: &Path,
        config: ResolvedSessionBrokerConfig,
    ) -> EnsureSessionBrokerAvailableOptions {
        let env = BTreeMap::from([
            (
                "XDG_RUNTIME_DIR".into(),
                root.to_string_lossy().into_owned(),
            ),
            ("PATH".into(), "/usr/bin".into()),
        ]);
        EnsureSessionBrokerAvailableOptions {
            config,
            launch: DaemonLaunchOptions {
                cwd: PathBuf::from("/repo"),
                env,
                argv: vec!["workdeck".into(), "diff".into()],
                exec_path: "/usr/bin/workdeck".into(),
            },
            timeout: Duration::from_millis(300),
            interval: Duration::from_millis(10),
            lock_stale_after: DEFAULT_DAEMON_LOCK_STALE,
            timeout_message: None,
            hooks: SessionBrokerAvailabilityHooks::default(),
        }
    }

    #[test]
    fn reads_only_bounded_exact_launch_metadata_as_generation_hint() {
        let root = tempfile::tempdir().unwrap();
        let config = test_config(47_657);
        let env = BTreeMap::from([(
            "XDG_RUNTIME_DIR".into(),
            root.path().to_string_lossy().into_owned(),
        )]);
        let paths = resolve_session_broker_runtime_paths(&config, &env);
        fs::create_dir_all(&paths.runtime_dir).unwrap();
        let metadata = json!({
            "pid": 123,
            "host": config.host,
            "port": config.port,
            "command": "/fixture/workdeck",
            "args": ["daemon", "serve"],
            "launchedAt": "2026-01-01T00:00:00.000Z",
            "launchedByPid": 122,
            "launchCwd": "/fixture"
        });
        fs::write(&paths.metadata_path, serde_json::to_vec(&metadata).unwrap()).unwrap();
        let first = read_session_broker_launch_fingerprint(&config, &env).unwrap();
        assert_eq!(
            first,
            r#"{"pid":123,"host":"127.0.0.1","port":47657,"command":"/fixture/workdeck","args":["daemon","serve"],"launchedAt":"2026-01-01T00:00:00.000Z","launchedByPid":122,"launchCwd":"/fixture"}"#
        );
        let mut changed = metadata.clone();
        changed["pid"] = json!(124);
        fs::write(&paths.metadata_path, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert_ne!(
            read_session_broker_launch_fingerprint(&config, &env).as_deref(),
            Some(first.as_str())
        );
        for malformed in [
            json!([]),
            json!({
                "pid": 123, "host": "127.0.0.1", "port": 47657,
                "command": "/fixture/workdeck", "args": ["daemon", "serve"],
                "launchedAt": "x", "launchedByPid": 122, "launchCwd": "/fixture",
                "extra": true
            }),
            json!({
                "pid": 123, "host": "127.0.0.1", "port": 47657,
                "command": "/fixture/workdeck", "args": {},
                "launchedAt": "x", "launchedByPid": 122, "launchCwd": "/fixture"
            }),
        ] {
            fs::write(
                &paths.metadata_path,
                serde_json::to_vec(&malformed).unwrap(),
            )
            .unwrap();
            assert_eq!(read_session_broker_launch_fingerprint(&config, &env), None);
        }
        fs::write(
            &paths.metadata_path,
            br#"{"__proto__":true,"pid":123,"host":"127.0.0.1","port":47657,"command":"/fixture/workdeck","args":["daemon","serve"],"launchedAt":"x","launchedByPid":122,"launchCwd":"/fixture"}"#,
        )
        .unwrap();
        assert_eq!(read_session_broker_launch_fingerprint(&config, &env), None);
        fs::write(
            &paths.metadata_path,
            vec![b'x'; MAX_DAEMON_LAUNCH_METADATA_BYTES as usize + 1],
        )
        .unwrap();
        assert_eq!(read_session_broker_launch_fingerprint(&config, &env), None);
    }

    #[test]
    fn strictly_parses_minimal_and_legacy_health_responses() {
        assert_eq!(
            parse_session_broker_health(&json!({"ok": true})),
            Some(ParsedSessionBrokerHealth {
                ok: true,
                pid: None,
                sessions: None,
                pending_commands: None,
                started_at: None,
                uptime_ms: None,
                session_api: None,
                session_capabilities: None,
                session_socket: None,
                stale_session_ttl_ms: None,
            })
        );
        let rich = parse_session_broker_health(&json!({
            "ok": true,
            "pid": 123,
            "sessions": 1,
            "pendingCommands": 0,
            "startedAt": "2026-04-15T00:00:00.000Z",
            "uptimeMs": 10,
            "staleSessionTtlMs": 45_000,
            "paths": {"health": "/health", "socket": "/session"}
        }))
        .unwrap();
        assert_eq!(rich.pid, Some(123));
        assert_eq!(rich.sessions, Some(1));
        for value in [
            json!(null),
            json!([]),
            json!({"ok": "yes"}),
            json!({"ok": true, "pid": 1.5}),
            json!({"ok": true, "extra": 1}),
        ] {
            assert_eq!(parse_session_broker_health(&value), None);
        }
    }

    #[test]
    fn native_rewrite_ignores_obsolete_script_entrypoints() {
        let argv = vec!["runtime".into(), "src/main.tsx".into(), "diff".into()];
        assert_eq!(
            resolve_daemon_launch_command(&argv, "/usr/bin/workdeck"),
            DaemonLaunchCommand {
                command: "/usr/bin/workdeck".into(),
                args: vec!["daemon".into(), "serve".into()]
            }
        );
    }

    #[test]
    fn falls_back_to_relaunching_the_current_native_executable() {
        let argv = vec!["/usr/local/bin/workdeck".into(), "diff".into()];
        assert_eq!(
            resolve_daemon_launch_command(&argv, "/usr/local/bin/workdeck"),
            DaemonLaunchCommand {
                command: "/usr/local/bin/workdeck".into(),
                args: vec!["daemon".into(), "serve".into()]
            }
        );
    }

    #[test]
    fn native_rewrite_ignores_unix_virtual_entrypoint_shapes() {
        let argv = vec![
            "runtime".into(),
            "/virtual/root/workdeck".into(),
            "show".into(),
        ];
        assert_eq!(
            resolve_daemon_launch_command(&argv, "/opt/workdeck"),
            DaemonLaunchCommand {
                command: "/opt/workdeck".into(),
                args: vec!["daemon".into(), "serve".into()]
            }
        );
    }

    #[test]
    fn native_rewrite_ignores_windows_virtual_entrypoint_shapes() {
        let argv = vec![
            "runtime".into(),
            "B:\\virtual\\root\\workdeck.exe".into(),
            "diff".into(),
        ];
        assert_eq!(
            resolve_daemon_launch_command(&argv, "C:\\bin\\workdeck.exe"),
            DaemonLaunchCommand {
                command: "C:\\bin\\workdeck.exe".into(),
                args: vec!["daemon".into(), "serve".into()]
            }
        );
    }

    #[test]
    fn detects_whether_some_process_is_listening_on_daemon_port() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = u32::from(listener.local_addr().unwrap().port());
        let config = test_config(port);
        assert!(is_loopback_port_reachable(
            &config,
            Duration::from_millis(500)
        ));
        drop(listener);
        assert!(!is_loopback_port_reachable(
            &config,
            Duration::from_millis(100)
        ));
    }

    #[test]
    fn coordinates_concurrent_ensure_calls_so_only_one_launcher_runs() {
        let root = tempfile::tempdir().unwrap();
        let healthy = Arc::new(AtomicBool::new(false));
        let launches = Arc::new(AtomicUsize::new(0));
        let mut options = test_options(root.path(), test_config(47_657));
        let health_state = Arc::clone(&healthy);
        let launch_state = Arc::clone(&healthy);
        let launch_count = Arc::clone(&launches);
        options.hooks = SessionBrokerAvailabilityHooks {
            is_healthy: Arc::new(move |_| health_state.load(Ordering::SeqCst)),
            is_port_reachable: Arc::new(|_, _| false),
            launch_daemon: Arc::new(move |_| {
                launch_count.fetch_add(1, Ordering::SeqCst);
                let healthy = Arc::clone(&launch_state);
                thread::spawn(move || {
                    thread::sleep(Duration::from_millis(25));
                    healthy.store(true, Ordering::SeqCst);
                });
                Ok(LaunchedSessionBrokerDaemon {
                    pid: std::process::id(),
                })
            }),
        };
        let mut threads = Vec::new();
        for _ in 0..6 {
            let options = options.clone();
            threads.push(thread::spawn(move || {
                ensure_session_broker_available(&options)
            }));
        }
        for thread in threads {
            thread.join().unwrap().unwrap();
        }
        assert_eq!(launches.load(Ordering::SeqCst), 1);
        let paths = resolve_session_broker_runtime_paths(&options.config, &options.launch.env);
        assert!(!paths.lock_path.exists());
        let metadata: Value =
            serde_json::from_slice(&fs::read(paths.metadata_path).unwrap()).unwrap();
        assert_eq!(metadata["pid"], json!(std::process::id()));
        assert_eq!(metadata["host"], json!("127.0.0.1"));
        assert_eq!(metadata["port"], json!(47_657));
        assert_eq!(metadata["command"], json!("/usr/bin/workdeck"));
        assert_eq!(metadata["args"], json!(["daemon", "serve"]));
    }

    #[test]
    fn recovers_stale_lock_and_overwrites_stale_metadata() {
        let root = tempfile::tempdir().unwrap();
        let healthy = Arc::new(AtomicBool::new(false));
        let launches = Arc::new(AtomicUsize::new(0));
        let mut options = test_options(root.path(), test_config(47_657));
        let paths = resolve_session_broker_runtime_paths(&options.config, &options.launch.env);
        fs::create_dir_all(&paths.runtime_dir).unwrap();
        fs::write(
            &paths.lock_path,
            serde_json::to_vec_pretty(&json!({
                "ownerPid": 999_999,
                "host": options.config.host,
                "port": options.config.port,
                "acquiredAt": "1970-01-01T00:00:00.000Z"
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            &paths.metadata_path,
            serde_json::to_vec_pretty(&json!({
                "pid": 999_999,
                "host": options.config.host,
                "port": options.config.port,
                "command": "/usr/bin/workdeck",
                "args": ["daemon", "serve"],
                "launchedAt": "1970-01-01T00:00:00.000Z",
                "launchedByPid": 999_999,
                "launchCwd": "/stale"
            }))
            .unwrap(),
        )
        .unwrap();
        let health_state = Arc::clone(&healthy);
        let launch_state = Arc::clone(&healthy);
        let launch_count = Arc::clone(&launches);
        options.hooks = SessionBrokerAvailabilityHooks {
            is_healthy: Arc::new(move |_| health_state.load(Ordering::SeqCst)),
            is_port_reachable: Arc::new(|_, _| false),
            launch_daemon: Arc::new(move |_| {
                launch_count.fetch_add(1, Ordering::SeqCst);
                launch_state.store(true, Ordering::SeqCst);
                Ok(LaunchedSessionBrokerDaemon { pid: 54_321 })
            }),
        };
        ensure_session_broker_available(&options).unwrap();
        assert_eq!(launches.load(Ordering::SeqCst), 1);
        assert!(!paths.lock_path.exists());
        let metadata: Value =
            serde_json::from_slice(&fs::read(paths.metadata_path).unwrap()).unwrap();
        assert_eq!(metadata["pid"], json!(54_321));
        assert_eq!(metadata["launchedByPid"], json!(std::process::id()));
        assert_eq!(metadata["launchCwd"], json!("/repo"));
    }

    #[test]
    fn healthy_fast_path_does_not_create_runtime_state() {
        let root = tempfile::tempdir().unwrap();
        let mut options = test_options(root.path(), test_config(47_657));
        options.hooks.is_healthy = Arc::new(|_| true);
        ensure_session_broker_available(&options).unwrap();
        assert!(!root.path().join("workdeck-mcp").exists());
    }

    #[test]
    fn reports_port_conflict_and_custom_timeout_distinctly() {
        let root = tempfile::tempdir().unwrap();
        let mut conflict = test_options(root.path(), test_config(47_657));
        conflict.timeout = Duration::from_millis(1);
        conflict.interval = Duration::from_millis(1);
        conflict.hooks = SessionBrokerAvailabilityHooks {
            is_healthy: Arc::new(|_| false),
            is_port_reachable: Arc::new(|_, _| true),
            launch_daemon: Arc::new(|_| Ok(LaunchedSessionBrokerDaemon { pid: 42 })),
        };
        assert!(matches!(
            ensure_session_broker_available(&conflict),
            Err(BrokerLauncherError::PortConflict { .. })
        ));

        let mut timeout = conflict;
        timeout.timeout_message = Some("fixture timeout".into());
        timeout.hooks.is_port_reachable = Arc::new(|_, _| false);
        assert_eq!(
            ensure_session_broker_available(&timeout)
                .unwrap_err()
                .to_string(),
            "fixture timeout"
        );
    }

    #[test]
    fn reads_health_over_bounded_native_http_transport() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let config = test_config(u32::from(listener.local_addr().unwrap().port()));
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 128];
            while request.len() < 1024 && !request.windows(4).any(|part| part == b"\r\n\r\n") {
                let bytes = socket.read(&mut chunk).unwrap();
                if bytes == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..bytes]);
            }
            assert!(String::from_utf8_lossy(&request).starts_with("GET /health "));
            let body = br#"{"ok":true,"sessions":2}"#;
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            socket.write_all(body).unwrap();
        });
        let health = read_session_broker_health(&config, Duration::from_secs(1)).unwrap();
        assert_eq!(health.sessions, Some(2));
        server.join().unwrap();
    }

    #[test]
    fn runtime_paths_are_branded_scoped_and_files_are_private() {
        let root = tempfile::tempdir().unwrap();
        let mut options = test_options(root.path(), test_config(47_657));
        let healthy = Arc::new(AtomicBool::new(false));
        let health_state = Arc::clone(&healthy);
        let launch_state = Arc::clone(&healthy);
        options.hooks = SessionBrokerAvailabilityHooks {
            is_healthy: Arc::new(move |_| health_state.load(Ordering::SeqCst)),
            is_port_reachable: Arc::new(|_, _| false),
            launch_daemon: Arc::new(move |_| {
                launch_state.store(true, Ordering::SeqCst);
                Ok(LaunchedSessionBrokerDaemon {
                    pid: std::process::id(),
                })
            }),
        };
        ensure_session_broker_available(&options).unwrap();
        let paths = resolve_session_broker_runtime_paths(&options.config, &options.launch.env);
        assert_eq!(paths.runtime_dir, root.path().join("workdeck-mcp"));
        assert!(paths.metadata_path.ends_with("daemon-127-0-0-1-47657.json"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(paths.runtime_dir)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(paths.metadata_path)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
}
