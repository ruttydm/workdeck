//! PM07 journeys through the actual normal-startup executable and terminal.
use super::*;
use workdeck_pm::{ContextRequest, IssueMutation, QuestionQuery, QuestionState, UpdateIssue};

fn selected_line(text: &str) -> Option<String> {
    text.lines()
        .find(|line| line.contains('›'))
        .map(str::to_owned)
}

fn select(session: &mut Session, label: &str) {
    let mut frame = session.wait(|text| selected_line(text).is_some());
    for _ in 0..80 {
        let selected = selected_line(&frame).unwrap();
        if selected.contains(label) {
            return;
        }
        session.write(b"j");
        frame = session.wait(|text| selected_line(text).is_some_and(|line| line != selected));
    }
    panic!("Could not select {label}:\n{frame}");
}

fn seed(repository: &Repository, title: &str, body: &str) -> IssueRecord {
    let mut input = CreateIssue::new(title, body);
    input.fields.insert(
        "files".into(),
        serde_json::json!([{"path":"source.rs","line":2}]),
    );
    serde_json::from_value(
        repository
            .create_issue(&input, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap()
}

#[test]
fn pm07_context_question_answer_handoff_and_fresh_terminal_resume() {
    let (directory, repository) = repository();
    let issue = seed(
        &repository,
        "Continuity task",
        "The issue is the starting point",
    );
    git(directory.path(), &["add", ".workdeck"]);
    git(
        directory.path(),
        &["commit", "--quiet", "-m", "Continuity fixture"],
    );
    let mut session = startup(directory.path(), 110);
    session.wait(|text| text.contains("F3 Issues"));
    session.write(b"\x1bORi");
    session.wait(|text| text.contains("Task context") && text.contains("Continuity task"));
    session.write(b"3n");
    session.wait(|text| text.contains("Create question") && text.contains("Blocks implementation"));
    session.write(b"\tMay this API change?\t\x15true");
    session.write(b"\x1bOQ");
    session.wait(|text| !text.contains("Create question") && text.contains("No changes to review"));
    session.write(b"\x1bOR");
    session.wait(|text| text.contains("Create question") && text.contains("May this API change?"));
    session.write(b"\x13");
    session.wait(|text| {
        text.contains("May this API change?")
            && text.contains("Open")
            && !text.contains("Create question")
    });
    let questions = repository.questions(&QuestionQuery::default()).unwrap();
    assert_eq!(questions.len(), 1);
    let question = &questions[0];
    assert!(question.metadata.blocks_work);
    assert_eq!(
        repository
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .source,
        issue.source
    );

    session.write(b"2w");
    session.wait(|text| text.contains("0 eligible") && text.contains("excluded"));
    session.write(b"2");
    session.wait(|text| text.contains("ResolveQuestion"));
    select(&mut session, "ResolveQuestion");
    session.write(b"\ra");
    session.wait(|text| text.contains("Answer question") && text.contains("Ctrl-S save"));
    session.write(b"\tKeep the documented compatibility contract\x13");
    session.wait(|text| {
        text.contains("Answered")
            && text.contains("Keep the documented compatibility contract")
            && !text.contains("Answer question")
    });
    assert_eq!(
        repository
            .question(&question.metadata.id)
            .unwrap()
            .metadata
            .state,
        QuestionState::Answered
    );
    session.write(b"2w");
    session.wait(|text| text.contains("1 eligible") && text.contains("0 excluded"));
    session.write(b"2");
    select(&mut session, "RecordHandoff");
    session.write(b"\r");
    session.wait(|text| text.contains("Create handoff") && text.contains("Summary"));
    session.write(b"\tA different session can continue\tRead the contract\tExecution remains pending\tAdd the focused regression\x13");
    session.wait(|text| {
        text.contains("Declared handoff")
            && text.contains("A different session can continue")
            && !text.contains("Create handoff")
    });
    let handoff = repository.handoffs(&issue.metadata.id).unwrap().remove(0);
    assert_eq!(handoff.metadata.attempted, ["Read the contract"]);
    assert_eq!(
        handoff.metadata.uncertainties,
        ["Execution remains pending"]
    );
    assert_eq!(handoff.metadata.next_steps, ["Add the focused regression"]);
    assert_eq!(
        repository
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .source,
        issue.source
    );
    session.quit();

    // A distinct process resumes from only the persisted issue and handoff.
    repository
        .mutate_issue(
            issue.metadata.id.as_str(),
            None,
            &IssueMutation::Update {
                input: UpdateIssue {
                    fields: Default::default(),
                    body: Some("A later requirement changed the task basis".into()),
                },
            },
            &RequestId::new(),
        )
        .unwrap();
    let mut resumed = startup(directory.path(), 78);
    resumed.wait(|text| text.contains("F3 Issues"));
    resumed.write(b"\x1bORi4");
    resumed.wait(|text| {
        text.contains("Task context")
            && text.contains("Stale")
            && text.contains("A different session can continue")
    });
    assert_eq!(
        repository
            .handoff(&issue.metadata.id, &handoff.metadata.id)
            .unwrap(),
        handoff
    );
    resumed.write(b"3");
    resumed.wait(|text| text.contains("Answered") && text.contains("Stale"));
    resumed.write(b"a");
    resumed.wait(|text| {
        text.contains("reviewed subject source changed") && !text.contains("Answer question")
    });
    resumed.quit();
    assert!(!directory.path().join(".agents").exists());
}

#[test]
fn pm07_context_budget_source_navigation_and_review_return_at_narrow_and_wide_sizes() {
    let (directory, repository) = repository();
    let issue = seed(
        &repository,
        "Bounded source task",
        &"Long accepted description. ".repeat(10000),
    );
    git(directory.path(), &["add", ".workdeck"]);
    git(
        directory.path(),
        &["commit", "--quiet", "-m", "Bounded context fixture"],
    );
    let packet = repository
        .context(&ContextRequest::new(issue.metadata.id.as_str(), 64 * 1024))
        .unwrap();
    assert!(packet.budget.omitted_entries > 0);
    let mut session = startup(directory.path(), 78);
    session.wait(|text| text.contains("F3 Issues"));
    session.write(b"\x1bORi");
    session.wait(|text| {
        text.contains("Task context") && text.contains("omitted") && text.contains("unknown")
    });
    for width in [78, 180] {
        session.resize(width, 34);
        session.wait(|text| text.contains("Task context") && text.contains("omitted"));
        select(&mut session, "↳ source.rs");
        session.write(b"\r");
        session.wait(|text| text.contains("linked source line") && !text.contains("Task context"));
        session.write(b"\x1bOR");
        session.wait(|text| {
            text.contains("Task context")
                && text.contains("omitted")
                && text.contains("↳ source.rs")
        });
    }
    fs::write(
        directory.path().join("source.rs"),
        "direct edit after context inspection\n",
    )
    .unwrap();
    session.write(b"\r");
    session.wait(|text| text.contains("Task context") && text.contains("context inputs changed"));
    session.quit();
    assert_eq!(
        repository
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .source,
        issue.source
    );
    assert!(!directory.path().join(".agents").exists());
}
