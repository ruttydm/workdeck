use super::{MigrationDraft, MigrationKind, MigrationNotice, PreviewOptions, scan::Input};
use crate::{
    ErrorCode, ImportedCompletion, ImportedPlanningProvenance, IssueMetadata, LabelsMetadata,
    PlanningImportFormat, PlanningKind, PlanningMetadata, PmError, Priority, Result, Revision,
    SchemaVersion, SourceLink, Timestamp, WorkflowCategory,
    documents::{MAX_DOCUMENT_BYTES, MarkdownDocument, YamlDocument},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Default)]
pub(crate) struct Converted {
    pub drafts: Vec<MigrationDraft>,
    pub notices: Vec<MigrationNotice>,
    pub errors: Vec<PmError>,
}

pub(super) fn convert(
    input: &Input,
    options: &PreviewOptions,
    source_reference: &Path,
) -> Result<Converted> {
    convert_with_origin(input, options, source_reference, None)
}

pub(super) struct JsonOrigin<'a> {
    pub record: &'a Value,
    pub selector: &'a str,
}

pub(super) fn convert_with_origin(
    input: &Input,
    options: &PreviewOptions,
    source_reference: &Path,
    origin: Option<&JsonOrigin<'_>>,
) -> Result<Converted> {
    let bytes = input
        .bytes
        .as_deref()
        .expect("only readable files are converted");
    let mut result = Converted::default();
    match input.kind {
        MigrationKind::Issue => {
            if input.path.components().count() != 2
                || input.path.extension().is_none_or(|value| value != "toml")
            {
                return Err(invalid(
                    &input.path,
                    "legacy issues belong directly in issues/<WD-ID>.toml",
                ));
            }
            let table = parse_table(&input.path, bytes)?;
            let metadata = convert_issue(
                input,
                &table,
                options,
                source_reference,
                origin,
                &mut result.notices,
            )?;
            let body = string(&table, "description", &input.path)?.unwrap_or_default();
            let path = PathBuf::from(format!("issues/{}/item.md", metadata.id));
            let content = markdown(&path, &metadata, &body)?;
            result
                .drafts
                .push(draft(input, MigrationKind::Issue, path, content));
        }
        MigrationKind::Project | MigrationKind::Cycle | MigrationKind::Labels => {
            convert_references(
                input,
                &parse_table(&input.path, bytes)?,
                source_reference,
                origin,
                &mut result,
            )?;
        }
        MigrationKind::AppConfig => {
            let _table = parse_table(&input.path, bytes)?;
            // The lossless app-settings patch is handled separately from PM YAML.
            let content = super::app_config::convert(input, &mut result.notices)?;
            result.drafts.push(draft(
                input,
                MigrationKind::AppConfig,
                "config.toml".into(),
                content,
            ));
        }
        MigrationKind::ImportedSession => {
            if input.path.components().count() != 2
                || input.path.extension().is_none_or(|value| value != "toml")
            {
                return Err(invalid(
                    &input.path,
                    "imported sessions require agents/<id>.toml",
                ));
            }
            let table = parse_table(&input.path, bytes)?;
            let id = required_string(&table, "id", &input.path)?;
            safe_reference(&id, &input.path)?;
            if input.path.file_stem().and_then(|stem| stem.to_str()) != Some(id.as_str()) {
                return Err(invalid(
                    &input.path,
                    "imported session ID differs from its filename",
                ));
            }
            required_string(&table, "title", &input.path)?;
            notice(
                &mut result.notices,
                input,
                "historical_session",
                "Session commands, tests, handoffs, and cwd remain inert historical annotations; no claim, live session, or check evidence is created.",
            );
            result.drafts.push(draft(
                input,
                MigrationKind::ImportedSession,
                format!("imported-sessions/{id}.toml").into(),
                bytes.to_vec(),
            ));
        }
        MigrationKind::ImportedEvents => {
            let text = text(&input.path, bytes)?;
            for (index, line) in text.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                let event: Value = serde_json::from_str(line).map_err(|error| {
                    let mut diagnostic = invalid(
                        &input.path,
                        format!("invalid historical event JSON: {error}"),
                    );
                    diagnostic.line = Some(index + 1);
                    diagnostic.column = Some(error.column());
                    diagnostic
                })?;
                if !event.is_object()
                    || event
                        .get("kind")
                        .and_then(Value::as_str)
                        .is_none_or(|kind| kind.trim().is_empty())
                {
                    return Err(invalid(
                        &input.path,
                        "historical events require an object with a nonempty kind",
                    ));
                }
            }
            notice(
                &mut result.notices,
                input,
                "historical_events",
                "Event bytes are preserved as imported history, never as verification receipts.",
            );
            result.drafts.push(draft(
                input,
                MigrationKind::ImportedEvents,
                "imported-history/events.jsonl".into(),
                bytes.to_vec(),
            ));
        }
        MigrationKind::ImportedHandoff => {
            let target = Path::new("imported-handoffs").join(
                input
                    .path
                    .strip_prefix("handoffs")
                    .expect("classified handoff"),
            );
            safe_destination(&target)?;
            notice(
                &mut result.notices,
                input,
                "historical_handoff",
                "Uninterpreted handoff bytes remain imported context; their instructions are not executed.",
            );
            result.drafts.push(draft(
                input,
                MigrationKind::ImportedHandoff,
                target,
                bytes.to_vec(),
            ));
        }
        MigrationKind::Extension => {
            safe_destination(&input.path)?;
            result.drafts.push(draft(
                input,
                MigrationKind::Extension,
                input.path.clone(),
                bytes.to_vec(),
            ));
            result.errors.push(invalid(&input.path, "repository extension migration requires an explicit trust/disposition plan before cutover; preview never executes the payload"));
        }
        MigrationKind::Disposable => {
            notice(
                &mut result.notices,
                input,
                "disposable_index",
                "Legacy index content is inventoried but has no authoritative destination; native indexes are rebuilt.",
            );
        }
        MigrationKind::Unknown => {
            result.errors.push(invalid(&input.path, "unclassified legacy file requires an explicit preservation mapping before migration can be complete"));
        }
        MigrationKind::Configuration => {
            unreachable!("generated configuration is not a legacy input")
        }
    }
    Ok(result)
}

fn convert_issue(
    input: &Input,
    table: &toml::Table,
    options: &PreviewOptions,
    source_reference: &Path,
    origin: Option<&JsonOrigin<'_>>,
    notices: &mut Vec<MigrationNotice>,
) -> Result<IssueMetadata> {
    let path = &input.path;
    let id: crate::IssueId = required_string(table, "key", path)?
        .parse()
        .map_err(|error: PmError| invalid(path, error.message))?;
    if path.file_stem().and_then(|stem| stem.to_str()) != Some(id.as_str()) {
        return Err(invalid(
            path,
            "legacy issue identity differs from its filename",
        ));
    }
    let title = required_string(table, "title", path)?;
    let created = timestamp(table, "created_at", path)?.ok_or_else(|| {
        invalid(
            path,
            "missing historical created_at; migration must not invent a creation time",
        )
    })?;
    let updated = timestamp(table, "updated_at", path)?.ok_or_else(|| {
        invalid(
            path,
            "missing historical updated_at; migration must not invent a modification time",
        )
    })?;
    let mut issue = IssueMetadata::new(&options.config, &title, created)?;
    issue.id = id;
    // Preserve valid authored title whitespace rather than the constructor's new-input trimming.
    issue.title = title;
    issue.updated_at = updated;
    let original_status = string(table, "status", path)?;
    let status = match original_status.as_deref().unwrap_or("todo") {
        "inbox" => "inbox",
        "backlog" => "backlog",
        "todo" => "ready",
        "in-progress" => "in_progress",
        "in-review" => "in_review",
        "done" => "done",
        other => {
            return Err(invalid(
                path,
                format!(
                    "unknown legacy issue status {other:?}; an explicit workflow mapping is required"
                ),
            ));
        }
    };
    issue.status = options.config.workflow.state(status)?.id.clone();
    let expected_category = match status {
        "inbox" => WorkflowCategory::Triage,
        "backlog" => WorkflowCategory::Backlog,
        "ready" => WorkflowCategory::Unstarted,
        "in_progress" => WorkflowCategory::Started,
        "in_review" => WorkflowCategory::Review,
        "done" => WorkflowCategory::Completed,
        _ => unreachable!("explicit legacy status mapping"),
    };
    if options.config.workflow.state(status)?.category != expected_category {
        return Err(invalid(
            path,
            format!(
                "target workflow state {status:?} changes the legacy semantic category; an explicit compatible mapping is required"
            ),
        ));
    }
    if original_status.is_none() {
        notice(
            notices,
            input,
            "default_status",
            "Absent legacy status defaults to todo and maps to ready.",
        );
    }
    let priority = string(table, "priority", path)?;
    issue.priority = match priority.as_deref().unwrap_or("medium") {
        "none" => Priority::None,
        "low" => Priority::Low,
        "medium" => Priority::Medium,
        "high" => Priority::High,
        "urgent" => Priority::Urgent,
        other => return Err(invalid(path, format!("unknown legacy priority {other:?}"))),
    };
    if priority.is_none() {
        notice(
            notices,
            input,
            "default_priority",
            "Absent legacy priority defaults to medium.",
        );
    }
    for field in [
        "description",
        "project",
        "cycle",
        "assignee",
        "due_at",
        "labels",
        "linked_files",
        "linked_commits",
    ] {
        if !table.contains_key(field) {
            notice(
                notices,
                input,
                &format!("default_{field}"),
                &format!(
                    "Absent legacy {field} retains its documented empty value or collection; no historical value is invented."
                ),
            );
        } else if table.get(field).and_then(toml::Value::as_str) == Some("")
            && matches!(field, "project" | "cycle" | "assignee" | "due_at")
        {
            notice(
                notices,
                input,
                &format!("empty_{field}"),
                &format!(
                    "Explicit empty legacy {field} becomes absent; the original empty string is preserved in migration metadata."
                ),
            );
        }
    }
    issue.project = optional_nonempty(table, "project", path)?;
    issue.cycle = optional_nonempty(table, "cycle", path)?;
    issue.assignee = optional_nonempty(table, "assignee", path)?;
    issue.due_at = optional_nonempty(table, "due_at", path)?;
    issue.labels = strings(table, "labels", path)?;
    issue.commits = strings(table, "linked_commits", path)?;
    issue.files = strings(table, "linked_files", path)?
        .into_iter()
        .map(|path| SourceLink {
            path,
            line: None,
            end_line: None,
        })
        .collect();
    if status == "done" {
        if options.config.workflow.state(status)?.category != WorkflowCategory::Completed {
            return Err(invalid(
                path,
                "the target done state must retain the completed category",
            ));
        }
        issue.imported_completion = Some(ImportedCompletion::new(
            path_text(source_reference)?,
            input.hash.clone().expect("read input hash"),
            options.imported_at,
        ));
        notice(
            notices,
            input,
            "historical_completion",
            "Done is an imported historical declaration with an unknown exact completion time, not manual acceptance or check evidence.",
        );
    }
    issue.custom.insert(
        "legacy".into(),
        provenance(
            input,
            table,
            source_reference,
            origin,
            &[
                "key",
                "title",
                "description",
                "status",
                "priority",
                "project",
                "cycle",
                "assignee",
                "created_at",
                "updated_at",
                "due_at",
                "labels",
                "linked_files",
                "linked_commits",
            ],
        ),
    );
    issue
        .validate(&options.config)
        .map_err(|error| error.at(path))?;
    Ok(issue)
}

fn convert_references(
    input: &Input,
    table: &toml::Table,
    source_reference: &Path,
    origin: Option<&JsonOrigin<'_>>,
    result: &mut Converted,
) -> Result<()> {
    let (key, kind) = match input.kind {
        MigrationKind::Project => ("projects", PlanningKind::Project),
        MigrationKind::Cycle => ("cycles", PlanningKind::Cycle),
        MigrationKind::Labels => ("labels", PlanningKind::Label),
        _ => unreachable!(),
    };
    let rows = match table.get(key) {
        None if table.is_empty() => &[][..],
        Some(toml::Value::Array(rows)) => rows.as_slice(),
        _ => {
            return Err(invalid(
                &input.path,
                format!("legacy reference data requires [[{key}]] records"),
            ));
        }
    };
    let mut labels = Vec::new();
    let mut ids = std::collections::BTreeSet::new();
    for (index, row) in rows.iter().enumerate() {
        let conversion = (|| -> Result<_> {
            let row = row
                .as_table()
                .ok_or_else(|| invalid(&input.path, format!("{key}[{index}] must be a table")))?;
            let id = required_string(row, "id", &input.path)?;
            safe_reference(&id, &input.path)?;
            if !ids.insert(id.to_ascii_lowercase()) {
                return Err(invalid(
                    &input.path,
                    format!("duplicate or case-colliding {key} identity {id:?}"),
                ));
            }
            let name = required_string(row, "name", &input.path)?;
            let metadata = PlanningMetadata {
                initiative: None,
                project: None,
                lead: None,
                scope: None,
                goal: None,
                targets: Vec::new(),
                exit_criteria: Vec::new(),
                outcomes: Vec::new(),
                schema: SchemaVersion::CURRENT,
                id,
                revision: Revision::INITIAL,
                name,
                status: if kind == PlanningKind::Label {
                    None
                } else {
                    optional_nonempty(row, "status", &input.path)?
                },
                starts_at: if kind == PlanningKind::Cycle {
                    optional_nonempty(row, "starts_at", &input.path)?
                } else {
                    None
                },
                ends_at: if kind == PlanningKind::Cycle {
                    optional_nonempty(row, "ends_at", &input.path)?
                } else {
                    None
                },
                color: if kind == PlanningKind::Label {
                    optional_nonempty(row, "color", &input.path)?
                } else {
                    None
                },
                created_at: timestamp(row, "created_at", &input.path)?,
                updated_at: timestamp(row, "updated_at", &input.path)?,
                imported: Some(ImportedPlanningProvenance {
                    source: path_text(source_reference)?,
                    format: if origin.is_some() {
                        PlanningImportFormat::LegacyJson
                    } else {
                        PlanningImportFormat::LegacyToml
                    },
                    content: input.hash.clone().expect("read input hash"),
                }),
                archived: false,
                custom: BTreeMap::from([(
                    "legacy".into(),
                    provenance(
                        input,
                        row,
                        source_reference,
                        origin,
                        &[
                            "id",
                            "name",
                            "description",
                            "status",
                            "starts_at",
                            "ends_at",
                            "color",
                            "created_at",
                            "updated_at",
                        ],
                    ),
                )]),
                extra: BTreeMap::new(),
            };
            metadata
                .validate(kind)
                .map_err(|error| error.at(&input.path))?;
            let body = if kind == PlanningKind::Project {
                string(row, "description", &input.path)?.unwrap_or_default()
            } else {
                String::new()
            };
            Ok((metadata, body))
        })();
        match conversion {
            Ok((metadata, _)) if kind == PlanningKind::Label => labels.push(metadata),
            Ok((metadata, body)) => {
                let target = PathBuf::from(format!("{key}/{}/item.md", metadata.id));
                match markdown(&target, &metadata, &body) {
                    Ok(content) => {
                        result
                            .drafts
                            .push(draft(input, input.kind.clone(), target, content))
                    }
                    Err(error) => result.errors.push(error),
                }
            }
            Err(error) => result.errors.push(error),
        }
    }
    if kind == PlanningKind::Label {
        labels.sort_by(|left, right| left.id.cmp(&right.id));
        let target = PathBuf::from("labels.yml");
        let metadata = LabelsMetadata {
            schema: SchemaVersion::CURRENT,
            labels,
            custom: BTreeMap::from([(
                "legacy".into(),
                provenance(input, table, source_reference, origin, &[key]),
            )]),
            extra: BTreeMap::new(),
        };
        let content = yaml(&target, &metadata)?;
        result
            .drafts
            .push(draft(input, MigrationKind::Labels, target, content));
    } else {
        let container = table
            .iter()
            .filter(|(field, _)| field.as_str() != key)
            .map(|(field, value)| (field.clone(), value.clone()))
            .collect::<toml::Table>();
        if !container.is_empty() {
            let target = PathBuf::from(format!("imported-history/{key}-metadata.yml"));
            let content = yaml(
                &target,
                &json!({"schema":1,"source":input.path,"source_content":input.hash,"metadata":tagged(&toml::Value::Table(container))}),
            )?;
            result
                .drafts
                .push(draft(input, input.kind.clone(), target, content));
        }
    }
    Ok(())
}

fn provenance(
    input: &Input,
    table: &toml::Table,
    source_reference: &Path,
    origin: Option<&JsonOrigin<'_>>,
    recognized: &[&str],
) -> Value {
    if let Some(origin) = origin {
        let fields = origin.record.as_object().expect("validated export object");
        let extra = fields
            .iter()
            .filter(|(key, _)| !recognized.contains(&key.as_str()))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>();
        let original = fields
            .iter()
            .filter(|(key, _)| {
                recognized.contains(&key.as_str())
                    && (key.as_str() != "description"
                        || !matches!(input.kind, MigrationKind::Issue | MigrationKind::Project))
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>();
        return json!({"format":"workdeck_json_v0","source_path":source_reference,"source_content":input.hash,"record_selector":origin.selector,"original":original,"extra":extra});
    }
    let extra = table
        .iter()
        .filter(|(key, _)| !recognized.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<toml::Table>();
    let original = table
        .iter()
        .filter(|(key, _)| {
            recognized.contains(&key.as_str())
                && (key.as_str() != "description"
                    || !matches!(input.kind, MigrationKind::Issue | MigrationKind::Project))
        })
        .map(|(key, value)| (key.clone(), tagged(value)))
        .collect::<BTreeMap<_, _>>();
    json!({"format":"workdeck_toml_v0","source_path":source_reference,"source_content":input.hash,"original":original,"extra":tagged(&toml::Value::Table(extra))})
}

/// Every TOML variant retains an explicit type. Float text supports nan/inf and
/// negative zero; date/time values never become indistinguishable plain strings.
pub fn tagged(value: &toml::Value) -> Value {
    match value {
        toml::Value::String(value) => json!({"type":"string","value":value}),
        toml::Value::Integer(value) => json!({"type":"integer","value":value}),
        toml::Value::Float(value) => json!({"type":"float","value":value.to_string()}),
        toml::Value::Boolean(value) => json!({"type":"boolean","value":value}),
        toml::Value::Datetime(value) => json!({"type":"datetime","value":value.to_string()}),
        toml::Value::Array(values) => {
            json!({"type":"array","value":values.iter().map(tagged).collect::<Vec<_>>()})
        }
        toml::Value::Table(values) => {
            json!({"type":"table","value":values.iter().map(|(key,value)|(key.clone(),tagged(value))).collect::<BTreeMap<_,_>>()})
        }
    }
}

fn timestamp(table: &toml::Table, key: &str, path: &Path) -> Result<Option<Timestamp>> {
    string(table, key, path)?
        .map(|value| {
            value
                .parse()
                .map_err(|error| invalid(path, format!("invalid historical {key}: {error}")))
        })
        .transpose()
}
fn optional_nonempty(table: &toml::Table, key: &str, path: &Path) -> Result<Option<String>> {
    Ok(string(table, key, path)?.filter(|value| !value.is_empty()))
}
fn required_string(table: &toml::Table, key: &str, path: &Path) -> Result<String> {
    string(table, key, path)?
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| invalid(path, format!("missing or empty required field {key}")))
}
fn string(table: &toml::Table, key: &str, path: &Path) -> Result<Option<String>> {
    match table.get(key) {
        None => Ok(None),
        Some(toml::Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(invalid(
            path,
            format!("legacy field {key} must be a string"),
        )),
    }
}
fn strings(table: &toml::Table, key: &str, path: &Path) -> Result<Vec<String>> {
    match table.get(key) {
        None => Ok(Vec::new()),
        Some(toml::Value::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| invalid(path, format!("legacy {key} values must be strings")))
            })
            .collect(),
        Some(_) => Err(invalid(path, format!("legacy {key} must be an array"))),
    }
}
pub(super) fn parse_table(path: &Path, bytes: &[u8]) -> Result<toml::Table> {
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(invalid(path, "legacy TOML document exceeds 2 MiB"));
    }
    toml::from_str(text(path, bytes)?).map_err(|error: toml::de::Error| {
        let mut diagnostic = invalid(path, format!("invalid legacy TOML: {}", error.message()));
        if let Some(span) = error.span() {
            let prefix = &bytes[..span.start.min(bytes.len())];
            diagnostic.line = Some(prefix.iter().filter(|byte| **byte == b'\n').count() + 1);
            diagnostic.column = Some(
                prefix
                    .iter()
                    .rev()
                    .take_while(|byte| **byte != b'\n')
                    .count()
                    + 1,
            );
        }
        diagnostic
    })
}
fn text<'a>(path: &Path, bytes: &'a [u8]) -> Result<&'a str> {
    std::str::from_utf8(bytes).map_err(|_| invalid(path, "legacy text must be UTF-8"))
}
fn path_text(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| invalid(path, "migration paths must be UTF-8"))
}
fn safe_reference(id: &str, path: &Path) -> Result<()> {
    if id.len() > 96
        || id.is_empty()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(invalid(
            path,
            format!("unsafe legacy reference identity {id:?}"),
        ));
    }
    safe_destination(Path::new(id)).map_err(|error| error.at(path))
}
fn safe_destination(path: &Path) -> Result<()> {
    SourceLink {
        path: path_text(path)?,
        line: None,
        end_line: None,
    }
    .validate()
}
fn invalid(path: &Path, message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::InvalidSchema, message).at(path)
}
pub(super) fn notice(notices: &mut Vec<MigrationNotice>, input: &Input, code: &str, message: &str) {
    notices.push(MigrationNotice {
        code: code.into(),
        path: input.path.clone(),
        message: message.into(),
    });
}
fn draft(
    input: &Input,
    kind: MigrationKind,
    destination_path: PathBuf,
    content: Vec<u8>,
) -> MigrationDraft {
    MigrationDraft {
        kind,
        source_path: Some(input.path.clone()),
        source_content: input.hash.clone(),
        destination_path,
        content_hash: crate::ContentHash::of(&content),
        content,
    }
}
pub(super) fn yaml(path: &Path, value: &impl serde::Serialize) -> Result<Vec<u8>> {
    let text = serde_yaml_ng::to_string(value).map_err(|error| invalid(path, error.to_string()))?;
    YamlDocument::parse(path, &text)?;
    Ok(text.into_bytes())
}
fn markdown(path: &Path, value: &impl serde::Serialize, body: &str) -> Result<Vec<u8>> {
    let header =
        serde_yaml_ng::to_string(value).map_err(|error| invalid(path, error.to_string()))?;
    let text = format!("---\n{header}---\n{body}");
    MarkdownDocument::parse(path, &text)?;
    Ok(text.into_bytes())
}
