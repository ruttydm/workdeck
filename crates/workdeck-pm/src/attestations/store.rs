use super::{records::*, *};
use crate::transactions::{FaultPoint, FileChange, MutationReceipt, PreparedOperation, Snapshot};
use serde_json::json;
use std::{collections::BTreeMap, path::Path};

pub(crate) fn load(
    snapshot: &Snapshot<'_>,
    repository: &RepositoryId,
) -> Result<Vec<ImportedCheckReportRecord>> {
    let paths = snapshot.list_bounded(Path::new("attestations"), 512)?;
    if paths.len() > 256 {
        return Err(invalid("attestation catalog exceeds 256 reports"));
    }
    let mut records = BTreeMap::new();
    let mut total = 0usize;
    for path in paths {
        let bytes = snapshot
            .read_bounded(&path, MAX_IMPORTED_REPORT_BYTES)?
            .ok_or_else(|| invalid("attestation disappeared"))?;
        total = total.saturating_add(bytes.len());
        if total > 64 * 1024 * 1024 {
            return Err(invalid("attestation catalog exceeds 64 MiB"));
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
        let record: ImportedCheckReportRecord =
            serde_json::from_value(receipt.result.clone()).map_err(|e| invalid(&e.to_string()))?;
        if &record.record.repository != repository
            || records.get(&record.record.id) != Some(&record)
            || admitted.insert(record.record.id.clone(), ()).is_some()
        {
            return Err(invalid(
                "attestation is missing, changed, cross-repository or has duplicate admission receipts",
            ).at(&record.path));
        }
    }
    if let Some(record) = records
        .values()
        .find(|record| !admitted.contains_key(&record.record.id))
    {
        return Err(invalid("attestation lacks its original import receipt").at(&record.path));
    }
    Ok(records.into_values().collect())
}
impl Repository {
    pub fn import_check_report(
        &self,
        input: &ImportCheckReportRequest,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.import_check_report_with_faults(input, request, |_| Ok(()))
    }
    #[doc(hidden)]
    pub fn import_check_report_with_faults(
        &self,
        input: &ImportCheckReportRequest,
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
                    return Err(invalid("attestation catalog exceeds 256 reports"));
                }
                crate::organization::validate_actor(snapshot, &config.repository, &input.actor)?;
                let imported_at = chrono::Utc::now();
                let authenticated = authenticate(input, imported_at)?;
                if authenticated.source.repository != config.repository {
                    return Err(invalid("signed report belongs to a different repository"));
                }
                if let Some(proof) = &input.red_green {
                    proof.verify_import(
                        self.root()
                            .parent()
                            .ok_or_else(|| invalid("missing worktree root"))?,
                        input,
                    )?;
                }
                let record = ImportedCheckReport {
                    schema: SchemaVersion::CURRENT,
                    repository: config.repository.clone(),
                    id: AttestationId::new(),
                    request_id: request.clone(),
                    imported_at,
                    input: input.clone(),
                    admission: (&authenticated).into(),
                };
                let path = path(&record.id);
                let bytes =
                    serde_json::to_vec_pretty(&record).map_err(|e| invalid(&e.to_string()))?;
                if existing
                    .iter()
                    .map(|r| r.document.len())
                    .sum::<usize>()
                    .saturating_add(bytes.len())
                    > 64 * 1024 * 1024
                {
                    return Err(invalid("attestation catalog exceeds 64 MiB"));
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
    pub fn imported_check_report_summaries(&self) -> Result<Vec<ImportedCheckReportSummary>> {
        Ok(self
            .imported_check_reports()?
            .into_iter()
            .map(Into::into)
            .collect())
    }
    pub fn imported_check_reports(&self) -> Result<Vec<ImportedCheckReportRecord>> {
        self.store()?
            .with_snapshot(|snapshot| load(snapshot, self.identity()))
    }
    pub fn imported_check_report(&self, id: &AttestationId) -> Result<ImportedCheckReportRecord> {
        self.imported_check_reports()?
            .into_iter()
            .find(|r| &r.record.id == id)
            .ok_or_else(|| PmError::new(ErrorCode::NotFound, "imported check report not found"))
    }
    pub fn reauthenticate_imported_report(
        &self,
        id: &AttestationId,
        policy: &ProducerTrustPolicy,
        expected_policy: &ContentHash,
        expected_commit: &GitOid,
    ) -> Result<AuthenticatedCheckReport> {
        let record = self.imported_check_report(id)?;
        let envelope = SignedCheckReport::from_json(record.record.input.envelope.as_bytes())?;
        authenticate_check_report(
            &envelope,
            policy,
            expected_policy,
            expected_commit,
            chrono::Utc::now(),
        )
    }
}
