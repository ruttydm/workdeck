use super::*;
use crate::questions::validation::{extensions, pins, schema, text};
use crate::{
    documents::MarkdownDocument,
    repository::config_from_snapshot,
    transactions::{
        ChangedPath, FaultPoint, FileChange, MutationReceipt, PreparedOperation, Snapshot,
        canonical_hash,
    },
    *,
};
use serde_json::json;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
const MAX_TOTAL: usize = 32 * 1024 * 1024;
fn issue_path(id: &IssueId) -> PathBuf {
    Path::new("issues").join(id.as_str()).join("item.md")
}
pub(crate) fn path(issue: &IssueId, id: &HandoffId) -> PathBuf {
    Path::new("issues")
        .join(issue.as_str())
        .join("handoffs")
        .join(format!("{id}.md"))
}
pub(crate) fn validate_path(value: &Path) -> Result<(IssueId, HandoffId)> {
    let parts = value.iter().map(|s| s.to_str()).collect::<Vec<_>>();
    if parts.len() != 4 || parts[0] != Some("issues") || parts[2] != Some("handoffs") {
        return Err(invalid("handoff path must be issues/<ISSUE>/handoffs/<H-ID>.md").at(value));
    }
    let issue: IssueId = parts[1]
        .ok_or_else(|| invalid("issue path must be UTF-8"))?
        .parse()?;
    let id: HandoffId = value
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| invalid("handoff filename must be UTF-8"))?
        .parse()?;
    if path(&issue, &id) != value {
        return Err(invalid("handoff filename differs from canonical identity").at(value));
    }
    Ok((issue, id))
}
fn validate_input(input: &CreateHandoff, repository: &RepositoryId) -> Result<()> {
    text(&input.actor, "handoff actor", 256, false)?;
    text(&input.body, "handoff summary", 64 * 1024, true)?;
    extensions(&input.custom, &input.extra)?;
    if &input.anchor.repository != repository {
        return Err(invalid("handoff anchor belongs to another repository"));
    }
    pins(&input.anchor.source_pins)?;
    let issue_path = issue_path(&input.anchor.issue);
    if !input
        .anchor
        .source_pins
        .iter()
        .any(|p| p.path == issue_path && p.content == input.anchor.issue_source.content)
    {
        return Err(invalid("handoff anchor must pin its exact issue source"));
    }
    for list in [&input.attempted, &input.uncertainties, &input.next_steps] {
        if list.len() > 128 {
            return Err(invalid("handoff sections support at most 128 entries"));
        }
        for value in list {
            text(value, "handoff annotation", 4096, true)?;
        }
    }
    if input.evidence_refs.len() > 128
        || input.questions.len() > 128
        || input.pending_operations.len() > 128
    {
        return Err(invalid(
            "handoff reference limit is 128 entries per section",
        ));
    }
    let mut evidence = BTreeSet::new();
    for r in &input.evidence_refs {
        if !evidence.insert(&r.id) {
            return Err(invalid("duplicate handoff evidence reference"));
        }
    }
    let mut questions = BTreeSet::new();
    for q in &input.questions {
        if !questions.insert(q) {
            return Err(invalid("duplicate handoff question reference"));
        }
    }
    let mut requests = BTreeSet::new();
    for op in &input.pending_operations {
        if op.operation_id.is_some() != op.receipt_content.is_some()
            || !requests.insert(&op.request_id)
        {
            return Err(invalid(
                "operation references require paired operation/hash and distinct request IDs",
            ));
        }
    }
    Ok(())
}
fn input(record: &HandoffRecord) -> CreateHandoff {
    let m = &record.metadata;
    CreateHandoff {
        actor: m.actor.clone(),
        anchor: m.anchor.clone(),
        body: record.body.clone(),
        attempted: m.attempted.clone(),
        uncertainties: m.uncertainties.clone(),
        evidence_refs: m.evidence_refs.clone(),
        questions: m.questions.clone(),
        pending_operations: m.pending_operations.clone(),
        next_steps: m.next_steps.clone(),
        custom: m.custom.clone(),
        extra: m.extra.clone(),
    }
}
pub(crate) fn parse(path: &Path, bytes: &[u8], repository: &RepositoryId) -> Result<HandoffRecord> {
    let (issue, id) = validate_path(path)?;
    if bytes.len() > MAX_HANDOFF_BYTES {
        return Err(invalid("handoff exceeds 128 KiB").at(path));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("handoff must be UTF-8"))?;
    let doc = MarkdownDocument::parse(path, text)?;
    schema(doc.metadata(), path)?;
    let metadata: HandoffMetadata = doc.deserialize()?;
    if metadata.issue != issue
        || metadata.id != id
        || &metadata.repository != repository
        || metadata.anchor.issue != issue
    {
        return Err(invalid("handoff identity, anchor, and path disagree").at(path));
    }
    let record = HandoffRecord {
        metadata,
        body: doc.body().into(),
        path: path.into(),
        content: ContentHash::of(bytes),
        document: text.into(),
    };
    validate_input(&input(&record), repository)?;
    Ok(record)
}
fn all(root: &Path, snapshot: &Snapshot<'_>, config: &Config) -> Result<Vec<HandoffRecord>> {
    let mut records = Vec::new();
    let mut ids = BTreeSet::new();
    let mut total = 0usize;
    for path in snapshot.list_bounded(Path::new("issues"), 20_000)? {
        if path.iter().nth(2).is_none_or(|s| s != "handoffs") {
            continue;
        }
        if records.len() >= MAX_HANDOFFS {
            return Err(invalid("handoff catalog exceeds 4096 records"));
        }
        let bytes = snapshot
            .read_bounded(&path, MAX_HANDOFF_BYTES)?
            .ok_or_else(|| invalid("handoff disappeared"))?;
        total = total.saturating_add(bytes.len());
        if total > MAX_TOTAL {
            return Err(invalid("handoff catalog exceeds 32 MiB"));
        }
        let record = parse(&path, &bytes, &config.repository)?;
        if !ids.insert(record.metadata.id.clone()) {
            return Err(invalid("handoff ID is duplicated across issue directories"));
        }
        if snapshot
            .read_bounded(
                &issue_path(&record.metadata.issue),
                crate::documents::MAX_DOCUMENT_BYTES,
            )?
            .is_none()
        {
            return Err(invalid("handoff issue item is missing").at(root.join(&path)));
        }
        records.push(record);
    }
    records.sort_by(|a, b| {
        a.metadata
            .created_at
            .cmp(&b.metadata.created_at)
            .then_with(|| a.metadata.id.cmp(&b.metadata.id))
    });
    Ok(records)
}
pub(crate) fn load_handoffs(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    issue: &IssueId,
) -> Result<Vec<HandoffRecord>> {
    Ok(all(root, snapshot, config)?
        .into_iter()
        .filter(|r| &r.metadata.issue == issue)
        .collect())
}
pub(crate) fn load_handoff(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    issue: &IssueId,
    id: &HandoffId,
) -> Result<HandoffRecord> {
    load_handoffs(root, snapshot, config, issue)?
        .into_iter()
        .find(|r| &r.metadata.id == id)
        .ok_or_else(|| PmError::new(ErrorCode::NotFound, "handoff not found"))
}
fn live_refs(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    input: &CreateHandoff,
) -> Result<()> {
    validate_input(input, &config.repository)?;
    crate::organization::validate_actor(snapshot, &config.repository, &input.actor)?;
    let (_, _, inactive) = crate::questions::validation::subject_record(
        root,
        snapshot,
        config,
        &SubjectRef::Issue(input.anchor.issue.clone()),
    )?;
    if inactive {
        return Err(PmError::new(
            ErrorCode::PolicyBlocked,
            "new handoff requires an active issue",
        ));
    }
    for r in &input.evidence_refs {
        let path = crate::evidence::store::path(&r.id);
        let bytes = snapshot
            .read_bounded(&path, crate::MAX_EVIDENCE_BYTES)?
            .ok_or_else(|| PmError::new(ErrorCode::NotFound, "handoff evidence missing"))?;
        crate::evidence::store::parse(&path, &bytes, &config.repository)?;
        if ContentHash::of(&bytes) != r.content {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "handoff evidence changed",
            ));
        }
    }
    for id in &input.questions {
        let q = crate::questions::load_question(root, snapshot, config, id)?;
        if !q
            .metadata
            .subjects
            .iter()
            .any(|s| s.subject == SubjectRef::Issue(input.anchor.issue.clone()))
        {
            return Err(invalid(
                "explicit handoff question must include its issue subject",
            ));
        }
    }
    let mut remaining_receipts = 32 * 1024 * 1024usize;
    for op in &input.pending_operations {
        if let (Some(id), Some(content)) = (&op.operation_id, &op.receipt_content) {
            let path = Path::new("operations").join(format!("{id}.yml"));
            let bytes = snapshot
                .read_bounded(
                    &path,
                    crate::documents::MAX_DOCUMENT_BYTES.min(remaining_receipts),
                )?
                .ok_or_else(|| {
                    PmError::new(ErrorCode::NotFound, "referenced operation receipt missing")
                })?;
            remaining_receipts = remaining_receipts.saturating_sub(bytes.len());
            let receipt: MutationReceipt =
                serde_yaml_ng::from_slice(&bytes).map_err(|e| invalid(e.to_string()))?;
            crate::transactions::validate_receipt(&receipt)?;
            if receipt.operation_id != *id
                || receipt.request_id != op.request_id
                || receipt.repository.as_ref() != Some(&config.repository)
                || ContentHash::of(&bytes) != *content
            {
                return Err(PmError::new(
                    ErrorCode::StaleSource,
                    "handoff operation reference differs from its durable receipt",
                ));
            }
        }
    }
    Ok(())
}
impl Repository {
    pub fn handoffs(&self, issue: &IssueId) -> Result<Vec<HandoffRecord>> {
        self.store()?.with_snapshot(|snapshot| {
            let config = config_from_snapshot(self.root(), snapshot)?;
            crate::issues::resolve_issue(self.root(), snapshot, &config, issue.as_str())?;
            load_handoffs(self.root(), snapshot, &config, issue)
        })
    }
    pub fn handoff(&self, issue: &IssueId, id: &HandoffId) -> Result<HandoffRecord> {
        self.store()?.with_snapshot(|snapshot| {
            let config = config_from_snapshot(self.root(), snapshot)?;
            crate::handoffs::load_handoff(self.root(), snapshot, &config, issue, id)
        })
    }
    pub fn create_handoff(
        &self,
        input: &CreateHandoff,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.create_handoff_with_faults(input, request, |_| Ok(()))
    }
    #[doc(hidden)]
    pub fn create_handoff_with_faults(
        &self,
        input: &CreateHandoff,
        request: &RequestId,
        fault: impl FnMut(FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        let receipt = self.store()?.transact_with_faults(
            request,
            "handoff.create",
            &json!(input),
            |snapshot| {
                let config = config_from_snapshot(self.root(), snapshot)?;
                live_refs(self.root(), snapshot, &config, input)?;
                let records = all(self.root(), snapshot, &config)?;
                if records.len() >= MAX_HANDOFFS {
                    return Err(invalid("handoff catalog exceeds 4096 records"));
                }
                let current = crate::context::anchor_for_issue(
                    self.root(),
                    snapshot,
                    &config,
                    &input.anchor.issue,
                )?;
                if current != input.anchor {
                    return Err(PmError::new(
                        ErrorCode::StaleSource,
                        "handoff context anchor changed",
                    )
                    .details(json!({"expected":input.anchor,"current":current})));
                }
                let id = HandoffId::new();
                let metadata = HandoffMetadata {
                    schema: SchemaVersion::CURRENT,
                    repository: config.repository.clone(),
                    id: id.clone(),
                    issue: input.anchor.issue.clone(),
                    created_at: chrono::Utc::now(),
                    actor: input.actor.clone(),
                    anchor: input.anchor.clone(),
                    attempted: input.attempted.clone(),
                    uncertainties: input.uncertainties.clone(),
                    evidence_refs: input.evidence_refs.clone(),
                    questions: input.questions.clone(),
                    pending_operations: input.pending_operations.clone(),
                    next_steps: input.next_steps.clone(),
                    custom: input.custom.clone(),
                    extra: input.extra.clone(),
                };
                let path = path(&input.anchor.issue, &id);
                let text = format!(
                    "---\n{}---\n{}",
                    serde_yaml_ng::to_string(&metadata).map_err(|e| invalid(e.to_string()))?,
                    input.body
                );
                let record = parse(&path, text.as_bytes(), &config.repository)?;
                if records.iter().map(|r| r.document.len()).sum::<usize>() + record.document.len()
                    > MAX_TOTAL
                {
                    return Err(invalid("handoff catalog exceeds 32 MiB"));
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
pub(crate) fn validate_receipt(receipt: &MutationReceipt) -> Result<()> {
    if receipt.operation != "handoff.create" {
        return Ok(());
    }
    let record: HandoffRecord =
        serde_json::from_value(receipt.result.clone()).map_err(|e| invalid(e.to_string()))?;
    if parse(
        &record.path,
        record.document.as_bytes(),
        &record.metadata.repository,
    )? != record
        || receipt.repository.as_ref() != Some(&record.metadata.repository)
        || receipt.input_hash != canonical_hash(&json!(input(&record)))?
        || receipt.changed
            != vec![ChangedPath {
                path: record.path.clone(),
                before: None,
                after: Some(record.content.clone()),
            }]
    {
        return Err(invalid(
            "handoff receipt differs from exact source, identity, or authored intent",
        ));
    }
    Ok(())
}
pub(crate) fn inspect(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
) -> (usize, Vec<PmError>) {
    let paths = match snapshot.list_bounded(Path::new("issues"), 20_000) {
        Ok(p) => p,
        Err(e) => return (0, vec![e]),
    };
    let mut records = Vec::new();
    let mut errors = Vec::new();
    let mut count = 0;
    let mut total = 0usize;
    let mut ids = BTreeSet::new();
    for path in paths {
        if path.iter().nth(2).is_none_or(|s| s != "handoffs") {
            continue;
        }
        count += 1;
        match snapshot
            .read_bounded(&path, MAX_HANDOFF_BYTES)
            .and_then(|b| b.ok_or_else(|| invalid("handoff disappeared")))
            .and_then(|b| {
                total = total.saturating_add(b.len());
                parse(&path, &b, &config.repository)
            }) {
            Ok(record) => {
                if !ids.insert(record.metadata.id.clone()) {
                    errors.push(invalid("duplicate global handoff ID").at(&path));
                }
                let item = issue_path(&record.metadata.issue);
                match snapshot.read_bounded(&item, crate::documents::MAX_DOCUMENT_BYTES) {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        errors.push(invalid("orphan handoff: issue item missing").at(&path))
                    }
                    Err(e) => errors.push(e),
                }
                records.push(record);
            }
            Err(e) => errors.push(e.at(&path)),
        }
        if total > MAX_TOTAL || count > MAX_HANDOFFS {
            errors.push(invalid("handoff catalog exceeds bounds"));
            break;
        }
    }
    let _ = root;
    (count, errors)
}
pub(crate) fn validate_import(
    root: &Path,
    projected: &Snapshot<'_>,
    config: &Config,
    path: &Path,
) -> Result<()> {
    let bytes = projected
        .read_bounded(path, MAX_HANDOFF_BYTES)?
        .ok_or_else(|| invalid("imported handoff missing"))?;
    let record = parse(path, &bytes, &config.repository)?;
    // Imported continuity remains historical. Actor/reference validation does not
    // upgrade its captured basis to today's context or admit verification.
    crate::organization::validate_actor(projected, &config.repository, &record.metadata.actor)?;
    let item = issue_path(&record.metadata.issue);
    if projected
        .read_bounded(&item, crate::documents::MAX_DOCUMENT_BYTES)?
        .is_none()
    {
        return Err(invalid("imported handoff issue is missing").at(root.join(path)));
    }
    Ok(())
}
