//! `workdeck daemon status` and `workdeck daemon restart`.

use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};
use workdeck_core::SessionCommandOutput;

use crate::{
    DaemonBuild, DaemonLaunchLockGuard, DaemonSkewDirection,
    LaunchSessionBrokerDaemonAndRecordOptions, ResolvedSessionBrokerConfig,
    SessionBrokerAdminStatusV1, SessionBrokerLaunchMetadata, WORKDECK_BUILD_RELATION_NEWER,
    WORKDECK_BUILD_RELATION_OLDER, WORKDECK_DAEMON_RESTART_COMMAND,
    WORKDECK_WINDOW_RELAUNCH_CLAUSE, WorkdeckDaemonAdminProbe, WorkdeckDaemonStopRequest,
    compare_daemon_build, current_daemon_build, daemon_restart_disconnects,
    is_session_broker_healthy, launch_session_broker_daemon_and_record,
    probe_workdeck_session_daemon_admin_status, read_session_broker_launch_metadata,
    request_workdeck_session_daemon_stop, resolve_session_broker_config, stringify_json,
    try_acquire_daemon_launch_lock, wait_for_session_broker_health,
};

const STOP_TIMEOUT: Duration = Duration::from_secs(5);
const START_TIMEOUT: Duration = Duration::from_secs(5);
const HEALTH_PROBE_TIMEOUT: Duration = Duration::from_millis(500);

/// The two daemon control commands the CLI dispatches here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonControlCommandInput {
    Status {
        output: SessionCommandOutput,
    },
    Restart {
        output: SessionCommandOutput,
        yes: bool,
    },
}

/// One user-visible failure: the headline and the remedy lines that follow it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonCommandError {
    pub message: String,
    pub hints: Vec<String>,
}

impl DaemonCommandError {
    fn message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            hints: Vec::new(),
        }
    }

    fn with_hints(message: impl Into<String>, hints: &[&str]) -> Self {
        Self {
            message: message.into(),
            hints: hints.iter().map(|hint| (*hint).into()).collect(),
        }
    }
}

impl std::fmt::Display for DaemonCommandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for DaemonCommandError {}

/// Output and confirmation surface; production writes to the process streams.
pub trait DaemonCommandIo {
    fn stdout(&mut self, text: &str);
    fn stderr(&mut self, text: &str);
    /// Ask one yes/no question; `None` when stdin is not a terminal.
    fn confirm(&mut self, question: &str) -> Option<bool>;
}

/// One attempt to take the launch lock: `None` while another live process holds it.
pub type DaemonLaunchLockAttempt = Result<Option<Box<dyn DaemonLaunchLockHandle>>, String>;

/// Collaborators the commands reach the daemon and the process table through; tests fake these.
pub struct DaemonCommandDependencies {
    pub client_build: DaemonBuild,
    pub probe_admin_status: Arc<dyn Fn() -> WorkdeckDaemonAdminProbe + Send + Sync>,
    pub request_stop: Arc<dyn Fn() -> WorkdeckDaemonStopRequest + Send + Sync>,
    pub read_launch_metadata: Arc<dyn Fn() -> Option<SessionBrokerLaunchMetadata> + Send + Sync>,
    pub is_healthy: Arc<dyn Fn() -> bool + Send + Sync>,
    pub acquire_launch_lock: Arc<dyn Fn() -> DaemonLaunchLockAttempt + Send + Sync>,
    pub launch_daemon: Arc<dyn Fn() -> Result<SessionBrokerLaunchMetadata, String> + Send + Sync>,
    /// Wait for health to appear (`true`) or disappear (`false`); returns whether it did.
    pub wait_for_health: Arc<dyn Fn(bool) -> bool + Send + Sync>,
    pub kill_process: Arc<dyn Fn(u64) -> Result<(), String> + Send + Sync>,
    pub is_terminal: bool,
}

/// Wire the real daemon, filesystem, and process collaborators.
pub fn create_daemon_command_dependencies(
    env: &BTreeMap<String, String>,
) -> Result<DaemonCommandDependencies, String> {
    let config = resolve_session_broker_config(env).map_err(|error| error.to_string())?;
    Ok(create_daemon_command_dependencies_for_config(config, env))
}

/// Bind the collaborators to one resolved broker config.
pub fn create_daemon_command_dependencies_for_config(
    config: ResolvedSessionBrokerConfig,
    env: &BTreeMap<String, String>,
) -> DaemonCommandDependencies {
    let probe_config = config.clone();
    let probe_env = env.clone();
    let stop_config = config.clone();
    let stop_env = env.clone();
    let metadata_config = config.clone();
    let metadata_env = env.clone();
    let healthy_config = config.clone();
    let lock_config = config.clone();
    let lock_env = env.clone();
    let launch_config = config.clone();
    let wait_config = config;
    DaemonCommandDependencies {
        client_build: current_daemon_build(),
        probe_admin_status: Arc::new(move || {
            probe_workdeck_session_daemon_admin_status(
                &probe_config,
                &probe_env,
                crate::default_admin_probe_timeout(),
            )
        }),
        request_stop: Arc::new(move || {
            request_workdeck_session_daemon_stop(
                &stop_config,
                &stop_env,
                crate::default_admin_probe_timeout(),
            )
        }),
        read_launch_metadata: Arc::new(move || {
            read_session_broker_launch_metadata(&metadata_config, &metadata_env)
        }),
        is_healthy: Arc::new(move || {
            is_session_broker_healthy(&healthy_config, HEALTH_PROBE_TIMEOUT)
        }),
        acquire_launch_lock: Arc::new(move || {
            try_acquire_daemon_launch_lock(&lock_config, &lock_env)
                .map_err(|error| error.to_string())
                .map(|lock| {
                    lock.map(|guard| {
                        Box::new(GuardedDaemonLaunchLock(guard)) as Box<dyn DaemonLaunchLockHandle>
                    })
                })
        }),
        launch_daemon: Arc::new(move || {
            launch_session_broker_daemon_and_record(&LaunchSessionBrokerDaemonAndRecordOptions {
                config: Some(launch_config.clone()),
                launch: None,
                launch_daemon: None,
            })
            .map_err(|error| error.to_string())
        }),
        wait_for_health: Arc::new(move |expected| {
            wait_for_session_broker_health(&wait_config, expected, STOP_TIMEOUT.max(START_TIMEOUT))
        }),
        kill_process: Arc::new(send_sigterm),
        is_terminal: std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
    }
}

#[cfg(unix)]
fn send_sigterm(pid: u64) -> Result<(), String> {
    let pid = i32::try_from(pid).map_err(|_| format!("invalid pid {pid}"))?;
    // SAFETY: signal zero-argument libc call; the return value decides the outcome.
    let result = unsafe { libc::kill(pid, libc::SIGTERM) };
    if result == 0 {
        Ok(())
    } else {
        Err(format!(
            "Could not signal pid {pid}: {}",
            std::io::Error::last_os_error()
        ))
    }
}

#[cfg(not(unix))]
fn send_sigterm(pid: u64) -> Result<(), String> {
    Err(format!(
        "Signalling pid {pid} is not supported on this platform."
    ))
}

/// Release handle for the daemon launch lock held across one restart.
pub trait DaemonLaunchLockHandle {
    fn release(&mut self);
}

/// The production lock, backed by the launcher's on-disk lock file.
struct GuardedDaemonLaunchLock(DaemonLaunchLockGuard);

impl DaemonLaunchLockHandle for GuardedDaemonLaunchLock {
    fn release(&mut self) {
        self.0.release();
    }
}

/// What `status` learned, in the shape both commands and both output formats consume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonStatusReport {
    None,
    PreAdmin {
        launch: Option<SessionBrokerLaunchMetadata>,
    },
    Status {
        status: SessionBrokerAdminStatusV1,
        direction: DaemonSkewDirection,
    },
}

/// Ask the daemon what it is, falling back to launch metadata and then to "nothing running".
pub fn read_daemon_status_report(deps: &DaemonCommandDependencies) -> DaemonStatusReport {
    match (deps.probe_admin_status)() {
        WorkdeckDaemonAdminProbe::Status(status) => DaemonStatusReport::Status {
            direction: compare_daemon_build(
                status.daemon_version,
                deps.client_build.daemon_version,
            ),
            status,
        },
        WorkdeckDaemonAdminProbe::Unsupported => DaemonStatusReport::PreAdmin {
            launch: (deps.read_launch_metadata)(),
        },
        WorkdeckDaemonAdminProbe::Unavailable => {
            if (deps.is_healthy)() {
                DaemonStatusReport::PreAdmin {
                    launch: (deps.read_launch_metadata)(),
                }
            } else {
                DaemonStatusReport::None
            }
        }
    }
}

/// Format a millisecond duration as a compact `1d 2h 3m` style string.
#[must_use]
pub fn format_uptime(uptime_ms: u64) -> String {
    let total_minutes = uptime_ms / 60_000;
    let days = total_minutes / 1_440;
    let hours = (total_minutes % 1_440) / 60;
    let minutes = total_minutes % 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        format!("{}s", uptime_ms / 1_000)
    }
}

/// The daemon command line as launch metadata recorded it, command plus arguments.
fn launch_command_line(launch: &SessionBrokerLaunchMetadata) -> String {
    std::iter::once(launch.command.as_str())
        .chain(launch.args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The launch metadata line shared by the pre-admin summary and the bootstrap prompt.
fn describe_launch(launch: &SessionBrokerLaunchMetadata) -> String {
    format!(
        "pid {}, started {}, command {}",
        launch.pid,
        launch.launched_at,
        launch_command_line(launch)
    )
}

/// Render the status summary lines shown by both commands.
#[must_use]
pub fn format_daemon_status_report(report: &DaemonStatusReport) -> Vec<String> {
    match report {
        DaemonStatusReport::None => vec!["No session daemon is running.".into()],
        DaemonStatusReport::PreAdmin { launch } => {
            let first = launch.as_ref().map_or_else(
                || {
                    "A session daemon is running, but it is from a build that predates `workdeck daemon status` and cannot report itself; no launch metadata was found.".into()
                },
                |launch| {
                    format!(
                        "A session daemon is running ({}), but it is from a build that predates `workdeck daemon status` and cannot report itself.",
                        describe_launch(launch)
                    )
                },
            );
            vec![
                first,
                format!("This CLI is {WORKDECK_BUILD_RELATION_NEWER}."),
            ]
        }
        DaemonStatusReport::Status { status, direction } => {
            let mut lines = vec![format!(
                "Session daemon {}, pid {}, up {} (started {}).",
                status.app_version,
                status.pid,
                format_uptime(status.uptime_ms),
                status.started_at
            )];
            if *direction != DaemonSkewDirection::Matched {
                lines.push(format!(
                    "This CLI is {}, so the daemon refuses it.",
                    if *direction == DaemonSkewDirection::ClientNewer {
                        WORKDECK_BUILD_RELATION_NEWER
                    } else {
                        WORKDECK_BUILD_RELATION_OLDER
                    }
                ));
            }
            if status.sessions.is_empty() {
                lines.push("No windows are attached.".into());
            } else {
                let count = status.sessions.len();
                if *direction == DaemonSkewDirection::Matched {
                    lines.push(format!("Attached windows ({count}):"));
                } else {
                    lines.push(format!(
                        "Attached windows ({count}). A restart disconnects them; they {WORKDECK_WINDOW_RELAUNCH_CLAUSE}"
                    ));
                }
                for session in &status.sessions {
                    lines.push(format!(
                        "  {}  {}  {}",
                        session.session_id.chars().take(8).collect::<String>(),
                        session.title,
                        session.cwd
                    ));
                }
            }
            lines
        }
    }
}

/// The JSON body for `status --json` and the `before`/`after` halves of `restart --json`.
#[must_use]
pub fn status_report_json(report: &DaemonStatusReport, client_build: &DaemonBuild) -> Value {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct AttachedSession<'a> {
        session_id: &'a str,
        title: &'a str,
        cwd: &'a str,
        pid: u64,
        client_daemon_version: u64,
        older_build: bool,
    }
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct DaemonFacts<'a> {
        daemon_version: u64,
        app_version: &'a str,
        pid: u64,
        started_at: &'a str,
        uptime_ms: u64,
    }
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct LaunchFacts<'a> {
        pid: u64,
        command: String,
        launched_at: &'a str,
    }
    let (daemon, direction, attached_sessions) = match report {
        DaemonStatusReport::Status { status, direction } => {
            let attached = status
                .sessions
                .iter()
                .map(|session| AttachedSession {
                    session_id: &session.session_id,
                    title: &session.title,
                    cwd: &session.cwd,
                    pid: session.pid,
                    client_daemon_version: session.client_daemon_version,
                    older_build: compare_daemon_build(
                        session.client_daemon_version,
                        client_build.daemon_version,
                    ) != DaemonSkewDirection::Matched,
                })
                .collect::<Vec<_>>();
            (
                Some(DaemonFacts {
                    daemon_version: status.daemon_version,
                    app_version: &status.app_version,
                    pid: status.pid,
                    started_at: &status.started_at,
                    uptime_ms: status.uptime_ms,
                }),
                Some(direction),
                Some(attached),
            )
        }
        _ => (None, None, None),
    };
    let launch = match report {
        DaemonStatusReport::PreAdmin {
            launch: Some(launch),
        } => Some(LaunchFacts {
            pid: launch.pid,
            command: launch_command_line(launch),
            launched_at: &launch.launched_at,
        }),
        _ => None,
    };
    json!({
        "cli": {
            "daemonVersion": client_build.daemon_version,
            "appVersion": client_build.app_version,
        },
        "daemon": daemon.map(serde_json::to_value).and_then(Result::ok),
        "running": !matches!(report, DaemonStatusReport::None),
        "supportsAdminScope": matches!(report, DaemonStatusReport::Status { .. }),
        "direction": direction.map(serde_json::to_value).and_then(Result::ok),
        "attachedSessions": attached_sessions
            .map(|sessions| sessions
                .iter()
                .map(|session| serde_json::to_value(session).unwrap_or(Value::Null))
                .collect::<Vec<_>>()),
        "launch": launch.map(serde_json::to_value).and_then(Result::ok),
    })
}

/// Run `workdeck daemon status`.
pub fn run_daemon_status_command(
    input_output: SessionCommandOutput,
    io: &mut dyn DaemonCommandIo,
    deps: &DaemonCommandDependencies,
) -> Result<i32, DaemonCommandError> {
    let report = read_daemon_status_report(deps);
    match input_output {
        SessionCommandOutput::Json => {
            io.stdout(
                &stringify_json(&status_report_json(&report, &deps.client_build))
                    .map_err(|error| DaemonCommandError::message(error.to_string()))?,
            );
        }
        SessionCommandOutput::Text => {
            io.stdout(&format!(
                "{}\n",
                format_daemon_status_report(&report).join("\n")
            ));
        }
    }
    Ok(0)
}

/// The confirmation shown before any restart; names the cost in windows.
fn restart_question(report: &DaemonStatusReport) -> String {
    let count = match report {
        DaemonStatusReport::Status { status, .. } => Some(status.sessions.len()),
        _ => None,
    };
    format!(
        "{}. They {WORKDECK_WINDOW_RELAUNCH_CLAUSE} Continue? [y/N] ",
        daemon_restart_disconnects(count)
    )
}

/// Ask one question, honoring `--yes` and refusing to guess without a terminal.
fn confirm_or_fail(
    question: &str,
    yes: bool,
    io: &mut dyn DaemonCommandIo,
    deps: &DaemonCommandDependencies,
) -> Result<bool, DaemonCommandError> {
    if yes {
        return Ok(true);
    }
    if !deps.is_terminal {
        return Err(DaemonCommandError::with_hints(
            "`workdeck daemon restart` needs confirmation and stdin is not a terminal.",
            &["Re-run with --yes to restart without a prompt."],
        ));
    }
    Ok(io.confirm(question).unwrap_or(false))
}

/// Run `workdeck daemon restart`.
pub fn run_daemon_restart_command(
    input: DaemonControlCommandInput,
    io: &mut dyn DaemonCommandIo,
    deps: &DaemonCommandDependencies,
) -> Result<i32, DaemonCommandError> {
    let DaemonControlCommandInput::Restart { output, yes } = input else {
        unreachable!("status is dispatched by run_daemon_control_command");
    };
    let json = output == SessionCommandOutput::Json;
    let log = |io: &mut dyn DaemonCommandIo, line: &str| {
        if !json {
            io.stdout(&format!("{line}\n"));
        }
    };
    let before = read_daemon_status_report(deps);
    for line in format_daemon_status_report(&before) {
        log(io, &line);
    }

    if !matches!(before, DaemonStatusReport::None)
        && !confirm_or_fail(&restart_question(&before), yes, io, deps)?
    {
        log(io, "Restart cancelled.");
        return Ok(1);
    }

    // Hold the launch lock across stop and start so an attached window's reconnect loop cannot
    // race in and respawn the binary that is being replaced.
    let mut lock = (deps.acquire_launch_lock)().map_err(DaemonCommandError::message)?;
    if lock.is_none() {
        return Err(DaemonCommandError::message(
            "Another Workdeck process is starting the session daemon right now; retry in a moment.",
        ));
    }
    let result = restart_locked(&before, yes, io, deps, &log);
    if let Some(lock) = lock.as_mut() {
        lock.release();
    }
    match result {
        Ok(()) => {}
        // A declined confirmation inside the locked section is a cancel, not a failure.
        Err(error) if error.message == "Restart cancelled." => return Ok(1),
        Err(error) => return Err(error),
    }

    let after = read_daemon_status_report(deps);
    if json {
        io.stdout(
            &stringify_json(&json!({
                "restarted": true,
                "before": status_report_json(&before, &deps.client_build),
                "after": status_report_json(&after, &deps.client_build),
            }))
            .map_err(|error| DaemonCommandError::message(error.to_string()))?,
        );
    } else if let DaemonStatusReport::Status { status, .. } = &after {
        log(
            io,
            &format!(
                "Started session daemon {}, pid {}.",
                status.app_version, status.pid
            ),
        );
    } else {
        log(io, "Started a replacement session daemon.");
    }
    Ok(0)
}

/// Stop-and-replace work that runs while the launch lock is held.
fn restart_locked(
    before: &DaemonStatusReport,
    yes: bool,
    io: &mut dyn DaemonCommandIo,
    deps: &DaemonCommandDependencies,
    log: &impl Fn(&mut dyn DaemonCommandIo, &str),
) -> Result<(), DaemonCommandError> {
    match before {
        DaemonStatusReport::Status { .. } => {
            let stop = (deps.request_stop)();
            if stop != WorkdeckDaemonStopRequest::Stopping {
                return Err(DaemonCommandError::message(format!(
                    "The session daemon did not accept the stop request ({stop:?})."
                )));
            }
            log(io, "Asked the session daemon to stop.");
        }
        DaemonStatusReport::PreAdmin { launch } => {
            // The daemon being upgraded away from does not speak the admin scope. Signalling a
            // pid is the one-time bootstrap path and only ever happens after its own explicit
            // confirmation.
            let Some(launch) = launch.as_ref() else {
                return Err(DaemonCommandError::with_hints(
                    format!(
                        "This daemon predates {WORKDECK_DAEMON_RESTART_COMMAND} and its launch metadata is missing, so it cannot be stopped safely."
                    ),
                    &["Stop it by hand, then run `workdeck daemon restart` again."],
                ));
            };
            let command = launch_command_line(launch);
            let question = format!(
                "This daemon predates {WORKDECK_DAEMON_RESTART_COMMAND}. Send SIGTERM to pid {} ({command})? [y/N] ",
                launch.pid
            );
            // The confirmation must be asked while the lock is held but before the signal;
            // declining unwinds through the caller, which releases the lock.
            if !confirm_or_fail(&question, yes, io, deps)? {
                log(io, "Restart cancelled.");
                return Err(DaemonCommandError::cancelled());
            }
            (deps.kill_process)(launch.pid).map_err(DaemonCommandError::message)?;
            log(io, &format!("Sent SIGTERM to pid {}.", launch.pid));
        }
        DaemonStatusReport::None => {}
    }

    if !matches!(before, DaemonStatusReport::None) && !(deps.wait_for_health)(false) {
        return Err(DaemonCommandError::with_hints(
            "The session daemon is still answering after the stop request.",
            &["Wait a moment and retry, or stop the process by hand."],
        ));
    }

    (deps.launch_daemon)().map_err(|_| {
        DaemonCommandError::message("Failed to launch the replacement session daemon.")
    })?;
    if !(deps.wait_for_health)(true) {
        return Err(DaemonCommandError::with_hints(
            "The replacement session daemon did not become healthy.",
            &["Run `workdeck daemon serve` in a terminal to see why it fails to start."],
        ));
    }
    Ok(())
}

impl DaemonCommandError {
    /// Sentinel for a declined bootstrap confirmation; unwinds as a cancel, not a failure.
    fn cancelled() -> Self {
        Self {
            message: "Restart cancelled.".into(),
            hints: Vec::new(),
        }
    }
}

/// Dispatch one daemon control command.
pub fn run_daemon_control_command(
    input: DaemonControlCommandInput,
    io: &mut dyn DaemonCommandIo,
    deps: &DaemonCommandDependencies,
) -> Result<i32, DaemonCommandError> {
    match input {
        DaemonControlCommandInput::Status { output } => run_daemon_status_command(output, io, deps),
        input @ DaemonControlCommandInput::Restart { .. } => {
            run_daemon_restart_command(input, io, deps)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SessionBrokerAdminSessionV1, stringify_json};
    use std::sync::{Arc, Mutex};

    fn client_build() -> DaemonBuild {
        DaemonBuild {
            daemon_version: 15,
            app_version: "0.22.0".into(),
        }
    }

    fn admin_status(
        daemon_version: u64,
        app_version: &str,
        sessions: usize,
    ) -> WorkdeckDaemonAdminProbe {
        WorkdeckDaemonAdminProbe::Status(SessionBrokerAdminStatusV1 {
            admin_scope_version: 1,
            daemon_version,
            app_version: app_version.into(),
            pid: 4242,
            started_at: "2026-09-08T09:42:00.000Z".into(),
            uptime_ms: 3 * 3_600_000 + 12 * 60_000,
            sessions: (1..=sessions)
                .map(|index| SessionBrokerAdminSessionV1 {
                    session_id: format!("session-{index}-0000-0000"),
                    title: format!("review {index}"),
                    cwd: format!("/repo/{index}"),
                    pid: 99 + index as u64,
                    client_daemon_version: daemon_version,
                })
                .collect(),
        })
    }

    fn launch_metadata() -> SessionBrokerLaunchMetadata {
        SessionBrokerLaunchMetadata {
            pid: 777,
            host: "127.0.0.1".into(),
            port: 47_657,
            command: "/usr/local/bin/workdeck".into(),
            args: vec!["daemon".into(), "serve".into()],
            launched_at: "2026-09-08T09:42:00.000Z".into(),
            launched_by_pid: 1,
            launch_cwd: "/repo".into(),
        }
    }

    /// A lock whose release is visible in the journal, like the production file lock.
    struct JournalingLock {
        journal: Arc<Mutex<Vec<String>>>,
    }

    impl DaemonLaunchLockHandle for JournalingLock {
        fn release(&mut self) {
            self.journal
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push("unlock".into());
        }
    }

    /// The scripted daemon's injectable collaborators.
    struct FakeDaemon {
        deps: DaemonCommandDependencies,
    }

    fn fake_daemon(
        probes: Vec<WorkdeckDaemonAdminProbe>,
        journal: Arc<Mutex<Vec<String>>>,
        healthy: bool,
        stop_result: WorkdeckDaemonStopRequest,
        launch: Option<SessionBrokerLaunchMetadata>,
        is_terminal: bool,
        lock_available: bool,
    ) -> FakeDaemon {
        let probes = Arc::new(Mutex::new(probes));
        let healthy_state = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(healthy));
        let healthy_for_stop = Arc::clone(&healthy_state);
        let journal_for_stop = Arc::clone(&journal);
        let journal_for_lock = Arc::clone(&journal);
        let journal_for_launch = Arc::clone(&journal);
        let journal_for_wait = Arc::clone(&journal);
        let journal_for_kill = Arc::clone(&journal);
        let launch_for_launch = launch.clone();
        let healthy_for_launch = Arc::clone(&healthy_state);
        let healthy_for_wait = Arc::clone(&healthy_state);
        let healthy_for_kill = Arc::clone(&healthy_state);
        let deps = DaemonCommandDependencies {
            client_build: client_build(),
            probe_admin_status: Arc::new(move || {
                let mut queue = probes.lock().unwrap_or_else(|error| error.into_inner());
                if queue.len() > 1 {
                    queue.remove(0)
                } else {
                    queue
                        .first()
                        .cloned()
                        .unwrap_or(WorkdeckDaemonAdminProbe::Unavailable)
                }
            }),
            request_stop: Arc::new(move || {
                journal_for_stop
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .push("stop".into());
                if stop_result == WorkdeckDaemonStopRequest::Stopping {
                    healthy_for_stop.store(false, std::sync::atomic::Ordering::SeqCst);
                }
                stop_result
            }),
            read_launch_metadata: Arc::new(move || launch.clone()),
            is_healthy: Arc::new(move || healthy_state.load(std::sync::atomic::Ordering::SeqCst)),
            acquire_launch_lock: Arc::new(move || {
                if !lock_available {
                    return Ok(None);
                }
                journal_for_lock
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .push("lock".into());
                let journal = Arc::clone(&journal_for_lock);
                Ok(Some(Box::new(JournalingLock { journal })))
            }),
            launch_daemon: Arc::new(move || {
                journal_for_launch
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .push("launch".into());
                healthy_for_launch.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(launch_for_launch
                    .clone()
                    .map(|mut metadata| {
                        metadata.pid = 9999;
                        metadata
                    })
                    .unwrap_or_else(launch_metadata))
            }),
            wait_for_health: Arc::new(move |expected| {
                let health = healthy_for_wait.load(std::sync::atomic::Ordering::SeqCst);
                journal_for_wait
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .push(format!("wait:{}", if expected { "up" } else { "down" }));
                health == expected
            }),
            kill_process: Arc::new(move |pid| {
                journal_for_kill
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .push(format!("kill:{pid}:SIGTERM"));
                healthy_for_kill.store(false, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }),
            is_terminal,
        };
        FakeDaemon { deps }
    }

    /// The default scripted daemon used by most status tests.
    fn default_fake(probes: Vec<WorkdeckDaemonAdminProbe>) -> FakeDaemon {
        fake_daemon(
            probes,
            Arc::new(Mutex::new(Vec::new())),
            true,
            WorkdeckDaemonStopRequest::Stopping,
            Some(launch_metadata()),
            true,
            true,
        )
    }

    #[derive(Default)]
    struct RecordingIo {
        out: Vec<String>,
        questions: Vec<String>,
        answers: Vec<bool>,
    }

    impl DaemonCommandIo for RecordingIo {
        fn stdout(&mut self, text: &str) {
            self.out.push(text.into());
        }

        fn stderr(&mut self, text: &str) {
            self.out.push(format!("[stderr] {text}"));
        }

        fn confirm(&mut self, question: &str) -> Option<bool> {
            self.questions.push(question.into());
            Some(self.answers.remove(0))
        }
    }

    fn io_with(answers: &[bool]) -> RecordingIo {
        RecordingIo {
            answers: answers.to_vec(),
            ..RecordingIo::default()
        }
    }

    #[test]
    fn status_summarizes_a_matching_daemon_and_its_attached_windows() {
        let fake = default_fake(vec![admin_status(15, "0.22.0", 2)]);
        let mut io = RecordingIo::default();
        let code =
            run_daemon_status_command(SessionCommandOutput::Text, &mut io, &fake.deps).unwrap();
        assert_eq!(code, 0);
        assert_eq!(
            io.out.join(""),
            [
                "Session daemon 0.22.0, pid 4242, up 3h 12m (started 2026-09-08T09:42:00.000Z).",
                "Attached windows (2):",
                "  session-  review 1  /repo/1",
                "  session-  review 2  /repo/2",
                "",
            ]
            .join("\n")
        );
    }

    #[test]
    fn status_reports_skew_in_either_direction_without_per_window_markers() {
        for (revision, direction, relation) in [
            (
                12_u64,
                DaemonSkewDirection::ClientNewer,
                "a newer Workdeck build",
            ),
            (
                16,
                DaemonSkewDirection::ClientOlder,
                "an older Workdeck build",
            ),
        ] {
            let report = DaemonStatusReport::Status {
                status: match admin_status(revision, "0.22.0", 2) {
                    WorkdeckDaemonAdminProbe::Status(status) => status,
                    _ => unreachable!(),
                },
                direction,
            };
            assert_eq!(
                format_daemon_status_report(&report),
                vec![
                    String::from(
                        "Session daemon 0.22.0, pid 4242, up 3h 12m (started 2026-09-08T09:42:00.000Z).",
                    ),
                    format!("This CLI is {relation}, so the daemon refuses it."),
                    String::from(
                        "Attached windows (2). A restart disconnects them; they must be relaunched, losing their notes."
                    ),
                    String::from("  session-  review 1  /repo/1"),
                    String::from("  session-  review 2  /repo/2"),
                ]
            );
        }
    }

    #[test]
    fn status_reports_no_attached_windows_without_a_restart_warning() {
        let report = match admin_status(12, "0.21.1", 0) {
            WorkdeckDaemonAdminProbe::Status(status) => DaemonStatusReport::Status {
                status,
                direction: DaemonSkewDirection::ClientNewer,
            },
            _ => unreachable!(),
        };
        assert_eq!(
            format_daemon_status_report(&report),
            vec![
                "Session daemon 0.21.1, pid 4242, up 3h 12m (started 2026-09-08T09:42:00.000Z).",
                "This CLI is a newer Workdeck build, so the daemon refuses it.",
                "No windows are attached.",
            ]
        );
    }

    #[test]
    fn status_reports_missing_launch_metadata_without_inventing_a_build_number() {
        assert_eq!(
            format_daemon_status_report(&DaemonStatusReport::PreAdmin { launch: None }),
            vec![
                "A session daemon is running, but it is from a build that predates `workdeck daemon status` and cannot report itself; no launch metadata was found.",
                "This CLI is a newer Workdeck build.",
            ]
        );
    }

    #[test]
    fn status_reports_the_launch_metadata_for_a_pre_admin_daemon() {
        let fake = default_fake(vec![WorkdeckDaemonAdminProbe::Unsupported]);
        let mut io = RecordingIo::default();
        run_daemon_status_command(SessionCommandOutput::Text, &mut io, &fake.deps).unwrap();
        assert_eq!(
            io.out.join(""),
            "A session daemon is running (pid 777, started 2026-09-08T09:42:00.000Z, command /usr/local/bin/workdeck daemon serve), but it is from a build that predates `workdeck daemon status` and cannot report itself.\nThis CLI is a newer Workdeck build.\n"
        );
    }

    #[test]
    fn status_says_so_when_no_daemon_is_running_with_exit_zero() {
        let fake = fake_daemon(
            vec![WorkdeckDaemonAdminProbe::Unavailable],
            Arc::new(Mutex::new(Vec::new())),
            false,
            WorkdeckDaemonStopRequest::Stopping,
            Some(launch_metadata()),
            true,
            true,
        );
        let mut io = RecordingIo::default();
        let code =
            run_daemon_status_command(SessionCommandOutput::Text, &mut io, &fake.deps).unwrap();
        assert_eq!(code, 0);
        assert_eq!(io.out.join(""), "No session daemon is running.\n");
    }

    #[test]
    fn status_emits_the_structured_status_as_json() {
        let fake = default_fake(vec![admin_status(12, "0.21.1", 1)]);
        let mut io = RecordingIo::default();
        run_daemon_status_command(SessionCommandOutput::Json, &mut io, &fake.deps).unwrap();
        let body: serde_json::Value = serde_json::from_str(io.out.join("").trim()).unwrap();
        assert_eq!(
            body,
            json!({
                "cli": {"daemonVersion": 15, "appVersion": "0.22.0"},
                "daemon": {
                    "daemonVersion": 12,
                    "appVersion": "0.21.1",
                    "pid": 4242,
                    "startedAt": "2026-09-08T09:42:00.000Z",
                    "uptimeMs": 11_520_000,
                },
                "running": true,
                "supportsAdminScope": true,
                "direction": "client-newer",
                "attachedSessions": [{
                    "sessionId": "session-1-0000-0000",
                    "title": "review 1",
                    "cwd": "/repo/1",
                    "pid": 100,
                    "clientDaemonVersion": 12,
                    "olderBuild": true,
                }],
                "launch": serde_json::Value::Null,
            })
        );
    }

    #[test]
    fn restart_confirms_holds_the_launch_lock_and_reports_the_new_daemon() {
        let journal = Arc::new(Mutex::new(Vec::new()));
        let fake = fake_daemon(
            vec![admin_status(12, "0.21.1", 2), admin_status(15, "0.22.0", 0)],
            Arc::clone(&journal),
            true,
            WorkdeckDaemonStopRequest::Stopping,
            Some(launch_metadata()),
            true,
            true,
        );
        let mut io = io_with(&[true]);
        let code = run_daemon_restart_command(
            DaemonControlCommandInput::Restart {
                output: SessionCommandOutput::Text,
                yes: false,
            },
            &mut io,
            &fake.deps,
        )
        .unwrap();
        assert_eq!(code, 0);
        assert_eq!(
            io.questions,
            [
                "Restarting disconnects 2 attached windows. They must be relaunched, losing their notes. Continue? [y/N] "
            ]
        );
        assert_eq!(
            journal
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_slice(),
            ["lock", "stop", "wait:down", "launch", "wait:up", "unlock"]
                .iter()
                .map(|entry| entry.to_string())
                .collect::<Vec<_>>()
                .as_slice()
        );
        let output = io.out.join("");
        assert!(
            output.contains("Started session daemon 0.22.0, pid 4242."),
            "{output}"
        );
    }

    #[test]
    fn restart_uses_singular_window_wording_when_only_one_is_attached() {
        let fake = default_fake(vec![admin_status(12, "0.21.1", 1)]);
        let mut io = io_with(&[false]);
        run_daemon_restart_command(
            DaemonControlCommandInput::Restart {
                output: SessionCommandOutput::Text,
                yes: false,
            },
            &mut io,
            &fake.deps,
        )
        .unwrap();
        assert_eq!(
            io.questions,
            [
                "Restarting disconnects 1 attached window. They must be relaunched, losing their notes. Continue? [y/N] "
            ]
        );
    }

    #[test]
    fn restart_cancels_without_touching_the_daemon_when_the_user_declines() {
        let journal = Arc::new(Mutex::new(Vec::new()));
        let fake = fake_daemon(
            vec![admin_status(12, "0.21.1", 2)],
            Arc::clone(&journal),
            true,
            WorkdeckDaemonStopRequest::Stopping,
            Some(launch_metadata()),
            true,
            true,
        );
        let mut io = io_with(&[false]);
        let code = run_daemon_restart_command(
            DaemonControlCommandInput::Restart {
                output: SessionCommandOutput::Text,
                yes: false,
            },
            &mut io,
            &fake.deps,
        )
        .unwrap();
        assert_eq!(code, 1);
        assert!(
            journal
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .is_empty()
        );
        assert!(io.out.join("").contains("Restart cancelled."));
    }

    #[test]
    fn restart_refuses_to_prompt_without_a_terminal_unless_yes_is_given() {
        let journal = Arc::new(Mutex::new(Vec::new()));
        let fake = fake_daemon(
            vec![admin_status(12, "0.21.1", 2)],
            Arc::clone(&journal),
            true,
            WorkdeckDaemonStopRequest::Stopping,
            Some(launch_metadata()),
            false,
            true,
        );
        let mut io = RecordingIo::default();
        let error = run_daemon_restart_command(
            DaemonControlCommandInput::Restart {
                output: SessionCommandOutput::Text,
                yes: false,
            },
            &mut io,
            &fake.deps,
        )
        .unwrap_err();
        assert!(error.message.contains("stdin is not a terminal"));
        assert!(
            journal
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .is_empty()
        );
        // The summary still printed so a script's log says what would have been restarted.
        assert!(io.out.join("").contains("Attached windows (2)"));

        let yes_journal = Arc::new(Mutex::new(Vec::new()));
        let yes_fake = fake_daemon(
            vec![admin_status(12, "0.21.1", 2), admin_status(15, "0.22.0", 0)],
            Arc::clone(&yes_journal),
            true,
            WorkdeckDaemonStopRequest::Stopping,
            Some(launch_metadata()),
            false,
            true,
        );
        let mut yes_io = RecordingIo::default();
        let code = run_daemon_restart_command(
            DaemonControlCommandInput::Restart {
                output: SessionCommandOutput::Json,
                yes: true,
            },
            &mut yes_io,
            &yes_fake.deps,
        )
        .unwrap();
        assert_eq!(code, 0);
        assert_eq!(
            yes_journal
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_slice(),
            ["lock", "stop", "wait:down", "launch", "wait:up", "unlock"]
                .iter()
                .map(|entry| entry.to_string())
                .collect::<Vec<_>>()
                .as_slice()
        );
    }

    #[test]
    fn restart_falls_back_to_a_separately_confirmed_sigterm_for_a_pre_admin_daemon() {
        let journal = Arc::new(Mutex::new(Vec::new()));
        let fake = fake_daemon(
            vec![
                WorkdeckDaemonAdminProbe::Unsupported,
                admin_status(15, "0.22.0", 0),
            ],
            Arc::clone(&journal),
            true,
            WorkdeckDaemonStopRequest::Stopping,
            Some(launch_metadata()),
            true,
            true,
        );
        let mut io = io_with(&[true, true]);
        let code = run_daemon_restart_command(
            DaemonControlCommandInput::Restart {
                output: SessionCommandOutput::Text,
                yes: false,
            },
            &mut io,
            &fake.deps,
        )
        .unwrap();
        assert_eq!(code, 0);
        assert_eq!(
            io.questions[0],
            "Restarting disconnects an unknown number of attached windows. They must be relaunched, losing their notes. Continue? [y/N] "
        );
        assert_eq!(
            io.questions[1],
            "This daemon predates `workdeck daemon restart`. Send SIGTERM to pid 777 (/usr/local/bin/workdeck daemon serve)? [y/N] "
        );
        assert_eq!(
            journal
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_slice(),
            [
                "lock",
                "kill:777:SIGTERM",
                "wait:down",
                "launch",
                "wait:up",
                "unlock"
            ]
            .iter()
            .map(|entry| entry.to_string())
            .collect::<Vec<_>>()
            .as_slice()
        );
    }

    #[test]
    fn restart_never_signals_a_pid_when_the_bootstrap_confirmation_is_declined() {
        let journal = Arc::new(Mutex::new(Vec::new()));
        let fake = fake_daemon(
            vec![WorkdeckDaemonAdminProbe::Unsupported],
            Arc::clone(&journal),
            true,
            WorkdeckDaemonStopRequest::Stopping,
            Some(launch_metadata()),
            true,
            true,
        );
        let mut io = io_with(&[true, false]);
        run_daemon_restart_command(
            DaemonControlCommandInput::Restart {
                output: SessionCommandOutput::Text,
                yes: false,
            },
            &mut io,
            &fake.deps,
        )
        .unwrap();
        assert_eq!(
            journal
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_slice(),
            ["lock".to_owned(), "unlock".to_owned()].as_slice()
        );
    }

    #[test]
    fn restart_refuses_the_bootstrap_path_when_no_launch_metadata_names_the_pid() {
        let journal = Arc::new(Mutex::new(Vec::new()));
        let fake = fake_daemon(
            vec![WorkdeckDaemonAdminProbe::Unsupported],
            Arc::clone(&journal),
            true,
            WorkdeckDaemonStopRequest::Stopping,
            None,
            true,
            true,
        );
        let mut io = RecordingIo::default();
        let error = run_daemon_restart_command(
            DaemonControlCommandInput::Restart {
                output: SessionCommandOutput::Text,
                yes: true,
            },
            &mut io,
            &fake.deps,
        )
        .unwrap_err();
        assert!(error.message.contains("launch metadata is missing"));
        assert_eq!(
            journal
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_slice(),
            ["lock".to_owned(), "unlock".to_owned()].as_slice()
        );
    }

    #[test]
    fn restart_starts_a_daemon_without_prompting_when_none_is_running() {
        let journal = Arc::new(Mutex::new(Vec::new()));
        let fake = fake_daemon(
            vec![
                WorkdeckDaemonAdminProbe::Unavailable,
                admin_status(15, "0.22.0", 0),
            ],
            Arc::clone(&journal),
            false,
            WorkdeckDaemonStopRequest::Stopping,
            Some(launch_metadata()),
            true,
            true,
        );
        let mut io = RecordingIo::default();
        let code = run_daemon_restart_command(
            DaemonControlCommandInput::Restart {
                output: SessionCommandOutput::Text,
                yes: false,
            },
            &mut io,
            &fake.deps,
        )
        .unwrap();
        assert_eq!(code, 0);
        assert!(io.questions.is_empty());
        assert_eq!(
            journal
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_slice(),
            ["lock", "launch", "wait:up", "unlock"]
                .iter()
                .map(|entry| entry.to_string())
                .collect::<Vec<_>>()
                .as_slice()
        );
    }

    #[test]
    fn restart_fails_when_another_process_holds_the_launch_lock() {
        let fake = fake_daemon(
            vec![admin_status(12, "0.21.1", 2)],
            Arc::new(Mutex::new(Vec::new())),
            true,
            WorkdeckDaemonStopRequest::Stopping,
            Some(launch_metadata()),
            true,
            false,
        );
        let mut io = RecordingIo::default();
        let error = run_daemon_restart_command(
            DaemonControlCommandInput::Restart {
                output: SessionCommandOutput::Text,
                yes: true,
            },
            &mut io,
            &fake.deps,
        )
        .unwrap_err();
        assert!(
            error
                .message
                .contains("Another Workdeck process is starting the session daemon")
        );
    }

    #[test]
    fn restart_emits_before_and_after_status_as_json() {
        let fake = default_fake(vec![
            admin_status(12, "0.21.1", 2),
            admin_status(15, "0.22.0", 0),
        ]);
        let mut io = RecordingIo::default();
        run_daemon_restart_command(
            DaemonControlCommandInput::Restart {
                output: SessionCommandOutput::Json,
                yes: true,
            },
            &mut io,
            &fake.deps,
        )
        .unwrap();
        let body: serde_json::Value = serde_json::from_str(io.out.join("").trim()).unwrap();
        assert_eq!(body["restarted"], json!(true));
        assert_eq!(body["before"]["daemon"]["daemonVersion"], json!(12));
        assert_eq!(body["before"]["direction"], json!("client-newer"));
        assert_eq!(body["after"]["daemon"]["daemonVersion"], json!(15));
        assert_eq!(body["after"]["direction"], json!("matched"));
        assert_eq!(body["after"]["attachedSessions"], json!([]));
    }

    #[test]
    fn the_restarts_stop_refusal_is_reported_with_the_outcome() {
        let fake = fake_daemon(
            vec![admin_status(12, "0.21.1", 2)],
            Arc::new(Mutex::new(Vec::new())),
            true,
            WorkdeckDaemonStopRequest::Unsupported,
            Some(launch_metadata()),
            true,
            true,
        );
        let mut io = RecordingIo::default();
        let error = run_daemon_restart_command(
            DaemonControlCommandInput::Restart {
                output: SessionCommandOutput::Text,
                yes: true,
            },
            &mut io,
            &fake.deps,
        )
        .unwrap_err();
        assert_eq!(
            error.message,
            "The session daemon did not accept the stop request (Unsupported)."
        );
    }

    #[test]
    fn uptime_renders_compact_durations() {
        assert_eq!(format_uptime(0), "0s");
        assert_eq!(format_uptime(59_999), "59s");
        assert_eq!(format_uptime(60_000), "1m");
        assert_eq!(format_uptime(3 * 3_600_000 + 12 * 60_000), "3h 12m");
        assert_eq!(format_uptime(26 * 3_600_000), "1d 2h");
    }

    #[test]
    fn restart_json_is_stable_json_without_inline_padding() {
        let fake = default_fake(vec![admin_status(12, "0.21.1", 0)]);
        let mut io = RecordingIo::default();
        run_daemon_status_command(SessionCommandOutput::Json, &mut io, &fake.deps).unwrap();
        assert!(
            io.out[0].starts_with("{\n"),
            "status JSON is rendered for humans and tools alike: {}",
            io.out[0]
        );
        assert!(stringify_json(&json!({"a": 1})).unwrap().ends_with("\n"));
    }
}
