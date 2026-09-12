use std::{fs, path::Path};
use workdeck_pm::*;

pub(super) fn exercise(
    repo: &Repository,
    root: &Path,
    committed: &ReviewCoverageRequest,
    issue: &IssueId,
) {
    let mut value = serde_json::to_value(committed).unwrap();
    value["working_tree"] = true.into();
    let working: ReviewCoverageRequest = serde_json::from_value(value).unwrap();
    let clean = repo.contract_review_coverage(&working).unwrap();
    assert!(clean.authenticated, "{clean:?}");
    let path = repo.root().join("checks/unit.yml");
    let original = fs::read(&path).unwrap();
    let mut definition: serde_json::Value = serde_yaml_ng::from_slice(&original).unwrap();
    definition["expectation"]["allowed_exit_codes"] = serde_json::json!([0, 1, 2]);
    fs::write(&path, serde_yaml_ng::to_string(&definition).unwrap()).unwrap();
    let dirty = repo.contract_review_coverage(&working).unwrap();
    assert!(!dirty.authenticated);
    assert_eq!(dirty.rows[0].state, ReviewCoverageState::Stale);
    assert!(
        repo.contract_review_coverage(committed)
            .unwrap()
            .authenticated
    );
    fs::write(&path, original).unwrap();
    let evaluator = root.join("evaluators/unit.sh");
    let original = fs::read(&evaluator).unwrap();
    fs::write(&evaluator, b"exit 1\n").unwrap();
    let dirty = repo.contract_review_coverage(&working).unwrap();
    assert!(!dirty.authenticated);
    assert_eq!(dirty.rows[0].state, ReviewCoverageState::Stale);
    let context = repo
        .context(&ContextRequest::new(issue.as_str(), 64 * 1024))
        .unwrap();
    assert!(context.sections.iter().flat_map(|s| &s.entries).any(|e| matches!(&e.content,
        ContextContent::ContractReview { summary, .. } if summary.state == ReviewCoverageState::Stale)));
    fs::write(&evaluator, b"exit 2\n").unwrap();
    let second = repo.contract_review_coverage(&working).unwrap();
    assert_ne!(dirty.fingerprint, second.fingerprint);
    fs::write(&evaluator, &original).unwrap();
    fs::remove_file(&evaluator).unwrap();
    let missing = repo.contract_review_coverage(&working).unwrap();
    assert!(!missing.authenticated);
    assert!(matches!(
        missing.rows[0].state,
        ReviewCoverageState::Unknown | ReviewCoverageState::Stale
    ));
    fs::write(&evaluator, &original).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let mode = fs::metadata(&evaluator).unwrap().permissions().mode();
        fs::set_permissions(&evaluator, fs::Permissions::from_mode(mode ^ 0o111)).unwrap();
        let changed = repo.contract_review_coverage(&working).unwrap();
        assert!(!changed.authenticated);
        assert_eq!(changed.rows[0].state, ReviewCoverageState::Stale);
        fs::set_permissions(&evaluator, fs::Permissions::from_mode(mode)).unwrap();
        fs::remove_file(&evaluator).unwrap();
        symlink(root.join("application.txt"), &evaluator).unwrap();
        let unsafe_input = repo.contract_review_coverage(&working).unwrap();
        assert!(!unsafe_input.authenticated);
        assert_eq!(unsafe_input.rows[0].state, ReviewCoverageState::Unknown);
        fs::remove_file(&evaluator).unwrap();
        fs::write(&evaluator, &original).unwrap();
    }
    let added = root.join("evaluators/added.sh");
    fs::write(&added, b"exit 0\n").unwrap();
    assert!(
        !repo
            .contract_review_coverage(&working)
            .unwrap()
            .authenticated
    );
    fs::remove_file(&added).unwrap();
    assert!(
        repo.contract_review_coverage(&working)
            .unwrap()
            .authenticated
    );
    let error = repo
        .context_with_faults(&ContextRequest::new(issue.as_str(), 64 * 1024), |point| {
            if point == ContextFaultPoint::BeforeSourceValidation {
                fs::write(&evaluator, b"raced\n").unwrap();
            }
            Ok(())
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    fs::write(&evaluator, original).unwrap();
}
