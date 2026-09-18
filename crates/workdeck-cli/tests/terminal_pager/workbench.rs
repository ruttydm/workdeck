//! Mounted normal-startup planning coverage using the native PTY harness.
use super::*;
use workdeck_pm::{CreateIssue, IssueRecord, Repository, RequestId};

#[path = "workbench_checks.rs"]
mod checks;
#[path = "workbench_claims.rs"]
mod claims;
#[path = "workbench_context.rs"]
mod context;
#[path = "workbench_review_coverage.rs"]
mod review_coverage;
#[path = "workbench_sources.rs"]
mod sources;

fn git(root: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}

fn repository() -> (tempfile::TempDir, Repository) {
    let directory = tempfile::tempdir().unwrap();
    git(directory.path(), &["init", "--quiet"]);
    git(directory.path(), &["config", "user.name", "Workbench test"]);
    git(
        directory.path(),
        &["config", "user.email", "workbench@example.test"],
    );
    fs::write(
        directory.path().join("source.rs"),
        "first source line\nlinked source line\nlast source line\n",
    )
    .unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    git(directory.path(), &["add", "."]);
    git(
        directory.path(),
        &["commit", "--quiet", "-m", "Initial repository"],
    );
    (directory, repository)
}

fn startup(root: &std::path::Path, width: u16) -> Session {
    Session::launch_in("", &["--no-extensions"], false, width, 34, None, Some(root))
}

#[test]
fn issue_graph_keeps_outside_filter_prerequisites_visible_and_returns_to_issues() {
    for width in [78, 180] {
        let (directory, repository) = repository();
        let mut records = Vec::new();
        for title in ["Visible graph task", "Hidden prerequisite"] {
            let record: IssueRecord = serde_json::from_value(
                repository
                    .create_issue(&CreateIssue::new(title, ""), &RequestId::new())
                    .unwrap()
                    .result,
            )
            .unwrap();
            records.push(record);
        }
        repository
            .mutate_issue_graph(
                records[0].metadata.id.as_str(),
                None,
                None,
                &workdeck_pm::IssueGraphMutation::AddPrerequisite {
                    prerequisite: records[1].metadata.id.to_string(),
                },
                &RequestId::new(),
            )
            .unwrap();
        // Keep the review canvas clean so this assertion observes the issue
        // filter, rather than a separate diff containing the hidden title.
        git(directory.path(), &["add", ".workdeck"]);
        git(
            directory.path(),
            &["commit", "--quiet", "-m", "Graph fixture"],
        );
        let mut session = startup(directory.path(), width);
        session.wait(|text| text.contains("F3 Issues"));
        session.write(b"\x1bOR");
        session.wait(|text| text.contains("Visible graph task"));
        session.write(b"/");
        session.wait(|text| text.contains("Filter issues"));
        session.write(b"Visible graph task\x13");
        session.wait(|text| {
            text.contains("Visible graph task")
                && !text.contains("Filter issues")
                && !text.contains("Hidden prerequisite")
        });
        session.write(b"b");
        session.wait(|text| {
            text.contains("Issue graph")
                && text.contains("Hidden prerequisite")
                && text.contains("Ready for work: false")
        });
        session.write(b"\r");
        session.wait(|text| {
            text.contains(&format!("Issue graph · {}", records[1].metadata.id))
                && text.contains("Ready for work: true")
        });
        session.write(b"\x1b");
        session.wait(|text| {
            !text.contains("Issue graph ·")
                && text.contains("Visible graph task")
                && !text.contains("Hidden prerequisite")
        });
        session.quit();
    }
}

#[test]
fn native_features_author_in_terminal_and_refresh_shared_issue_coverage() {
    for width in [78, 180] {
        let (directory, repository) = repository();
        let mut session = startup(directory.path(), width);
        session.wait(|text| text.contains("F3 Issues"));
        session.write(b"\x1bORv");
        session.wait(|text| text.contains("Features") && text.contains("No features yet"));
        session.write(b"n");
        session.wait(|text| text.contains("Create feature") && text.contains("Ctrl-S save"));
        session.write(b"PTY capability\tFeature scope\x13");
        session.wait(|text| text.contains("PTY capability") && !text.contains("Create feature"));
        let feature = repository.list_features().unwrap().remove(0);
        assert_eq!(feature.body, "Feature scope");
        session.write(b"e");
        session.wait(|text| text.contains("Edit feature"));
        session.write(b"\x15Renamed capability\x13");
        session.wait(|text| text.contains("Renamed capability") && !text.contains("Edit feature"));
        assert_eq!(
            repository
                .feature(feature.metadata.id.as_str())
                .unwrap()
                .metadata
                .name,
            "Renamed capability"
        );
        let output = Command::new(env!("CARGO_BIN_EXE_workdeck"))
            .current_dir(directory.path())
            .args([
                "issue",
                "create",
                "Coverage member",
                "--feature",
                feature.metadata.id.as_str(),
                "--json",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let coverage = repository
            .feature_coverage(&workdeck_pm::FeatureCoverageQuery::new(
                feature.metadata.id.as_str(),
            ))
            .unwrap();
        assert_eq!(coverage.issues.len(), 1);
        session.write(b"r");
        session.wait(|text| text.contains("Coverage member"));
        session.write(b"a");
        session.wait(|text| text.contains("No features yet"));
        assert!(
            repository
                .feature(feature.metadata.id.as_str())
                .unwrap()
                .metadata
                .archived
        );
        session.write(b"x");
        session.wait(|text| text.contains("Renamed capability [archived]"));
        session.write(b"a");
        session.wait(|text| text.contains("Renamed capability") && !text.contains("[archived]"));
        assert!(
            !repository
                .feature(feature.metadata.id.as_str())
                .unwrap()
                .metadata
                .archived
        );
        session.quit();
    }
}

#[test]
fn clean_and_dirty_normal_startup_create_open_edit_issues_and_restore_terminal() {
    for dirty in [false, true] {
        let (directory, repository) = repository();
        if dirty {
            fs::write(
                directory.path().join("source.rs"),
                "first source line\ndirty source line\nlast source line\n",
            )
            .unwrap();
        }
        let mut session = startup(directory.path(), 110);
        session.wait(|text| text.contains("F2 Review") && text.contains("F3 Issues"));
        if dirty {
            session.wait(|text| text.contains("dirty source line"));
        }
        session.write(b"\x1bOR"); // F3
        session.wait(|text| text.contains("No issues yet"));
        session.write(b"n");
        session.wait(|text| text.contains("Create issue") && text.contains("Ctrl-S save"));
        session.write(b"PTY created issue\tBody from terminal\x13");
        session.wait(|text| text.contains("PTY created issue") && !text.contains("Ctrl-S save"));
        let issue = repository.list_issues().unwrap().remove(0);
        assert_eq!(issue.body, "Body from terminal");
        session.write(b"\r");
        session.wait(|text| text.contains("Body from terminal"));
        session.write(b"e");
        session.wait(|text| text.contains("Edit issue"));
        session.write(b"\x15PTY edited issue\x13");
        session.wait(|text| text.contains("PTY edited issue") && !text.contains("Ctrl-S save"));
        assert_eq!(
            repository
                .show_issue(issue.metadata.id.as_str())
                .unwrap()
                .metadata
                .title,
            "PTY edited issue"
        );
        if dirty {
            session.write(b"\x1bOQ");
            session.wait(|text| text.contains("dirty source line"));
            session.write(b"\x1bOS"); // F4
            session.wait(|text| text.contains("Create issue") && text.contains("Review source.rs"));
            session.write(b"\x15From selected file\x13");
            session
                .wait(|text| text.contains("From selected file") && !text.contains("Ctrl-S save"));
            let linked = repository
                .list_issues()
                .unwrap()
                .into_iter()
                .find(|issue| issue.metadata.title == "From selected file")
                .unwrap();
            assert_eq!(linked.metadata.files[0].path, "source.rs");
            session.resize(180, 34);
            session.wait(|text| {
                text.contains("From selected file") && text.contains("dirty source line")
            });
            session.resize(90, 34);
            session.wait(|text| {
                text.contains("From selected file") && !text.contains("dirty source line")
            });
        }
        session.quit();
        assert!(!directory.path().join(".agents").exists());
    }
}

#[test]
fn selected_review_note_persists_as_an_issue_and_remains_in_the_review() {
    let (directory, repository) = repository();
    let changed = "first source line\nreview note target\nlast source line\n";
    fs::write(directory.path().join("source.rs"), changed).unwrap();
    let mut session = startup(directory.path(), 110);
    session.wait(|text| text.contains("review note target") && text.contains("F3 Issues"));
    session.write(b"c");
    session.wait(|text| text.contains("Draft note") && text.contains("^S save Esc cancel"));
    session.write(b"Explain this change before merging\x13");
    session.wait(|text| {
        text.contains("Explain this change before merging") && !text.contains("Draft note")
    });
    session.write(b"\x1bOS"); // F4: create from the selected review note.
    session.wait(|text| {
        text.contains("Create issue") && text.contains("Explain this change before merging")
    });
    session.write(b"\x15Issue from selected note\x13");
    session.wait(|text| text.contains("Issue from selected note") && !text.contains("Ctrl-S save"));
    let issue = repository.list_issues().unwrap().remove(0);
    assert_eq!(issue.metadata.title, "Issue from selected note");
    assert_eq!(issue.body, "Explain this change before merging");
    assert_eq!(issue.metadata.files.len(), 1);
    assert_eq!(issue.metadata.files[0].path, "source.rs");
    assert!(issue.metadata.files[0].line.is_some());
    session.write(b"\x1bOQ"); // F2: the original review note remains.
    session.wait(|text| {
        text.contains("review note target") && text.contains("Explain this change before merging")
    });
    session.quit();
    assert_eq!(
        Repository::discover(directory.path())
            .unwrap()
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .body,
        issue.body
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("source.rs")).unwrap(),
        changed
    );
    assert!(!directory.path().join(".agents").exists());
}

#[test]
fn priority_labels_and_unassignment_publish_shared_receipts_from_native_forms() {
    let (directory, repository) = repository();
    for id in ["bug", "api-reviewed"] {
        let mut label = workdeck_pm::CreatePlanning::new(id);
        label.id = Some(id.into());
        repository
            .create_planning(workdeck_pm::PlanningKind::Label, &label, &RequestId::new())
            .unwrap();
    }
    let mut input = CreateIssue::new("Native property actions", "Keep this body");
    input
        .fields
        .insert("assignee".into(), serde_json::json!("agent"));
    repository.create_issue(&input, &RequestId::new()).unwrap();
    let original = repository.list_issues().unwrap().remove(0);
    let operations = directory.path().join(".workdeck/operations");
    let receipt_count = fs::read_dir(&operations).unwrap().count();
    let mut session = startup(directory.path(), 110);
    session.wait(|text| text.contains("F3 Issues"));
    session.write(b"\x1bOR");
    // The dirty fixture title also appears in the startup review, so wait for
    // the selected indexed row itself before sending the property action.
    session.wait(|text| text.contains("› Native property actions"));
    session.write(b"p");
    session.wait(|text| text.contains("Change priority"));
    session.write(b"\x15invalid\x13");
    session.wait(|text| text.contains("unknown priority"));
    assert_eq!(
        repository
            .show_issue(original.metadata.id.as_str())
            .unwrap()
            .source,
        original.source
    );
    session.write(b"\x15high\x13");
    session.wait(|text| text.contains("Native property actions") && !text.contains("Ctrl-S save"));
    assert_eq!(
        repository
            .show_issue(original.metadata.id.as_str())
            .unwrap()
            .metadata
            .priority,
        workdeck_pm::Priority::High
    );
    session.write(b"l");
    session.wait(|text| text.contains("Edit labels"));
    session.write(b"bug\rapi-reviewed");
    session.write(b"\x1bOQ");
    session.wait(|text| !text.contains("Edit labels"));
    session.write(b"\x1bOR");
    session.wait(|text| text.contains("Edit labels") && text.contains("api-reviewed"));
    session.write(b"\x13");
    session.wait(|text| text.contains("Native property actions") && !text.contains("Ctrl-S save"));
    assert_eq!(
        repository
            .show_issue(original.metadata.id.as_str())
            .unwrap()
            .metadata
            .labels,
        ["bug", "api-reviewed"]
    );
    session.resize(75, 34);
    session.write(b"l");
    session.wait(|text| text.contains("Edit labels"));
    session.write(b"\x15\x13");
    session.wait(|text| text.contains("Native property actions") && !text.contains("Ctrl-S save"));
    session.write(b"a");
    session.wait(|text| text.contains("Assign issue"));
    session.write(b"\x15\x13");
    session.wait(|text| text.contains("Native property actions") && !text.contains("Ctrl-S save"));
    let current = repository
        .show_issue(original.metadata.id.as_str())
        .unwrap();
    assert!(current.metadata.labels.is_empty());
    assert_eq!(current.metadata.assignee, None);
    assert_eq!(current.body, "Keep this body");
    assert_eq!(fs::read_dir(operations).unwrap().count(), receipt_count + 4);
    session.quit();
    assert!(!directory.path().join(".agents").exists());
}

#[test]
fn copy_reference_and_link_current_file_preserve_native_review_context() {
    let (directory, repository) = repository();
    repository
        .create_issue(
            &CreateIssue::new("Existing issue to link", "Preserved planning"),
            &RequestId::new(),
        )
        .unwrap();
    let original = repository.list_issues().unwrap().remove(0);
    git(directory.path(), &["add", "."]);
    git(
        directory.path(),
        &["commit", "--quiet", "-m", "Existing planning issue"],
    );
    let source = "first source line\ncurrent review file to link\nlast source line\n";
    fs::write(directory.path().join("source.rs"), source).unwrap();
    let mut session = Session::launch_in(
        "",
        &["--no-extensions"],
        false,
        110,
        34,
        None,
        Some(directory.path()),
    );
    session.wait(|text| text.contains("current review file to link") && text.contains("F3 Issues"));
    session.write(b"\x1bOR");
    session.wait(|text| text.contains("Existing issue to link"));
    session.write(b"y");
    session.wait(|text| text.contains("Copy requested through terminal clipboard"));
    assert_eq!(
        repository
            .show_issue(original.metadata.id.as_str())
            .unwrap()
            .source,
        original.source
    );
    session.write(b"\x1bOQ");
    session.wait(|text| text.contains("current review file to link"));
    session.write(b"\x1b[1;2S"); // Shift-F4
    session.wait(|text| {
        text.contains("Link current file")
            && text.contains("File: source.rs")
            && text.contains("Existing issue to link")
    });
    session.write(b"\r");
    session.wait(|text| {
        text.contains("Existing issue to link") && !text.contains("Link current file")
    });
    let linked = repository
        .show_issue(original.metadata.id.as_str())
        .unwrap();
    assert_eq!(linked.metadata.files.len(), 1);
    assert_eq!(linked.metadata.files[0].path, "source.rs");
    assert_eq!(linked.body, "Preserved planning");
    session.write(b"\x1bOQ");
    session.wait(|text| text.contains("current review file to link") && text.contains("source.rs"));
    session.quit();
    assert_eq!(
        fs::read_to_string(directory.path().join("source.rs")).unwrap(),
        source
    );
    assert!(!directory.path().join(".agents").exists());
}

#[test]
fn clean_file_and_commit_jumps_return_to_the_selected_issue_and_retain_draft() {
    let (directory, repository) = repository();
    let commit = git(directory.path(), &["rev-parse", "HEAD"]);
    let mut input = CreateIssue::new("Linked planning issue", "Planning description");
    input.fields.insert(
        "files".into(),
        serde_json::json!([{"path":"source.rs","line":2}]),
    );
    input
        .fields
        .insert("commits".into(), serde_json::json!([commit]));
    let issue: IssueRecord = serde_json::from_value(
        repository
            .create_issue(&input, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    git(directory.path(), &["add", "."]);
    git(
        directory.path(),
        &["commit", "--quiet", "-m", "Planning fixture"],
    );
    assert!(git(directory.path(), &["status", "--porcelain"]).is_empty());
    let mut session = startup(directory.path(), 100);
    session.wait(|text| text.contains("Linked planning issue"));
    session.write(b"f");
    session
        .wait(|text| text.contains("linked source line") && !text.contains("Planning description"));
    session.write(b"\x1bOR");
    session.wait(|text| text.contains("Linked planning issue"));
    session.write(b"g");
    session.wait(|text| text.contains("show ") && text.contains(".workdeck/config.yml"));
    session.write(b"\x1bOR");
    session.wait(|text| text.contains("Linked planning issue"));
    session.write(b"e");
    session.wait(|text| text.contains("Edit issue"));
    session.write(b"\x15Retained PTY draft\x1bOQ"); // F2
    session.wait(|text| text.contains("No changes to review"));
    session.write(b"\x1bOR");
    session.wait(|text| text.contains("Retained PTY draft") && text.contains("Edit issue"));
    assert_eq!(
        repository
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .metadata
            .title,
        "Linked planning issue"
    );
    session.write(b"\x13");
    session.wait(|text| text.contains("Retained PTY draft") && !text.contains("Ctrl-S save"));
    assert_eq!(
        repository
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .metadata
            .title,
        "Retained PTY draft"
    );
    session.quit();
}

#[test]
fn uninitialized_normal_startup_is_read_only_and_keeps_review_navigation() {
    let directory = tempfile::tempdir().unwrap();
    git(directory.path(), &["init", "--quiet"]);
    let mut session = startup(directory.path(), 90);
    session.wait(|text| text.contains("F3 Issues") && text.contains("workdeck init"));
    session.write(b"\x1bOQ");
    session.wait(|text| text.contains("No changes to review"));
    session.quit();
    assert!(!directory.path().join(".agents").exists());
    assert!(!directory.path().join(".workdeck").exists());
}

fn session_cli(session: &mut Session, port: u16, args: &[&str]) -> serde_json::Value {
    let directory = session.directory.path().to_owned();
    let args = args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
    let worker = std::thread::spawn(move || {
        let output = Command::new(env!("CARGO_BIN_EXE_workdeck"))
            .arg("session")
            .args(&args)
            .current_dir(&directory)
            .env("XDG_CONFIG_HOME", directory.join("config"))
            .env("XDG_RUNTIME_DIR", directory.join("runtime"))
            .env("WORKDECK_MCP_PORT", port.to_string())
            .env("WORKDECK_MCP_DISABLE", "0")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "session {args:?}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    while !worker.is_finished() {
        assert!(Instant::now() < deadline, "session command timed out");
        session.wait_for(Duration::from_millis(25), |_| false);
    }
    worker.join().unwrap()
}

#[test]
fn broker_navigation_remains_live_while_issues_are_visible() {
    let (directory, repository) = repository();
    repository
        .create_issue(
            &CreateIssue::new("Session planning issue", "Body"),
            &RequestId::new(),
        )
        .unwrap();
    fs::write(
        directory.path().join("source.rs"),
        "first source line\nbroker target line\nlast source line\n",
    )
    .unwrap();
    let reservation = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = reservation.local_addr().unwrap().port();
    drop(reservation);
    let mut session = Session::launch_in(
        "",
        &["--no-extensions"],
        false,
        110,
        34,
        Some(port),
        Some(directory.path()),
    );
    session.wait(|text| text.contains("F3 Issues"));
    session.write(b"\x1bOR");
    session.wait(|text| text.contains("Session planning issue"));
    let listed = session_cli(&mut session, port, &["list", "--json"]);
    let id = listed["sessions"][0]["sessionId"]
        .as_str()
        .expect("mounted native review is registered");
    let navigated = session_cli(
        &mut session,
        port,
        &[
            "navigate",
            id,
            "--file",
            "source.rs",
            "--new-line",
            "2",
            "--json",
        ],
    );
    assert_eq!(navigated["result"]["filePath"], "source.rs");
    assert_eq!(navigated["result"]["line"], 2);
    session.wait(|text| text.contains("Session planning issue"));
    session.write(b"\x1bOQ");
    session.wait(|text| text.contains("broker target line"));
    session.quit();
}

#[test]
fn multiple_file_and_commit_links_allow_opening_the_second_target() {
    let (directory, repository) = repository();
    let first = git(directory.path(), &["rev-parse", "HEAD"]);
    fs::write(
        directory.path().join("second.rs"),
        "second linked file content\n",
    )
    .unwrap();
    git(directory.path(), &["add", "second.rs"]);
    git(
        directory.path(),
        &["commit", "--quiet", "-m", "Second linked commit"],
    );
    let second = git(directory.path(), &["rev-parse", "HEAD"]);
    let mut input = CreateIssue::new("Choose another target", "Body");
    input.fields.insert(
        "files".into(),
        serde_json::json!([{"path":"source.rs","line":2},{"path":"second.rs","line":1}]),
    );
    input
        .fields
        .insert("commits".into(), serde_json::json!([first, second]));
    let issue: IssueRecord = serde_json::from_value(
        repository
            .create_issue(&input, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    git(directory.path(), &["add", "."]);
    git(
        directory.path(),
        &["commit", "--quiet", "-m", "Linked choices fixture"],
    );
    let mut session = startup(directory.path(), 100);
    session.wait(|text| text.contains("Choose another target"));
    session.write(b"f");
    session.wait(|text| text.contains("Choose file") && text.contains("2. second.rs"));
    session.write(b"\x152\r");
    session.wait(|text| text.contains("second linked file content"));
    session.write(b"\x1bOR");
    session.wait(|text| text.contains("Choose another target"));
    session.write(b"g");
    session.wait(|text| text.contains("Choose commit") && text.contains(&second));
    session.write(b"\x152\r");
    session.wait(|text| text.contains("show ") && text.contains("second linked file content"));
    session.write(b"\x1bOR");
    session.wait(|text| text.contains("Choose another target"));
    assert_eq!(
        repository
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .source,
        issue.source
    );
    session.quit();
}

#[test]
fn projects_and_cycles_author_in_terminal_and_match_shared_membership() {
    use workdeck_pm::{PlanningKind, PlanningMembershipQuery};
    let (directory, repository) = repository();
    let mut session = startup(directory.path(), 110);
    session.wait(|text| text.contains("F3 Issues"));
    session.write(b"\x1b[23~"); // F11 projects; F10 retains the native menu.
    session.wait(|text| text.contains("No projects"));
    session.write(b"n");
    session.wait(|text| text.contains("Ctrl-S save"));
    session.write(b"Terminal project\tProject scope from terminal\x13");
    session.wait(|text| text.contains("Terminal project") && !text.contains("Ctrl-S save"));
    let project = repository
        .list_planning(PlanningKind::Project)
        .unwrap()
        .remove(0);
    assert_eq!(project.body, "Project scope from terminal");
    session.write(b"\x1b[24~"); // F12 cycles.
    session.wait(|text| text.contains("No cycles"));
    session.write(b"n");
    session.wait(|text| text.contains("Ctrl-S save"));
    session.write(b"Terminal cycle\tCycle intent\x13");
    session.wait(|text| text.contains("Terminal cycle") && !text.contains("Ctrl-S save"));
    let cycle = repository
        .list_planning(PlanningKind::Cycle)
        .unwrap()
        .remove(0);
    let mut milestone = workdeck_pm::CreatePlanning::new("Terminal milestone");
    milestone.id = Some("first-outcome".into());
    milestone
        .fields
        .insert("project".into(), serde_json::json!(project.metadata.id));
    repository
        .create_planning(PlanningKind::Milestone, &milestone, &RequestId::new())
        .unwrap();
    let mut input = CreateIssue::new("Member from shared API", "Membership survives archive");
    input
        .fields
        .insert("milestone".into(), serde_json::json!("first-outcome"));
    input
        .fields
        .insert("project".into(), serde_json::json!(project.metadata.id));
    input
        .fields
        .insert("cycle".into(), serde_json::json!(cycle.metadata.id));
    repository.create_issue(&input, &RequestId::new()).unwrap();
    session.write(b"r");
    session.wait(|text| text.contains("Member from shared API"));
    let members = repository
        .planning_membership(&PlanningMembershipQuery::new(
            PlanningKind::Cycle,
            &cycle.metadata.id,
        ))
        .unwrap();
    assert_eq!(members.issues.len(), 1);
    let cli_members = Command::new(env!("CARGO_BIN_EXE_workdeck"))
        .current_dir(directory.path())
        .env("XDG_CONFIG_HOME", directory.path().join("isolated-config"))
        .args(["cycle", "show", &cycle.metadata.id, "--members", "--json"])
        .output()
        .unwrap();
    assert!(
        cli_members.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_members.stderr)
    );
    let cli_members: serde_json::Value = serde_json::from_slice(&cli_members.stdout).unwrap();
    assert_eq!(
        cli_members["result"],
        serde_json::to_value(&members).unwrap()
    );
    session.write(b"\r");
    session.wait(|text| text.contains("Member from shared API") && text.contains("Issues"));
    session.write(b"\x1b[23~");
    session
        .wait(|text| text.contains("Terminal project") && text.contains("Member from shared API"));
    session.write(b"a");
    session.wait(|text| text.contains("No projects"));
    assert!(
        repository.list_planning(PlanningKind::Project).unwrap()[0]
            .metadata
            .archived
    );
    assert_eq!(
        repository.list_issues().unwrap()[0]
            .metadata
            .project
            .as_deref(),
        Some(project.metadata.id.as_str())
    );
    session.write(b"x");
    session.wait(|text| text.contains("Terminal project"));
    session.write(b"a");
    session
        .wait(|text| text.contains("Terminal project") && text.contains("Member from shared API"));
    let mut next_cycle = workdeck_pm::CreatePlanning::new("Next cycle");
    next_cycle.id = Some("next-cycle".into());
    repository
        .create_planning(PlanningKind::Cycle, &next_cycle, &RequestId::new())
        .unwrap();
    let reassigned = Command::new(env!("CARGO_BIN_EXE_workdeck"))
        .current_dir(directory.path())
        .env("XDG_CONFIG_HOME", directory.path().join("isolated-config"))
        .args([
            "issue",
            "update",
            members.issues[0].metadata.id.as_str(),
            "--cycle",
            "next-cycle",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        reassigned.status.success(),
        "{}",
        String::from_utf8_lossy(&reassigned.stderr)
    );
    session.write(b"\x1b[24~");
    session
        .wait(|text| text.contains("Terminal cycle") && !text.contains("Member from shared API"));
    let current = repository
        .show_issue(members.issues[0].metadata.id.as_str())
        .unwrap();
    assert_eq!(current.metadata.cycle.as_deref(), Some("next-cycle"));
    assert_eq!(current.metadata.milestone.as_deref(), Some("first-outcome"));
    session.quit();
    assert!(
        !repository.list_planning(PlanningKind::Project).unwrap()[0]
            .metadata
            .archived
    );
    assert!(!directory.path().join(".agents").exists());
}

#[test]
fn indexed_board_groups_navigates_edits_and_returns_to_list_in_narrow_and_wide_terminals() {
    for width in [62, 150] {
        let (directory, repository) = repository();
        for assignee in ["Alice", "Bob", "Carol"] {
            repository
                .create_issue(
                    &CreateIssue {
                        title: format!("{assignee} board task"),
                        body: format!("{assignee} exact source"),
                        fields: std::collections::BTreeMap::from([(
                            "assignee".into(),
                            serde_json::json!(assignee),
                        )]),
                    },
                    &RequestId::new(),
                )
                .unwrap();
        }
        git(directory.path(), &["add", ".workdeck"]);
        git(
            directory.path(),
            &["commit", "--quiet", "-m", "Board fixture"],
        );
        let mut session = startup(directory.path(), width);
        session.wait(|text| text.contains("F3 Issues"));
        session.write(b"\x1bOR");
        session.wait(|text| text.contains("Alice board task"));
        session.write(b"w");
        session.wait(|text| text.contains("Issues board") && text.contains("Status"));
        session.write(b"z");
        session.wait(|text| text.contains("Issues board") && text.contains("Priority"));
        session.write(b"z");
        session.wait(|text| {
            text.contains("Issues board")
                && text.contains("Assignee")
                && text.contains("Bob board task")
        });
        session.write(b"\x1b[C\r");
        session.wait(|text| text.contains("Opened source") && text.contains("---"));
        session.write(b"\x1b[6;2~");
        session.wait(|text| text.contains("Bob exact source"));
        session.write(b"e");
        session.wait(|text| text.contains("Edit issue") && text.contains("Bob board task"));
        session.write(b"\x15Edited Bob board task\x13");
        session.wait(|text| {
            text.contains("Issues board")
                && text.contains("Edited Bob board task")
                && !text.contains("Edit issue")
        });
        session.write(b"w");
        session
            .wait(|text| !text.contains("Issues board") && text.contains("Edited Bob board task"));
        let records = repository.list_issues().unwrap();
        assert!(
            records
                .iter()
                .any(|record| record.metadata.title == "Alice board task")
        );
        assert!(
            records
                .iter()
                .any(|record| record.metadata.title == "Edited Bob board task"
                    && record.metadata.assignee.as_deref() == Some("Bob"))
        );
        session.quit();
    }
}

#[test]
fn native_feature_tree_collapses_edits_and_retains_parent_navigation_in_terminal() {
    for width in [78, 180] {
        let (directory, repository) = repository();
        let mut root_input = workdeck_pm::CreateFeature::new("Zulu parent");
        root_input.body = "Root scope in tree".into();
        let root: workdeck_pm::FeatureOutcome = serde_json::from_value(
            repository
                .create_feature(&root_input, &RequestId::new())
                .unwrap()
                .result,
        )
        .unwrap();
        let mut child_input = workdeck_pm::CreateFeature::new("Alpha child");
        child_input.body = "Child scope in tree".into();
        child_input
            .fields
            .insert("parent".into(), serde_json::json!(root.record.metadata.id));
        let child: workdeck_pm::FeatureOutcome = serde_json::from_value(
            repository
                .create_feature(&child_input, &RequestId::new())
                .unwrap()
                .result,
        )
        .unwrap();
        git(directory.path(), &["add", ".workdeck"]);
        git(
            directory.path(),
            &["commit", "--quiet", "-m", "Feature tree fixture"],
        );
        let mut session = startup(directory.path(), width);
        session.wait(|text| text.contains("F3 Issues"));
        session.write(b"\x1bORv");
        session.wait(|text| text.contains("Features") && text.contains("Alpha child"));
        session.write(b"t");
        session.wait(|text| text.contains("Feature tree") && text.contains("Zulu parent"));
        session.write(b"\x1b[H");
        session.wait(|text| text.contains("Root scope in tree"));
        session.write(b"\x1b[D");
        session.wait(|text| {
            text.contains("Feature tree")
                && text.contains("Zulu parent")
                && !text.contains("Alpha child")
        });
        session.write(b"\x1b[C");
        session.wait(|text| text.contains("Feature tree") && text.contains("Alpha child"));
        session.write(b"\x1b[B");
        session.wait(|text| text.contains("Child scope in tree"));
        session.write(b"e");
        session.wait(|text| text.contains("Edit feature") && text.contains("Alpha child"));
        session.write(b"\x15Edited tree child\x13");
        session.wait(|text| {
            text.contains("Feature tree")
                && text.contains("Edited tree child")
                && !text.contains("Edit feature")
        });
        session.write(b"\x1b[D");
        session.wait(|text| text.contains("Root scope in tree"));
        session.write(b"\x1bOQ\x1bORv");
        session.wait(|text| text.contains("Feature tree") && text.contains("Root scope in tree"));
        session.write(b"t");
        session.wait(|text| {
            text.contains("Features")
                && !text.contains("Feature tree")
                && text.contains("Edited tree child")
        });
        let saved = repository
            .feature(child.record.metadata.id.as_str())
            .unwrap();
        assert_eq!(saved.metadata.name, "Edited tree child");
        assert_eq!(
            saved.metadata.parent.as_ref(),
            Some(&root.record.metadata.id)
        );
        session.quit();
    }
}

#[test]
fn native_feature_filter_retains_drafts_and_labels_empty_and_outside_parent_views() {
    for width in [78, 180] {
        let (directory, repository) = repository();
        let root: workdeck_pm::FeatureOutcome = serde_json::from_value(
            repository
                .create_feature(
                    &workdeck_pm::CreateFeature::new("Zulu parent"),
                    &RequestId::new(),
                )
                .unwrap()
                .result,
        )
        .unwrap();
        let mut input = workdeck_pm::CreateFeature::new("Alpha child");
        input
            .fields
            .insert("parent".into(), serde_json::json!(root.record.metadata.id));
        let child: workdeck_pm::FeatureOutcome = serde_json::from_value(
            repository
                .create_feature(&input, &RequestId::new())
                .unwrap()
                .result,
        )
        .unwrap();
        git(directory.path(), &["add", ".workdeck"]);
        git(
            directory.path(),
            &["commit", "--quiet", "-m", "Feature filter fixture"],
        );
        let mut session = startup(directory.path(), width);
        session.wait(|text| text.contains("F3 Issues"));
        session.write(b"\x1bORv");
        session.wait(|text| text.contains("Features") && text.contains("Alpha child"));
        session.write(b"e");
        session.wait(|text| text.contains("Edit feature"));
        session.write(b"\x15Unsaved feature title\x1b");
        session.wait(|text| !text.contains("Edit feature"));
        session.write(b"/");
        session.wait(|text| text.contains("Filter features"));
        session.write(b"no-such-capability\x13");
        session.wait(|text| {
            text.contains("No features match this filter") && !text.contains("No features yet")
        });
        session.write(b"/");
        session.wait(|text| text.contains("Filter features"));
        session.write(b"\x15Alpha\x13");
        session.wait(|text| text.contains("Alpha child") && !text.contains("Filter features"));
        session.write(b"t");
        session.wait(|text| text.contains("Feature tree") && text.contains("parent outside"));
        session.write(b"e");
        session
            .wait(|text| text.contains("Edit feature") && text.contains("Unsaved feature title"));
        session.write(b"\x13");
        session.wait(|text| {
            text.contains("No features match this filter") && !text.contains("Edit feature")
        });
        session.write(b"/");
        session.wait(|text| text.contains("Filter features"));
        session.write(b"\x15\x13");
        session.wait(|text| {
            text.contains("Feature tree")
                && text.contains("Unsaved feature title")
                && text.contains("Zulu parent")
        });
        let saved = repository
            .feature(child.record.metadata.id.as_str())
            .unwrap();
        assert_eq!(saved.metadata.name, "Unsaved feature title");
        assert_eq!(
            saved.metadata.parent.as_ref(),
            Some(&root.record.metadata.id)
        );
        assert_eq!(saved.metadata.maturity, child.record.metadata.maturity);
        session.quit();
    }
}

#[test]
fn activity_timeline_opens_source_and_restores_issue_draft_in_terminal() {
    for width in [72, 160] {
        let (directory, repository) = repository();
        repository
            .create_issue(
                &CreateIssue::new("Activity terminal task", ""),
                &RequestId::new(),
            )
            .unwrap();
        git(directory.path(), &["add", ".workdeck"]);
        git(
            directory.path(),
            &["commit", "--quiet", "-m", "Timeline fixture"],
        );
        let mut session = startup(directory.path(), width);
        session.wait(|text| text.contains("F3 Issues"));
        session.write(b"\x1bOR");
        session.wait(|text| text.contains("Activity terminal task"));
        session.write(b"e");
        session.wait(|text| text.contains("Edit issue"));
        session.write(b"\x15Retained activity draft");
        session.write(b"\x1b[24;2~");
        session.wait(|text| text.contains("newest first") && text.contains("Operation"));
        session.write(b"\r");
        session.wait(|text| text.contains("issue.create") && text.contains("operations/"));
        session.write(b"\x1bOR");
        session
            .wait(|text| text.contains("Edit issue") && text.contains("Retained activity draft"));
        session.write(b"\x1b");
        session.wait(|text| !text.contains("Edit issue"));
        session.quit();
        assert_eq!(
            repository.list_issues().unwrap()[0].metadata.title,
            "Activity terminal task"
        );
    }
}

#[test]
fn cycle_carryover_reviews_stale_membership_then_applies_a_fresh_plan_in_terminal() {
    for width in [62, 160] {
        let (directory, repository) = repository();
        for (id, name) in [("current", "A Current"), ("next", "B Next")] {
            repository
                .create_planning(
                    workdeck_pm::PlanningKind::Cycle,
                    &workdeck_pm::CreatePlanning {
                        id: Some(id.into()),
                        ..workdeck_pm::CreatePlanning::new(name)
                    },
                    &RequestId::new(),
                )
                .unwrap();
        }
        let add = |title: &str| {
            serde_json::from_value::<IssueRecord>(
                repository
                    .create_issue(
                        &CreateIssue {
                            title: title.into(),
                            body: "".into(),
                            fields: std::collections::BTreeMap::from([(
                                "cycle".into(),
                                serde_json::json!("current"),
                            )]),
                        },
                        &RequestId::new(),
                    )
                    .unwrap()
                    .result,
            )
            .unwrap()
        };
        let first = add("Original carryover task");
        git(directory.path(), &["add", ".workdeck"]);
        git(
            directory.path(),
            &["commit", "--quiet", "-m", "Carryover fixture"],
        );
        let mut session = startup(directory.path(), width);
        session.wait(|text| text.contains("F3 Issues"));
        session.write(b"\x1b[24~");
        session.wait(|text| text.contains("A Current") && text.contains("u carryover"));
        session.write(b"u");
        session
            .wait(|text| text.contains("Cycle carryover") && text.contains("Destination cycle ID"));
        session.write(b"next\x13");
        session.wait(|text| {
            text.contains("Review cycle carryover") && text.contains("Move 1 unfinished")
        });
        assert_eq!(
            repository
                .show_issue(first.metadata.id.as_str())
                .unwrap()
                .source,
            first.source
        );
        session.write(b"\x1bOQ");
        session.wait(|text| !text.contains("Review cycle carryover"));
        session.write(b"\x1b[24~");
        session.wait(|text| text.contains("Review cycle carryover"));
        let late = add("Late member");
        session.write(b"x");
        session.wait(|text| text.contains("preview changed") && text.contains("Ctrl-D"));
        assert_eq!(
            repository
                .show_issue(first.metadata.id.as_str())
                .unwrap()
                .source,
            first.source
        );
        session.write(b"\x04");
        session
            .wait(|text| !text.contains("Review cycle carryover") && text.contains("u carryover"));
        session.write(b"u");
        session.wait(|text| text.contains("Destination cycle ID"));
        session.write(b"next\x13");
        session.wait(|text| text.contains("Move 2 unfinished") && text.contains("Late member"));
        session.write(b"x");
        session.wait(|text| text.contains("Carryover saved"));
        for original in [first, late] {
            let moved = repository
                .show_issue(original.metadata.id.as_str())
                .unwrap();
            assert_eq!(moved.metadata.cycle.as_deref(), Some("next"));
            assert_eq!(moved.metadata.status, original.metadata.status);
        }
        session.write(b"\x1b");
        session.wait(|text| !text.contains("Carryover saved"));
        session.quit();
    }
}

#[test]
fn my_work_reads_registered_sources_and_preserves_an_owner_draft_in_terminal() {
    use workdeck_pm::{SourceSelector, registry::*};
    for width in [62, 160] {
        let (directory, owner) = repository();
        let target = tempfile::tempdir().unwrap();
        let target_repo = Repository::init(target.path(), "WD").unwrap();
        let mut input = CreateIssue::new("Remote assignment", "Registered terminal excerpt");
        input
            .fields
            .insert("assignee".into(), serde_json::json!("local"));
        let target_record: IssueRecord = serde_json::from_value(
            target_repo
                .create_issue(&input, &RequestId::new())
                .unwrap()
                .result,
        )
        .unwrap();
        let store = RegistryStore::open(&owner).unwrap();
        store
            .mutate(
                &RegistryRequest {
                    expected: store.snapshot().unwrap().source,
                    mutation: RegistryMutation::Register {
                        checkout: inspect_checkout(
                            "secondary",
                            target.path(),
                            SourceSelector::WorkingTree,
                        )
                        .unwrap(),
                    },
                },
                &RequestId::new(),
            )
            .unwrap();
        let mut session = startup(directory.path(), width);
        session.wait(|text| text.contains("F3 Issues"));
        session.write(b"\x1bORn");
        session.wait(|text| text.contains("Create issue"));
        session.write(b"Owner retained draft");
        session.write(b"\x1b[20;2~");
        session.wait(|text| {
            text.contains("My work")
                && text.contains("secondary")
                && text.contains("Remote assignment")
        });
        session.write(b"\r");
        session.wait(|text| text.contains("issues/") && text.contains("Local"));
        if width < 110 {
            session.write(b"\x1b[6;2~");
        }
        session.wait(|text| text.contains("Registered terminal excerpt"));
        fs::write(target_repo.root().join(target_record.path), "broken issue").unwrap();
        session.write(b"r");
        session
            .wait(|text| text.contains("partial") && text.contains("Registered terminal excerpt"));
        session.write(b"s");
        session
            .wait(|text| text.contains("Registered sources") && text.contains("unavailable/stale"));
        session.write(b"\x1bOQ");
        session.wait(|text| !text.contains("Registered sources"));
        session.write(b"\x1b[20;2~");
        session
            .wait(|text| text.contains("Registered sources") && text.contains("unavailable/stale"));
        session.write(b"\x1bOR");
        session.wait(|text| text.contains("Create issue") && text.contains("Owner retained draft"));
        session.write(b"\x13");
        session
            .wait(|text| !text.contains("Create issue") && text.contains("Owner retained draft"));
        session.quit();
        assert_eq!(
            owner.list_issues().unwrap()[0].metadata.title,
            "Owner retained draft"
        );
        assert!(!target_repo.root().join(".local/repositories").exists());
    }
}

#[test]
fn registered_checkout_switch_preserves_drafts_and_review_roots_in_terminal() {
    use workdeck_pm::{SourceSelector, registry::*};
    for width in [62, 160] {
        let (directory, owner) = repository();
        let (target, target_repo) = repository();
        fs::write(
            target.path().join("source.rs"),
            "Target checkout file snapshot\nSelected source detail\n",
        )
        .unwrap();
        let mut input = CreateIssue::new("Target source task", "Only in the selected checkout");
        input.fields.insert(
            "files".into(),
            serde_json::json!([{"path":"source.rs","line":1}]),
        );
        target_repo.create_issue(&input, &RequestId::new()).unwrap();
        git(target.path(), &["add", "."]);
        git(
            target.path(),
            &["commit", "--quiet", "-m", "Target fixture"],
        );
        let store = RegistryStore::open(&owner).unwrap();
        store
            .mutate(
                &RegistryRequest {
                    expected: store.snapshot().unwrap().source,
                    mutation: RegistryMutation::Register {
                        checkout: inspect_checkout(
                            "secondary",
                            target.path(),
                            SourceSelector::WorkingTree,
                        )
                        .unwrap(),
                    },
                },
                &RequestId::new(),
            )
            .unwrap();
        let nested_cwd = directory.path().join("nested");
        fs::create_dir(&nested_cwd).unwrap();
        let mut session = startup(&nested_cwd, width);
        session.wait(|text| text.contains("F3 Issues"));
        session.write(b"\x1bORn");
        session.wait(|text| text.contains("Create issue"));
        session.write(b"Owner switch draft");
        session.write(b"\x1b[20;2~");
        session.wait(|text| text.contains("My work") && text.contains("o open source"));
        session.write(b"s");
        session.wait(|text| text.contains("Registered sources") && text.contains("secondary"));
        session.write(b"o");
        session.wait(|text| !text.contains("My work") && text.contains("Target source task"));
        session.write(b"f");
        session.wait(|text| text.contains("Target checkout file snapshot"));
        session.write(b"\x1b[20;2~");
        session.wait(|text| text.contains("My work"));
        session.write(b"b");
        session.wait(|text| text.contains("Create issue") && text.contains("Owner switch draft"));
        session.write(b"\x1b[20;2~");
        session.wait(|text| text.contains("My work"));
        session.write(b"b");
        session.wait(|text| {
            !text.contains("My work") && text.contains("Target checkout file snapshot")
        });
        session.write(b"\x1bOQ");
        session.wait(|text| {
            text.contains("Target checkout file snapshot") && !text.contains("first source line")
        });
        session.write(b"\x1bOR");
        session.wait(|text| {
            text.contains("Target source task") && !text.contains("Target checkout file snapshot")
        });
        session.write(b"n");
        session.wait(|text| text.contains("Create issue"));
        session.write(b"Target switch draft");
        session.write(b"\x1b[20;2~");
        session.wait(|text| text.contains("My work") && text.contains("secondary"));
        session.write(b"b");
        session.wait(|text| text.contains("Create issue") && text.contains("Owner switch draft"));
        session.write(b"\x13");
        session.wait(|text| !text.contains("Create issue") && text.contains("Owner switch draft"));
        session.write(b"\x1b[20;2~");
        session.wait(|text| text.contains("My work"));
        session.write(b"b");
        session.wait(|text| text.contains("Create issue") && text.contains("Target switch draft"));
        session.write(b"\x13");
        session.wait(|text| !text.contains("Create issue") && text.contains("Target switch draft"));
        session.write(b"\x1bOQ");
        session
            .wait(|text| !text.contains("Create issue") && text.contains("No changes to review"));
        session.quit();
        assert_eq!(owner.list_issues().unwrap().len(), 1);
        assert_eq!(
            owner.list_issues().unwrap()[0].metadata.title,
            "Owner switch draft"
        );
        let titles = target_repo
            .list_issues()
            .unwrap()
            .into_iter()
            .map(|issue| issue.metadata.title)
            .collect::<Vec<_>>();
        assert!(titles.contains(&"Target switch draft".to_owned()));
        assert!(!titles.contains(&"Owner switch draft".to_owned()));
        assert!(!target_repo.root().join(".local/repositories").exists());
    }
}

#[test]
fn my_work_review_and_overdue_facets_preserve_native_drafts_in_narrow_and_wide_terminals() {
    use workdeck_pm::{SourceSelector, registry::*};
    for width in [62, 160] {
        let (directory, repository) = repository();
        for (title, fields) in [
            (
                "Requested review task",
                serde_json::json!({"reviewer":"local","status":"in_review","assignee":"other"}),
            ),
            (
                "Overdue assigned task",
                serde_json::json!({"assignee":"local","due_at":"2000-01-01"}),
            ),
        ] {
            let mut input = CreateIssue::new(title, "Retained source body");
            input.fields = serde_json::from_value(fields).unwrap();
            repository.create_issue(&input, &RequestId::new()).unwrap();
        }
        let store = RegistryStore::open(&repository).unwrap();
        store
            .mutate(
                &RegistryRequest {
                    expected: store.snapshot().unwrap().source,
                    mutation: RegistryMutation::Register {
                        checkout: inspect_checkout(
                            "self",
                            directory.path(),
                            SourceSelector::WorkingTree,
                        )
                        .unwrap(),
                    },
                },
                &RequestId::new(),
            )
            .unwrap();
        let mut session = startup(directory.path(), width);
        session.wait(|text| text.contains("F3 Issues"));
        session.write(b"\x1bORn");
        session.wait(|text| text.contains("Create issue"));
        session.write(b"Draft across work facets");
        session.write(b"\x1b[20;2~");
        session.wait(|text| text.contains("My work") && text.contains("Overdue assigned task"));
        session.write(b"2");
        session.wait(|text| {
            text.contains("Review requests")
                && text.contains("Requested review task")
                && !text.contains("Overdue assigned task")
        });
        session.write(b"3");
        session.wait(|text| {
            text.contains("My work · Overdue")
                && text.contains("Overdue assigned task")
                && !text.contains("Requested review task")
        });
        session.write(b"1");
        session.wait(|text| text.contains("Assignments") && text.contains("Overdue assigned task"));
        session.write(b"\x1bOR");
        session.wait(|text| {
            text.contains("Create issue") && text.contains("Draft across work facets")
        });
        session.write(b"\x13");
        session.wait(|text| {
            !text.contains("Create issue") && text.contains("Draft across work facets")
        });
        session.quit();
        assert_eq!(repository.list_issues().unwrap().len(), 3);
    }
}

#[test]
fn my_work_blocked_evidence_explains_prerequisites_in_narrow_and_wide_terminals() {
    use workdeck_pm::{SourceSelector, registry::*};
    for width in [62, 160] {
        let (directory, repository) = repository();
        let prerequisite: IssueRecord = serde_json::from_value(
            repository
                .create_issue(
                    &CreateIssue::new("Prerequisite task", "Contract"),
                    &RequestId::new(),
                )
                .unwrap()
                .result,
        )
        .unwrap();
        let mut input = CreateIssue::new("Blocked source task", "Dependent contract");
        input.fields = serde_json::from_value(
            serde_json::json!({"assignee":"local", "prerequisites":[prerequisite.metadata.id]}),
        )
        .unwrap();
        repository.create_issue(&input, &RequestId::new()).unwrap();
        let store = RegistryStore::open(&repository).unwrap();
        store
            .mutate(
                &RegistryRequest {
                    expected: store.snapshot().unwrap().source,
                    mutation: RegistryMutation::Register {
                        checkout: inspect_checkout(
                            "self",
                            directory.path(),
                            SourceSelector::WorkingTree,
                        )
                        .unwrap(),
                    },
                },
                &RequestId::new(),
            )
            .unwrap();
        let mut session = startup(directory.path(), width);
        session.wait(|text| text.contains("F3 Issues"));
        session.write(b"\x1bORn");
        session.wait(|text| text.contains("Create issue"));
        session.write(b"Draft beside blockers");
        session.write(b"\x1b[20;2~");
        session.wait(|text| text.contains("My work"));
        session.write(b"4");
        session.wait(|text| {
            text.contains("My work · Blocked") && text.contains("Blocked source task")
        });
        session.write(b"e");
        session.wait(|text| {
            text.contains("Work evidence") && text.contains("incomplete_prerequisite")
        });
        session.write(b"\x1bOR");
        session
            .wait(|text| text.contains("Create issue") && text.contains("Draft beside blockers"));
        session.write(b"\x13");
        session
            .wait(|text| !text.contains("Create issue") && text.contains("Draft beside blockers"));
        session.quit();
        assert_eq!(repository.list_issues().unwrap().len(), 3);
    }
}

#[test]
fn my_work_claimed_evidence_keeps_drafts_and_scope_visible_in_narrow_and_wide_terminals() {
    use workdeck_pm::{AcquireClaim, ClaimRequest, SourceSelector, registry::*};
    for width in [62, 160] {
        let (directory, repository) = repository();
        let issue: IssueRecord = serde_json::from_value(
            repository
                .create_issue(
                    &CreateIssue::new("Claimed source task", "Contract"),
                    &RequestId::new(),
                )
                .unwrap()
                .result,
        )
        .unwrap();
        repository
            .mutate_local_claim(
                &ClaimRequest::Acquire {
                    input: Box::new(AcquireClaim {
                        actor: "local".into(),
                        contract: repository.local_claim_contract(&issue.metadata.id).unwrap(),
                        ttl_seconds: None,
                        recovery: None,
                    }),
                },
                &RequestId::new(),
            )
            .unwrap();
        let store = RegistryStore::open(&repository).unwrap();
        store
            .mutate(
                &RegistryRequest {
                    expected: store.snapshot().unwrap().source,
                    mutation: RegistryMutation::Register {
                        checkout: inspect_checkout(
                            "self",
                            directory.path(),
                            SourceSelector::WorkingTree,
                        )
                        .unwrap(),
                    },
                },
                &RequestId::new(),
            )
            .unwrap();
        let mut session = startup(directory.path(), width);
        session.wait(|text| text.contains("F3 Issues"));
        session.write(b"\x1bORn");
        session.wait(|text| text.contains("Create issue"));
        session.write(b"Draft beside claims");
        session.write(b"\x1b[20;2~");
        session.wait(|text| text.contains("My work"));
        session.write(b"5");
        session.wait(|text| {
            text.contains("My work · Claimed") && text.contains("Claimed source task")
        });
        session.write(b"e");
        session.wait(|text| text.contains("Work evidence") && text.contains("local source only"));
        session.write(b"\x1bOR");
        session.wait(|text| text.contains("Create issue") && text.contains("Draft beside claims"));
        session.write(b"\x13");
        session.wait(|text| !text.contains("Create issue") && text.contains("Draft beside claims"));
        session.quit();
        assert_eq!(repository.list_issues().unwrap().len(), 2);
    }
}
