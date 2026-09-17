use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use workdeck_pm::transactions::MutationReceipt;
use workdeck_pm::*;

fn fixture() -> (tempfile::TempDir, Repository, IssueRecord) {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let issue = new_issue(&repo);
    (temp, repo, issue)
}

fn new_issue(repo: &Repository) -> IssueRecord {
    serde_json::from_value(
        repo.create_issue(
            &CreateIssue::new("Bounded work", "Keep this source readable.\n"),
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap()
}

fn acquire(repo: &Repository, issue: &IssueId) -> ClaimRequest {
    ClaimRequest::Acquire {
        input: Box::new(AcquireClaim {
            actor: "worker".into(),
            contract: repo.local_claim_contract(issue).unwrap(),
            ttl_seconds: None,
            recovery: None,
        }),
    }
}

fn after(receipt: MutationReceipt) -> ClaimRecord {
    serde_json::from_value::<ClaimChange>(receipt.result)
        .unwrap()
        .after
}

fn authority(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, relative: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(root.join(relative)).unwrap() {
            let entry = entry.unwrap();
            if relative.as_os_str().is_empty()
                && matches!(
                    entry.file_name().to_str(),
                    Some(".tmp" | ".local" | ".index" | "index")
                )
            {
                continue;
            }
            let path = relative.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                visit(root, &path, files);
            } else {
                files.insert(path, std::fs::read(entry.path()).unwrap());
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, Path::new(""), &mut files);
    files
}

fn operation_usage(repo: &Repository) -> (usize, usize) {
    authority(repo.root())
        .into_iter()
        .filter(|(path, _)| path.starts_with("operations"))
        .fold((0, 0), |(count, bytes), (_, document)| {
            (count + 1, bytes + document.len())
        })
}

fn rejects_without_publication(
    repo: &Repository,
    request: &ClaimRequest,
    limits: &ClaimCatalogLimits,
) {
    let before = authority(repo.root());
    let claims = repo
        .local_claims_with_limits(limits)
        .unwrap()
        .into_iter()
        .map(|status| status.claim)
        .collect::<Vec<_>>();
    let error = repo
        .mutate_local_claim_with_limits(request, &RequestId::new(), limits)
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidInput, "{error}");
    assert_eq!(
        authority(repo.root()),
        before,
        "rejected capacity must not publish a claim or its receipt"
    );
    assert_eq!(
        repo.local_claims_with_limits(limits)
            .unwrap()
            .into_iter()
            .map(|status| status.claim)
            .collect::<Vec<_>>(),
        claims
    );
    assert!(
        repo.pending_operations().unwrap().is_empty(),
        "capacity rejection must precede the recovery barrier"
    );
}

#[test]
fn next_claim_cannot_exceed_the_catalog_record_limit() {
    let (_temp, repo, first) = fixture();
    let second = new_issue(&repo);
    let limits = ClaimCatalogLimits {
        max_claims: 1,
        ..Default::default()
    };
    let request = acquire(&repo, &first.metadata.id);
    let id = RequestId::new();
    let receipt = repo
        .mutate_local_claim_with_limits(&request, &id, &limits)
        .unwrap();
    rejects_without_publication(&repo, &acquire(&repo, &second.metadata.id), &limits);
    assert_eq!(
        repo.mutate_local_claim_with_limits(&request, &id, &limits)
            .unwrap(),
        receipt
    );
}

#[test]
fn growing_claim_cannot_exceed_the_catalog_document_byte_limit() {
    let (_temp, repo, issue) = fixture();
    let owned = after(
        repo.mutate_local_claim(&acquire(&repo, &issue.metadata.id), &RequestId::new())
            .unwrap(),
    );
    let limits = ClaimCatalogLimits {
        max_claim_bytes: std::fs::read(repo.root().join(&owned.path)).unwrap().len(),
        ..Default::default()
    };
    let request = ClaimRequest::Mutate {
        issue: issue.metadata.id,
        expected: owned.precondition(),
        mutation: ClaimMutation::Release {
            actor: "worker".into(),
            reason: "Reviewed bounded handoff. ".repeat(20),
        },
    };
    rejects_without_publication(&repo, &request, &limits);
}

#[test]
fn next_claim_receipt_cannot_exceed_the_operation_count_limit() {
    let (_temp, repo, issue) = fixture();
    let limits = ClaimCatalogLimits {
        max_operations: operation_usage(&repo).0,
        ..Default::default()
    };
    rejects_without_publication(&repo, &acquire(&repo, &issue.metadata.id), &limits);
}

#[test]
fn next_claim_receipt_cannot_exceed_the_operation_byte_limit() {
    let (_temp, repo, issue) = fixture();
    let limits = ClaimCatalogLimits {
        max_operation_bytes: operation_usage(&repo).1,
        ..Default::default()
    };
    rejects_without_publication(&repo, &acquire(&repo, &issue.metadata.id), &limits);
}

#[test]
fn exact_future_receipt_size_fits_and_one_byte_less_rejects() {
    let (_temp, repo, issue) = fixture();
    let now = chrono::Utc::now();
    let owned = after(
        repo.mutate_local_claim_with_faults(
            &acquire(&repo, &issue.metadata.id),
            &RequestId::new(),
            || now,
            |_| Ok(()),
        )
        .unwrap(),
    );
    let request = ClaimRequest::Mutate {
        issue: issue.metadata.id,
        expected: owned.precondition(),
        mutation: ClaimMutation::Release {
            actor: "worker".into(),
            reason: "Exact receipt boundary, including \"quoted\" text. and Unicode é.".into(),
        },
    };
    let id: RequestId = "exact-boundary-request".parse().unwrap();
    let before = authority(repo.root());
    let before_bytes = operation_usage(&repo).1;
    let receipt = repo
        .mutate_local_claim_with_faults(&request, &id, || now, |_| Ok(()))
        .unwrap();
    let receipt_path = PathBuf::from(format!("operations/{}.yml", receipt.operation_id));
    let receipt_bytes = std::fs::read(repo.root().join(receipt_path)).unwrap().len();

    for short in [true, false] {
        let copy = tempfile::tempdir().unwrap();
        let root = copy.path().join(".workdeck");
        for (path, bytes) in &before {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        let repo = Repository::open_source(&root).unwrap();
        let limits = ClaimCatalogLimits {
            max_operation_bytes: before_bytes + receipt_bytes - usize::from(short),
            ..Default::default()
        };
        let result = repo.mutate_local_claim_with_limits_and_faults(
            &request,
            &id,
            &limits,
            || now,
            |_| Ok(()),
        );
        if short {
            assert_eq!(result.unwrap_err().code, ErrorCode::InvalidInput);
            assert_eq!(authority(repo.root()), before);
        } else {
            let actual = result.unwrap();
            assert_eq!(actual.result, receipt.result);
            assert_eq!(operation_usage(&repo).1, limits.max_operation_bytes);
            assert_eq!(repo.local_claims_with_limits(&limits).unwrap().len(), 1);
            assert_eq!(
                repo.mutate_local_claim_with_limits_and_faults(
                    &request,
                    &id,
                    &limits,
                    || now,
                    |_| Ok(())
                )
                .unwrap(),
                actual
            );
        }
    }
}
