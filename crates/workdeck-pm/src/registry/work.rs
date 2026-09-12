//! Bounded reports over explicitly registered sources; no global write authority.
use super::*;
use crate::{IssueQuery, SourceToken, projection::*};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum MyWorkFacet {
    #[default]
    Assigned,
    ReviewRequested,
    Overdue,
    Blocked,
    Claimed,
}
impl MyWorkFacet {
    pub fn title(self) -> &'static str {
        match self {
            Self::Assigned => "Assignments",
            Self::ReviewRequested => "Review requests",
            Self::Overdue => "Overdue",
            Self::Blocked => "Blocked",
            Self::Claimed => "Claimed",
        }
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MyWorkRequest {
    pub assignee: String,
    pub facet: MyWorkFacet,
    pub as_of: Option<crate::Timestamp>,
    pub aliases: Vec<String>,
    pub limit: usize,
    pub cursor: Option<String>,
    pub timeout_ms: u64,
}
impl Default for MyWorkRequest {
    fn default() -> Self {
        Self {
            assignee: String::new(),
            facet: MyWorkFacet::Assigned,
            as_of: None,
            aliases: Vec::new(),
            limit: 20,
            cursor: None,
            timeout_ms: 30_000,
        }
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MyWorkSource {
    pub checkout: RegisteredCheckout,
    pub projection: Option<ProjectionStatus>,
    pub matches: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_sources: Vec<crate::SourceObservation>,
    pub error: Option<PmError>,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisteredWorkRow {
    pub alias: String,
    pub row: ProjectionRow,
    #[serde(default, skip_serializing_if = "MyWorkEvidence::is_empty")]
    pub evidence: MyWorkEvidence,
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MyWorkReport {
    pub registry: SourceToken,
    pub facet: MyWorkFacet,
    pub as_of: Option<crate::Timestamp>,
    pub assignee: String,
    /// Groups follow alias order; each group uses the native default issue order.
    pub sources: Vec<MyWorkSource>,
    pub rows: Vec<RegisteredWorkRow>,
    pub known_total: usize,
    pub all_sources_available: bool,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MyWorkFaultPoint {
    AfterProjection,
    BeforeEvidenceRevalidation,
}

impl RegistryStore {
    pub fn my_work(&self, input: &MyWorkRequest) -> Result<MyWorkReport> {
        self.my_work_with_faults(input, |_| Ok(()))
    }
    #[doc(hidden)]
    pub fn my_work_with_faults(
        &self,
        input: &MyWorkRequest,
        mut fault: impl FnMut(MyWorkFaultPoint) -> Result<()>,
    ) -> Result<MyWorkReport> {
        if input.assignee.is_empty()
            || !(1..=100).contains(&input.limit)
            || !(1..=60_000).contains(&input.timeout_ms)
            || input.aliases.len() > MAX_REGISTERED_CHECKOUTS
        {
            return Err(invalid(
                "My work requires an assignee, limit 1–100 and timeout 1–60000ms",
            ));
        }
        IssueQuery {
            assignee: Some(input.assignee.clone()),
            ..Default::default()
        }
        .validate()?;
        if input.facet == MyWorkFacet::Claimed && input.as_of.is_none() {
            return Err(invalid(
                "Claimed work requires an explicit as_of instant; reuse it when paging",
            ));
        }
        use crate::WorkflowCategory;
        let query = match input.facet {
            MyWorkFacet::Claimed => IssueQuery {
                archive: crate::ArchiveFilter::All,
                ..Default::default()
            },
            MyWorkFacet::Assigned | MyWorkFacet::Blocked => IssueQuery {
                assignee: Some(input.assignee.clone()),
                ..Default::default()
            },
            MyWorkFacet::ReviewRequested => IssueQuery {
                reviewer: Some(input.assignee.clone()),
                workflow_categories: vec![WorkflowCategory::Review],
                ..Default::default()
            },
            MyWorkFacet::Overdue => IssueQuery {
                assignee: Some(input.assignee.clone()),
                due_before: Some(input.as_of.ok_or_else(|| {
                    invalid("Overdue work requires an explicit as_of instant; reuse it when paging")
                })?),
                workflow_categories: vec![
                    WorkflowCategory::Triage,
                    WorkflowCategory::Backlog,
                    WorkflowCategory::Unstarted,
                    WorkflowCategory::Started,
                    WorkflowCategory::Review,
                    WorkflowCategory::Verification,
                ],
                ..Default::default()
            },
        };
        query.validate()?;
        let aliases: BTreeSet<_> = input.aliases.iter().collect();
        if aliases.len() != input.aliases.len() {
            return Err(invalid("checkout aliases must be unique"));
        }
        for alias in &aliases {
            super::validate_alias(alias)?;
        }
        let snapshot = self.snapshot()?;
        for alias in &aliases {
            if !snapshot.entries.iter().any(|entry| &entry.alias == *alias) {
                return Err(PmError::new(
                    ErrorCode::NotFound,
                    format!("checkout alias {alias:?} is not registered"),
                ));
            }
        }
        let (expected_fingerprint, offset) = match &input.cursor {
            Some(cursor) => {
                let (hash, offset) = cursor
                    .split_once(':')
                    .ok_or_else(|| invalid("invalid My work cursor"))?;
                (
                    Some(hash.parse::<ContentHash>()?),
                    offset
                        .parse::<usize>()
                        .map_err(|_| invalid("invalid My work offset"))?,
                )
            }
            None => (None, 0),
        };
        let deadline = Instant::now() + Duration::from_millis(input.timeout_ms);
        let mut sources = Vec::new();
        let mut rows = Vec::new();
        let mut total = 0usize;
        for entry in snapshot
            .entries
            .iter()
            .filter(|entry| aliases.is_empty() || aliases.contains(&entry.alias))
        {
            let before_rows = rows.len();
            let before_total = total;
            let mut member = MyWorkSource {
                checkout: entry.clone(),
                projection: None,
                matches: None,
                evidence_sources: Vec::new(),
                error: None,
            };
            let result = (|| {
                entry.resolve()?;
                let remaining =
                    deadline
                        .checked_duration_since(Instant::now())
                        .ok_or_else(|| {
                            PmError::new(ErrorCode::Io, "My work observation budget expired")
                        })?;
                let mut limits = ProjectionLimits::default();
                limits.source.timeout_seconds = remaining.as_secs().saturating_add(1).min(60);
                limits.query_timeout_ms = u64::try_from(remaining.as_millis())
                    .unwrap_or(60_000)
                    .clamp(1, 60_000);
                let mut store =
                    ProjectionStore::open(&entry.checkout, entry.source.clone(), limits)?;
                let cached = store.load();
                let mut view = match cached {
                    Ok(view) => view,
                    Err(error) if error.code == ErrorCode::UnsafePath => return Err(error),
                    Err(_) => None,
                };
                match store.refresh(&ProjectionRefreshRequest::default()) {
                    Ok(ProjectionRefresh::Published(published)) => view = Some(*published),
                    Ok(ProjectionRefresh::Unchanged(id) | ProjectionRefresh::Superseded(id)) => {
                        if view.as_ref().is_none_or(|view| view.id() != &id) {
                            view = store.load()?;
                        }
                    }
                    Err(error) => member.error = Some(error),
                }
                member.projection = Some(store.status().clone());
                let view = view.ok_or_else(|| {
                    PmError::new(
                        ErrorCode::NotFound,
                        "no usable index for registered checkout",
                    )
                })?;
                if Instant::now() >= deadline {
                    return Err(PmError::new(
                        ErrorCode::Io,
                        "My work observation budget expired",
                    ));
                }
                fault(MyWorkFaultPoint::AfterProjection)?;
                let evidence = match input.facet {
                    MyWorkFacet::Blocked => Some(super::work_evidence::CapturedEvidence::blocked(
                        entry,
                        &view,
                        &input.assignee,
                        deadline,
                    )?),
                    MyWorkFacet::Claimed => Some(super::work_evidence::CapturedEvidence::claimed(
                        entry,
                        &view,
                        &input.assignee,
                        input.as_of.expect("validated claim time"),
                        deadline,
                    )?),
                    _ => None,
                };
                let mut query = query.clone();
                if let Some(evidence) = &evidence {
                    query.ids = Some(evidence.rows.keys().cloned().collect());
                    member.evidence_sources = evidence.observations();
                }
                let handle = view.query(&ProjectionQuery::Issues {
                    query,
                    group_by: None,
                })?;
                if let Some(evidence) = &evidence
                    && evidence.rows.len() != handle.total
                {
                    return Err(PmError::new(ErrorCode::StaleSource, "Evidence subjects do not match indexed planning membership; inspect the selected planning and claim sources")
                        .details(serde_json::json!({"evidence_subjects":evidence.rows.len(), "indexed_subjects":handle.total})));
                }
                member.matches = Some(handle.total);
                let end = total
                    .checked_add(handle.total)
                    .ok_or_else(|| invalid("My work total overflow"))?;
                if offset < end && rows.len() < input.limit {
                    let start = offset.saturating_sub(total);
                    let page = view.page(&handle, start, input.limit - rows.len())?;
                    rows.extend(page.rows.into_iter().map(|row| {
                        RegisteredWorkRow {
                            alias: entry.alias.clone(),
                            evidence: evidence
                                .as_ref()
                                .and_then(|evidence| {
                                    row.token
                                        .key
                                        .id
                                        .parse::<crate::IssueId>()
                                        .ok()
                                        .and_then(|id| evidence.rows.get(&id))
                                })
                                .cloned()
                                .unwrap_or_default(),
                            row,
                        }
                    }));
                }
                if let Some(evidence) = &evidence {
                    fault(MyWorkFaultPoint::BeforeEvidenceRevalidation)?;
                    evidence.revalidate(deadline)?;
                }
                total = end;
                // Mapping changes never silently authorize another checkout's rows.
                entry.resolve()?;
                Ok(())
            })();
            if let Err(error) = result {
                rows.truncate(before_rows);
                total = before_total;
                member.matches = None;
                member.error = Some(error);
            }
            sources.push(member);
        }
        if self.snapshot()?.source != snapshot.source {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "repository registry changed during My work capture",
            ));
        }
        let identities: Vec<_> = sources
            .iter()
            .map(|source| {
                serde_json::json!({
                    "checkout": source.checkout,
                    "view": source.projection.as_ref().and_then(|status| status.view.as_ref()),
                    "state": source.projection.as_ref().map(|status| status.state),
                    "matches": source.matches,
                    "evidence_sources": source.evidence_sources.iter().map(|source| &source.identity).collect::<Vec<_>>(),
                    "error": source.error
                })
            })
            .collect();
        let fingerprint = ContentHash::of(
            &serde_json::to_vec(&(
                &snapshot.source,
                &input.assignee,
                input.facet,
                input.as_of,
                &input.aliases,
                input.limit,
                &identities,
            ))
            .map_err(|e| invalid(e.to_string()))?,
        );
        if expected_fingerprint.is_some_and(|expected| expected != fingerprint) {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "My work membership or source generation changed; restart pagination",
            ));
        }
        if offset > total {
            return Err(invalid(
                "My work cursor is outside the available result set",
            ));
        }
        let next = offset.saturating_add(rows.len());
        Ok(MyWorkReport {
            facet: input.facet,
            as_of: input.as_of,
            registry: snapshot.source,
            assignee: input.assignee.clone(),
            all_sources_available: sources.iter().all(|source| source.error.is_none()),
            sources,
            rows,
            known_total: total,
            next_cursor: (next < total).then(|| format!("{fingerprint}:{next}")),
        })
    }
}
