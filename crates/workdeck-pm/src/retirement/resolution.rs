//! Reviewed removal of incoming issue associations before permanent retirement.
use super::*;
use crate::{IssueMutation, UpdateIssue, transactions::ChangedPath};
use std::collections::BTreeMap;

pub(super) const OPERATION: &str = "reference.retire";
const MAX_ENTRIES: usize = 20_000;
const MAX_AFFECTED: usize = 1_000;
const MAX_HISTORY_BYTES: usize = 64 * 1024 * 1024;

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceRetirementChange {
    pub issue: IssueId,
    pub path: PathBuf,
    pub field: String,
    pub source: SourceToken,
    pub before: Value,
    pub after: Value,
    pub history: Vec<RetainedFile>,
    /// Binds every semantic field and body that this operation must preserve.
    pub preserved: ContentHash,
    pub updated_at: Timestamp,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceRetirementPlan {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub target: RetirementTarget,
    pub source: SourceToken,
    pub fingerprint: ContentHash,
    pub allowed: bool,
    pub affected: Vec<ReferenceRetirementChange>,
    pub blockers: Vec<PmError>,
    /// Existing retirement scope binds configuration, all issue membership,
    /// target source bytes, and its independent authored history.
    pub membership: RetirementPreview,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceRetirementOutcome {
    pub plan: ReferenceRetirementPlan,
    pub retirement: RetirementOutcome,
    pub affected: Vec<IssueRecord>,
    pub input: RetirementInput,
}

impl Repository {
    pub fn reference_retirement_preview(
        &self,
        target: &RetirementTarget,
    ) -> Result<ReferenceRetirementPlan> {
        self.store()?.with_snapshot(|snapshot| {
            let config = config_from_snapshot(self.root(), snapshot)?;
            build(self.root(), snapshot, &config, target).map(|prepared| prepared.plan)
        })
    }

    pub fn retire_reference(
        &self,
        input: &RetirementInput,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.retire_reference_with_faults(input, request, |_| Ok(()))
    }

    /// The same durable transaction path with explicit crash/race instrumentation.
    #[doc(hidden)]
    pub fn retire_reference_with_faults(
        &self,
        input: &RetirementInput,
        request: &RequestId,
        fault: impl FnMut(FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        let receipt = self.store()?.transact_with_faults(
            request,
            OPERATION,
            &value(input)?,
            |snapshot| {
                let expected = input.expected_preview.as_ref().ok_or_else(|| {
                    PmError::new(ErrorCode::InvalidInput, "reference retirement requires a reviewed resolution fingerprint")
                        .hint("Preview reference retirement, then apply with its expected-preview fingerprint.")
                })?;
                let config = config_from_snapshot(self.root(), snapshot)?;
                let mut prepared = build(self.root(), snapshot, &config, &input.target)?;
                if expected != &prepared.plan.fingerprint
                    || input.expected.as_ref().is_some_and(|source| source != &prepared.plan.source)
                {
                    return Err(PmError::new(ErrorCode::StaleSource, "reference resolution source, membership, policy, or history changed after review")
                        .at(prepared.original.path()));
                }
                if !prepared.plan.allowed {
                    return Err(PmError::new(ErrorCode::PolicyBlocked, "reference resolution is blocked by issue mutation policy")
                        .hint("Explicitly reopen accepted work before changing its associations, then review a new resolution plan.")
                        .details(value(&prepared.plan)?));
                }
                let retirement = prepare_tombstone(self.root(), snapshot, &config, &input.target, &prepared.original, request)?;
                prepared.changes.extend(retirement.changes);
                let outcome = ReferenceRetirementOutcome {
                    plan: prepared.plan,
                    retirement: serde_json::from_value(retirement.result).map_err(|error| invalid(error.to_string()))?,
                    affected: prepared.affected,
                    input: input.clone(),
                };
                let result = value(&outcome)?;
                // Qualify the complete result before the transaction engine can
                // publish any source bytes. The engine assigns the actual ID;
                // this proof has no dependency on that generated identity.
                validate_receipt(&MutationReceipt {
                    schema_version: SchemaVersion::CURRENT,
                    repository: Some(config.repository.clone()),
                    operation_id: "OP-00000000000000000000000000".parse().expect("valid fixed proof identity"),
                    request_id: request.clone(), operation: OPERATION.into(),
                    input_hash: crate::transactions::canonical_hash(&value(input)?)?,
                    result: result.clone(),
                    changed: prepared.changes.iter().map(|change| ChangedPath {
                        path: change.path.clone(), before: change.expected.clone(),
                        after: change.content.as_deref().map(ContentHash::of),
                    }).collect(),
                }, &config.repository)?;
                Ok(PreparedOperation { changes: prepared.changes, result })
            },
            fault,
        )?;
        // Validate only the historical result on replay. Current issue content
        // may have legitimately changed since this operation succeeded.
        validate_receipt(&receipt, self.identity())?;
        Ok(receipt)
    }
}

struct PreparedResolution {
    plan: ReferenceRetirementPlan,
    original: RetiredRecord,
    changes: Vec<FileChange>,
    affected: Vec<IssueRecord>,
}

fn build(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    target: &RetirementTarget,
) -> Result<PreparedResolution> {
    target.validate()?;
    if matches!(
        target.kind,
        RetirementKind::Issue | RetirementKind::Feature | RetirementKind::Gate
    ) {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "reference resolution supports planning records",
        ));
    }
    // Set strict traversal budgets before existing shared loaders consult these
    // prefixes; successful limits are retained in transaction recovery readsets.
    for prefix in [
        "issues",
        "operations",
        "tombstones",
        target.kind.directory(),
    ] {
        snapshot.list_bounded(Path::new(prefix), MAX_ENTRIES)?;
    }
    let (membership, original) = build_preview(root, snapshot, config, target)?;
    if membership.blockers.len() > MAX_AFFECTED {
        return Err(PmError::new(
            ErrorCode::Unsupported,
            "reference resolution exceeds 1000 affected issues",
        ));
    }
    let issues: BTreeMap<_, _> = load_issues(root, snapshot, config)?
        .into_iter()
        .map(|issue| (issue.metadata.id.clone(), issue))
        .collect();
    let mut affected = Vec::new();
    let mut rows = Vec::new();
    let mut blockers = membership.planning_blockers.iter().map(|member| PmError::new(ErrorCode::PolicyBlocked,
        format!("incoming {:?} {} {} association must be explicitly reassigned or cleared before retirement; archival retains the link", member.kind, member.id, member.field)).at(&member.path)).collect::<Vec<_>>();
    blockers.extend(membership.record_blockers.iter().map(|member| PmError::new(ErrorCode::PolicyBlocked,
        format!("incoming {:?} {} {} association must be explicitly reassigned or cleared before retirement; archival retains the link", member.kind, member.id, member.field)).at(&member.path)));
    blockers.extend(membership.question_blockers.iter().map(|member| {
        PmError::new(
            ErrorCode::PolicyBlocked,
            format!(
                "open question {} must be answered or superseded before retirement",
                member.question
            ),
        )
        .at(&member.path)
    }));
    let mut changes = Vec::new();
    let mut history_bytes = 0usize;
    for member in &membership.blockers {
        let issue = issues
            .get(&member.issue)
            .ok_or_else(|| corrupt("reviewed issue membership disappeared"))?;
        let before = association(issue, &member.field)?;
        let after = cleared(target, &member.field, &before)?;
        rows.push(ReferenceRetirementChange {
            issue: issue.metadata.id.clone(),
            path: issue.path.clone(),
            field: member.field.clone(),
            source: issue.source.clone(),
            before,
            after: after.clone(),
            history: retained_history(snapshot, issue, &mut history_bytes)?,
            preserved: preserved(issue, &member.field)?,
            updated_at: issue.metadata.updated_at,
        });
        let mutation = IssueMutation::Update {
            input: UpdateIssue {
                fields: BTreeMap::from([(member.field.clone(), after)]),
                body: None,
            },
        };
        match crate::issues::prepare_issue_mutation(
            root,
            snapshot,
            config,
            issue.clone(),
            Some(&issue.source),
            &mutation,
        ) {
            Ok(prepared) => {
                affected.push(
                    serde_json::from_value(prepared.result)
                        .map_err(|error| invalid(error.to_string()))?,
                );
                changes.extend(prepared.changes);
            }
            Err(error) if error.code == ErrorCode::PolicyBlocked => {
                blockers.push(error.at(&issue.path))
            }
            Err(error) => return Err(error),
        }
    }
    let mut plan = ReferenceRetirementPlan {
        schema: SchemaVersion::CURRENT,
        repository: config.repository.clone(),
        target: target.clone(),
        source: membership.source.clone(),
        fingerprint: ContentHash::of(&[]),
        allowed: blockers.is_empty(),
        affected: rows,
        blockers,
        membership,
    };
    plan.fingerprint = fingerprint(&plan)?;
    Ok(PreparedResolution {
        plan,
        original,
        changes,
        affected,
    })
}

fn fingerprint(plan: &ReferenceRetirementPlan) -> Result<ContentHash> {
    hash(&(
        plan.schema,
        &plan.repository,
        &plan.target,
        &plan.source,
        plan.allowed,
        &plan.affected,
        &plan.blockers,
        &plan.membership,
    ))
}

fn association(issue: &IssueRecord, field: &str) -> Result<Value> {
    if field == "labels" {
        return Ok(json!(issue.metadata.labels));
    }
    if field == "targets" {
        return Ok(json!(issue.metadata.targets));
    }
    Ok(value(&issue.metadata)?
        .get(field)
        .cloned()
        .unwrap_or(Value::Null))
}

fn cleared(target: &RetirementTarget, field: &str, before: &Value) -> Result<Value> {
    match target.kind {
        RetirementKind::Project | RetirementKind::Cycle | RetirementKind::Milestone
            if field
                == match target.kind {
                    RetirementKind::Project => "project",
                    RetirementKind::Cycle => "cycle",
                    _ => "milestone",
                }
                && before
                    .as_str()
                    .is_some_and(|id| id.eq_ignore_ascii_case(&target.id)) =>
        {
            Ok(Value::Null)
        }
        RetirementKind::Label | RetirementKind::Target
            if field
                == if target.kind == RetirementKind::Label {
                    "labels"
                } else {
                    "targets"
                } =>
        {
            let values = before
                .as_array()
                .ok_or_else(|| corrupt("reviewed labels must be a list"))?;
            if !values.iter().all(|label| label.is_string()) {
                return Err(corrupt("reviewed labels must contain strings"));
            }
            let after: Vec<_> = values
                .iter()
                .filter(|label| {
                    !label
                        .as_str()
                        .expect("strings checked")
                        .eq_ignore_ascii_case(&target.id)
                })
                .cloned()
                .collect();
            if after.len() == values.len() {
                return Err(corrupt(
                    "reviewed labels do not reference the retired identity",
                ));
            }
            Ok(json!(after))
        }
        _ => Err(corrupt(
            "reviewed association does not match the retirement target",
        )),
    }
}

fn preserved(issue: &IssueRecord, field: &str) -> Result<ContentHash> {
    let mut metadata = value(&issue.metadata)?;
    let fields = metadata
        .as_object_mut()
        .expect("metadata serializes as object");
    for key in [field, "revision", "updated_at"] {
        fields.remove(key);
    }
    crate::transactions::canonical_hash(
        &json!({"metadata":metadata,"body":issue.body,"path":issue.path,"retirement":issue.retirement}),
    )
}

fn retained_history(
    snapshot: &Snapshot<'_>,
    issue: &IssueRecord,
    total: &mut usize,
) -> Result<Vec<RetainedFile>> {
    let record = RetiredRecord::Issue(Box::new(issue.clone()));
    let mut history = Vec::new();
    for path in snapshot.list_bounded(
        issue.path.parent().expect("canonical issue path"),
        MAX_ENTRIES,
    )? {
        if path == issue.path {
            continue;
        }
        let limit = if is_attachment_payload(&record, &path) {
            crate::MAX_ATTACHMENT_BYTES
        } else {
            MAX_DOCUMENT_BYTES
        };
        let bytes = snapshot
            .read_bounded(&path, limit.min(MAX_HISTORY_BYTES - *total))?
            .ok_or_else(|| corrupt("issue history disappeared").at(&path))?;
        *total += bytes.len();
        history.push(RetainedFile {
            path,
            content: ContentHash::of(&bytes),
        });
    }
    Ok(history)
}

pub(super) fn validate_receipt(
    receipt: &MutationReceipt,
    repository: &RepositoryId,
) -> Result<Tombstone> {
    crate::transactions::validate_receipt(receipt)?;
    let outcome: ReferenceRetirementOutcome = serde_json::from_value(receipt.result.clone())
        .map_err(|error| corrupt(format!("reference retirement result is malformed: {error}")))?;
    let plan = &outcome.plan;
    let marker = &outcome.retirement.tombstone;
    let base = &plan.membership;
    if receipt.operation != OPERATION
        || !plan.allowed
        || !plan.blockers.is_empty()
        || plan.repository != *repository
        || matches!(
            plan.target.kind,
            RetirementKind::Issue | RetirementKind::Feature | RetirementKind::Gate
        )
        || plan.fingerprint != fingerprint(plan)?
        || plan.target != outcome.input.target
        || outcome.input.expected_preview.as_ref() != Some(&plan.fingerprint)
        || outcome
            .input
            .expected
            .as_ref()
            .is_some_and(|source| source != &plan.source)
        || receipt.input_hash != crate::transactions::canonical_hash(&value(&outcome.input)?)?
        || marker.target != plan.target
        || marker.previous != plan.source
        || base.repository != plan.repository
        || base.target != plan.target
        || base.source != plan.source
        || !base.planning_blockers.is_empty()
        || !base.record_blockers.is_empty()
        || !base.question_blockers.is_empty()
        || base.allowed != base.blockers.is_empty()
        || base.blockers.len() != plan.affected.len()
        || plan.affected.len() != outcome.affected.len()
        || plan.affected.len() > MAX_AFFECTED
    {
        return Err(corrupt(
            "reference retirement result disagrees with its reviewed plan or original input",
        ));
    }
    let mut expected = Vec::new();
    let mut ids = BTreeSet::new();
    for ((row, record), member) in plan
        .affected
        .iter()
        .zip(&outcome.affected)
        .zip(&base.blockers)
    {
        let canonical = PathBuf::from(format!("issues/{}/item.md", row.issue));
        if !ids.insert(&row.issue)
            || row.path != canonical
            || row.issue != record.metadata.id
            || row.path != record.path
            || row.issue != member.issue
            || row.path != member.path
            || row.field != member.field
            || row.source != member.source
            || row.after != cleared(&plan.target, &row.field, &row.before)?
            || association(record, &row.field)? != row.after
            || record.source.revision != row.source.revision.next()?
            || record.metadata.revision != record.source.revision
            || record.metadata.updated_at < row.updated_at
            || record.retirement.is_some()
            || row.preserved != preserved(record, &row.field)?
        {
            return Err(corrupt(
                "resolved issue result disagrees with its reviewed source or preserved content",
            ));
        }
        let mut paths = BTreeSet::new();
        for retained in &row.history {
            crate::SourceLink {
                path: retained.path.to_string_lossy().into_owned(),
                line: None,
                end_line: None,
            }
            .validate()?;
            if retained.path == canonical
                || !retained
                    .path
                    .starts_with(canonical.parent().expect("canonical path"))
                || !paths.insert(retained.path.to_string_lossy().to_lowercase())
            {
                return Err(corrupt(
                    "reviewed issue history must contain unique canonical issue-local paths",
                ));
            }
        }
        expected.push(ChangedPath {
            path: row.path.clone(),
            before: Some(row.source.content.clone()),
            after: Some(record.source.content.clone()),
        });
    }
    // Reuse the original tombstone proof validator on exactly its two writes;
    // separately require the full composite changed set to match every issue.
    let mut ordinary = receipt.clone();
    ordinary.operation = super::OPERATION.into();
    ordinary.result = value(&outcome.retirement)?;
    ordinary
        .changed
        .retain(|change| change.path == marker.record_path || change.path == marker.target.path());
    let validated = super::validate_receipt(&ordinary, repository)?;
    expected.extend(ordinary.changed);
    if receipt.changed.len() != expected.len()
        || receipt
            .changed
            .iter()
            .any(|change| !expected.contains(change))
    {
        return Err(corrupt(
            "reference retirement receipt includes missing or unrelated writes",
        ));
    }
    Ok(validated)
}
