//! End-to-end daemon status/restart and build-skew coverage through the real binary.
//!
//! One daemon impersonates the previous build revision through the internal override so a
//! second process can observe the full skew contract: a refused session command returns the
//! structured `daemon-build-mismatch` in-band, `daemon status` names both builds and the
//! direction, and `daemon restart --yes` replaces the daemon with one from the current build.

use std::net::TcpStream;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

struct DaemonProcess(std::process::Child);

impl Drop for DaemonProcess {
    fn drop(&mut self) {
        // This guard owns this exact child, including on assertion failure.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn reserve_port() -> u16 {
    let reservation = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = reservation.local_addr().unwrap().port();
    drop(reservation);
    port
}

fn daemon_health(port: u16) -> bool {
    TcpStream::connect(("127.0.0.1", port)).is_ok()
}

fn wait_for_health(port: u16, expected: bool, label: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while daemon_health(port) != expected {
        assert!(Instant::now() < deadline, "health never settled: {label}");
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn run_workdeck(
    directory: &tempfile::TempDir,
    port: u16,
    extra_env: &[(&str, &str)],
    args: &[&str],
) -> Output {
    Command::new(env!("CARGO_BIN_EXE_workdeck"))
        .args(args)
        .env("XDG_CONFIG_HOME", directory.path().join("config"))
        .env("XDG_RUNTIME_DIR", directory.path().join("runtime"))
        .env("WORKDECK_MCP_HOST", "127.0.0.1")
        .env("WORKDECK_MCP_PORT", port.to_string())
        .env_remove("WORKDECK_INTERNAL_SESSION_DAEMON_VERSION")
        .envs(extra_env.iter().map(|(key, value)| (*key, *value)))
        .stdin(Stdio::null())
        .output()
        .expect("the workdeck binary runs")
}

fn spawn_daemon(
    directory: &tempfile::TempDir,
    port: u16,
    extra_env: &[(&str, &str)],
) -> DaemonProcess {
    let child = Command::new(env!("CARGO_BIN_EXE_workdeck"))
        .args(["daemon", "serve"])
        .env("XDG_CONFIG_HOME", directory.path().join("config"))
        .env("XDG_RUNTIME_DIR", directory.path().join("runtime"))
        .env("WORKDECK_MCP_HOST", "127.0.0.1")
        .env("WORKDECK_MCP_PORT", port.to_string())
        .envs(extra_env.iter().map(|(key, value)| (*key, *value)))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the daemon starts");
    DaemonProcess(child)
}

fn json_stdout(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout was not JSON ({error}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

fn wait_for_child_exit(child: &mut std::process::Child, label: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait().unwrap() {
            Some(status) => {
                assert!(status.success(), "{label} exited with {status}");
                return;
            }
            None => {
                assert!(Instant::now() < deadline, "{label} never exited");
                std::thread::sleep(Duration::from_millis(25));
            }
        }
    }
}

#[test]
fn daemon_status_and_restart_resolve_a_version_skew_between_real_processes() {
    let directory = tempfile::tempdir().unwrap();
    let port = reserve_port();

    // 1. A daemon from the previous build revision (via the internal test override).
    let mut old_daemon = spawn_daemon(
        &directory,
        port,
        &[("WORKDECK_INTERNAL_SESSION_DAEMON_VERSION", "11")],
    );
    wait_for_health(port, true, "the old daemon binds its port");

    // 2. A current CLI is refused with the structured mismatch, in-band under --json.
    let refused = run_workdeck(&directory, port, &[], &["session", "list", "--json"]);
    assert_eq!(refused.status.code(), Some(1));
    let error = json_stdout(&refused)["error"]
        .as_object()
        .expect("the mismatch error is in-band JSON")
        .clone();
    assert_eq!(error["kind"], "daemon-build-mismatch");
    assert_eq!(error["daemon"]["daemonVersion"], 11);
    assert_eq!(
        error["cli"]["daemonVersion"],
        serde_json::json!(workdeck_session::WORKDECK_SESSION_DAEMON_VERSION)
    );
    assert_eq!(error["recommendedAction"], "restart-daemon");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .starts_with("The session daemon is"),
        "{error:?}"
    );

    // 3. `daemon status --json` names both builds, the direction, and the CLI's own build.
    let status = run_workdeck(&directory, port, &[], &["daemon", "status", "--json"]);
    assert_eq!(status.status.code(), Some(0));
    let body = json_stdout(&status);
    assert_eq!(body["running"], true);
    assert_eq!(body["supportsAdminScope"], true);
    assert_eq!(body["direction"], "client-newer");
    assert_eq!(body["daemon"]["daemonVersion"], 11);
    let old_pid = body["daemon"]["pid"].as_u64().unwrap();

    // 4. `daemon restart --yes --json` replaces it with one from this build; the old process
    //    exits on its own after the signed stop.
    let restart = run_workdeck(
        &directory,
        port,
        &[],
        &["daemon", "restart", "--yes", "--json"],
    );
    assert_eq!(restart.status.code(), Some(0));
    let body = json_stdout(&restart);
    assert_eq!(body["restarted"], true);
    assert_eq!(body["before"]["direction"], "client-newer");
    assert_eq!(body["after"]["direction"], "matched");
    assert_eq!(
        body["after"]["daemon"]["daemonVersion"],
        serde_json::json!(workdeck_session::WORKDECK_SESSION_DAEMON_VERSION)
    );
    let new_pid = body["after"]["daemon"]["pid"].as_u64().unwrap();
    assert_ne!(old_pid, new_pid, "restart must spawn a fresh daemon");
    wait_for_child_exit(&mut old_daemon.0, "the stopped daemon");

    // 5. The replacement accepts session commands from this build.
    let listed = run_workdeck(&directory, port, &[], &["session", "list", "--json"]);
    assert_eq!(listed.status.code(), Some(0));
    assert_eq!(json_stdout(&listed)["sessions"], serde_json::json!([]));

    // 6. Cleanup: this test owns the replacement daemon its restart spawned.
    // SAFETY: the pid comes from this test's own restart; no user process is signalled.
    assert_eq!(
        unsafe { libc::kill(new_pid as _, libc::SIGTERM) },
        0,
        "the replacement daemon must still be running"
    );
    wait_for_health(port, false, "the replacement daemon exits");
}

#[test]
fn daemon_status_reports_no_daemon_without_touching_the_process_table() {
    let directory = tempfile::tempdir().unwrap();
    let port = reserve_port();
    let status = run_workdeck(&directory, port, &[], &["daemon", "status"]);
    assert_eq!(status.status.code(), Some(0));
    let text = String::from_utf8_lossy(&status.stdout);
    assert_eq!(text.trim(), "No session daemon is running.");
}

#[test]
fn daemon_restart_refuses_confirmation_without_a_terminal() {
    let directory = tempfile::tempdir().unwrap();
    let port = reserve_port();
    let daemon = spawn_daemon(&directory, port, &[]);
    wait_for_health(port, true, "the daemon binds its port");
    // Stdin is null in this harness, so the confirmation cannot be asked for.
    let restart = run_workdeck(&directory, port, &[], &["daemon", "restart"]);
    assert_eq!(restart.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&restart.stderr);
    assert!(stderr.contains("stdin is not a terminal"), "{stderr}");
    assert!(stderr.contains("--yes"), "{stderr}");
    // The daemon was not replaced.
    let status = run_workdeck(&directory, port, &[], &["daemon", "status", "--json"]);
    assert_eq!(json_stdout(&status)["running"], true);
    drop(daemon);
}
