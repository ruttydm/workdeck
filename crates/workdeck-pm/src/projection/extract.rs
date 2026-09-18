//! Display/index columns extracted only after the native source validators have
//! admitted the complete source. This module never substitutes write policy.
use crate::{ContentHash, ErrorCode, PmError, Result, SnapshotKind, documents::MarkdownDocument};
use rusqlite::{Transaction, params};
use serde_json::Value;
use std::path::Path;

pub(in crate::projection) fn kind_name(kind: SnapshotKind) -> String {
    serde_json::to_value(kind)
        .expect("enum serialization")
        .as_str()
        .expect("string enum")
        .to_owned()
}
pub(in crate::projection) fn key(kind: &str, id: &str) -> String {
    serde_json::to_string(&(kind, id)).expect("string tuple serialization")
}
pub(in crate::projection) fn planning_kind(kind: crate::PlanningKind) -> SnapshotKind {
    match kind {
        crate::PlanningKind::Initiative => SnapshotKind::Initiative,
        crate::PlanningKind::Project => SnapshotKind::Project,
        crate::PlanningKind::Milestone => SnapshotKind::Milestone,
        crate::PlanningKind::Cycle => SnapshotKind::Cycle,
        crate::PlanningKind::Target => SnapshotKind::Target,
        crate::PlanningKind::Label => SnapshotKind::Labels,
    }
}
fn invalid(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::InvalidSchema, message)
}
fn sql(error: rusqlite::Error) -> PmError {
    super::super::schema::sql_error(error)
}
fn value<T: serde::Serialize>(value: T) -> Result<Value> {
    serde_json::to_value(value).map_err(|e| invalid(e.to_string()))
}
fn string(value: &Value, name: &str) -> Option<String> {
    value.get(name).and_then(Value::as_str).map(str::to_owned)
}
fn timestamp(value: &Value, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        value
            .get(*name)
            .and_then(Value::as_str)
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|t| {
                t.with_timezone(&chrono::Utc)
                    .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
            })
    })
}

pub(super) fn insert(
    tx: &Transaction<'_>,
    path: &Path,
    kind: SnapshotKind,
    bytes: &[u8],
) -> Result<()> {
    let text = std::str::from_utf8(bytes).ok();
    let content = ContentHash::of(bytes);
    let name = kind_name(kind);
    let path_text = path
        .to_str()
        .ok_or_else(|| invalid("projection path is not UTF-8"))?;
    tx.execute(
        "INSERT INTO projection_documents(path,kind,content,bytes,document) VALUES(?1,?2,?3,?4,?5)",
        params![
            path_text,
            name,
            content.as_str(),
            bytes.len() as i64,
            if kind == SnapshotKind::AttachmentContent {
                None
            } else {
                text
            }
        ],
    )
    .map_err(sql)?;
    let (metadata, body) = match kind {
        SnapshotKind::ContractReview => {
            let record: crate::ImportedContractReview =
                serde_json::from_slice(bytes).map_err(|e| invalid(e.to_string()).at(path))?;
            let title = format!(
                "Imported contract review by {}",
                record.admission.reviewers.join(", ")
            );
            let body = format!(
                "{} {}",
                record.admission.approval.head.commit, record.input.actor
            );
            (
                serde_json::json!({
                    "id":record.id, "repository":record.repository, "title":title,
                    "actor":record.input.actor, "status":"approved",
                    "created_at":record.imported_at, "updated_at":record.imported_at,
                    "admission":record.admission,
                }),
                body,
            )
        }
        SnapshotKind::Attestation => {
            let record: crate::ImportedCheckReport =
                serde_json::from_slice(bytes).map_err(|e| invalid(e.to_string()).at(path))?;
            let title = format!("Imported report from {}", record.admission.producer.id);
            let body = format!("{} {}", record.admission.source.commit, record.input.actor);
            (
                serde_json::json!({
                    "id":record.id, "repository":record.repository, "title":title,
                    "actor":record.input.actor, "status":record.admission.observed_state,
                    "created_at":record.imported_at, "updated_at":record.imported_at,
                    "admission":record.admission,
                }),
                body,
            )
        }

        SnapshotKind::Issue
        | SnapshotKind::Feature
        | SnapshotKind::Initiative
        | SnapshotKind::Project
        | SnapshotKind::Milestone
        | SnapshotKind::Cycle
        | SnapshotKind::Target
        | SnapshotKind::Comment
        | SnapshotKind::Question
        | SnapshotKind::Handoff
        | SnapshotKind::IssueTemplate => {
            let doc = MarkdownDocument::parse(
                path,
                text.ok_or_else(|| invalid("planning document is not UTF-8"))?,
            )?;
            let metadata = match kind {
                SnapshotKind::Issue => value(doc.deserialize::<crate::IssueMetadata>()?)?,
                SnapshotKind::Feature => value(doc.deserialize::<crate::FeatureMetadata>()?)?,
                SnapshotKind::Initiative
                | SnapshotKind::Project
                | SnapshotKind::Milestone
                | SnapshotKind::Cycle
                | SnapshotKind::Target => value(doc.deserialize::<crate::PlanningMetadata>()?)?,
                _ => doc.deserialize::<Value>()?,
            };
            (metadata, doc.body().to_owned())
        }
        SnapshotKind::Wiki | SnapshotKind::ImportedHandoff | SnapshotKind::AttachmentContent => {
            (Value::Null, text.unwrap_or_default().to_owned())
        }
        SnapshotKind::ImportedSession => {
            let parsed: toml::Value =
                toml::from_str(text.ok_or_else(|| invalid("recorded session is not UTF-8"))?)
                    .map_err(|e| invalid(e.to_string()))?;
            (value(parsed)?, text.unwrap_or_default().to_owned())
        }
        SnapshotKind::ImportedHistory if path.extension().is_some_and(|e| e == "jsonl") => {
            (Value::Null, text.unwrap_or_default().to_owned())
        }
        _ => {
            let metadata = match text {
                Some(text) => serde_yaml_ng::from_str::<Value>(text)
                    .map_err(|e| invalid(e.to_string()).at(path))?,
                None => Value::Null,
            };
            (metadata, text.unwrap_or_default().to_owned())
        }
    };
    // Labels share one source document and source hash; their logical IDs remain
    // individually addressable. Empty registries retain an aggregate row.
    if kind == SnapshotKind::Labels
        && let Some(labels) = metadata.get("labels").and_then(Value::as_array)
        && !labels.is_empty()
    {
        for label in labels {
            insert_row(tx, path_text, &name, &content, label, &body, None)?;
        }
    } else if kind == SnapshotKind::Users
        && let Some(users) = metadata.get("users").and_then(Value::as_object)
        && !users.is_empty()
    {
        for (id, user) in users {
            insert_row(tx, path_text, &name, &content, user, "", Some(id))?;
        }
    } else {
        insert_row(tx, path_text, &name, &content, &metadata, &body, None)?;
    }
    Ok(())
}

fn insert_row(
    tx: &Transaction<'_>,
    path: &str,
    kind: &str,
    content: &ContentHash,
    m: &Value,
    body: &str,
    explicit_id: Option<&str>,
) -> Result<()> {
    let id = explicit_id
        .map(str::to_owned)
        .or_else(|| string(m, "id"))
        .or_else(|| {
            if kind == "claim" {
                string(m, "issue")
            } else {
                None
            }
        })
        .or_else(|| string(m, "operation_id"))
        .unwrap_or_else(|| path.to_owned());
    let row_key = key(kind, &id);
    let title = string(m, "title")
        .or_else(|| string(m, "name"))
        .or_else(|| string(m, "summary"))
        .or_else(|| string(m, "operation"))
        .unwrap_or_else(|| id.clone());
    let archived = m.get("archived").and_then(Value::as_bool).unwrap_or(false);
    let status = string(m, "status").or_else(|| string(m, "state"));
    let priority = string(m, "priority").and_then(|s| match s.as_str() {
        "none" => Some(0),
        "low" => Some(1),
        "medium" => Some(2),
        "high" => Some(3),
        "urgent" => Some(4),
        _ => None,
    });
    let assignee = string(m, "assignee")
        .or_else(|| string(m, "lead"))
        .or_else(|| string(m, "actor"))
        .or_else(|| string(m, "author"));
    let created = timestamp(
        m,
        &[
            "created_at",
            "recorded_at",
            "observed_at",
            "acquired_at",
            "started_at",
            "reserved_at",
        ],
    );
    let updated = timestamp(
        m,
        &[
            "updated_at",
            "finished_at",
            "recorded_at",
            "observed_at",
            "created_at",
        ],
    );
    // Exact IssueQuery search fields, including literal labels and assignee.
    let mut search = vec![id.to_lowercase(), title.to_lowercase(), body.to_lowercase()];
    if let Some(labels) = m.get("labels").and_then(Value::as_array) {
        search.extend(
            labels
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_lowercase),
        );
    }
    if let Some(assignee) = &assignee {
        search.push(assignee.to_lowercase());
    }
    tx.execute("INSERT INTO projection_records(key,kind,id,path,content,title,title_lower,search,archived,retired,status,priority,assignee,project,cycle,milestone,due_at,parent,created_at,updated_at,decision,maturity,availability) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,0,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22)",params![row_key,kind,id,path,content.as_str(),title,title.to_lowercase(),search.join("\u{0}"),archived,status,priority,assignee,string(m,"project"),string(m,"cycle"),string(m,"milestone"),string(m,"due_at"),string(m,"parent"),created,updated,string(m,"decision"),string(m,"maturity"),string(m,"availability")]).map_err(sql)?;
    let indexed_body = if kind == "attachment_content" {
        String::new()
    } else {
        format!(
            "{}\n{body}",
            serde_json::to_string(m).map_err(|e| invalid(e.to_string()))?
        )
    };
    tx.execute(
        "INSERT INTO projection_search(key,title,body) VALUES(?1,?2,?3)",
        params![row_key, title, indexed_body],
    )
    .map_err(sql)?;
    for (field, value) in [
        ("reviewer", string(m, "reviewer")),
        ("due_instant", timestamp(m, &["due_at"])),
        (
            "due_date",
            string(m, "due_at")
                .filter(|value| chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok()),
        ),
    ] {
        if let Some(value) = value {
            tx.execute(
                "INSERT INTO projection_values(key,field,value) VALUES(?1,?2,?3)",
                params![row_key, field, value],
            )
            .map_err(sql)?;
        }
    }
    for field in [
        "labels",
        "targets",
        "projects",
        "milestones",
        "features",
        "gates",
        "prerequisites",
    ] {
        if let Some(values) = m.get(field).and_then(Value::as_array) {
            for value in values.iter().filter_map(Value::as_str) {
                tx.execute(
                    "INSERT OR IGNORE INTO projection_values(key,field,value) VALUES(?1,?2,?3)",
                    params![row_key, field, value],
                )
                .map_err(sql)?;
                let target_kind = match field {
                    "labels" => "labels",
                    "targets" => "target",
                    "projects" => "project",
                    "milestones" => "milestone",
                    "features" => "feature",
                    "gates" => "gate",
                    _ => kind,
                };
                edge(tx, (kind, &id), (target_kind, value), field, path, content)?;
            }
        }
    }
    for (field, target_kind) in [
        ("parent", kind),
        ("initiative", "initiative"),
        ("project", "project"),
        ("cycle", "cycle"),
        ("milestone", "milestone"),
        ("issue", "issue"),
    ] {
        if let Some(value) = string(m, field) {
            edge(tx, (kind, &id), (target_kind, &value), field, path, content)?;
        }
    }
    for field in ["issues", "features"] {
        if matches!(kind, "issue_relation" | "feature_relation")
            && let Some(pair) = m.get(field).and_then(Value::as_array)
            && let [a, b] = pair.as_slice()
            && let (Some(a), Some(b)) = (a.as_str(), b.as_str())
        {
            let owner = if field == "issues" {
                "issue"
            } else {
                "feature"
            };
            edge(tx, (owner, a), (owner, b), "related", path, content)?;
            edge(tx, (owner, b), (owner, a), "related", path, content)?;
        }
    }
    // Keep typed authored subject citations distinct from graph prerequisites.
    if let Some(subject) = m.get("subject") {
        subject_edge(tx, kind, &id, subject, path, content)?;
    }
    for subject in m
        .get("subjects")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(subject) = subject.get("subject") {
            subject_edge(tx, kind, &id, subject, path, content)?;
        }
    }
    let declaration = (kind == "evidence").then(|| m.get("declaration")).flatten();
    for link in declaration
        .and_then(|d| d.get("links"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if link.get("kind").and_then(Value::as_str) == Some("attestation")
            && let Some(target) = link.get("id").and_then(Value::as_str)
        {
            edge(
                tx,
                (kind, &id),
                ("attestation", target),
                "attestation",
                path,
                content,
            )?;
        }
    }
    let criteria = m
        .get("criterion")
        .into_iter()
        .chain(declaration.and_then(|d| d.get("criterion")))
        .chain(
            m.get("requirements")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|r| r.get("criterion").or_else(|| r.get("owner").map(|_| r))),
        );
    for criterion in criteria {
        if let Some(owner) = criterion.get("owner")
            && let (Some(owner_kind), Some(owner_id)) = (
                owner.get("kind").and_then(Value::as_str),
                owner.get("id").and_then(Value::as_str),
            )
        {
            edge(
                tx,
                (kind, &id),
                (owner_kind, owner_id),
                "criterion",
                path,
                content,
            )?;
        }
    }
    Ok(())
}
fn subject_edge(
    tx: &Transaction<'_>,
    kind: &str,
    id: &str,
    subject: &Value,
    path: &str,
    content: &ContentHash,
) -> Result<()> {
    if let (Some(target_kind), Some(target)) = (
        subject.get("kind").and_then(Value::as_str),
        subject.get("reference").and_then(Value::as_str),
    ) && matches!(
        target_kind,
        "issue" | "feature" | "gate" | "milestone" | "project"
    ) {
        edge(
            tx,
            (kind, id),
            (target_kind, target),
            "subject",
            path,
            content,
        )?;
    }
    Ok(())
}
pub(super) fn edge(
    tx: &Transaction<'_>,
    (from_kind, from_id): (&str, &str),
    (to_kind, to_id): (&str, &str),
    relation: &str,
    path: &str,
    content: &ContentHash,
) -> Result<()> {
    tx.execute("INSERT OR IGNORE INTO projection_edges(from_kind,from_id,to_kind,to_id,relation,path,content) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![from_kind,from_id,to_kind,to_id,relation,path,content.as_str()]).map_err(sql)?;
    Ok(())
}
