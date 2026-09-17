#![cfg(unix)]
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use workdeck_pm::*;
fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    let mut command = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_str().is_some_and(|key| key.starts_with("GIT_")) {
            command.env_remove(key);
        }
    }
    let output = command
        .current_dir(root)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
fn fixture() -> (tempfile::TempDir, Repository, IssueRecord) {
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), &["init", "-b", "main"]);
    git(temp.path(), &["config", "user.name", "Fixture"]);
    git(
        temp.path(),
        &["config", "user.email", "fixture@example.invalid"],
    );
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let receipt = repository
        .create_issue(
            &CreateIssue::new("Accepted title", "body"),
            &RequestId::new(),
        )
        .unwrap();
    let issue: IssueRecord = serde_json::from_value(receipt.result).unwrap();
    fs::write(temp.path().join("code.txt"), "original").unwrap();
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "fixture"]);
    (temp, repository, issue)
}
fn capture(root: &Path, selector: SourceSelector) -> PlanningSourceView {
    sources::capture(root, &selector, &SourceCaptureLimits::default()).unwrap()
}

#[test]
fn config_activates_typed_shared_fields_without_changing_absent_serialization() {
    let config = Config::new("WD").unwrap();
    let value = serde_json::to_value(&config).unwrap();
    assert!(value.get("sources").is_none());
    assert!(value.get("claims").is_none());
    assert_eq!(
        serde_json::from_value::<Config>(value.clone()).unwrap(),
        config
    );
    let mut shared = value;
    shared["sources"] = serde_json::json!({"remote":"origin","accepted_ref":"refs/heads/main","coordination_ref":"refs/heads/workdeck-coordination","proposal_namespace":"refs/heads/workdeck-proposals"});
    shared["claims"] = serde_json::json!({});
    let parsed: Config = serde_json::from_value(shared.clone()).unwrap();
    parsed.validate().unwrap();
    shared["sources"]["coordination_ref"] = serde_json::json!("refs/heads/main");
    assert!(
        serde_json::from_value::<Config>(shared)
            .unwrap()
            .validate()
            .is_err()
    );
    for bad in [
        "HEAD",
        "main:other",
        "refs/heads/../x",
        "refs/heads/x.lock",
        "refs/heads/x~1",
    ] {
        assert!(bad.parse::<GitRefName>().is_err(), "{bad}");
    }
}

#[test]
fn staged_source_keeps_immutable_index_bytes_and_ignores_live_issue_edits() {
    let (temp, repository, issue) = fixture();
    let index_before = fs::read(temp.path().join(".git/index")).unwrap();
    let head = git(temp.path(), &["rev-parse", "HEAD"]);
    repository
        .update_issue(
            issue.metadata.id.as_str(),
            &issue.source,
            &UpdateIssue {
                fields: std::collections::BTreeMap::from([(
                    "title".into(),
                    serde_json::json!("Local proposal title"),
                )]),
                body: None,
            },
            &RequestId::new(),
        )
        .unwrap();
    fs::write(temp.path().join("code.txt"), "dirty code").unwrap();
    let view = capture(
        temp.path(),
        SourceSelector::Staged {
            index: IndexSelection::Default,
        },
    );
    assert_eq!(
        view.snapshot
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .metadata
            .title,
        "Accepted title"
    );
    assert_eq!(view.observation.identity.role, SourceRole::Staged);
    assert!(view.observation.identity.index_content.is_some());
    view.revalidate().unwrap();
    assert_eq!(
        fs::read(temp.path().join(".git/index")).unwrap(),
        index_before
    );
    assert_eq!(git(temp.path(), &["rev-parse", "HEAD"]), head);
    assert_eq!(
        fs::read(temp.path().join("code.txt")).unwrap(),
        b"dirty code"
    );
    let live = capture(temp.path(), SourceSelector::WorkingTree);
    assert_eq!(
        live.snapshot.query_issues(&IssueQuery::all()).unwrap()[0]
            .metadata
            .title,
        "Local proposal title"
    );
    assert_eq!(live.observation.identity.role, SourceRole::Local);
}

#[test]
fn staged_source_admits_staged_config_when_working_config_is_malformed() {
    let (temp, repository, issue) = fixture();
    fs::write(repository.root().join("config.yml"), "schema: broken\n").unwrap();
    let view = capture(
        temp.path(),
        SourceSelector::Staged {
            index: IndexSelection::Default,
        },
    );
    assert_eq!(
        view.snapshot
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .metadata
            .title,
        "Accepted title"
    );
    assert!(
        sources::capture(
            temp.path(),
            &SourceSelector::WorkingTree,
            &SourceCaptureLimits::default()
        )
        .is_err()
    );
}

#[test]
fn explicit_index_selection_and_post_capture_change_are_not_silently_ignored() {
    let (temp, _, issue) = fixture();
    let alternate = temp.path().join("candidate.index");
    fs::copy(temp.path().join(".git/index"), &alternate).unwrap();
    let view = capture(
        temp.path(),
        SourceSelector::Staged {
            index: IndexSelection::Explicit {
                path: PathBuf::from("candidate.index"),
            },
        },
    );
    assert_eq!(
        view.snapshot
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .metadata
            .title,
        "Accepted title"
    );
    fs::write(&alternate, b"broken-index").unwrap();
    assert_eq!(view.revalidate().unwrap_err().code, ErrorCode::StaleSource);
    assert_eq!(
        view.snapshot
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .metadata
            .title,
        "Accepted title"
    );
}

fn shared_fixture() -> (
    tempfile::TempDir,
    tempfile::TempDir,
    Repository,
    IssueRecord,
) {
    let (temp, repository, issue) = fixture();
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "--bare"]);
    git(
        temp.path(),
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    let mut config = repository.config().unwrap();
    config.sources = Some(SharedSources {
        remote: "origin".into(),
        accepted_ref: "refs/heads/main".parse().unwrap(),
        coordination_ref: "refs/heads/workdeck-coordination".parse().unwrap(),
        proposal_namespace: "refs/heads/workdeck-proposals".parse().unwrap(),
    });
    fs::write(
        repository.root().join("config.yml"),
        serde_yaml_ng::to_string(&config).unwrap(),
    )
    .unwrap();
    git(temp.path(), &["add", ".workdeck/config.yml"]);
    git(temp.path(), &["commit", "-m", "shared config"]);
    git(temp.path(), &["push", "origin", "main"]);
    (temp, remote, repository, issue)
}

#[test]
fn explicit_fetch_and_sync_preserve_developer_head_index_worktree_and_replay_identity() {
    let (temp, _remote, repository, issue) = shared_fixture();
    fs::write(temp.path().join("code.txt"), "staged application change").unwrap();
    git(temp.path(), &["add", "code.txt"]);
    fs::write(temp.path().join("code.txt"), "unstaged application change").unwrap();
    let index = fs::read(temp.path().join(".git/index")).unwrap();
    let head = git(temp.path(), &["rev-parse", "HEAD"]);
    let request = fetch_request(&repository);
    let id = RequestId::new();
    let fetched = repository.fetch_sources(&request, &id).unwrap();
    assert!(!fetched.replayed);
    assert!(
        fetched
            .observations
            .iter()
            .any(|o| o.reference.as_str() == "refs/heads/main" && o.commit.is_some())
    );
    let replay = repository.fetch_sources(&request, &id).unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.operation_id, fetched.operation_id);
    let synced = repository
        .sync_sources(&request, &RequestId::new())
        .unwrap();
    assert!(!synced.materialized.is_empty());
    assert!(
        synced
            .materialized
            .iter()
            .all(|p| p.starts_with(repository.root().join(".local")))
    );
    let accepted = capture(temp.path(), SourceSelector::Accepted);
    assert_eq!(
        accepted
            .snapshot
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .metadata
            .title,
        "Accepted title"
    );
    assert!(accepted.observation.remote_observation.is_some());
    assert_eq!(fs::read(temp.path().join(".git/index")).unwrap(), index);
    assert_eq!(git(temp.path(), &["rev-parse", "HEAD"]), head);
    assert_eq!(
        fs::read(temp.path().join("code.txt")).unwrap(),
        b"unstaged application change"
    );
    let wrong = SourceFetchRequest {
        expected_config: ContentHash::of(b"other config"),
        expected_binding: request.expected_binding.clone(),
    };
    assert_eq!(
        repository.fetch_sources(&wrong, &id).unwrap_err().code,
        ErrorCode::IdempotencyConflict
    );
}

#[test]
fn accepted_view_is_separate_from_dirty_proposal_and_an_existing_view_is_immutable() {
    let (temp, _remote, repository, issue) = shared_fixture();
    let request = fetch_request(&repository);
    repository
        .fetch_sources(&request, &RequestId::new())
        .unwrap();
    let accepted = capture(temp.path(), SourceSelector::Accepted);
    repository
        .update_issue(
            issue.metadata.id.as_str(),
            &issue.source,
            &UpdateIssue {
                fields: std::collections::BTreeMap::from([(
                    "title".into(),
                    serde_json::json!("Proposal"),
                )]),
                body: None,
            },
            &RequestId::new(),
        )
        .unwrap();
    let working = capture(temp.path(), SourceSelector::WorkingTree);
    assert_eq!(working.observation.identity.role, SourceRole::Proposal);
    assert_eq!(
        working
            .snapshot
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .metadata
            .title,
        "Proposal"
    );
    assert_eq!(
        accepted
            .snapshot
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .metadata
            .title,
        "Accepted title"
    );
    accepted.revalidate().unwrap();
}

#[test]
fn local_source_status_does_not_require_or_initialize_git() {
    let temp = tempfile::tempdir().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let record: IssueRecord = serde_json::from_value(
        repository
            .create_issue(
                &CreateIssue::new("Local task", "No remote"),
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap();
    let status = repository.source_status().unwrap();
    assert_eq!(status.working.identity.role, SourceRole::Local);
    assert!(status.accepted.is_none());
    assert!(status.coordination.is_none());
    assert!(status.errors.is_empty());
    let view = capture(temp.path(), SourceSelector::WorkingTree);
    assert_eq!(
        view.snapshot
            .show_issue(record.metadata.id.as_str())
            .unwrap()
            .metadata
            .title,
        "Local task"
    );
    view.revalidate().unwrap();
    repository
        .update_issue(
            record.metadata.id.as_str(),
            &record.source,
            &UpdateIssue {
                fields: std::collections::BTreeMap::from([(
                    "title".into(),
                    serde_json::json!("Changed"),
                )]),
                body: None,
            },
            &RequestId::new(),
        )
        .unwrap();
    assert_eq!(view.revalidate().unwrap_err().code, ErrorCode::StaleSource);
    assert_eq!(
        view.snapshot
            .show_issue(record.metadata.id.as_str())
            .unwrap()
            .metadata
            .title,
        "Local task"
    );
    assert!(!temp.path().join(".git").exists());
}

#[test]
fn source_status_rejects_a_replaced_repository_identity() {
    let temp = tempfile::tempdir().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let replacement = tempfile::tempdir().unwrap();
    let other = Repository::init(replacement.path(), "WD").unwrap();
    fs::copy(
        other.root().join("config.yml"),
        repository.root().join("config.yml"),
    )
    .unwrap();
    let error = repository.source_status().unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    assert!(!temp.path().join(".git").exists());
}

#[test]
fn fetched_view_and_replay_reject_changed_remote_binding_without_contacting_the_replacement() {
    let (temp, _remote, repository, issue) = shared_fixture();
    let request = fetch_request(&repository);
    let id = RequestId::new();
    repository.fetch_sources(&request, &id).unwrap();
    let view = capture(temp.path(), SourceSelector::Accepted);
    assert_eq!(
        view.snapshot
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .metadata
            .title,
        "Accepted title"
    );
    let replacement = tempfile::tempdir().unwrap();
    git(replacement.path(), &["init", "--bare"]);
    git(
        temp.path(),
        &[
            "remote",
            "set-url",
            "origin",
            replacement.path().to_str().unwrap(),
        ],
    );
    assert_eq!(
        repository.fetch_sources(&request, &id).unwrap_err().code,
        ErrorCode::StaleSource
    );
    assert_eq!(view.revalidate().unwrap_err().code, ErrorCode::StaleSource);
    assert!(git(replacement.path(), &["for-each-ref"]).is_empty());
}

#[test]
fn shared_operations_reject_local_config_includes_before_remote_mutation() {
    let (temp, remote, repository, _issue) = shared_fixture();
    let request = fetch_request(&repository);
    let included = temp.path().join("extra-git-config");
    fs::write(&included, "[core]\n  filemode = false\n").unwrap();
    git(
        temp.path(),
        &["config", "include.path", included.to_str().unwrap()],
    );
    let before = git(remote.path(), &["for-each-ref"]);
    let error = repository
        .fetch_sources(&request, &RequestId::new())
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert!(error.message.contains("include"));
    assert_eq!(git(remote.path(), &["for-each-ref"]), before);
    assert_eq!(
        fs::read(&included).unwrap(),
        b"[core]\n  filemode = false\n"
    );
}

#[test]
fn isolated_shared_profile_ignores_ambient_global_url_rewrites() {
    if let Some(root) = std::env::var_os("WORKDECK_SOURCE_CHILD_ROOT") {
        let repository = Repository::open_source(&PathBuf::from(root).join(".workdeck")).unwrap();
        let request = fetch_request(&repository);
        repository
            .fetch_sources(&request, &RequestId::new())
            .unwrap();
        return;
    }
    let (temp, remote, _repository, _issue) = shared_fixture();
    let global = tempfile::tempdir().unwrap();
    fs::create_dir(global.path().join("git")).unwrap();
    let config = format!(
        "[url \"/this-remote-must-never-be-used/\"]\n  insteadOf = {}\n",
        remote.path().display()
    );
    fs::write(global.path().join("git/config"), config).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "isolated_shared_profile_ignores_ambient_global_url_rewrites",
            "--nocapture",
        ])
        .env("WORKDECK_SOURCE_CHILD_ROOT", temp.path())
        .env("XDG_CONFIG_HOME", global.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn reviewed_fetch_rejects_a_remote_url_change_even_when_ref_contents_match() {
    let (temp, remote, repository, _issue) = shared_fixture();
    let reviewed = repository.source_status().unwrap();
    let request = SourceFetchRequest {
        expected_config: reviewed.config,
        expected_binding: reviewed.binding.unwrap(),
    };
    let replacement = tempfile::tempdir().unwrap();
    git(
        replacement.path(),
        &["clone", "--mirror", remote.path().to_str().unwrap(), "."],
    );
    git(
        temp.path(),
        &[
            "remote",
            "set-url",
            "origin",
            replacement.path().to_str().unwrap(),
        ],
    );
    let before = git(replacement.path(), &["for-each-ref"]);
    let error = repository
        .fetch_sources(&request, &RequestId::new())
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    assert_eq!(git(replacement.path(), &["for-each-ref"]), before);
}

#[test]
fn local_shared_refs_remain_readable_without_a_publication_remote() {
    let (temp, _remote, repository, issue) = shared_fixture();
    let bound = capture(temp.path(), SourceSelector::Accepted);
    assert!(bound.publication_binding().is_some());
    git(temp.path(), &["branch", "workdeck-proposals/local-read"]);
    git(temp.path(), &["remote", "remove", "origin"]);
    assert_eq!(bound.revalidate().unwrap_err().code, ErrorCode::StaleSource);
    for selector in [
        SourceSelector::WorkingTree,
        SourceSelector::Accepted,
        SourceSelector::Proposal {
            reference: "refs/heads/workdeck-proposals/local-read".parse().unwrap(),
        },
    ] {
        let view = capture(temp.path(), selector);
        assert!(view.publication_binding().is_none());
        assert_eq!(view.observation.freshness, SourceFreshness::Unknown);
        assert!(
            view.observation
                .reason_codes
                .iter()
                .any(|reason| reason == "publication_binding_unavailable")
        );
        assert_eq!(
            view.snapshot
                .show_issue(issue.metadata.id.as_str())
                .unwrap()
                .metadata
                .title,
            "Accepted title"
        );
        view.revalidate().unwrap();
    }
    let status = repository.source_status().unwrap();
    assert!(status.binding.is_none());
    assert!(status.shared.is_some());
    assert_eq!(status.accepted.unwrap().freshness, SourceFreshness::Unknown);
    assert!(
        status
            .errors
            .iter()
            .any(|error| error.code == ErrorCode::PolicyBlocked)
    );
    let request = SourceFetchRequest {
        expected_config: status.config,
        expected_binding: ContentHash::of(b"unavailable"),
    };
    assert!(
        repository
            .fetch_sources(&request, &RequestId::new())
            .is_err()
    );
    assert!(!repository.root().join("claims").exists());
}

fn fetch_request(repository: &Repository) -> SourceFetchRequest {
    let status = repository.source_status().unwrap();
    SourceFetchRequest {
        expected_config: status.config,
        expected_binding: status.binding.unwrap(),
    }
}
