//! Explicit local claim authoring in the mounted native terminal.
use super::*;
use workdeck_pm::{ClaimGuarantee, ClaimState};

struct OwnedTransport {
    directory: tempfile::TempDir,
    gate: std::path::PathBuf,
    pid: std::path::PathBuf,
    child: std::path::PathBuf,
}
impl OwnedTransport {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let gate = directory.path().join("hold");
        let pid = directory.path().join("transport.pid");
        let child = directory.path().join("transport-child.pid");
        fs::write(&gate, "test owns this finite gate").unwrap();
        let wrapper = directory.path().join("git");
        fs::write(
            &wrapper,
            r#"#!/bin/sh
case " $* " in
  *" push "*)
    if test -f "$WD_TEST_GATE"; then
      printf '%s' "$$" > "$WD_TEST_PID"
      /bin/sleep 25 & owned_child=$!
      printf '%s' "$owned_child" > "$WD_TEST_CHILD"
      trap 'kill "$owned_child" 2>/dev/null; wait "$owned_child" 2>/dev/null' EXIT
      attempts=0
      while test -f "$WD_TEST_GATE" && test "$attempts" -lt 250; do
        /bin/sleep 0.02
        attempts=$((attempts + 1))
      done
      kill "$owned_child" 2>/dev/null
      wait "$owned_child" 2>/dev/null
      trap - EXIT
    fi
    ;;
esac
exec /usr/bin/git "$@"
"#,
        )
        .unwrap();
        fs::set_permissions(wrapper, fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            directory,
            gate,
            pid,
            child,
        }
    }
    fn read_pid(path: &std::path::Path) -> Option<i32> {
        fs::read_to_string(path).ok()?.parse().ok()
    }
    fn release(&self) {
        let _ = fs::remove_file(&self.gate);
    }
}
impl Drop for OwnedTransport {
    fn drop(&mut self) {
        self.release();
        // These PID files belong only to this test's explicit transport gate.
        // Clean up even when a terminal assertion panics before normal join.
        for path in [&self.child, &self.pid] {
            if let Some(pid) = Self::read_pid(path) {
                unsafe {
                    libc::kill(pid, libc::SIGKILL);
                }
            }
        }
    }
}
fn assert_gone(pid: i32) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while unsafe { libc::kill(pid, 0) } == 0 {
        assert!(
            Instant::now() < deadline,
            "owned transport process {pid} survived completion"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn pm09_claim_inspect_acquire_retained_form_renew_release_and_review_return() {
    for width in [72, 132] {
        let (directory, repository) = repository();
        let issue: IssueRecord = serde_json::from_value(
            repository
                .create_issue(
                    &CreateIssue::new("Terminal claim task", "Read before coordinating"),
                    &RequestId::new(),
                )
                .unwrap()
                .result,
        )
        .unwrap();
        git(directory.path(), &["add", ".workdeck"]);
        git(
            directory.path(),
            &["commit", "--quiet", "-m", "Claim fixture"],
        );
        let before_index = fs::read(directory.path().join(".git/index")).unwrap();
        let mut session = startup(directory.path(), width);
        session.wait(|text| text.contains("F3 Issues"));
        session.write(b"\x1bORi7");
        session.wait(|text| text.contains("Work claims") && text.contains("explicit coordination"));
        assert!(repository.claims().unwrap().is_empty());
        session.write(b"n");
        session.wait(|text| text.contains("Acquire claim") && text.contains("TTL seconds"));
        session.write(b"\x15terminal-owner\t600");
        session.write(b"\x1bOQ");
        session
            .wait(|text| !text.contains("Acquire claim") && text.contains("No changes to review"));
        session.write(b"\x1bOR");
        session.wait(|text| text.contains("Acquire claim") && text.contains("terminal-owner"));
        session.write(b"\x13");
        session.wait(|text| {
            text.contains("LocalSourceOnly")
                && text.contains("Usable")
                && !text.contains("Acquire claim")
        });
        let first = repository.claims().unwrap().remove(0);
        assert_eq!(first.claim.metadata.actor, "terminal-owner");
        assert_eq!(first.assessment.guarantee, ClaimGuarantee::LocalSourceOnly);
        session.write(b"u");
        session.wait(|text| text.contains("Renew claim"));
        session.write(b"\x15terminal-owner\x13");
        session.wait(|text| text.contains("LocalSourceOnly") && !text.contains("Renew claim"));
        let renewed = repository.claims().unwrap().remove(0);
        assert!(renewed.claim.metadata.generation > first.claim.metadata.generation);
        session.write(b"d");
        session.wait(|text| text.contains("Release claim") && text.contains("Reason"));
        session.write(b"\x15terminal-owner\tExplicitly handing back this task\x13");
        session.wait(|text| text.contains("Released") && !text.contains("Release claim"));
        assert_eq!(
            repository.claims().unwrap()[0].claim.metadata.state,
            ClaimState::Released
        );
        assert_eq!(
            repository
                .show_issue(issue.metadata.id.as_str())
                .unwrap()
                .source,
            issue.source
        );
        assert_eq!(
            fs::read(directory.path().join(".git/index")).unwrap(),
            before_index
        );
        session.write(b"n");
        session.wait(|text| text.contains("Acquire claim"));
        session.write(b"\x15terminal-owner\x13");
        session.wait(|text| text.contains("Usable") && !text.contains("Acquire claim"));
        session.write(b"e");
        session.wait(|text| text.contains("Complete and release claim") && text.contains("Reason"));
        session.write(b"\x15terminal-owner\tCompleted in the inspected terminal\x13");
        session.wait(|text| {
            text.contains("Claimed completion recorded")
                && text.contains("Separate release recorded: true")
        });
        assert_eq!(
            repository
                .show_issue(issue.metadata.id.as_str())
                .unwrap()
                .metadata
                .status,
            "done"
        );
        assert_eq!(
            repository.claims().unwrap()[0].claim.metadata.state,
            ClaimState::Released
        );
        assert_eq!(
            fs::read(directory.path().join(".git/index")).unwrap(),
            before_index
        );
        session.write(b"\x1bOQ");
        session.wait(|text| !text.contains("Work claims") && text.contains("F3 Issues"));
        session.quit();
    }
}

#[test]
fn pm09_pending_shared_publication_defers_interrupts_and_quit_then_joins_owned_children() {
    let (directory, repository) = repository();
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "--bare", "--quiet"]);
    git(directory.path(), &["branch", "-M", "main"]);
    git(
        directory.path(),
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    let issue: IssueRecord = serde_json::from_value(
        repository
            .create_issue(
                &CreateIssue::new("Owned shared claim", "Accepted contract"),
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap();
    let mut config = repository.config().unwrap();
    config.sources = Some(workdeck_pm::SharedSources {
        remote: "origin".into(),
        accepted_ref: "refs/heads/main".parse().unwrap(),
        coordination_ref: "refs/heads/workdeck-coordination".parse().unwrap(),
        proposal_namespace: "refs/heads/workdeck-proposals".parse().unwrap(),
    });
    fs::write(
        repository.root().join("config.yml"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    git(directory.path(), &["add", ".workdeck"]);
    git(
        directory.path(),
        &["commit", "--quiet", "-m", "Shared claim fixture"],
    );
    git(directory.path(), &["push", "--quiet", "origin", "main"]);
    let index = fs::read(directory.path().join(".git/index")).unwrap();
    let transport = OwnedTransport::new();
    let path = std::env::join_paths(
        std::iter::once(transport.directory.path().to_owned()).chain(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        )),
    )
    .unwrap();
    let mut session = Session::launch_in_environment(
        "",
        &["--no-extensions"],
        false,
        132,
        34,
        None,
        Some(directory.path()),
        None,
        &[
            ("PATH", path.as_os_str()),
            ("WD_TEST_GATE", transport.gate.as_os_str()),
            ("WD_TEST_PID", transport.pid.as_os_str()),
            ("WD_TEST_CHILD", transport.child.as_os_str()),
        ],
    );
    session.wait(|text| text.contains("F3 Issues"));
    session.write(b"\x1bORi7n");
    session.wait(|text| text.contains("Acquire claim"));
    session.write(b"\x15terminal-owner\x13");
    session.wait(|text| {
        text.contains("pending") && OwnedTransport::read_pid(&transport.child).is_some()
    });
    let pid = OwnedTransport::read_pid(&transport.pid).unwrap();
    let child = OwnedTransport::read_pid(&transport.child).unwrap();
    for _ in 0..2 {
        assert_eq!(
            unsafe { libc::kill(session.child.id() as i32, libc::SIGINT) },
            0
        );
    }
    session.write(b"\x1bOQ");
    session.wait(|text| !text.contains("Acquire claim") && text.contains("Publication pending"));
    session.write(b"q");
    session.wait(|text| text.contains("deferred") && text.contains("Publication pending"));
    assert!(session.child.try_wait().unwrap().is_none());
    assert_eq!(unsafe { libc::kill(pid, 0) }, 0);
    assert_eq!(unsafe { libc::kill(child, 0) }, 0);
    transport.release();
    session.write(b"\x1bOR");
    session.wait(|text| text.contains("SharedConfirmed") && !text.contains("Acquire claim"));
    assert_gone(pid);
    fs::remove_file(&transport.pid).unwrap();
    assert_gone(child);
    fs::remove_file(&transport.child).unwrap();
    assert_eq!(
        repository.claims().unwrap()[0].claim.metadata.actor,
        "terminal-owner"
    );
    assert_eq!(
        repository
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .source,
        issue.source
    );
    assert_eq!(
        fs::read(directory.path().join(".git/index")).unwrap(),
        index
    );
    session.write(b"\x1bOQ");
    session.wait(|text| !text.contains("Work claims"));
    session.quit();
}
