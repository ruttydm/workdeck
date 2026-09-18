#![cfg(unix)]
use assert_cmd::prelude::*;
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use workdeck_pm::Repository;

fn run(root: &Path, args: &[&str]) -> Output {
    Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root)
        .env("XDG_CONFIG_HOME", root.join("isolated-config"))
        .args(args)
        .output()
        .unwrap()
}
fn value(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{error}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}
fn fixture(script: &str) -> (tempfile::TempDir, Repository) {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    fs::write(temp.path().join("input.txt"), "first").unwrap();
    for (path, document) in [
        (
            "commands/test.yml",
            json!({"schema":1,"repository":repo.identity(),"id":"test","name":"Test",
            "recipe":{"kind":"argv","argv":[{"kind":"literal","value":"sh"},{"kind":"literal","value":"-c"},{"kind":"literal","value":script}]},
            "cwd":".","tools":[{"name":"sh","executable":"/bin/sh"}],"inputs":{"files":["input.txt"]},
            "bounds":{"timeout_seconds":3,"stdout_bytes":1024,"stderr_bytes":1024}}),
        ),
        (
            "checks/unit.yml",
            json!({"schema":1,"repository":repo.identity(),"id":"unit","name":"Unit","command":"test","expectation":{"kind":"process","allowed_exit_codes":[0]}}),
        ),
        (
            "check-profiles/quick.yml",
            json!({"schema":1,"repository":repo.identity(),"id":"quick","name":"Quick","checks":["unit"]}),
        ),
    ] {
        let path = repo.root().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, serde_json::to_vec(&document).unwrap()).unwrap();
    }
    (temp, repo)
}
fn plan(root: &Path) -> String {
    let output = run(
        root,
        &[
            "check",
            "plan",
            "--profile",
            "quick",
            "--json",
            "--no-input",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    value(&output)["result"]["plan"]["fingerprint"]
        .as_str()
        .unwrap()
        .into()
}
fn execute(root: &Path, plan: &str, request: &str) -> Output {
    run(
        root,
        &[
            "check",
            "run",
            "--plan",
            plan,
            "--expected-plan",
            plan,
            "--actor",
            "test-agent",
            "--request-id",
            request,
            "--json",
            "--no-input",
        ],
    )
}
#[test]
fn discovery_and_plan_are_inert_then_explicit_run_replays_once() {
    let (temp, repo) = fixture("printf x >> sentinel; printf output");
    for args in [
        vec!["command", "list"],
        vec!["command", "show", "test"],
        vec!["command", "validate"],
        vec!["check", "list"],
        vec!["check", "show", "unit"],
        vec!["check", "validate"],
        vec!["check", "profile", "list"],
        vec!["check", "profile", "show", "quick"],
        vec!["check", "profile", "validate"],
    ] {
        let mut args = args;
        args.extend(["--json", "--no-input"]);
        let output = run(temp.path(), &args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(value(&output)["ok"], true);
    }
    let fingerprint = plan(temp.path());
    assert!(!temp.path().join("sentinel").exists());
    let first = execute(temp.path(), &fingerprint, "cli-once");
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first = value(&first);
    assert_eq!(first["result"]["state"], "passed");
    assert_eq!(first["result"]["receipts"].as_array().unwrap().len(), 2);
    let replay = execute(temp.path(), &fingerprint, "cli-once");
    assert!(replay.status.success());
    assert_eq!(value(&replay)["result"]["replayed"], true);
    assert_eq!(fs::read(temp.path().join("sentinel")).unwrap(), b"x");
    assert!(repo.doctor().unwrap().valid);
    let run_id = first["result"]["run_id"].as_str().unwrap();
    for args in [
        vec!["check", "status", run_id],
        vec!["check", "explain", run_id],
        vec!["check", "results", "--status", "passed"],
    ] {
        let mut args = args;
        args.push("--json");
        let output = run(temp.path(), &args);
        assert!(output.status.success());
        assert_eq!(value(&output)["ok"], true);
    }
    fs::write(temp.path().join("input.txt"), "changed").unwrap();
    let stale = execute(temp.path(), &fingerprint, "cli-once");
    assert_eq!(stale.status.code(), Some(4));
    assert_eq!(value(&stale)["result"]["state"], "stale");
    assert_eq!(fs::read(temp.path().join("sentinel")).unwrap(), b"x");
}
#[test]
fn failed_execution_is_one_machine_document_and_nonzero_exit() {
    let (temp, _) = fixture("printf x >> sentinel; exit 9");
    let fingerprint = plan(temp.path());
    let output = execute(temp.path(), &fingerprint, "cli-failed");
    assert_eq!(output.status.code(), Some(1));
    let document = value(&output);
    assert_eq!(document["result"]["state"], "failed");
    assert_eq!(document["result"]["receipts"].as_array().unwrap().len(), 2);
    assert_eq!(
        output.stdout.iter().filter(|&&byte| byte == b'\n').count(),
        1
    );
}
#[test]
fn wrong_plan_or_missing_request_rejects_before_process_effects() {
    let (temp, repo) = fixture("printf x >> sentinel");
    let fingerprint = plan(temp.path());
    for args in [
        vec![
            "check",
            "run",
            "--plan",
            &fingerprint,
            "--expected-plan",
            &fingerprint,
            "--actor",
            "agent",
            "--json",
        ],
        vec![
            "check",
            "run",
            "--plan",
            &fingerprint,
            "--expected-plan",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--actor",
            "agent",
            "--request-id",
            "bad-plan",
            "--json",
        ],
    ] {
        let output = run(temp.path(), &args);
        assert!(!output.status.success());
        assert_eq!(value(&output)["ok"], false);
    }
    assert!(!temp.path().join("sentinel").exists());
    assert!(repo.check_results(&Default::default()).unwrap().is_empty());
}
#[test]
fn results_cursor_is_bound_to_membership_and_source_state() {
    let (temp, _) = fixture("exit 0");
    let fingerprint = plan(temp.path());
    for request in ["page-one", "page-two", "page-three"] {
        assert!(execute(temp.path(), &fingerprint, request).status.success());
    }
    let first = run(temp.path(), &["check", "results", "--limit", "2", "--json"]);
    assert!(first.status.success());
    let first = value(&first);
    assert_eq!(first["result"]["total"], 3);
    assert_eq!(first["result"]["records"].as_array().unwrap().len(), 2);
    let cursor = serde_json::to_string(&first["result"]["next_cursor"]).unwrap();
    let second = run(
        temp.path(),
        &[
            "check", "results", "--limit", "2", "--cursor", &cursor, "--json",
        ],
    );
    assert!(second.status.success());
    assert_eq!(
        value(&second)["result"]["records"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    fs::write(temp.path().join("input.txt"), "changed").unwrap();
    let stale = run(
        temp.path(),
        &[
            "check", "results", "--limit", "2", "--cursor", &cursor, "--json",
        ],
    );
    assert!(!stale.status.success());
    assert_eq!(value(&stale)["ok"], false);
}

#[test]
fn projection_failure_preserves_committed_run_receipts_for_retry() {
    let (temp, _) = fixture("printf x >> sentinel");
    let fingerprint = plan(temp.path());
    let output = run(
        temp.path(),
        &[
            "check",
            "run",
            "--plan",
            &fingerprint,
            "--expected-plan",
            &fingerprint,
            "--actor",
            "agent",
            "--request-id",
            "projection",
            "--fields",
            "does_not_exist",
            "--json",
        ],
    );
    assert!(!output.status.success());
    let document = value(&output);
    assert_eq!(document["ok"], false);
    let text = document.to_string();
    assert!(text.contains("mutation_committed"), "{document}");
    assert!(text.contains("execution.finish"), "{document}");
    assert!(text.contains("inspect_run_status"), "{document}");
    assert!(output.stdout.len() <= 16 * 1024);
    assert_eq!(fs::read(temp.path().join("sentinel")).unwrap(), b"x");
    let replay = execute(temp.path(), &fingerprint, "projection");
    // Actor is part of exact request identity; this retry deliberately differs.
    assert!(!replay.status.success());
    assert_eq!(fs::read(temp.path().join("sentinel")).unwrap(), b"x");
    let retry = run(
        temp.path(),
        &[
            "check",
            "run",
            "--plan",
            &fingerprint,
            "--expected-plan",
            &fingerprint,
            "--actor",
            "agent",
            "--request-id",
            "projection",
            "--json",
        ],
    );
    assert!(retry.status.success());
    assert_eq!(value(&retry)["result"]["replayed"], true);
    assert_eq!(fs::read(temp.path().join("sentinel")).unwrap(), b"x");
}

#[test]
fn direct_command_plan_uses_same_runner_and_rejects_check_family_mismatch() {
    let (temp, _) = fixture("printf x >> sentinel");
    let output = run(temp.path(), &["command", "plan", "test", "--json"]);
    assert!(output.status.success());
    let plan = value(&output);
    let fingerprint = plan["result"]["plan"]["fingerprint"].as_str().unwrap();
    let wrong = execute(temp.path(), fingerprint, "wrong-family");
    assert!(!wrong.status.success());
    assert!(!temp.path().join("sentinel").exists());
    let output = run(
        temp.path(),
        &[
            "command",
            "run",
            "--plan",
            fingerprint,
            "--expected-plan",
            fingerprint,
            "--actor",
            "agent",
            "--request-id",
            "direct",
            "--json",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(value(&output)["result"]["state"], "passed");
    assert_eq!(fs::read(temp.path().join("sentinel")).unwrap(), b"x");
    let output = value(&output);
    let explanation = run(
        temp.path(),
        &[
            "check",
            "explain",
            output["result"]["run_id"].as_str().unwrap(),
            "--json",
        ],
    );
    assert!(explanation.status.success());
    assert_eq!(
        value(&explanation)["result"]["invocations"][0]["process"]["exit_code"],
        0
    );
}

#[test]
fn headless_interrupt_waits_for_owned_child_cleanup_and_retains_canceled_result() {
    use std::{
        process::Stdio,
        time::{Duration, Instant},
    };
    let (temp, repo) =
        fixture("trap '' TERM; /bin/sleep 30 & printf '%s' \"$!\" > child.pid; wait");
    let fingerprint = plan(temp.path());
    let mut child = Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(temp.path())
        .env("XDG_CONFIG_HOME", temp.path().join("isolated-config"))
        .args([
            "check",
            "run",
            "--plan",
            &fingerprint,
            "--expected-plan",
            &fingerprint,
            "--actor",
            "agent",
            "--request-id",
            "interrupt",
            "--json",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let pid = loop {
        if let Ok(text) = fs::read_to_string(temp.path().join("child.pid"))
            && let Ok(pid) = text.parse::<i32>()
        {
            break pid;
        }
        if child.try_wait().unwrap().is_some() || Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("run did not create its fixture child");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    for _ in 0..2 {
        unsafe {
            libc::kill(child.id() as i32, libc::SIGINT);
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
            let _ = child.kill();
            let _ = child.wait();
            panic!("interrupt did not finish cleanup");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let child_alive = unsafe { libc::kill(pid, 0) } == 0;
    if child_alive {
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
    }
    assert!(!child_alive, "owned child survived CLI completion");
    assert_eq!(status.code(), Some(130));
    let runs = repo.check_results(&Default::default()).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].state, workdeck_pm::RunState::Canceled);
    assert!(
        runs[0].results.as_ref().unwrap().result.invocations[0]
            .process
            .cleanup_complete
    );
}

#[test]
fn emitted_saved_plan_path_accepts_the_same_large_valid_plan_as_its_fingerprint() {
    let (temp, repo) = fixture("printf x >> sentinel");
    let original: Value =
        serde_json::from_slice(&fs::read(repo.root().join("commands/test.yml")).unwrap()).unwrap();
    for index in 0..5 {
        let mut definition = original.clone();
        definition["id"] = json!(format!("extra-{index}"));
        definition["custom"] = json!({"authored_reference":"x".repeat(220_000)});
        fs::write(
            repo.root().join(format!("commands/extra-{index}.yml")),
            serde_json::to_vec(&definition).unwrap(),
        )
        .unwrap();
    }
    let plan = repo
        .check_plan(&workdeck_pm::CheckPlanRequest {
            checks: vec!["unit".into()],
            ..Default::default()
        })
        .unwrap();
    plan.validate().unwrap();
    assert!(plan.blockers.is_empty());
    let bytes = serde_json::to_vec(&plan).unwrap();
    assert!(bytes.len() > 2 * 1024 * 1024);
    assert!(bytes.len() <= workdeck_pm::MAX_CHECK_PLAN_BYTES);
    let path = repo.save_check_plan(&plan).unwrap();
    let output = run(
        temp.path(),
        &[
            "check",
            "run",
            "--plan",
            path.to_str().unwrap(),
            "--expected-plan",
            &plan.fingerprint.to_string(),
            "--actor",
            "agent",
            "--request-id",
            "large-path",
            "--json",
        ],
    );
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(value(&output)["result"]["state"], "passed");
    assert_eq!(fs::read(temp.path().join("sentinel")).unwrap(), b"x");
}
