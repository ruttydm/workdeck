use super::*;
use crate::{transactions::Snapshot, *};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Context {
    Mutation,
    Audit,
}

pub(crate) struct Policy {
    pub users: UsersRegistry,
    pub schema: OrganizationSchema,
}
impl Policy {
    pub fn load(snapshot: &Snapshot<'_>, repository: &RepositoryId) -> Result<Self> {
        Ok(Self {
            users: store::users(snapshot, repository)?.registry,
            schema: store::schema(snapshot, repository)?.definition,
        })
    }
    fn identity(&self, value: &str) -> Result<()> {
        types::text(value, "identity")?;
        if self.users.mode == IdentityMode::Registered
            && self.users.users.get(value).is_none_or(|u| u.archived)
        {
            return Err(blocked(format!(
                "identity {value:?} is not an active registered user"
            )));
        }
        Ok(())
    }
    fn unit(&self, unit: &str) -> Result<()> {
        types::key(unit, "estimate unit")?;
        if self.schema.unit_mode == IdentityMode::Registered
            && self.schema.units.get(unit).is_none_or(|u| u.archived)
        {
            return Err(blocked(format!(
                "estimate unit {unit:?} is not active and registered"
            )));
        }
        Ok(())
    }
    fn custom(
        &self,
        scope: CustomScope,
        old: Option<&BTreeMap<String, Value>>,
        next: &BTreeMap<String, Value>,
        required: bool,
        context: Context,
    ) -> Vec<(String, String)> {
        let mut errors = Vec::new();
        for (key, definition) in &self.schema.fields {
            if !definition.scopes.contains(&scope) {
                continue;
            }
            let value = next.get(key);
            // Lifecycle-only repair (reopen/archive) preserves unchanged
            // historical values. Templates have no prior record, and all
            // content edits/completion request full validation.
            if !required && old.is_some_and(|before| before.get(key) == value) {
                continue;
            }
            if definition.archived {
                if context == Context::Mutation
                    && value.is_some()
                    && old.and_then(|old| old.get(key)) != value
                {
                    errors.push((
                        format!("custom.{key}"),
                        "archived field cannot receive new or changed values".into(),
                    ));
                }
                continue;
            }
            let Some(value) = value else {
                if required && definition.required {
                    errors.push((format!("custom.{key}"), "required value is missing".into()));
                }
                continue;
            };
            let good = match definition.field_type {
                CustomFieldType::Text => value
                    .as_str()
                    .is_some_and(|s| !s.trim().is_empty() && s.len() <= 64 * 1024),
                CustomFieldType::Boolean => value.is_boolean(),
                CustomFieldType::Integer => value.is_i64() || value.is_u64(),
                CustomFieldType::Date => value.as_str().is_some_and(|s| {
                    s.len() == 10 && chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok()
                }),
                CustomFieldType::Enum => value
                    .as_str()
                    .is_some_and(|s| definition.options.iter().any(|v| v == s)),
                CustomFieldType::MultiEnum => value.as_array().is_some_and(|items| {
                    let mut seen = BTreeSet::new();
                    (!required || !definition.required || !items.is_empty())
                        && items.len() <= 256
                        && items.iter().all(|item| {
                            item.as_str().is_some_and(|s| {
                                seen.insert(s) && definition.options.iter().any(|v| v == s)
                            })
                        })
                }),
                CustomFieldType::Identity => value
                    .as_str()
                    .is_some_and(|s| types::text(s, "identity").is_ok()),
            };
            if !good {
                errors.push((
                    format!("custom.{key}"),
                    format!("value does not satisfy {:?}", definition.field_type),
                ));
                continue;
            }
            if definition.field_type == CustomFieldType::Identity
                && old.and_then(|old| old.get(key)) != Some(value)
                && let Err(e) = self.identity(value.as_str().expect("string"))
            {
                errors.push((format!("custom.{key}"), e.message));
            }
        }
        errors
    }
    fn issue(
        &self,
        old: Option<&IssueMetadata>,
        next: &IssueMetadata,
        required: bool,
        context: Context,
    ) -> Vec<(String, String)> {
        let mut errors = self.custom(
            CustomScope::Issue,
            old.map(|o| &o.custom),
            &next.custom,
            required,
            context,
        );
        for (field, before, after) in [
            (
                "assignee",
                old.and_then(|o| o.assignee.as_deref()),
                next.assignee.as_deref(),
            ),
            (
                "reporter",
                old.and_then(|o| o.reporter.as_deref()),
                next.reporter.as_deref(),
            ),
            (
                "reviewer",
                old.and_then(|o| o.reviewer.as_deref()),
                next.reviewer.as_deref(),
            ),
        ] {
            if let Some(value) = after.filter(|_| before != after)
                && let Err(e) = self.identity(value)
            {
                errors.push((field.into(), e.message));
            }
        }
        if let Some(estimate) = &next.estimate
            && old.and_then(|o| o.estimate.as_ref()) != Some(estimate)
            && let Err(e) = self.unit(&estimate.unit)
        {
            errors.push(("estimate.unit".into(), e.message));
        }
        errors
    }
    fn planning(
        &self,
        kind: PlanningKind,
        old: Option<&PlanningMetadata>,
        next: &PlanningMetadata,
        required: bool,
        context: Context,
    ) -> Vec<(String, String)> {
        let mut errors = self.custom(
            kind.into(),
            old.map(|o| &o.custom),
            &next.custom,
            required,
            context,
        );
        if old.and_then(|o| o.lead.as_ref()) != next.lead.as_ref()
            && let Some(lead) = &next.lead
            && let Err(e) = self.identity(lead)
        {
            errors.push(("lead".into(), e.message));
        }
        errors
    }
    pub fn compliance(
        &self,
        root: &Path,
        snapshot: &Snapshot<'_>,
    ) -> Result<OrganizationCompliance> {
        if self.schema.fields.is_empty()
            && self.users.mode == IdentityMode::Open
            && self.schema.unit_mode == IdentityMode::Open
        {
            return Ok(OrganizationCompliance {
                compliant: true,
                ..Default::default()
            });
        }
        bound(snapshot)?;
        let config = crate::repository::config_from_snapshot(root, snapshot)?;
        // All subjects share this immutable snapshot. Load retirement history only
        // when a nonhistorical subject first needs it, then retain the same proof
        // catalog for the remaining subjects and their selected namespaces.
        let retirements = std::cell::OnceCell::new();
        let retired = |target: RetirementTarget| -> Result<bool> {
            retirements
                .get_or_init(|| {
                    crate::retirement::RetirementIndex::capture(root, snapshot, &config)
                })
                .as_ref()
                .map_err(Clone::clone)?
                .get(&target)
                .map(|marker| marker.is_some())
        };
        let mut report = OrganizationCompliance {
            compliant: true,
            ..Default::default()
        };
        for issue in crate::issues::load_issues(root, snapshot, &config)? {
            let historical = issue.metadata.archived
                || matches!(
                    config.workflow.state(&issue.metadata.status)?.category,
                    WorkflowCategory::Completed | WorkflowCategory::Canceled
                )
                || retired(RetirementTarget::new(
                    RetirementKind::Issue,
                    issue.metadata.id.as_str(),
                )?)?;
            report.checked_records += 1;
            for (field, message) in self.issue(None, &issue.metadata, true, Context::Audit) {
                report.violations.push(PolicyViolation {
                    path: issue.path.clone(),
                    field,
                    message,
                    historical,
                });
            }
        }
        for kind in [
            PlanningKind::Initiative,
            PlanningKind::Project,
            PlanningKind::Milestone,
            PlanningKind::Cycle,
            PlanningKind::Target,
            PlanningKind::Label,
        ] {
            for record in crate::planning::store::list_planning(root, snapshot, kind)? {
                let historical = record.metadata.archived
                    || retired(RetirementTarget::new(kind.into(), &record.metadata.id)?)?;
                report.checked_records += 1;
                for (field, message) in
                    self.planning(kind, None, &record.metadata, true, Context::Audit)
                {
                    report.violations.push(PolicyViolation {
                        path: record.path.clone(),
                        field: format!("{}:{field}", record.metadata.id),
                        message,
                        historical,
                    });
                }
            }
        }
        for record in crate::features::load_features(snapshot, &config)? {
            report.checked_records += 1;
            let historical = record.metadata.archived
                || retired(RetirementTarget::new(
                    RetirementKind::Feature,
                    record.metadata.id.as_str(),
                )?)?;
            let mut errors = self.custom(
                CustomScope::Feature,
                None,
                &record.metadata.custom,
                true,
                Context::Audit,
            );
            if let Some(lead) = &record.metadata.lead
                && let Err(e) = self.identity(lead)
            {
                errors.push(("lead".into(), e.message));
            }
            for (field, message) in errors {
                report.violations.push(PolicyViolation {
                    path: record.path.clone(),
                    field,
                    message,
                    historical,
                });
            }
        }
        for record in crate::gates::load_gates(snapshot, &config)? {
            report.checked_records += 1;
            let historical = record.definition.archived
                || retired(RetirementTarget::new(
                    RetirementKind::Gate,
                    record.definition.id.as_str(),
                )?)?;
            for (field, message) in self.custom(
                CustomScope::Gate,
                None,
                &record.definition.custom,
                true,
                Context::Audit,
            ) {
                report.violations.push(PolicyViolation {
                    path: record.path.clone(),
                    field,
                    message,
                    historical,
                });
            }
        }
        for record in crate::evidence::store::load_evidence(snapshot, &config)? {
            report.checked_records += 1;
            let declaration = &record.reference.declaration;
            let mut errors = self.custom(
                CustomScope::Evidence,
                None,
                &declaration.custom,
                true,
                Context::Audit,
            );
            if let Err(e) = self.identity(&declaration.provenance.actor) {
                errors.push(("provenance.actor".into(), e.message));
            }
            for (field, message) in errors {
                report.violations.push(PolicyViolation {
                    path: record.path.clone(),
                    field,
                    message,
                    historical: true,
                });
            }
        }
        for path in
            snapshot.list_bounded(Path::new("templates/issues"), MAX_ORGANIZATION_ENTRIES)?
        {
            if path.components().count() != 3 || path.extension().is_none_or(|e| e != "md") {
                continue;
            }
            let id = path
                .file_stem()
                .and_then(|id| id.to_str())
                .ok_or_else(|| invalid("template ID must be UTF-8"))?;
            let template = crate::templates::load_template_structural(root, snapshot, &config, id)?;
            let candidate = crate::templates::validate_defaults(&config, &template.defaults)?;
            for (field, message) in self.issue(None, &candidate, false, Context::Mutation) {
                report.violations.push(PolicyViolation {
                    path: path.clone(),
                    field,
                    message,
                    historical: false,
                });
            }
        }
        report.compliant = report.violations.is_empty();
        Ok(report)
    }
}
fn enforce(errors: Vec<(String, String)>) -> Result<()> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(blocked(
            errors
                .iter()
                .map(|(field, message)| format!("{field}: {message}"))
                .collect::<Vec<_>>()
                .join("; "),
        )
        .details(serde_json::json!({"violations":errors})))
    }
}
pub(crate) fn validate_actor(
    snapshot: &Snapshot<'_>,
    repository: &RepositoryId,
    actor: &str,
) -> Result<()> {
    Policy::load(snapshot, repository)?.identity(actor)
}
pub(crate) fn validate_issue_change(
    snapshot: &Snapshot<'_>,
    config: &Config,
    old: Option<&IssueMetadata>,
    next: &IssueMetadata,
    required: bool,
) -> Result<()> {
    enforce(Policy::load(snapshot, &config.repository)?.issue(
        old,
        next,
        required,
        Context::Mutation,
    ))
}
pub(crate) fn validate_planning_change(
    snapshot: &Snapshot<'_>,
    config: &Config,
    kind: PlanningKind,
    old: Option<&PlanningMetadata>,
    next: &PlanningMetadata,
    required: bool,
) -> Result<()> {
    enforce(Policy::load(snapshot, &config.repository)?.planning(
        kind,
        old,
        next,
        required,
        Context::Mutation,
    ))
}
pub(crate) fn validate_template(
    snapshot: &Snapshot<'_>,
    config: &Config,
    candidate: &IssueMetadata,
) -> Result<()> {
    validate_issue_change(snapshot, config, None, candidate, false)
}
pub(crate) fn inspect(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
) -> Result<OrganizationCompliance> {
    Policy::load(snapshot, &config.repository)?.compliance(root, snapshot)
}

pub(crate) fn bound(snapshot: &Snapshot<'_>) -> Result<()> {
    fingerprint_sources(snapshot).map(|_| ())
}
pub(crate) fn fingerprint_sources(
    snapshot: &Snapshot<'_>,
) -> Result<BTreeMap<PathBuf, ContentHash>> {
    // Policy is evaluated across the same full native datasets as source capture;
    // organization definition/history limits remain separate from this tree scan.
    let limits = SourceCaptureLimits::default();
    let mut sources = BTreeMap::new();
    let mut total = 0usize;
    for path in snapshot.list_bounded(Path::new(""), limits.max_entries)? {
        let relevant = matches!(
            path.to_str(),
            Some("config.yml" | "schema.yml" | "users.yml" | "labels.yml")
        ) || path.file_name().is_some_and(|name| name == "item.md")
            || path.starts_with("tombstones")
            || path.starts_with("templates/issues")
            || path.starts_with("features")
            || path.starts_with("gates")
            || path.starts_with("evidence");
        if !relevant {
            continue;
        }
        let bytes = snapshot
            .read_bounded(&path, crate::documents::MAX_DOCUMENT_BYTES)?
            .ok_or_else(|| {
                PmError::new(ErrorCode::StaleSource, "organization source disappeared").at(&path)
            })?;
        total = total
            .checked_add(bytes.len())
            .ok_or_else(|| invalid("organization source size overflow"))?;
        if total > limits.max_total_bytes || sources.len() >= limits.max_entries {
            return Err(invalid(format!(
                "organization policy scan exceeds {} documents or {} bytes",
                limits.max_entries, limits.max_total_bytes
            )));
        }
        sources.insert(path, ContentHash::of(&bytes));
    }
    Ok(sources)
}
pub(crate) fn schema_compatibility(
    old: &OrganizationSchema,
    next: &OrganizationSchema,
) -> Result<()> {
    for (id, field) in &old.fields {
        let Some(new) = next.fields.get(id) else {
            return Err(blocked(
                "custom field identities cannot be removed; archive them",
            ));
        };
        if field.field_type != new.field_type
            || !field.scopes.is_subset(&new.scopes)
            || field
                .options
                .iter()
                .any(|option| !new.options.contains(option))
        {
            return Err(blocked(format!(
                "custom field {id:?} cannot repurpose its type, remove scopes, or remove enum options"
            )));
        }
    }
    for id in old.units.keys() {
        if !next.units.contains_key(id) {
            return Err(blocked(
                "estimate unit identities cannot be removed; archive them",
            ));
        }
    }
    Ok(())
}
/// Registry replacement is ordinary policy activation, not a restoration path.
/// All snapshots still parse structurally before this prospective admission.
pub(crate) fn validate_import(
    root: &Path,
    current: &Snapshot<'_>,
    projected: &Snapshot<'_>,
    config: &Config,
    path: &Path,
) -> Result<()> {
    match path.to_str() {
        Some("schema.yml") => {
            let old = store::schema(current, &config.repository)?;
            let next = store::schema(projected, &config.repository)?;
            schema_compatibility(&old.definition, &next.definition)?;
            if old.source.is_some() && next.definition.revision <= old.definition.revision {
                return Err(blocked("replacement schema must advance revision"));
            }
        }
        Some("users.yml") => {
            let old = store::users(current, &config.repository)?;
            let next = store::users(projected, &config.repository)?;
            if old.source.is_some() && next.registry.revision <= old.registry.revision {
                return Err(blocked("replacement users registry must advance revision"));
            }
            for id in old.registry.users.keys() {
                if !next.registry.users.contains_key(id) {
                    return Err(blocked("user identities cannot be removed; archive them"));
                }
            }
        }
        _ => return Ok(()),
    }
    let report = Policy::load(projected, &config.repository)?.compliance(root, projected)?;
    if report.violations.iter().any(|v| !v.historical) {
        return Err(
            blocked("imported organization policy violates active records")
                .details(serde_json::json!(report)),
        );
    }
    Ok(())
}

/// Diagnose both optional authority files even when either one is malformed.
pub(crate) fn inspect_documents(
    snapshot: &Snapshot<'_>,
    repository: &RepositoryId,
) -> (usize, Vec<PmError>) {
    let mut count = 0;
    let mut errors = Vec::new();
    for name in ["users.yml", "schema.yml"] {
        let path = Path::new(name);
        match snapshot.read_bounded(path, MAX_ORGANIZATION_BYTES) {
            Ok(None) => continue,
            Err(error) => {
                count += 1;
                errors.push(error);
                continue;
            }
            Ok(Some(_)) => count += 1,
        }
        let result = if name == "users.yml" {
            store::users(snapshot, repository).map(|_| ())
        } else {
            store::schema(snapshot, repository).map(|_| ())
        };
        if let Err(error) = result {
            errors.push(error);
        }
    }
    (count, errors)
}

pub(crate) fn validate_feature_change(
    snapshot: &Snapshot<'_>,
    config: &Config,
    old: Option<&FeatureMetadata>,
    next: &FeatureMetadata,
    required: bool,
) -> Result<()> {
    let policy = Policy::load(snapshot, &config.repository)?;
    let mut errors = policy.custom(
        CustomScope::Feature,
        old.map(|o| &o.custom),
        &next.custom,
        required,
        Context::Mutation,
    );
    if let Some(lead) = next
        .lead
        .as_deref()
        .filter(|value| old.and_then(|o| o.lead.as_deref()) != Some(*value))
        && let Err(e) = policy.identity(lead)
    {
        errors.push(("lead".into(), e.message));
    }
    enforce(errors)
}
pub(crate) fn validate_gate_change(
    snapshot: &Snapshot<'_>,
    config: &Config,
    old: Option<&GateDefinition>,
    next: &GateDefinition,
    required: bool,
) -> Result<()> {
    enforce(Policy::load(snapshot, &config.repository)?.custom(
        CustomScope::Gate,
        old.map(|o| &o.custom),
        &next.custom,
        required,
        Context::Mutation,
    ))
}
pub(crate) fn validate_evidence_change(
    snapshot: &Snapshot<'_>,
    config: &Config,
    next: &DeclareEvidence,
) -> Result<()> {
    let policy = Policy::load(snapshot, &config.repository)?;
    policy.identity(&next.provenance.actor)?;
    enforce(policy.custom(
        CustomScope::Evidence,
        None,
        &next.custom,
        true,
        Context::Mutation,
    ))
}
