use crate::projection::{
    projector::extract::{kind_name, planning_kind},
    types::*,
};
use crate::{
    ArchiveFilter, Config, ErrorCode, IssueSort, IssueSortField, PmError, Result, SnapshotKind,
    SortDirection, TargetMatch,
};
use rusqlite::types::Value;

pub(super) struct Selection {
    pub sql: String,
    pub values: Vec<Value>,
}
struct Builder {
    predicates: Vec<String>,
    values: Vec<Value>,
}
impl Builder {
    fn bind(&mut self, value: impl Into<Value>) -> String {
        self.values.push(value.into());
        format!("?{}", self.values.len())
    }
    fn equal(&mut self, column: &str, value: impl Into<Value>) {
        let parameter = self.bind(value);
        self.predicates.push(format!("{column}={parameter}"));
    }
    fn optional(&mut self, column: &str, value: &Option<String>) -> Result<()> {
        if let Some(value) = value {
            filter(value)?;
            self.equal(column, value.clone());
        }
        Ok(())
    }
    fn archive(&mut self, filter: ArchiveFilter) {
        match filter {
            ArchiveFilter::All => (),
            ArchiveFilter::Active => self.predicates.push("r.archived=0".into()),
            ArchiveFilter::Archived => self.predicates.push("r.archived=1".into()),
        }
    }
    fn member(&mut self, field: &str, value: &str) {
        let value = self.bind(value.to_owned());
        self.predicates.push(format!("EXISTS(SELECT 1 FROM projection_values v WHERE v.key=r.key AND v.field='{field}' AND v.value={value})"));
    }
    fn text(&mut self, value: &str) -> Result<()> {
        text(value)?;
        let value = value.trim().to_lowercase();
        if !value.is_empty() {
            let value = self.bind(value);
            self.predicates.push(format!("instr(r.search,{value})>0"));
        }
        Ok(())
    }
}
fn invalid(message: &str) -> PmError {
    PmError::new(ErrorCode::InvalidInput, message)
}
fn text(value: &str) -> Result<()> {
    if value.len() > 4096 || value.chars().any(char::is_control) {
        Err(invalid(
            "projection text query must be at most 4096 bytes without control characters",
        ))
    } else {
        Ok(())
    }
}
fn filter(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 1000 || value.chars().any(char::is_control) {
        Err(invalid(
            "projection filters must be nonempty text up to 1000 bytes without control characters",
        ))
    } else {
        Ok(())
    }
}
fn enum_value<T: serde::Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .expect("enum")
        .as_str()
        .expect("string enum")
        .into()
}
pub(super) fn select(
    query: &ProjectionQuery,
    config: &Config,
    max_keys: usize,
) -> Result<Selection> {
    let mut b = Builder {
        predicates: vec![],
        values: vec![],
    };
    let mut group = "NULL".to_owned();
    let mut order = vec![];
    match query {
        ProjectionQuery::Issues { query, group_by } => {
            query.validate()?;
            b.equal("r.kind", "issue".to_owned());
            b.archive(query.archive);
            if let Some(ids) = &query.ids {
                let ids = b
                    .bind(serde_json::to_string(ids).map_err(|error| invalid(&error.to_string()))?);
                b.predicates
                    .push(format!("r.id IN (SELECT value FROM json_each({ids}))"));
            }
            b.text(&query.query)?;
            if let Some(status) = &query.status {
                let status = config.workflow.canonical_status(status).map_err(|mut e| {
                    if e.code == ErrorCode::InvalidSchema {
                        e.code = ErrorCode::InvalidInput;
                    }
                    e
                })?;
                b.equal("r.status", status.to_owned());
            }
            if let Some(priority) = query.priority {
                let rank = match priority {
                    crate::Priority::None => 0,
                    crate::Priority::Low => 1,
                    crate::Priority::Medium => 2,
                    crate::Priority::High => 3,
                    crate::Priority::Urgent => 4,
                };
                b.equal("r.priority", rank);
            }
            for (column, value) in [
                ("r.assignee", &query.assignee),
                ("r.project", &query.project),
                ("r.cycle", &query.cycle),
                ("r.milestone", &query.milestone),
                ("r.due_at", &query.due_at),
            ] {
                b.optional(column, value)?;
            }
            if let Some(reviewer) = &query.reviewer {
                b.member("reviewer", reviewer);
            }
            if !query.workflow_categories.is_empty() {
                let statuses = config
                    .workflow
                    .states
                    .iter()
                    .filter(|state| query.workflow_categories.contains(&state.category))
                    .map(|state| b.bind(state.id.clone()))
                    .collect::<Vec<_>>();
                b.predicates.push(if statuses.is_empty() {
                    "0".into()
                } else {
                    format!("r.status IN ({})", statuses.join(","))
                });
            }
            if let Some(instant) = &query.due_before {
                let date = b.bind(instant.format("%Y-%m-%d").to_string());
                let instant = b.bind(instant.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true));
                b.predicates.push(format!("EXISTS(SELECT 1 FROM projection_values v WHERE v.key=r.key AND ((v.field='due_date' AND v.value<{date}) OR (v.field='due_instant' AND v.value<{instant})))"));
            }
            if let Some(label) = &query.label {
                b.member("labels", label);
            }
            if !query.targets.is_empty() {
                let targets=query.targets.iter().map(|target|{let p=b.bind(target.clone());format!("EXISTS(SELECT 1 FROM projection_values v WHERE v.key=r.key AND v.field='effective_targets' AND v.value={p})")}).collect::<Vec<_>>();
                b.predicates.push(format!(
                    "({})",
                    targets.join(match query.target_match {
                        TargetMatch::All => " AND ",
                        TargetMatch::Any => " OR ",
                    })
                ));
            }
            if let Some(group_by) = group_by {
                group=match group_by {IssueGroupBy::Status=>"r.status",IssueGroupBy::Priority=>"CASE r.priority WHEN 0 THEN 'none' WHEN 1 THEN 'low' WHEN 2 THEN 'medium' WHEN 3 THEN 'high' WHEN 4 THEN 'urgent' END",IssueGroupBy::Assignee=>"r.assignee",IssueGroupBy::Project=>"r.project",IssueGroupBy::Cycle=>"r.cycle",IssueGroupBy::Milestone=>"r.milestone"}.into();
                order.push(format!("{group} ASC"));
            }
            let default = [IssueSort::default()];
            for sort in if query.sort.is_empty() {
                &default[..]
            } else {
                &query.sort[..]
            } {
                let column = match sort.field {
                    IssueSortField::CreatedAt => "r.created_at",
                    IssueSortField::UpdatedAt => "r.updated_at",
                    IssueSortField::Priority => "r.priority",
                    IssueSortField::Title => "r.title_lower",
                    IssueSortField::Id => "r.id",
                };
                order.push(format!(
                    "{column} {}",
                    if sort.direction == SortDirection::Descending {
                        "DESC"
                    } else {
                        "ASC"
                    }
                ));
            }
            order.push("r.id ASC".into());
        }
        ProjectionQuery::Features { query } => {
            if query.roots_only && query.parent.is_some() {
                return Err(invalid("feature parent and roots-only selectors conflict"));
            }
            b.equal("r.kind", "feature".to_owned());
            b.archive(query.archive);
            b.text(&query.query)?;
            if query.roots_only {
                b.predicates.push("r.parent IS NULL".into());
            }
            if let Some(parent) = &query.parent {
                b.equal("r.parent", parent.to_string());
            }
            for (field, value) in [
                ("projects", &query.project),
                ("milestones", &query.milestone),
                ("targets", &query.target),
            ] {
                if let Some(value) = value {
                    filter(value)?;
                    b.member(field, value);
                }
            }
            b.optional("r.assignee", &query.lead)?;
            if let Some(value) = query.decision {
                b.equal("r.decision", enum_value(value));
            }
            if let Some(value) = query.maturity {
                b.equal("r.maturity", enum_value(value));
            }
            if let Some(value) = query.availability {
                b.equal("r.availability", enum_value(value));
            }
            order.extend(["r.title_lower ASC".into(), "r.id ASC".into()]);
        }
        ProjectionQuery::Planning { query } => {
            b.equal("r.kind", kind_name(planning_kind(query.kind)));
            b.archive(query.archive);
            b.text(&query.query)?;
            // Empty registries retain a path-keyed inventory row, not a logical
            // label. Native label IDs cannot contain the dot in labels.yml.
            if query.kind == crate::PlanningKind::Label {
                b.predicates.push("r.id <> r.path".into());
            }
            b.optional("r.project", &query.project)?;
            if let Some(target) = &query.target {
                filter(target)?;
                b.member("targets", target);
            }
            order.extend(["r.title_lower ASC".into(), "r.id ASC".into()]);
        }
        ProjectionQuery::Records { family, query } => {
            if let Some(family) = family {
                b.equal("r.kind", kind_name(*family));
            }
            text(query)?;
            if !query.trim().is_empty() {
                let expression = query
                    .split_whitespace()
                    .map(|word| format!("\"{}\"", word.replace('"', "\"\"")))
                    .collect::<Vec<_>>()
                    .join(" AND ");
                let p = b.bind(expression);
                b.predicates.push(format!(
                    "r.key IN(SELECT key FROM projection_search WHERE projection_search MATCH {p})"
                ));
            }
            order.extend([
                "r.kind ASC".into(),
                "r.title_lower ASC".into(),
                "r.id ASC".into(),
            ]);
        }
        ProjectionQuery::Activity { query } => {
            if query.kinds.len() > 64 {
                return Err(invalid("activity query supports at most 64 kinds"));
            }
            let defaults = [
                SnapshotKind::Comment,
                SnapshotKind::TimeEntry,
                SnapshotKind::Evidence,
                SnapshotKind::Attestation,
                SnapshotKind::ContractReview,
                SnapshotKind::Question,
                SnapshotKind::Handoff,
                SnapshotKind::Operation,
                SnapshotKind::RunResult,
                SnapshotKind::Claim,
                SnapshotKind::ImportedHistory,
            ];
            let kinds = if query.kinds.is_empty() {
                &defaults[..]
            } else {
                &query.kinds[..]
            };
            let params = kinds
                .iter()
                .map(|kind| b.bind(kind_name(*kind)))
                .collect::<Vec<_>>();
            b.predicates
                .push(format!("r.kind IN({})", params.join(",")));
            if let Some(subject) = &query.subject {
                if subject.repository != config.repository {
                    return Err(PmError::new(
                        ErrorCode::NotFound,
                        "activity subject belongs to another repository",
                    ));
                }
                filter(&subject.id)?;
                let k = b.bind(kind_name(subject.kind));
                let id = b.bind(subject.id.clone());
                b.predicates.push(format!("((r.kind={k} AND r.id={id}) OR EXISTS(SELECT 1 FROM projection_edges e WHERE e.from_kind=r.kind AND e.from_id=r.id AND e.to_kind={k} AND e.to_id={id}))"));
            }
            if let (Some(after), Some(before)) = (&query.after, &query.before)
                && after > before
            {
                return Err(invalid("activity time interval is reversed"));
            }
            for (operator, value) in [(">=", &query.after), ("<=", &query.before)] {
                if let Some(value) = value {
                    let p = b.bind(value.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true));
                    b.predicates
                        .push(format!("COALESCE(r.updated_at,r.created_at){operator}{p}"));
                }
            }
            order.extend([
                "COALESCE(r.updated_at,r.created_at) DESC".into(),
                "r.kind ASC".into(),
                "r.id ASC".into(),
            ]);
        }
    }
    if b.predicates.is_empty() {
        b.predicates.push("1".into());
    }
    let limit = b.bind(max_keys.saturating_add(1) as i64);
    Ok(Selection {
        sql: format!(
            "SELECT r.key,{group} FROM projection_records r WHERE {} ORDER BY {} LIMIT {limit}",
            b.predicates.join(" AND "),
            order.join(",")
        ),
        values: b.values,
    })
}
