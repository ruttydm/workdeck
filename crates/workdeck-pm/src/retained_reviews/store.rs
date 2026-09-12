use super::{records::*, *};
use crate::transactions::{FaultPoint, FileChange, MutationReceipt, PreparedOperation, Snapshot};
use serde_json::json;
use std::{collections::BTreeMap, path::Path};

pub(crate) fn load(
    snapshot: &Snapshot<'_>,
    repository: &RepositoryId,
) -> Result<Vec<ImportedContractReviewRecord>> {
    let paths = snapshot.list_bounded(Path::new("contract-reviews"), 512)?;
    if paths.len() > 256 {
        return Err(invalid("contract review catalog exceeds 256 reviews"));
    }
    let mut records = BTreeMap::new();
    let mut total = 0usize;
    for path in paths {
        let bytes = snapshot
            .read_bounded(&path, MAX_IMPORTED_REVIEW_BYTES)?
            .ok_or_else(|| invalid("contract review disappeared"))?;
        total = total.saturating_add(bytes.len());
        if total > 16 * 1024 * 1024 {
            return Err(invalid("contract review catalog exceeds 16 MiB"));
        }
        let record = parse(&path, &bytes, repository).map_err(|error| error.at(&path))?;
        records.insert(record.record.id.clone(), record);
    }
    // Receipts also supply inventory: deleting the last record cannot erase its history.
    let receipts = crate::execution::records::receipt_catalog(snapshot)?;
    let mut admitted = BTreeMap::new();
    for receipt in receipts.values().filter(|r| r.operation == OPERATION) {
        validate_receipt(receipt).map_err(|error| {
            error.at(Path::new("operations").join(format!("{}.yml", receipt.operation_id)))
        })?;
        let record: ImportedContractReviewRecord =
            serde_json::from_value(receipt.result.clone()).map_err(|e| invalid(&e.to_string()))?;
        if &record.record.repository != repository
            || records.get(&record.record.id) != Some(&record)
            || admitted.insert(record.record.id.clone(), ()).is_some()
        {
            return Err(invalid(
                "contract review is missing, changed, cross-repository or has duplicate admission receipts",
            ).at(&record.path));
        }
    }
    if let Some(record) = records
        .values()
        .find(|record| !admitted.contains_key(&record.record.id))
    {
        return Err(invalid("contract review lacks its original import receipt").at(&record.path));
    }
    Ok(records.into_values().collect())
}
impl Repository {
    pub fn import_contract_review(
        &self,
        input: &ImportContractReviewRequest,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.import_contract_review_with_faults(input, request, |_| Ok(()))
    }
    #[doc(hidden)]
    pub fn import_contract_review_with_faults(
        &self,
        input: &ImportContractReviewRequest,
        request: &RequestId,
        fault: impl FnMut(FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        let receipt = self.store()?.transact_with_faults(
            request,
            OPERATION,
            &json!(input),
            |snapshot| {
                let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
                let existing = load(snapshot, &config.repository)?;
                if existing.len() >= 256 {
                    return Err(invalid("contract review catalog exceeds 256 reviews"));
                }
                crate::organization::validate_actor(snapshot, &config.repository, &input.actor)?;
                let imported_at = chrono::Utc::now();
                authenticate(input, imported_at)?;
                let reviewed = ci_validate_reviewed(
                    self.root()
                        .parent()
                        .ok_or_else(|| invalid("missing worktree root"))?,
                    &CiValidateRequest {
                        base: CiRevision::Commit {
                            oid: input.baseline.commit.clone(),
                        },
                        head: CiRevision::Commit {
                            oid: input.expected_commit.clone(),
                        },
                    },
                    &input.baseline,
                    &input.policy,
                    &input.expected_policy,
                    &SignedContractReview::from_json(input.envelope.as_bytes())?,
                    imported_at,
                )?;
                if !reviewed.valid {
                    return Err(PmError::new(
                        ErrorCode::PolicyBlocked,
                        "signed review candidate fails planning validation",
                    ));
                }
                let authenticated = reviewed.admission;
                if authenticated.approval.repository != config.repository {
                    return Err(invalid("signed review belongs to a different repository"));
                }
                let record = ImportedContractReview {
                    schema: SchemaVersion::CURRENT,
                    repository: config.repository.clone(),
                    id: ContractReviewId::new(),
                    request_id: request.clone(),
                    imported_at,
                    input: input.clone(),
                    admission: authenticated,
                };
                let path = path(&record.id);
                let bytes =
                    serde_json::to_vec_pretty(&record).map_err(|e| invalid(&e.to_string()))?;
                if existing
                    .iter()
                    .map(|r| r.document.len())
                    .sum::<usize>()
                    .saturating_add(bytes.len())
                    > 16 * 1024 * 1024
                {
                    return Err(invalid("contract review catalog exceeds 16 MiB"));
                }
                let record = parse(&path, &bytes, &config.repository)?;
                Ok(PreparedOperation {
                    changes: vec![FileChange {
                        path,
                        expected: None,
                        content: Some(bytes),
                    }],
                    result: json!(record),
                })
            },
            fault,
        )?;
        validate_receipt(&receipt)?;
        Ok(receipt)
    }
    pub fn imported_contract_review_summaries(&self) -> Result<Vec<ImportedContractReviewSummary>> {
        Ok(self
            .imported_contract_reviews()?
            .into_iter()
            .map(Into::into)
            .collect())
    }
    pub fn imported_contract_reviews(&self) -> Result<Vec<ImportedContractReviewRecord>> {
        self.store()?
            .with_snapshot(|snapshot| load(snapshot, self.identity()))
    }
    pub fn imported_contract_review(
        &self,
        id: &ContractReviewId,
    ) -> Result<ImportedContractReviewRecord> {
        self.imported_contract_reviews()?
            .into_iter()
            .find(|r| &r.record.id == id)
            .ok_or_else(|| PmError::new(ErrorCode::NotFound, "imported contract review not found"))
    }
    pub fn reauthenticate_contract_review(
        &self,
        id: &ContractReviewId,
        policy: &ContractReviewPolicy,
        expected_policy: &ContentHash,
        baseline: &CiBaselinePin,
        expected_commit: &GitOid,
    ) -> Result<CiReviewedValidation> {
        let record = self.imported_contract_review(id)?;
        let envelope = SignedContractReview::from_json(record.record.input.envelope.as_bytes())?;
        ci_validate_reviewed(
            self.root()
                .parent()
                .ok_or_else(|| invalid("missing worktree root"))?,
            &CiValidateRequest {
                base: CiRevision::Commit {
                    oid: baseline.commit.clone(),
                },
                head: CiRevision::Commit {
                    oid: expected_commit.clone(),
                },
            },
            baseline,
            policy,
            expected_policy,
            &envelope,
            chrono::Utc::now(),
        )
    }
}
