//! Actual normal-startup check planning, foreground execution and child cleanup.
use super::*;
use serde_json::json;
use workdeck_pm::{RunQuery, RunState};

fn seed(repository: &Repository, script: &str, report: bool) {
    repository
        .create_issue(
            &CreateIssue::new("Foreground check task", "Inspect, then run"),
            &RequestId::new(),
        )
        .unwrap();
    let command = json!({
        "schema":1,"repository":repository.identity(),"id":"test","name":"Test command",
        "recipe":{"kind":"shell","interpreter":"sh","script":script,
            "args":if report {json!([{"kind":"artifact","id":"report"}])}else{json!([])}},
        "cwd":".","tools":[{"name":"sh","executable":"/bin/sh"}],
        "inputs":{"files":["source.rs"]},
        "bounds":{"timeout_seconds":30,"stdout_bytes":1024,"stderr_bytes":1024},
        "artifacts":if report {json!([{"id":"report","name":"unit.xml","max_bytes":4096,"required":true}])}else{json!([])},
        "effects":[{"kind":"write","path":"ran.txt"},{"kind":"write","path":"child.pid"}]
    });
    let check = json!({
        "schema":1,"repository":repository.identity(),"id":"unit","name":"Unit checks","command":"test",
        "expectation":if report {json!({"kind":"junit","artifact":"report","suites":["unit"],"minimum_tests":1,"maximum_skipped":null,"allowed_exit_codes":[0]})}
            else{json!({"kind":"process","allowed_exit_codes":[0]})}
    });
    for (path, value) in [
        ("commands/test.yml", command),
        ("checks/unit.yml", check),
        (
            "check-profiles/quick.yml",
            json!({"schema":1,"repository":repository.identity(),"id":"quick","name":"Quick checks","checks":["unit"]}),
        ),
    ] {
        let path = repository.root().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    }
}

fn open_plan(session: &mut Session) {
    session.wait(|text| text.contains("F3 Issues"));
    session.write(b"\x1bORi5");
    session.wait(|text| text.contains("Check definitions") && text.contains("Quick checks"));
    session.write(b"p");
    session.wait(|text| {
        text.contains("Inspected check plan") && text.contains("No planner blockers.")
    });
}

fn select_row(session: &mut Session, label: &str) {
    for _ in 0..40 {
        let frame = session.wait(|text| text.lines().any(|line| line.contains('›')));
        let selected = frame.lines().find(|line| line.contains('›')).unwrap();
        if selected.contains(label) {
            return;
        }
        // Adjacent source citations can have the same caption and detail.
        // Scrolling changes the surrounding viewport even when that row's
        // text is identical; acknowledge the complete frame before the next key.
        let previous = frame;
        // Refresh retains the selected task summary. New check feedback is
        // inserted above that row, so inspect earlier rows without rebasing it.
        session.write(b"k");
        session.wait(|text| text != previous && text.lines().any(|line| line.contains('›')));
    }
    panic!("row {label} was not reachable");
}

#[test]
fn pm08_profile_plan_explicit_run_failure_explanation_and_review_return() {
    let (directory, repository) = repository();
    seed(
        &repository,
        "printf ran >> ran.txt; printf '<testsuite name=\"unit\" tests=\"1\" failures=\"1\"><testcase name=\"stale contract\"><failure message=\"stale request rejected\">source precondition</failure></testcase></testsuite>' > \"$1\"; exit 1",
        true,
    );
    let mut session = startup(directory.path(), 110);
    open_plan(&mut session);
    assert!(!directory.path().join("ran.txt").exists());
    session.write(b"\x1bOQ");
    session.wait(|text| !text.contains("Inspected check plan"));
    session.write(b"\x1bOR");
    session.wait(|text| {
        text.contains("Inspected check plan") && text.contains("No planner blockers.")
    });
    session.write(b"x");
    session.wait(|text| text.contains("Check results") && text.contains("Failed"));
    assert_eq!(fs::read(directory.path().join("ran.txt")).unwrap(), b"ran");
    session.write(b"jf");
    session.wait(|text| text.contains("Attention") && text.contains("check unit"));
    session.write(b"e\x1b[6~");
    session.wait(|text| text.contains("stale request rejected"));
    session.resize(78, 38);
    session.wait(|text| text.contains("Check results") && text.contains("local feedback"));
    session.write(b"f");
    session.wait(|text| text.contains("No matching results"));
    session.write(b"f");
    session.wait(|text| text.contains("Failed"));
    session.write(b"1r");
    session.wait(|text| text.contains("Task context") && text.contains("Local checks"));
    select_row(&mut session, "Local checks");
    session.wait(|text| text.contains("Inspected local feedback"));
    session.write(b"\r");
    session.wait(|text| text.contains("Check results") && text.contains("Failed"));
    assert_eq!(
        fs::read(directory.path().join("ran.txt")).unwrap(),
        b"ran",
        "opening context feedback must never execute another command"
    );
    session.write(b"\x1bOQ");
    session.wait(|text| !text.contains("Check results"));
    session.quit();
    let runs = repository.check_results(&RunQuery::default()).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].assessment.state, RunState::Failed);
    assert!(
        runs[0].results.as_ref().unwrap().result.invocations[0]
            .process
            .cleanup_complete
    );
}

fn child_started(session: &mut Session, root: &std::path::Path) -> i32 {
    session.write(b"x");
    session.wait(|text| text.contains("RUN ACTIVE"));
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Ok(value) = fs::read_to_string(root.join("child.pid"))
            && let Ok(pid) = value.parse()
        {
            return pid;
        }
        assert!(Instant::now() < deadline, "check child did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn child_gone(pid: i32) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while unsafe { libc::kill(pid, 0) } == 0 {
        assert!(
            Instant::now() < deadline,
            "owned descendant {pid} survived terminal cleanup"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
}

#[test]
fn pm08_active_check_defers_suspend_and_ctrl_c_cleans_children_without_quitting_review() {
    let (directory, repository) = repository();
    seed(
        &repository,
        "/bin/sleep 30 & printf '%s' \"$!\" > child.pid; wait",
        false,
    );
    let mut session = startup(directory.path(), 110);
    open_plan(&mut session);
    let child = child_started(&mut session, directory.path());
    session.write(b"\x1a");
    session.wait(|text| text.contains("Suspension deferred"));
    session.write(b"\x1bOQ");
    session
        .wait(|text| text.contains("CHECK RUN ACTIVE") && !text.contains("Inspected check plan"));
    session.write(b"\x03");
    child_gone(child);
    session.write(b"\x1bOR");
    session.wait(|text| text.contains("Check results") && text.contains("Canceled"));
    assert!(session.child.try_wait().unwrap().is_none());
    session.quit();
}

#[test]
fn pm08_quit_and_terminal_disconnect_join_owned_check_cleanup() {
    for disconnect in [false, true] {
        let (directory, repository) = repository();
        seed(
            &repository,
            "/bin/sleep 30 & printf '%s' \"$!\" > child.pid; wait",
            false,
        );
        let mut session = startup(directory.path(), 110);
        open_plan(&mut session);
        let child = child_started(&mut session, directory.path());
        if disconnect {
            drop(session.master.take());
            let deadline = Instant::now() + Duration::from_secs(4);
            loop {
                if session.child.try_wait().unwrap().is_some() {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "terminal disconnect did not finish foreground cleanup"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        } else {
            session.quit();
        }
        child_gone(child);
        let runs = repository.check_results(&RunQuery::default()).unwrap();
        assert_eq!(runs.len(), 1);
        assert!(
            runs[0].results.as_ref().unwrap().result.invocations[0]
                .process
                .cleanup_complete
        );
    }
}

#[test]
fn pm08_repeated_process_signals_cancel_owned_checks_before_any_terminal_exit() {
    let (directory, repository) = repository();
    seed(
        &repository,
        "trap '' TERM; /bin/sleep 30 & printf '%s' \"$!\" > child.pid; wait",
        false,
    );
    let mut session = startup(directory.path(), 110);
    open_plan(&mut session);
    let child = child_started(&mut session, directory.path());
    // Both signals target this fixture's Workdeck process, never the test host.
    for _ in 0..2 {
        assert_eq!(
            unsafe { libc::kill(session.child.id() as i32, libc::SIGINT) },
            0
        );
    }
    session.wait(|text| text.contains("Check results") && text.contains("Canceled"));
    child_gone(child);
    assert!(session.child.try_wait().unwrap().is_none());
    session.quit();
}
