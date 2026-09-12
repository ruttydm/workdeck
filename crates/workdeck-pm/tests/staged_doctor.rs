use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::TempDir;
use workdeck_pm::{
    ContentHash, CreateIssue, ErrorCode, IndexSelection, IssueGraphMutation, IssueRecord, PmError,
    Repository, RequestId, SourceRole, StagedDoctorRequest, doctor_staged,
    doctor_staged_with_faults,
};

fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    let mut command = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_str().is_some_and(|key| key.starts_with("GIT_")) {
            command.env_remove(key);
        }
    }
    let output = command
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
fn fixture() -> (TempDir, Repository, IssueRecord) {
    let directory = TempDir::new().unwrap();
    git(directory.path(), &["init", "--quiet"]);
    git(directory.path(), &["config", "user.name", "Staged Doctor"]);
    git(
        directory.path(),
        &["config", "user.email", "doctor@example.invalid"],
    );
    let repository = Repository::init(directory.path(), "WD").unwrap();
    let issue = create(&repository, "Indexed issue");
    fs::write(
        directory.path().join("application.txt"),
        "application before\n",
    )
    .unwrap();
    git(
        directory.path(),
        &["add", "--", ".workdeck", "application.txt"],
    );
    git(
        directory.path(),
        &["commit", "--quiet", "--no-gpg-sign", "-m", "initial"],
    );
    (directory, repository, issue)
}
fn create(repository: &Repository, title: &str) -> IssueRecord {
    serde_json::from_value(
        repository
            .create_issue(&CreateIssue::new(title, "Body"), &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap()
}
fn request() -> StagedDoctorRequest {
    StagedDoctorRequest {
        index: IndexSelection::Default,
        expected_index: None,
    }
}
fn index(root: &Path) -> PathBuf {
    PathBuf::from(
        String::from_utf8(git(
            root,
            &["rev-parse", "--path-format=absolute", "--git-path", "index"],
        ))
        .unwrap()
        .trim(),
    )
}
fn staged_path(path: &Path) -> String {
    Path::new(".workdeck").join(path).to_str().unwrap().into()
}

#[test]
fn staged_document_error_is_not_hidden_by_repaired_working_tree_and_reads_preserve_index() {
    let (directory, repository, issue) = fixture();
    let path = repository.root().join(&issue.path);
    let original = fs::read(&path).unwrap();
    fs::write(&path, "---\ninvalid: [\n---\nBody\n").unwrap();
    git(directory.path(), &["add", "--", &staged_path(&issue.path)]);
    fs::write(&path, &original).unwrap();
    fs::write(
        directory.path().join("application.txt"),
        "staged unrelated\n",
    )
    .unwrap();
    git(directory.path(), &["add", "--", "application.txt"]);
    fs::write(
        directory.path().join("application.txt"),
        "unstaged unrelated\n",
    )
    .unwrap();
    let before = fs::read(index(directory.path())).unwrap();
    let diagnosed = doctor_staged(directory.path(), &request()).unwrap();
    assert!(!diagnosed.report.valid);
    assert!(!diagnosed.report.errors.is_empty());
    assert_eq!(diagnosed.source.identity.role, SourceRole::Staged);
    assert_eq!(diagnosed.source.identity.repository, *repository.identity());
    assert_eq!(
        diagnosed.source.identity.index_content,
        Some(ContentHash::of(&before))
    );
    assert_eq!(fs::read(index(directory.path())).unwrap(), before);
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(
        fs::read(directory.path().join("application.txt")).unwrap(),
        b"unstaged unrelated\n"
    );
    assert!(!directory.path().join(".agents").exists());
}

#[test]
fn staged_configuration_is_authoritative_even_when_working_configuration_is_invalid_or_missing() {
    let (directory, repository, issue) = fixture();
    let config = repository.root().join("config.yml");
    fs::write(&config, "malformed: [").unwrap();
    fs::write(
        repository.root().join(&issue.path),
        "malformed working issue",
    )
    .unwrap();
    let before = fs::read(index(directory.path())).unwrap();
    for missing in [false, true] {
        if missing {
            fs::remove_file(&config).unwrap();
        }
        let report = doctor_staged(directory.path(), &request()).unwrap();
        assert!(report.report.valid, "{:?}", report.report.errors);
        assert_eq!(report.source.identity.repository, *repository.identity());
        assert_eq!(fs::read(index(directory.path())).unwrap(), before);
        assert_eq!(config.exists(), !missing);
    }
}

#[test]
fn staged_relation_closure_detects_deleted_prerequisite_retained_in_working_tree() {
    let (directory, repository, issue) = fixture();
    let prerequisite = create(&repository, "Required issue");
    repository
        .mutate_issue_graph(
            issue.metadata.id.as_str(),
            None,
            None,
            &IssueGraphMutation::AddPrerequisite {
                prerequisite: prerequisite.metadata.id.to_string(),
            },
            &RequestId::new(),
        )
        .unwrap();
    git(directory.path(), &["add", "--", ".workdeck"]);
    git(
        directory.path(),
        &[
            "update-index",
            "--force-remove",
            "--",
            &staged_path(&prerequisite.path),
        ],
    );
    let report = doctor_staged(directory.path(), &request()).unwrap();
    assert!(!report.report.valid);
    assert!(
        report
            .report
            .errors
            .iter()
            .any(|error| error.message.contains(prerequisite.metadata.id.as_str())),
        "{:?}",
        report.report.errors
    );
    assert!(repository.root().join(prerequisite.path).is_file());
}

#[test]
fn staged_historical_receipt_proof_is_checked_without_substituting_the_live_receipt() {
    let (directory, repository, _) = fixture();
    let receipt = repository
        .create_issue(&CreateIssue::new("Receipt subject", ""), &RequestId::new())
        .unwrap();
    git(directory.path(), &["add", "--", ".workdeck"]);
    let relative = PathBuf::from(format!("operations/{}.yml", receipt.operation_id));
    let path = repository.root().join(&relative);
    let original = fs::read(&path).unwrap();
    let mut forged = receipt;
    forged.result["source"]["content"] = serde_json::json!(ContentHash::of(b"unrelated source"));
    fs::write(&path, serde_yaml_ng::to_string(&forged).unwrap()).unwrap();
    git(directory.path(), &["add", "--", &staged_path(&relative)]);
    fs::write(&path, &original).unwrap();
    let report = doctor_staged(directory.path(), &request()).unwrap();
    assert!(!report.report.valid);
    assert!(
        report
            .report
            .errors
            .iter()
            .any(|error| error.message.contains("receipt") || error.message.contains("source")),
        "{:?}",
        report.report.errors
    );
    assert_eq!(fs::read(path).unwrap(), original);
}

#[test]
fn explicit_candidate_index_is_distinct_from_default_index_and_supports_reviewed_hash() {
    let (directory, repository, issue) = fixture();
    let alternate = directory.path().join("candidate.index");
    fs::copy(index(directory.path()), &alternate).unwrap();
    let original = fs::read(&alternate).unwrap();
    fs::write(repository.root().join(&issue.path), "---\nbroken: [\n---\n").unwrap();
    git(directory.path(), &["add", "--", &staged_path(&issue.path)]);
    assert!(
        !doctor_staged(directory.path(), &request())
            .unwrap()
            .report
            .valid
    );
    let explicit = StagedDoctorRequest {
        index: IndexSelection::Explicit {
            path: alternate.clone(),
        },
        expected_index: Some(ContentHash::of(&original)),
    };
    assert!(
        doctor_staged(directory.path(), &explicit)
            .unwrap()
            .report
            .valid
    );
    let stale = StagedDoctorRequest {
        expected_index: Some(ContentHash::of(b"other index")),
        ..explicit
    };
    assert_eq!(
        doctor_staged(directory.path(), &stale).unwrap_err().code,
        ErrorCode::StaleSource
    );
    assert_eq!(fs::read(alternate).unwrap(), original);
}

#[test]
fn index_changed_during_validation_is_rejected_without_overwriting_the_new_index() {
    let (directory, _, _) = fixture();
    let mut after = None;
    let error = doctor_staged_with_faults(directory.path(), &request(), |_| {
        fs::write(
            directory.path().join("application.txt"),
            "concurrent change\n",
        )
        .map_err(|error| PmError::new(ErrorCode::Io, error.to_string()))?;
        git(directory.path(), &["add", "--", "application.txt"]);
        after = Some(fs::read(index(directory.path())).unwrap());
        Ok(())
    })
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    assert_eq!(fs::read(index(directory.path())).unwrap(), after.unwrap());
}
