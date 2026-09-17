use super::*;
use crate::{
    documents::YamlDocument,
    repository::config_from_snapshot,
    transactions::{
        ChangedPath, FaultPoint, FileChange, MutationReceipt, PreparedOperation, Snapshot,
        canonical_hash,
    },
    *,
};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
fn invalid(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::InvalidSchema, message)
}
pub(crate) fn path(id: &EvidenceId) -> PathBuf {
    Path::new("evidence").join(format!("{id}.yml"))
}
pub(crate) fn validate_path(value: &Path) -> Result<EvidenceId> {
    let id: EvidenceId = value
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| invalid("invalid evidence filename"))?
        .parse()?;
    if path(&id) != value {
        return Err(invalid("evidence path must be evidence/<EVD-ID>.yml").at(value));
    }
    Ok(id)
}
pub(crate) fn parse(
    path: &Path,
    bytes: &[u8],
    repository: &RepositoryId,
) -> Result<EvidenceRecord> {
    let id = validate_path(path)?;
    if bytes.len() > MAX_EVIDENCE_BYTES {
        return Err(invalid("evidence record exceeds 128 KiB").at(path));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("evidence must be UTF-8"))?;
    let document = YamlDocument::parse(path, text)?;
    let schema = document
        .metadata()
        .get(serde_yaml_ng::Value::String("schema".into()))
        .and_then(|v| v.as_u64())
        .ok_or_else(|| invalid("evidence schema must be an integer"))?;
    SchemaVersion::try_from(schema)?;
    let reference: EvidenceReference = document.deserialize()?;
    reference.validate()?;
    if reference.id != id || &reference.repository != repository {
        return Err(invalid("evidence filename and repository identity disagree").at(path));
    }
    Ok(EvidenceRecord {
        reference,
        path: path.into(),
        content: ContentHash::of(bytes),
        document: text.into(),
    })
}
pub(crate) fn load_evidence(
    snapshot: &Snapshot<'_>,
    config: &Config,
) -> Result<Vec<EvidenceRecord>> {
    let mut records = Vec::new();
    let mut bytes = 0usize;
    for path in snapshot.list_bounded(Path::new("evidence"), MAX_EVIDENCE_ENTRIES * 2)? {
        if records.len() >= MAX_EVIDENCE_ENTRIES {
            return Err(invalid("evidence catalog exceeds 4096 records"));
        }
        let source = snapshot
            .read_bounded(&path, MAX_EVIDENCE_BYTES)?
            .ok_or_else(|| invalid("evidence disappeared").at(&path))?;
        bytes = bytes.saturating_add(source.len());
        if bytes > 32 * 1024 * 1024 {
            return Err(invalid("evidence catalog exceeds 32 MiB"));
        }
        records.push(parse(&path, &source, &config.repository)?);
    }
    validate_chains(&records)?;
    records.sort_by(|a, b| a.reference.id.cmp(&b.reference.id));
    Ok(records)
}
pub(crate) fn validate_chains(records: &[EvidenceRecord]) -> Result<()> {
    let by_id = records
        .iter()
        .map(|r| (&r.reference.id, r))
        .collect::<BTreeMap<_, _>>();
    if by_id.len() != records.len() {
        return Err(invalid("duplicate evidence identity"));
    }
    let mut replaced = BTreeSet::new();
    for r in records {
        if let Some(previous) = &r.reference.declaration.supersedes {
            let old = by_id
                .get(&previous.id)
                .ok_or_else(|| invalid("superseded evidence is missing"))?;
            if previous.content != old.content
                || r.reference.recorded_at < old.reference.recorded_at
            {
                return Err(invalid(
                    "supersession source or historical timestamp differs",
                ));
            }
            if !replaced.insert(&previous.id) {
                return Err(invalid("evidence supersession cannot fork"));
            }
        }
        let mut seen = BTreeSet::new();
        let mut current = Some(r);
        while let Some(record) = current {
            if !seen.insert(&record.reference.id) {
                return Err(invalid("evidence supersession cannot cycle"));
            }
            current = record
                .reference
                .declaration
                .supersedes
                .as_ref()
                .and_then(|s| by_id.get(&s.id).copied());
        }
    }
    Ok(())
}
pub(crate) fn active_at(records: &[EvidenceRecord], as_of: Timestamp) -> Vec<&EvidenceRecord> {
    let replaced = records
        .iter()
        .filter(|r| r.reference.recorded_at <= as_of)
        .filter_map(|r| r.reference.declaration.supersedes.as_ref().map(|s| &s.id))
        .collect::<BTreeSet<_>>();
    records
        .iter()
        .filter(|r| r.reference.recorded_at <= as_of && !replaced.contains(&r.reference.id))
        .collect()
}
impl Repository {
    pub fn evidence(&self, id: &EvidenceId) -> Result<EvidenceRecord> {
        self.store()?.with_snapshot(|snapshot| {
            let config = config_from_snapshot(self.root(), snapshot)?;
            load_evidence(snapshot, &config)?
                .into_iter()
                .find(|r| &r.reference.id == id)
                .ok_or_else(|| PmError::new(ErrorCode::NotFound, "evidence reference not found"))
        })
    }
    pub fn evidence_references(&self, query: &EvidenceQuery) -> Result<Vec<EvidenceRecord>> {
        if let Some(criterion) = &query.criterion {
            criterion.validate()?;
        }
        self.store()?.with_snapshot(|snapshot| {
            let config = config_from_snapshot(self.root(), snapshot)?;
            let records = load_evidence(snapshot, &config)?;
            let active = active_at(&records, Timestamp::MAX_UTC)
                .into_iter()
                .map(|r| r.reference.id.clone())
                .collect::<BTreeSet<_>>();
            Ok(records
                .into_iter()
                .filter(|r| {
                    (query.include_superseded || active.contains(&r.reference.id))
                        && query
                            .criterion
                            .as_ref()
                            .is_none_or(|c| c == &r.reference.declaration.criterion)
                        && query
                            .subject
                            .as_ref()
                            .is_none_or(|s| s == &r.reference.declaration.subject)
                })
                .collect())
        })
    }
    pub fn declare_evidence(
        &self,
        input: &DeclareEvidence,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.declare_evidence_with_faults(input, request, |_| Ok(()))
    }
    #[doc(hidden)]
    pub fn declare_evidence_with_faults(
        &self,
        input: &DeclareEvidence,
        request: &RequestId,
        fault: impl FnMut(FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        let receipt = self.store()?.transact_with_faults(
            request,
            "evidence.declare",
            &json!(input),
            |snapshot| {
                let config = config_from_snapshot(self.root(), snapshot)?;
                let records = load_evidence(snapshot, &config)?;
                if records.len() >= MAX_EVIDENCE_ENTRIES {
                    return Err(invalid("evidence catalog exceeds 4096 records"));
                }
                let now = chrono::Utc::now();
                input.validate(&config.repository, now)?;
                // A declaration may cite a historical definition. Its actor/custom policy is
                // prospective; its citation is never admitted as a verification result.
                crate::organization::validate_evidence_change(snapshot, &config, input)?;
                if let Some(previous) = &input.supersedes {
                    let old = records
                        .iter()
                        .find(|r| r.reference.id == previous.id)
                        .ok_or_else(|| {
                            PmError::new(ErrorCode::NotFound, "superseded evidence not found")
                        })?;
                    if old.content != previous.content {
                        return Err(PmError::new(
                            ErrorCode::StaleSource,
                            "superseded evidence content changed",
                        ));
                    }
                    if records.iter().any(|r| {
                        r.reference
                            .declaration
                            .supersedes
                            .as_ref()
                            .is_some_and(|s| s.id == previous.id)
                    }) {
                        return Err(PmError::new(
                            ErrorCode::Conflict,
                            "evidence was already superseded",
                        ));
                    }
                }
                let reference = EvidenceReference {
                    schema: SchemaVersion::CURRENT,
                    repository: config.repository.clone(),
                    id: EvidenceId::new(),
                    recorded_at: now,
                    provenance_kind: EvidenceProvenanceKind::Declared,
                    declaration: input.clone(),
                };
                let path = path(&reference.id);
                let text =
                    serde_yaml_ng::to_string(&reference).map_err(|e| invalid(e.to_string()))?;
                let record = parse(&path, text.as_bytes(), &config.repository)?;
                let mut next = records;
                next.push(record.clone());
                validate_chains(&next)?;
                if next.iter().map(|r| r.document.len()).sum::<usize>() > 32 * 1024 * 1024 {
                    return Err(invalid("evidence catalog exceeds 32 MiB"));
                }
                Ok(PreparedOperation {
                    changes: vec![FileChange {
                        path,
                        expected: None,
                        content: Some(text.into_bytes()),
                    }],
                    result: json!(record),
                })
            },
            fault,
        )?;
        validate_receipt(&receipt)?;
        Ok(receipt)
    }
}
/// Pure receipt proof. Replays retain the original declaration even after later
/// supersession, definition edits, or identity policy changes.
pub(crate) fn validate_receipt(receipt: &MutationReceipt) -> Result<()> {
    if receipt.operation != "evidence.declare" {
        return Ok(());
    }
    let record: EvidenceRecord =
        serde_json::from_value(receipt.result.clone()).map_err(|e| invalid(e.to_string()))?;
    let parsed = parse(
        &record.path,
        record.document.as_bytes(),
        &record.reference.repository,
    )?;
    if parsed != record
        || receipt.repository.as_ref() != Some(&record.reference.repository)
        || canonical_hash(&json!(record.reference.declaration))? != receipt.input_hash
        || receipt.changed
            != vec![ChangedPath {
                path: record.path.clone(),
                before: None,
                after: Some(record.content.clone()),
            }]
    {
        return Err(invalid(
            "evidence receipt differs from its exact source or declaration intent",
        ));
    }
    Ok(())
}
pub(crate) fn inspect_snapshot(snapshot: &Snapshot<'_>, config: &Config) -> (usize, Vec<PmError>) {
    let paths = match snapshot.list_bounded(Path::new("evidence"), MAX_EVIDENCE_ENTRIES * 2) {
        Ok(p) => p,
        Err(e) => return (0, vec![e]),
    };
    let count = paths.len();
    let mut records = Vec::new();
    let mut errors = Vec::new();
    let mut total = 0usize;
    for path in paths {
        match snapshot
            .read_bounded(&path, MAX_EVIDENCE_BYTES)
            .and_then(|bytes| bytes.ok_or_else(|| invalid("evidence disappeared")))
            .and_then(|bytes| {
                total = total.saturating_add(bytes.len());
                parse(&path, &bytes, &config.repository)
            }) {
            Ok(r) => records.push(r),
            Err(e) => errors.push(e.at(&path)),
        }
        if total > 32 * 1024 * 1024 || records.len() >= MAX_EVIDENCE_ENTRIES {
            break;
        }
    }
    if count > MAX_EVIDENCE_ENTRIES || total > 32 * 1024 * 1024 {
        errors.push(invalid("evidence catalog exceeds supported bounds"));
    }
    if let Err(e) = validate_chains(&records) {
        errors.push(e);
    }
    (count, errors)
}
