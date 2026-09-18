use serde_json::{Value, json};
use std::fs;
use tempfile::TempDir;
use workdeck_pm::*;

fn setup() -> (TempDir, Repository) {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    (temp, repository)
}
fn create(repository: &Repository, title: &str, fields: Value) -> IssueRecord {
    serde_json::from_value(
        repository
            .create_issue(
                &CreateIssue {
                    title: title.into(),
                    body: "Scope café 日本語 \"quotes\"\nDo this deliberately.".into(),
                    fields: serde_json::from_value(fields).unwrap(),
                },
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap()
}

#[test]
fn next_empty_is_explicit_and_selection_is_stable_ready_only() {
    let (_temp, repo) = setup();
    let empty = repo.next_issue(&NextIssueRequest::default()).unwrap();
    assert!(empty.selected.is_none());
    assert_eq!(empty.total, 0);
    let low = create(&repo, "Low", json!({"status":"ready","priority":"low"}));
    let urgent = create(
        &repo,
        "Urgent",
        json!({"status":"ready","priority":"urgent"}),
    );
    let archived = create(
        &repo,
        "Archived",
        json!({"status":"ready","priority":"urgent"}),
    );
    repo.archive_issue(
        archived.metadata.id.as_str(),
        &archived.source,
        true,
        &RequestId::new(),
    )
    .unwrap();
    create(
        &repo,
        "Started",
        json!({"status":"in_progress","priority":"urgent"}),
    );
    let selected = repo.next_issue(&NextIssueRequest::default()).unwrap();
    assert_eq!(selected.selected.unwrap().issue, urgent.metadata.id);
    assert_eq!(selected.eligible, 2);
    assert!(
        selected
            .candidates
            .iter()
            .any(|c| c.issue == low.metadata.id && c.eligible)
    );
    assert!(
        selected
            .candidates
            .iter()
            .all(|c| c.issue != archived.metadata.id || !c.eligible)
    );
}

#[test]
fn packet_enforces_exact_utf8_json_budget_and_reports_whole_entry_omissions() {
    let (_temp, repo) = setup();
    let issue = create(
        &repo,
        "Budget 日本語 \"quoted\"",
        json!({"acceptance":[{"id":"one","description":"café \"quote\" 日本語","checked":false}]}),
    );
    let full = repo
        .context(&ContextRequest::new(issue.metadata.id.as_str(), 32_000))
        .unwrap();
    let bytes = serde_json::to_vec(&full).unwrap();
    assert_eq!(bytes.len(), full.budget.used_bytes);
    assert!(bytes.len() <= 32_000);
    let minimum = full.budget.minimum_bytes;
    let exact = repo
        .context(&ContextRequest::new(issue.metadata.id.as_str(), minimum))
        .unwrap();
    assert!(serde_json::to_vec(&exact).unwrap().len() <= minimum);
    assert!(exact.budget.omitted_entries > 0);
    assert_eq!(
        exact.budget.omitted_entries,
        exact.sections.iter().map(|s| s.omitted).sum::<usize>()
    );
    let error = repo
        .context(&ContextRequest::new(issue.metadata.id.as_str(), 1))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert!(
        error.details.unwrap()["minimum_required_bytes"]
            .as_u64()
            .unwrap()
            > 1
    );
    assert_eq!(
        full,
        repo.context(&ContextRequest::new(issue.metadata.id.as_str(), 32_000))
            .unwrap()
    );
}

#[test]
fn stale_context_is_rejected_and_requirements_do_not_include_checked_declarations() {
    let (_temp, repo) = setup();
    let issue = create(
        &repo,
        "Original",
        json!({"acceptance":[{"id":"one","description":"Do the thing","checked":false}]}),
    );
    let old = repo
        .context(&ContextRequest::new(issue.metadata.id.as_str(), 32_000))
        .unwrap();
    let path = repo.root().join(&issue.path);
    let mut metadata = issue.metadata.clone();
    metadata.acceptance[0].checked = true;
    fs::write(
        &path,
        format!(
            "---\n{}---\n{}",
            serde_yaml_ng::to_string(&metadata).unwrap(),
            issue.body
        ),
    )
    .unwrap();
    assert!(
        repo.show_issue(issue.metadata.id.as_str())
            .unwrap()
            .metadata
            .acceptance[0]
            .checked
    );
    let new = repo
        .context(&ContextRequest::new(issue.metadata.id.as_str(), 32_000))
        .unwrap();
    assert_eq!(old.anchor.requirements, new.anchor.requirements);
    assert_ne!(old.anchor.fingerprint, new.anchor.fingerprint);
    let error = repo
        .next_actions(&NextActionRequest {
            issue: issue.metadata.id.to_string(),
            expected_context: Some(old.anchor.fingerprint),
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
}

#[test]
fn hidden_or_missing_prerequisites_never_produce_implementation_actions() {
    let (_temp, repo) = setup();
    let dependency = create(&repo, "Hidden", json!({"status":"ready"}));
    let issue = create(
        &repo,
        "Visible",
        json!({"status":"ready","prerequisites":[dependency.metadata.id]}),
    );
    let request = NextIssueRequest {
        query: IssueQuery {
            query: "Visible".into(),
            sort: Vec::new(),
            ..IssueQuery::default()
        },
        ..NextIssueRequest::default()
    };
    let selection = repo.next_issue(&request).unwrap();
    assert!(selection.selected.is_none());
    assert_eq!(selection.excluded, 1);
    let actions = repo
        .next_actions(&NextActionRequest::new(issue.metadata.id.as_str()))
        .unwrap();
    assert!(
        !actions
            .actions
            .iter()
            .any(|a| a.kind == NextActionKind::Implement && a.available)
    );
    assert!(
        actions
            .actions
            .iter()
            .any(|a| a.kind == NextActionKind::ResolvePrerequisite)
    );
    fs::remove_file(repo.root().join(dependency.path)).unwrap();
    let actions = repo
        .next_actions(&NextActionRequest::new(issue.metadata.id.as_str()))
        .unwrap();
    assert!(
        actions
            .conditions
            .iter()
            .any(|c| c.state == ConditionState::Unknown)
    );
}

fn entries(packet: &ContextPacket) -> impl Iterator<Item = &ContextContent> {
    packet
        .sections
        .iter()
        .flat_map(|s| s.entries.iter().map(|e| &e.content))
}

fn document_section(packet: &ContextPacket) -> Value {
    serde_json::to_value(packet).unwrap()["sections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|section| section["kind"] == "documents")
        .expect("context must expose declared document references")
        .clone()
}

#[test]
fn document_references_are_inert_literals_with_declaration_provenance() {
    let (temp, repo) = setup();
    fs::create_dir(temp.path().join("docs")).unwrap();
    let local_document = temp.path().join("docs/spec.md");
    fs::write(&local_document, "DOCUMENT_CONTENT_MUST_NOT_BE_FETCHED").unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let references = vec![
        "docs/spec.md".to_string(),
        format!(
            "http://{}/design?query=café&name=\"日本語\"",
            listener.local_addr().unwrap()
        ),
        "file:///outside/repository.md".to_string(),
        "../private.md".to_string(),
        "Design note: café 日本語".to_string(),
    ];
    let issue = create(&repo, "Declared documents", json!({"documents":references}));
    let request = ContextRequest::new(issue.metadata.id.as_str(), 64 * 1024);
    let packet = repo.context(&request).unwrap();
    let section = document_section(&packet);
    assert_eq!(section["total"], references.len());
    assert_eq!(section["omitted"], 0);
    for (entry, reference) in section["entries"]
        .as_array()
        .unwrap()
        .iter()
        .zip(&references)
    {
        assert_eq!(
            entry["content"],
            json!({
                "kind":"document", "reference":reference, "reason_code":"document_not_fetched"
            })
        );
        assert_eq!(
            entry["citations"],
            json!([{
                "target":{"kind":"issue","id":issue.metadata.id}, "source":issue.source.content
            }])
        );
    }
    assert!(
        !serde_json::to_string(&packet)
            .unwrap()
            .contains("DOCUMENT_CONTENT_MUST_NOT_BE_FETCHED")
    );
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    // Unread bytes are not claimed as part of the durable context identity.
    fs::write(&local_document, "Different unread bytes").unwrap();
    let again = repo.context(&request).unwrap();
    assert_eq!(packet.anchor, again.anchor);
    assert_eq!(packet, again);
    assert_eq!(
        serde_json::to_vec(&packet).unwrap().len(),
        packet.budget.used_bytes
    );
}

#[test]
fn document_references_have_bounded_capture_and_exact_budget_omissions() {
    let (_temp, repo) = setup();
    let references = (0..1027)
        .map(|i| format!("docs/{i}-café-日本語-\"quoted\".md"))
        .collect::<Vec<_>>();
    let issue = create(&repo, "Many documents", json!({"documents":references}));
    let full = repo
        .context(&ContextRequest::new(
            issue.metadata.id.as_str(),
            MAX_CONTEXT_BUDGET_BYTES,
        ))
        .unwrap();
    let section = document_section(&full);
    assert_eq!(section["total"], 1027);
    assert_eq!(section["entries"].as_array().unwrap().len(), 1024);
    assert_eq!(section["omitted"], 3);
    assert_eq!(section["coverage_complete"], true);
    assert_eq!(section["omission_reasons"], json!(["capture_limit"]));
    let small = repo
        .context(&ContextRequest::new(
            issue.metadata.id.as_str(),
            full.budget.minimum_bytes,
        ))
        .unwrap();
    let section = document_section(&small);
    assert_eq!(section["total"], 1027);
    assert_eq!(section["omitted"], 1027);
    assert!(section["entries"].as_array().unwrap().is_empty());
    assert_eq!(
        section["omission_reasons"],
        json!(["capture_limit", "budget"])
    );
    assert_eq!(small.anchor, full.anchor);
    assert_eq!(
        serde_json::to_vec(&small).unwrap().len(),
        small.budget.used_bytes
    );
    assert!(small.budget.used_bytes <= small.budget.limit_bytes);
}

#[cfg(unix)]
#[test]
fn document_reference_fifo_child() {
    let Some(root) = std::env::var_os("WORKDECK_CONTEXT_DOCUMENT_FIFO_ROOT") else {
        return;
    };
    let repo = Repository::open_source(std::path::Path::new(&root)).unwrap();
    let issue = repo.list_issues().unwrap().remove(0);
    let packet = repo
        .context(&ContextRequest::new(issue.metadata.id.as_str(), 64 * 1024))
        .unwrap();
    let section = document_section(&packet);
    assert_eq!(section["entries"].as_array().unwrap().len(), 2);
    assert!(
        section["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["content"]["reason_code"] == "document_not_fetched")
    );
}

#[cfg(unix)]
#[test]
fn document_references_do_not_open_fifos_or_symlinks() {
    let (temp, repo) = setup();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(temp.path().join("document-pipe"))
            .status()
            .unwrap()
            .success()
    );
    std::os::unix::fs::symlink("document-pipe", temp.path().join("document-link")).unwrap();
    create(
        &repo,
        "Inert document files",
        json!({"documents":["document-pipe","document-link"]}),
    );
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "document_reference_fifo_child", "--nocapture"])
        .env("WORKDECK_CONTEXT_DOCUMENT_FIFO_ROOT", repo.root())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "child status {status}");
            break;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("document reference tried to open a FIFO");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn scoped_source_and_instruction_bytes_are_pinned_and_revalidated() {
    let (temp, repo) = setup();
    fs::create_dir_all(temp.path().join("src/deep")).unwrap();
    fs::write(temp.path().join("AGENTS.md"), "Repository instruction").unwrap();
    fs::write(temp.path().join("src/AGENTS.md"), "Scoped instruction").unwrap();
    fs::write(temp.path().join("src/deep/code.rs"), "first\nsecond\nthird").unwrap();
    let issue = create(
        &repo,
        "Sources",
        json!({"files":[{"path":"src/deep/code.rs","line":2,"end_line":2}]}),
    );
    let request = ContextRequest::new(issue.metadata.id.as_str(), 64 * 1024);
    let packet = repo.context(&request).unwrap();
    assert!(
        entries(&packet)
            .any(|e| matches!(e,ContextContent::Source {excerpt:Some(s),..} if s=="second"))
    );
    assert_eq!(
        entries(&packet)
            .filter(|e| matches!(e, ContextContent::Instruction { .. }))
            .count(),
        2
    );
    let error = repo
        .context_with_faults(&request, |_| {
            fs::write(temp.path().join("src/AGENTS.md"), "Edited during capture").unwrap();
            Ok(())
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    let new = repo.context(&request).unwrap();
    assert_ne!(packet.anchor.fingerprint, new.anchor.fingerprint);
    assert_eq!(packet.anchor.requirements, new.anchor.requirements);
    let error = repo
        .context_with_faults(&request, |_| {
            let path = repo.root().join(&issue.path);
            fs::write(
                &path,
                fs::read_to_string(&path)
                    .unwrap()
                    .replace("Sources", "Changed directly"),
            )
            .unwrap();
            Ok(())
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
}

#[test]
fn explicit_alternate_planning_root_does_not_read_parent_instructions() {
    let (temp, repo) = setup();
    let issue = create(&repo, "Detached", json!({"files":[{"path":"secret.txt"}]}));
    let other = temp.path().join("planning-data");
    fs::rename(repo.root(), &other).unwrap();
    fs::write(temp.path().join("AGENTS.md"), "MUST_NOT_APPEAR").unwrap();
    fs::write(temp.path().join("secret.txt"), "PRIVATE_DETACHED_SOURCE").unwrap();
    let detached = Repository::open_source(&other).unwrap();
    let packet = detached
        .context(&ContextRequest::new(issue.metadata.id.as_str(), 64 * 1024))
        .unwrap();
    let output = serde_json::to_string(&packet).unwrap();
    assert!(output.contains("worktree_unbound"));
    assert!(!output.contains("MUST_NOT_APPEAR"));
    assert!(!output.contains("PRIVATE_DETACHED_SOURCE"));
}

#[cfg(unix)]
#[test]
fn static_fifo_symlink_and_oversized_sources_are_inert_and_omitted() {
    use std::os::unix::fs::symlink;
    let (temp, repo) = setup();
    fs::write(temp.path().join("outside.txt"), "DO_NOT_FOLLOW").unwrap();
    symlink("outside.txt", temp.path().join("link.txt")).unwrap();
    symlink("outside.txt", temp.path().join("AGENTS.md")).unwrap();
    let fifo = temp.path().join("pipe");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    fs::write(temp.path().join("big.txt"), vec![b'a'; 256 * 1024 + 1]).unwrap();
    fs::write(temp.path().join(".env"), "NEVER_COPY_SECRET").unwrap();
    let issue = create(
        &repo,
        "Unsafe",
        json!({"files":[{"path":"pipe"},{"path":"link.txt"},{"path":"big.txt"},{"path":".env"}]}),
    );
    let began = std::time::Instant::now();
    let packet = repo
        .context(&ContextRequest::new(issue.metadata.id.as_str(), 64 * 1024))
        .unwrap();
    assert!(began.elapsed() < std::time::Duration::from_secs(3));
    let output = serde_json::to_string(&packet).unwrap();
    for expected in [
        "non_regular_source",
        "unsafe_or_unreadable_source",
        "source_size_limit",
        "private_or_configuration_source",
    ] {
        assert!(output.contains(expected), "{expected}");
    }
    assert!(!output.contains("DO_NOT_FOLLOW"));
    assert!(!output.contains("NEVER_COPY_SECRET"));
}

#[cfg(unix)]
#[test]
fn replaced_source_parent_is_rejected_without_reading_the_replacement_target() {
    use std::os::unix::fs::symlink;
    let (temp, repo) = setup();
    fs::create_dir(temp.path().join("src")).unwrap();
    fs::write(temp.path().join("src/code.rs"), "original").unwrap();
    let outside = TempDir::new().unwrap();
    fs::write(outside.path().join("code.rs"), "external").unwrap();
    let issue = create(&repo, "Parent", json!({"files":[{"path":"src/code.rs"}]}));
    let error = repo
        .context_with_faults(
            &ContextRequest::new(issue.metadata.id.as_str(), 64 * 1024),
            |_| {
                fs::rename(temp.path().join("src"), temp.path().join("old-src")).unwrap();
                symlink(outside.path(), temp.path().join("src")).unwrap();
                Ok(())
            },
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    assert_eq!(
        fs::read_to_string(outside.path().join("code.rs")).unwrap(),
        "external"
    );
}

#[test]
fn question_answers_and_supersession_control_selection_from_current_applicability() {
    let (_temp, repo) = setup();
    let issue = create(
        &repo,
        "Question",
        json!({"acceptance":[{"id":"one","description":"Current requirement","checked":false}]}),
    );
    let criterion = repo
        .resolve_criterion(&CriterionOwner::Issue(issue.metadata.id.clone()), "one")
        .unwrap()
        .reference;
    let input = CreateQuestion {
        actor: "agent".into(),
        body: "Which behavior is intended?".into(),
        subjects: vec![QuestionSubject {
            subject: SubjectRef::Issue(issue.metadata.id.clone()),
            source: issue.source.clone(),
        }],
        requirements: vec![criterion],
        blocks_work: true,
        custom: Default::default(),
        extra: Default::default(),
    };
    let record: QuestionMutationResult = serde_json::from_value(
        repo.create_question(&input, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    assert!(
        repo.next_issue(&NextIssueRequest::default())
            .unwrap()
            .selected
            .is_none()
    );
    let packet = repo
        .context(&ContextRequest::new(issue.metadata.id.as_str(), 64 * 1024))
        .unwrap();
    assert!(entries(&packet).any(|e|matches!(e,ContextContent::Question {applicability,..} if applicability.blocks_implementation)));
    let answered: QuestionMutationResult = serde_json::from_value(
        repo.mutate_question(
            &record.question.metadata.id,
            &record.question.source,
            &QuestionMutation::Answer {
                actor: "human".into(),
                body: "Use the documented behavior".into(),
                decision_refs: Vec::new(),
            },
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    assert!(
        repo.next_issue(&NextIssueRequest::default())
            .unwrap()
            .selected
            .is_some()
    );
    let path = repo.root().join(&issue.path);
    fs::write(
        &path,
        fs::read_to_string(&path)
            .unwrap()
            .replace("Current requirement", "Changed requirement"),
    )
    .unwrap();
    let selection = repo.next_issue(&NextIssueRequest::default()).unwrap();
    assert!(selection.selected.is_none());
    assert!(
        selection.candidates[0]
            .reason_codes
            .contains(&"stale_decision".to_owned())
    );
    let current = repo.show_issue(issue.metadata.id.as_str()).unwrap();
    let replacement: QuestionMutationResult = serde_json::from_value(
        repo.create_question(
            &CreateQuestion {
                subjects: vec![QuestionSubject {
                    subject: SubjectRef::Issue(issue.metadata.id.clone()),
                    source: current.source,
                }],
                requirements: Vec::new(),
                blocks_work: false,
                ..input
            },
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    repo.mutate_question(
        &answered.question.metadata.id,
        &answered.question.source,
        &QuestionMutation::Supersede {
            actor: "human".into(),
            reason: "Explicitly replaced by a current nonblocking question".into(),
            replacement: replacement.question.metadata.id,
            replacement_source: replacement.question.source,
        },
        &RequestId::new(),
    )
    .unwrap();
    assert!(
        repo.next_issue(&NextIssueRequest::default())
            .unwrap()
            .selected
            .is_some()
    );
}

#[test]
fn new_handoff_does_not_stale_itself_and_resume_labels_changed_sources() {
    let (_temp, repo) = setup();
    let issue = create(&repo, "Continuity", json!({}));
    let request = ContextRequest::new(issue.metadata.id.as_str(), 64 * 1024);
    let initial = repo.context(&request).unwrap();
    let input = CreateHandoff {
        actor: "agent".into(),
        anchor: initial.anchor.clone(),
        body: "Attempted the parser repair".into(),
        attempted: vec!["Read parser".into()],
        uncertainties: vec!["Result has not been verified".into()],
        evidence_refs: Vec::new(),
        questions: Vec::new(),
        pending_operations: Vec::new(),
        next_steps: vec!["Inspect the test contract".into()],
        custom: Default::default(),
        extra: Default::default(),
    };
    repo.create_handoff(&input, &RequestId::new()).unwrap();
    let resumed = repo.context(&request).unwrap();
    assert_eq!(initial.anchor, resumed.anchor);
    assert!(entries(&resumed).any(|e| matches!(
        e,
        ContextContent::Handoff {
            freshness: ContextFreshness::Current,
            ..
        }
    )));
    let path = repo.root().join(issue.path);
    fs::write(
        &path,
        fs::read_to_string(&path)
            .unwrap()
            .replace("Continuity", "Changed scope"),
    )
    .unwrap();
    let changed = repo.context(&request).unwrap();
    assert!(entries(&changed).any(|e|matches!(e,ContextContent::Handoff {freshness:ContextFreshness::Stale,reason_codes,..} if reason_codes.contains(&"context_changed".into()))));
    assert_eq!(
        repo.create_handoff(&input, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
}

#[test]
fn next_pagination_is_source_bound_and_custom_sort_is_explicitly_rejected() {
    let (_temp, repo) = setup();
    for index in 0..3 {
        create(&repo, &format!("Issue {index}"), json!({}));
    }
    let first = repo
        .next_issue(&NextIssueRequest {
            limit: 1,
            ..NextIssueRequest::default()
        })
        .unwrap();
    let second = repo
        .next_issue(&NextIssueRequest {
            limit: 1,
            cursor: first.next_cursor.clone(),
            ..NextIssueRequest::default()
        })
        .unwrap();
    assert_ne!(first.candidates[0].issue, second.candidates[0].issue);
    create(&repo, "New source", json!({}));
    assert_eq!(
        repo.next_issue(&NextIssueRequest {
            limit: 1,
            cursor: first.next_cursor,
            ..NextIssueRequest::default()
        })
        .unwrap_err()
        .code,
        ErrorCode::StaleSource
    );
    assert_eq!(
        repo.next_issue(&NextIssueRequest {
            query: IssueQuery::default(),
            ..NextIssueRequest::default()
        })
        .unwrap_err()
        .code,
        ErrorCode::InvalidInput
    );
}

#[test]
fn source_excerpts_report_truncation_and_do_not_cross_nested_git_boundaries() {
    let (temp, repo) = setup();
    fs::write(
        temp.path().join("long.rs"),
        (0..100).map(|i| format!("Line {i}\n")).collect::<String>(),
    )
    .unwrap();
    fs::create_dir_all(temp.path().join("nested/.git")).unwrap();
    fs::write(
        temp.path().join("nested/code.rs"),
        "FOREIGN_REPOSITORY_SOURCE",
    )
    .unwrap();
    fs::write(
        temp.path().join("nested/AGENTS.md"),
        "FOREIGN_REPOSITORY_INSTRUCTIONS",
    )
    .unwrap();
    let issue = create(
        &repo,
        "Boundaries",
        json!({"files":[{"path":"long.rs"},{"path":"nested/code.rs"}]}),
    );
    let packet = repo
        .context(&ContextRequest::new(issue.metadata.id.as_str(), 64 * 1024))
        .unwrap();
    let output = serde_json::to_string(&packet).unwrap();
    assert!(output.contains("excerpt_truncated"));
    assert!(!output.contains("FOREIGN_REPOSITORY_SOURCE"));
    assert!(!output.contains("FOREIGN_REPOSITORY_INSTRUCTIONS"));
}

#[cfg(unix)]
#[test]
fn fifo_capture_child() {
    let Some(root) = std::env::var_os("WORKDECK_CONTEXT_FIFO_TEST_ROOT") else {
        return;
    };
    let repo = Repository::open_source(std::path::Path::new(&root)).unwrap();
    let issue = repo.list_issues().unwrap().remove(0);
    let packet = repo
        .context(&ContextRequest::new(issue.metadata.id.as_str(), 64 * 1024))
        .unwrap();
    assert!(
        serde_json::to_string(&packet)
            .unwrap()
            .contains("non_regular_source")
    );
}

#[cfg(unix)]
#[test]
fn fifo_capture_is_bounded_in_a_separate_process() {
    let (temp, repo) = setup();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(temp.path().join("pipe"))
            .status()
            .unwrap()
            .success()
    );
    create(&repo, "FIFO subprocess", json!({"files":[{"path":"pipe"}]}));
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "fifo_capture_child", "--nocapture"])
        .env("WORKDECK_CONTEXT_FIFO_TEST_ROOT", repo.root())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "child status {status}");
            break;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("FIFO context read exceeded 5 seconds");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn prerequisite_contract_changes_invalidate_requirements_but_checked_flags_do_not() {
    let (_temp, repo) = setup();
    let prerequisite = create(
        &repo,
        "Dependency",
        json!({"acceptance":[{"id":"one","description":"Old dependency requirement","checked":false}]}),
    );
    let issue = create(
        &repo,
        "Dependent",
        json!({"prerequisites":[prerequisite.metadata.id]}),
    );
    let request = ContextRequest::new(issue.metadata.id.as_str(), 64 * 1024);
    let first = repo.context(&request).unwrap();
    let path = repo.root().join(&prerequisite.path);
    let mut metadata = prerequisite.metadata.clone();
    metadata.acceptance[0].description = "New dependency requirement".into();
    fs::write(
        &path,
        format!(
            "---\n{}---\n{}",
            serde_yaml_ng::to_string(&metadata).unwrap(),
            prerequisite.body
        ),
    )
    .unwrap();
    let second = repo.context(&request).unwrap();
    assert_ne!(first.anchor.requirements, second.anchor.requirements);
    metadata.acceptance[0].checked = true;
    fs::write(
        &path,
        format!(
            "---\n{}---\n{}",
            serde_yaml_ng::to_string(&metadata).unwrap(),
            prerequisite.body
        ),
    )
    .unwrap();
    let third = repo.context(&request).unwrap();
    assert_eq!(second.anchor.requirements, third.anchor.requirements);
    assert_ne!(second.anchor.fingerprint, third.anchor.fingerprint);
}

#[test]
fn declared_evidence_has_explicit_time_freshness_and_cannot_satisfy_check_execution() {
    let (_temp, repo) = setup();
    let issue = create(
        &repo,
        "Evidence",
        json!({"acceptance":[{"id":"one","description":"Required behavior","checked":true}]}),
    );
    let now = chrono::Utc::now();
    let criterion = repo
        .resolve_criterion(&CriterionOwner::Issue(issue.metadata.id.clone()), "one")
        .unwrap()
        .reference;
    let declaration = DeclareEvidence {
        criterion,
        subject: ExactSubject {
            repository: repo.identity().clone(),
            kind: ExactSubjectKind::Source,
            content: ContentHash::of(b"declared source"),
        },
        producer: ProducerRef {
            id: "runner".into(),
            definition: ContentHash::of(b"producer"),
        },
        check: CheckRef {
            id: "test".into(),
            definition: ContentHash::of(b"check"),
        },
        result: ResultRef {
            id: "declared-result".into(),
            content: ContentHash::of(b"result"),
        },
        observed_at: now - chrono::Duration::seconds(1),
        expires_at: Some(now + chrono::Duration::seconds(5)),
        provenance: DeclaredProvenance {
            actor: "agent".into(),
            reason: "External annotation".into(),
        },
        links: Vec::new(),
        supersedes: None,
        custom: Default::default(),
        extra: Default::default(),
    };
    repo.declare_evidence(&declaration, &RequestId::new())
        .unwrap();
    let request = ContextRequest::new(issue.metadata.id.as_str(), 64 * 1024);
    let unknown = repo.context(&request).unwrap();
    assert!(entries(&unknown).any(|e|matches!(e,ContextContent::Evidence {freshness:ContextFreshness::Unknown,reason_codes,..} if reason_codes.contains(&"as_of_required_for_freshness".into()))));
    let current = repo
        .context(&ContextRequest {
            as_of: Some(now + chrono::Duration::seconds(1)),
            ..request.clone()
        })
        .unwrap();
    assert!(entries(&current).any(|e|matches!(e,ContextContent::Evidence {freshness:ContextFreshness::Current,reason_codes,..} if reason_codes.contains(&"producer_unadmitted".into()))));
    let expired = repo
        .context(&ContextRequest {
            as_of: Some(now + chrono::Duration::seconds(6)),
            ..request
        })
        .unwrap();
    assert!(entries(&expired).any(|e| matches!(
        e,
        ContextContent::Evidence {
            freshness: ContextFreshness::Stale,
            ..
        }
    )));
    assert_eq!(unknown.anchor, current.anchor);
    assert_eq!(unknown.anchor, expired.anchor);
    let mut config = repo.config().unwrap();
    config.acceptance.required_checks.push("test".into());
    fs::write(
        repo.root().join("config.yml"),
        serde_yaml_ng::to_string(&config).unwrap(),
    )
    .unwrap();
    let actions = repo
        .next_actions(&NextActionRequest::new(issue.metadata.id.as_str()))
        .unwrap();
    assert!(
        actions
            .actions
            .iter()
            .any(|a| a.kind == NextActionKind::RunChecks
                && a.available == cfg!(unix)
                && a.reason_code
                    == if cfg!(unix) {
                        "inspect_check_plan"
                    } else {
                        "check_execution_unavailable"
                    })
    );
    // PM08 exposes plan inspection, but this evidence declaration supplies
    // neither a command definition nor an actual local execution result.
    assert_eq!(
        repo.check_plan(&CheckPlanRequest {
            issue: Some(issue.metadata.id.to_string()),
            checks: vec!["test".into()],
            ..Default::default()
        })
        .unwrap_err()
        .code,
        ErrorCode::NotFound
    );
    assert!(repo.check_results(&RunQuery::default()).unwrap().is_empty());
    assert!(
        !repo
            .completion_report(issue.metadata.id.as_str())
            .unwrap()
            .allowed
    );
}

#[test]
fn overlap_explanations_are_advisory_and_distinguish_disjoint_lines() {
    let (_temp, repo) = setup();
    let prerequisite = create(&repo, "Shared dependency", json!({}));
    let issue = create(
        &repo,
        "Own",
        json!({"files":[{"path":"src/shared.rs","line":10,"end_line":20}],"prerequisites":[prerequisite.metadata.id]}),
    );
    let overlap = create(
        &repo,
        "Overlapping",
        json!({"files":[{"path":"src/shared.rs","line":15,"end_line":25}],"prerequisites":[prerequisite.metadata.id]}),
    );
    let disjoint = create(
        &repo,
        "Disjoint",
        json!({"files":[{"path":"src/shared.rs","line":30,"end_line":40}]}),
    );
    let packet = repo
        .context(&ContextRequest::new(issue.metadata.id.as_str(), 64 * 1024))
        .unwrap();
    let overlaps = entries(&packet)
        .filter_map(|entry| {
            if let ContextContent::Overlap { overlap } = entry {
                Some(overlap)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert!(overlaps.iter().all(|o| o.advisory));
    assert!(overlaps.iter().any(|o|o.issue==overlap.metadata.id&&matches!(o.basis,OverlapBasis::Path {..})));
    assert!(overlaps.iter().any(|o| o.issue == overlap.metadata.id
        && matches!(o.basis, OverlapBasis::SharedPrerequisite { .. })));
    assert!(!overlaps.iter().any(|o| o.issue == disjoint.metadata.id));
}

#[test]
fn packet_minimum_ignores_unrelated_backlog_size_and_next_titles_are_bounded() {
    let (_temp, repo) = setup();
    let issue = create(&repo, "Context subject", json!({}));
    let request = ContextRequest::new(issue.metadata.id.as_str(), 64 * 1024);
    let initial = repo.context(&request).unwrap();
    for n in 0..12 {
        create(&repo, &format!("Unrelated {n}"), json!({}));
    }
    let large = create(
        &repo,
        &"日本語 \"".repeat(5000),
        json!({"priority":"urgent"}),
    );
    let current = repo.context(&request).unwrap();
    assert_eq!(initial.budget.minimum_bytes, current.budget.minimum_bytes);
    assert_eq!(current.anchor.source_pins.len(), 2);
    let selected = repo
        .next_issue(&NextIssueRequest::default())
        .unwrap()
        .selected
        .unwrap();
    assert_eq!(selected.issue, large.metadata.id);
    assert!(selected.title_truncated);
    assert!(selected.title.len() <= 256);
    assert!(serde_json::to_vec(&selected).unwrap().len() < 8 * 1024);
}

#[test]
fn terminal_and_retired_issues_never_manufacture_work_or_handoff_actions() {
    let (_temp, repo) = setup();
    let done = create(&repo, "Completed", json!({}));
    repo.complete_issue(
        done.metadata.id.as_str(),
        &done.source,
        None,
        &RequestId::new(),
    )
    .unwrap();
    let canceled = create(&repo, "Canceled", json!({}));
    repo.mutate_issue(
        canceled.metadata.id.as_str(),
        Some(&canceled.source),
        &IssueMutation::Cancel,
        &RequestId::new(),
    )
    .unwrap();
    let retired = create(&repo, "Retired", json!({}));
    let preview = repo
        .retirement_preview_issue(retired.metadata.id.as_str())
        .unwrap();
    repo.retire_issue(
        retired.metadata.id.as_str(),
        Some(&preview.source),
        Some(&preview.fingerprint),
        &RequestId::new(),
    )
    .unwrap();
    let selected = repo.next_issue(&NextIssueRequest::default()).unwrap();
    assert!(selected.selected.is_none());
    assert_eq!(selected.eligible, 0);
    for issue in [done, canceled, retired] {
        assert!(
            repo.next_actions(&NextActionRequest::new(issue.metadata.id.as_str()))
                .unwrap()
                .actions
                .is_empty()
        );
    }
}

#[test]
fn overlap_inspection_limit_is_reported_without_losing_core_context() {
    let (_temp, repo) = setup();
    let files = (0..400)
        .map(|n| json!({"path":format!("src/file{n}.rs")}))
        .collect::<Vec<_>>();
    let issue = create(&repo, "Many paths", json!({"files":files}));
    create(&repo, "Other paths", json!({"files":files}));
    let packet = repo
        .context(&ContextRequest::new(issue.metadata.id.as_str(), 64 * 1024))
        .unwrap();
    let overlaps = packet
        .sections
        .iter()
        .find(|s| s.kind == ContextSectionKind::Overlaps)
        .unwrap();
    assert!(!overlaps.coverage_complete);
    assert!(
        overlaps
            .omission_reasons
            .iter()
            .any(|r| r.contains("inspection_limit"))
    );
    assert_eq!(packet.anchor.issue, issue.metadata.id);
    assert!(serde_json::to_vec(&packet).unwrap().len() <= 64 * 1024);
}

#[cfg(unix)]
#[test]
fn identical_cloned_sources_preserve_context_anchor_and_handoff_freshness() {
    fn copy_tree(source: &std::path::Path, destination: &std::path::Path) {
        fs::create_dir_all(destination).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            if [".tmp", ".local", ".index"]
                .iter()
                .any(|name| entry.file_name() == *name)
            {
                continue;
            }
            if entry.file_type().unwrap().is_dir() {
                copy_tree(&entry.path(), &destination.join(entry.file_name()));
            } else {
                fs::copy(entry.path(), destination.join(entry.file_name())).unwrap();
            }
        }
    }
    let (original, repo) = setup();
    fs::create_dir(original.path().join("src")).unwrap();
    fs::write(original.path().join("AGENTS.md"), "Root instructions").unwrap();
    fs::write(original.path().join("src/AGENTS.md"), "Scoped instructions").unwrap();
    fs::write(original.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(original.path().join("binary.dat"), [0, 1, 2]).unwrap();
    fs::write(
        original.path().join("oversized.dat"),
        vec![b'a'; 256 * 1024 + 1],
    )
    .unwrap();
    let issue = create(
        &repo,
        "Portable continuity",
        json!({"files":[{"path":"src/main.rs"},{"path":"binary.dat"},{"path":"oversized.dat"}]}),
    );
    let request = ContextRequest::new(issue.metadata.id.as_str(), 64 * 1024);
    let before = repo.context(&request).unwrap();
    repo.create_handoff(
        &CreateHandoff {
            actor: "agent".into(),
            anchor: before.anchor.clone(),
            body: "Continue the same source in another checkout".into(),
            attempted: Vec::new(),
            uncertainties: Vec::new(),
            evidence_refs: Vec::new(),
            questions: Vec::new(),
            pending_operations: Vec::new(),
            next_steps: Vec::new(),
            custom: Default::default(),
            extra: Default::default(),
        },
        &RequestId::new(),
    )
    .unwrap();
    let cloned = TempDir::new().unwrap();
    copy_tree(original.path(), cloned.path());
    let copy = Repository::open_source(&cloned.path().join(".workdeck")).unwrap();
    assert_eq!(copy.identity(), repo.identity());
    use std::os::unix::fs::MetadataExt;
    assert_ne!(
        fs::metadata(original.path().join("src/main.rs"))
            .unwrap()
            .ino(),
        fs::metadata(cloned.path().join("src/main.rs"))
            .unwrap()
            .ino()
    );
    let resumed = copy.context(&request).unwrap();
    let handoff_fresh = entries(&resumed).any(|entry| {
        matches!(
            entry,
            ContextContent::Handoff {
                freshness: ContextFreshness::Current,
                ..
            }
        )
    });
    assert!(
        before.anchor == resumed.anchor && handoff_fresh,
        "identical repository and content must preserve the anchor and fresh handoff; anchor_equal={}, handoff_fresh={handoff_fresh}",
        before.anchor == resumed.anchor
    );
}

#[cfg(unix)]
#[test]
fn identical_atomic_source_replacement_changes_only_within_capture_race_identity() {
    let (temp, repo) = setup();
    let source = temp.path().join("source.rs");
    fs::write(&source, "same bytes\n").unwrap();
    let issue = create(
        &repo,
        "Atomic replace",
        json!({"files":[{"path":"source.rs"}]}),
    );
    let request = ContextRequest::new(issue.metadata.id.as_str(), 64 * 1024);
    let before = repo.context(&request).unwrap();
    let replace = || {
        let temporary = temp.path().join("replacement.rs");
        fs::write(&temporary, "same bytes\n").unwrap();
        fs::rename(&temporary, &source).unwrap();
    };
    replace();
    let after = repo.context(&request).unwrap();
    assert_eq!(
        before.anchor, after.anchor,
        "an atomic save of identical content preserves durable identity"
    );
    repo.next_actions(&NextActionRequest {
        issue: issue.metadata.id.to_string(),
        expected_context: Some(before.anchor.fingerprint),
    })
    .unwrap();
    let error = repo
        .context_with_faults(&request, |_| {
            replace();
            Ok(())
        })
        .unwrap_err();
    assert_eq!(
        error.code,
        ErrorCode::StaleSource,
        "replacement during a capture is still rejected"
    );
}
