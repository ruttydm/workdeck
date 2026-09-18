//! Actual terminal source identity/citation workflows with a temporary bare remote.
use super::*;
use std::collections::BTreeMap;
use workdeck_pm::{PublicationState, SharedSources, UpdateIssue, sources::ProposalRequest};

fn shared_fixture() -> (
    tempfile::TempDir,
    tempfile::TempDir,
    Repository,
    IssueRecord,
) {
    let (directory, repository) = repository();
    let remote = tempfile::tempdir().unwrap();
    git(directory.path(), &["branch", "-M", "main"]);
    git(remote.path(), &["init", "--bare", "--quiet"]);
    git(
        directory.path(),
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    let issue: IssueRecord = serde_json::from_value(
        repository
            .create_issue(
                &CreateIssue::new("Accepted source task", "Accepted body"),
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap();
    let mut config = repository.config().unwrap();
    config.sources = Some(SharedSources {
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
        &["commit", "--quiet", "-m", "Accepted planning"],
    );
    git(directory.path(), &["push", "--quiet", "origin", "main"]);
    git(
        directory.path(),
        &["switch", "--quiet", "-c", "workdeck-proposals/pty"],
    );
    rename(&repository, &issue, "Proposed source task");
    git(directory.path(), &["add", ".workdeck"]);
    git(
        directory.path(),
        &["commit", "--quiet", "-m", "Proposed planning"],
    );
    git(
        directory.path(),
        &["push", "--quiet", "origin", "workdeck-proposals/pty"],
    );
    rename(&repository, &issue, "Working draft only");
    fs::write(directory.path().join("source.rs"), "staged code\n").unwrap();
    git(directory.path(), &["add", "source.rs"]);
    fs::write(directory.path().join("source.rs"), "unstaged code\n").unwrap();
    (directory, remote, repository, issue)
}
fn rename(repository: &Repository, issue: &IssueRecord, title: &str) {
    let current = repository.show_issue(issue.metadata.id.as_str()).unwrap();
    repository
        .update_issue(
            issue.metadata.id.as_str(),
            &current.source,
            &UpdateIssue {
                fields: BTreeMap::from([("title".into(), serde_json::json!(title))]),
                body: None,
            },
            &RequestId::new(),
        )
        .unwrap();
}

#[test]
fn pm09_accepted_proposal_citations_refresh_without_rebasing_opened_document() {
    let (directory, _remote, repository, issue) = shared_fixture();
    let index = fs::read(directory.path().join(".git/index")).unwrap();
    let head = git(directory.path(), &["rev-parse", "HEAD"]);
    let mut session = startup(directory.path(), 132);
    session.wait(|text| text.contains("F3 Issues"));
    session.write(b"\x1bORi6");
    session.wait(|text| text.contains("Planning sources") && text.contains("Captured issues"));
    session.write(b"\r");
    session.wait(|text| {
        text.contains("Working draft only") && text.contains("Exact captured document")
    });
    session.write(b"a\r");
    session.wait(|text| {
        text.contains("Accepted ref")
            && text.contains("Accepted source task")
            && text.contains("Exact captured document")
    });
    session.write(b"p");
    session.wait(|text| text.contains("Inspect proposal source"));
    session.write(b"\x15refs/heads/workdeck-proposals/pty\x13");
    session.wait(|text| {
        text.contains("Proposal refs/heads/workdeck-proposals/pty")
            && !text.contains("Inspect proposal source")
    });
    session.write(b"\r");
    session.wait(|text| {
        text.contains("Proposed source task") && text.contains("Exact captured document")
    });
    rename(&repository, &issue, "New proposal revision");
    let plan = repository
        .preview_proposal(&ProposalRequest {
            reference: "refs/heads/workdeck-proposals/pty".parse().unwrap(),
            title: "Advance the inspected proposal".into(),
        })
        .unwrap();
    assert_eq!(
        repository
            .publish_proposal(&plan, &RequestId::new())
            .unwrap()
            .state,
        PublicationState::Confirmed
    );
    session.write(b"r");
    session.wait(|text| {
        text.contains("Prior observation retained") && text.contains("Proposed source task")
    });
    session.write(b"\r");
    session.wait(|text| {
        text.contains("New proposal revision") && text.contains("Exact captured document")
    });
    session.resize(72, 38);
    session
        .wait(|text| text.contains("Planning sources") && text.contains("New proposal revision"));
    session.write(b"\x1bOQ");
    session.wait(|text| !text.contains("Planning sources") && text.contains("F3 Issues"));
    session.write(b"\x1bOR");
    session
        .wait(|text| text.contains("Planning sources") && text.contains("New proposal revision"));
    session.quit();
    assert_eq!(
        fs::read(directory.path().join(".git/index")).unwrap(),
        index
    );
    assert_eq!(git(directory.path(), &["rev-parse", "HEAD"]), head);
    assert_eq!(
        fs::read(directory.path().join("source.rs")).unwrap(),
        b"unstaged code\n"
    );
}

#[test]
fn pm09_source_operations_review_publish_and_resume_without_touching_developer_state() {
    let (directory, remote, repository, _issue) = shared_fixture();
    let reference = "refs/heads/workdeck-proposals/pty-actions";
    let index = fs::read(directory.path().join(".git/index")).unwrap();
    let head = git(directory.path(), &["rev-parse", "HEAD"]);
    let accepted = git(
        directory.path(),
        &["ls-remote", "origin", "refs/heads/main"],
    );
    let accepted_code = git(remote.path(), &["show", "refs/heads/main:source.rs"]);
    let mut session = startup(directory.path(), 132);
    session.wait(|text| text.contains("F3 Issues"));
    session.write(b"\x1bORi6o");
    session.wait(|text| text.contains("Source operations") && text.contains("No plan is selected"));
    session.write(b"f");
    session.wait(|text| {
        text.contains("Reviewed Fetch plan") && text.contains("Reviewed Git/remote binding")
    });
    session.write(b"x");
    session.wait(|text| {
        text.contains("Source refresh recorded") && !text.contains("Source operation pending")
    });
    session.write(b"\x04s");
    session.wait(|text| text.contains("Reviewed Sync plan"));
    session.write(b"x");
    session.wait(|text| {
        text.contains("Source refresh recorded") && !text.contains("Source operation pending")
    });
    session.write(b"\x04n");
    session.wait(|text| text.contains("Preview planning proposal"));
    session.write(format!("\x15{reference}\tTerminal reviewed proposal").as_bytes());
    session.write(b"\x1bOQ");
    session.wait(|text| {
        !text.contains("Source operations · explicit remote actions") && text.contains("F3 Issues")
    });
    session.write(b"\x1bOR");
    session.wait(|text| {
        text.contains("Preview planning proposal") && text.contains("Terminal reviewed proposal")
    });
    session.write(b"\x13");
    session.wait(|text| text.contains("Reviewed proposal plan") && text.contains(reference));
    assert!(
        git(directory.path(), &["ls-remote", "origin", reference]).is_empty(),
        "preview published a ref"
    );
    session.resize(72, 38);
    session
        .wait(|text| text.contains("Source operations") && text.contains("Reviewed proposal plan"));
    session.resize(132, 38);
    session.write(b"x");
    let confirmed = session.wait(|text| {
        text.contains("Proposal Confirmed") && !text.contains("Source operation pending")
    });
    let request: RequestId = confirmed
        .split("request ")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let original = repository.proposal_status(&request).unwrap();
    assert_eq!(original.state, PublicationState::Confirmed);
    session.write(b"v");
    session.wait(|text| {
        text.contains("Proposal Confirmed")
            && text.contains("local_intent_status_only")
            && !text.contains("Source operation pending")
    });
    session.quit();

    let mut resumed = startup(directory.path(), 132);
    resumed.wait(|text| text.contains("F3 Issues"));
    resumed.write(b"\x1bORi6ot");
    resumed.wait(|text| text.contains("Inspect original proposal request"));
    resumed.write(format!("{request}\x13").as_bytes());
    resumed.wait(|text| {
        text.contains("Proposal Confirmed") && !text.contains("Source operation pending")
    });
    resumed.write(b"u");
    resumed.wait(|text| {
        text.contains("Proposal Confirmed")
            && text.contains("replayed true")
            && text.contains("Current observation")
            && !text.contains("Source operation pending")
    });
    resumed.quit();
    assert_eq!(
        repository.proposal_status(&request).unwrap().candidate,
        original.candidate
    );
    assert_eq!(
        git(
            remote.path(),
            &["show", &format!("{}:source.rs", original.candidate)]
        ),
        accepted_code
    );
    assert_eq!(
        git(
            directory.path(),
            &["ls-remote", "origin", "refs/heads/main"]
        ),
        accepted
    );
    assert_eq!(
        fs::read(directory.path().join(".git/index")).unwrap(),
        index
    );
    assert_eq!(git(directory.path(), &["rev-parse", "HEAD"]), head);
    assert_eq!(
        fs::read(directory.path().join("source.rs")).unwrap(),
        b"unstaged code\n"
    );
}

#[test]
fn pm10_registered_ref_views_return_home_without_mutating_captured_planning() {
    use workdeck_pm::{SourceSelector, registry::*};
    for width in [62, 160] {
        let (directory, _remote, repository, issue) = shared_fixture();
        let store = RegistryStore::open(&repository).unwrap();
        for (alias, source) in [
            ("accepted", SourceSelector::Accepted),
            (
                "proposal",
                SourceSelector::Proposal {
                    reference: "refs/heads/workdeck-proposals/pty".parse().unwrap(),
                },
            ),
        ] {
            store
                .mutate(
                    &RegistryRequest {
                        expected: store.snapshot().unwrap().source,
                        mutation: RegistryMutation::Register {
                            checkout: inspect_checkout(alias, directory.path(), source).unwrap(),
                        },
                    },
                    &RequestId::new(),
                )
                .unwrap();
        }
        let index = fs::read(directory.path().join(".git/index")).unwrap();
        let head = git(directory.path(), &["rev-parse", "HEAD"]);
        let bytes = fs::read(repository.root().join(&issue.path)).unwrap();
        let mut session = startup(directory.path(), width);
        session.wait(|text| text.contains("F3 Issues"));
        session.write(b"\x1bORn");
        session.wait(|text| text.contains("Create issue"));
        session.write(b"Native return draft");
        session.write(b"\x1b[20;2~");
        session.wait(|text| text.contains("My work"));
        session.write(b"s");
        session.wait(|text| text.contains("Registered sources") && text.contains("accepted"));
        session.write(b"o");
        session.wait(|text| {
            text.contains("Read-only planning") && text.contains("Accepted source task")
        });
        session.write(b"neac\x13\r");
        session.wait(|text| text.contains("revision: 1"));
        if width == 62 {
            session.write(b"\x1b[6;2~");
        }
        session.wait(|text| text.contains("Accepted body"));
        assert_eq!(
            fs::read(repository.root().join(&issue.path)).unwrap(),
            bytes
        );
        assert_eq!(
            fs::read(directory.path().join(".git/index")).unwrap(),
            index
        );
        assert_eq!(git(directory.path(), &["rev-parse", "HEAD"]), head);
        session.write(b"\x1b[20;2~");
        session.wait(|text| text.contains("My work"));
        session.write(b"\x1b[Bo");
        session.wait(|text| {
            text.contains("Read-only planning") && text.contains("Proposed source task")
        });
        session.write(b"\x1b[20;2~");
        session.wait(|text| text.contains("My work"));
        session.write(b"h");
        session.wait(|text| text.contains("Create issue") && text.contains("Native return draft"));
        session.write(b"\x13");
        session.wait(|text| !text.contains("Create issue") && text.contains("Native return draft"));
        session.quit();
        assert!(
            repository
                .list_issues()
                .unwrap()
                .iter()
                .any(|issue| issue.metadata.title == "Native return draft")
        );
    }
}
