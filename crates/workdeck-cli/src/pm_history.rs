//! Compatibility annotations use shared PM transactions and remain inert.
use super::{AgentCommand, Command, EventsCommand, pm_cli};
use clap::Args;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};
use workdeck_pm::{
    NewRecordedSession, RecordedSession, Repository, RequestId, Result, SessionCollection,
    SessionMutation,
};

#[derive(Debug, Default, Args)]
pub(super) struct HistoryOptions {
    #[arg(
        long,
        global = true,
        help = "Stable idempotency key for a native annotation mutation"
    )]
    request_id: Option<String>,
    #[arg(
        long,
        global = true,
        help = "Expected SHA-256 of the recorded session TOML"
    )]
    expected_content: Option<String>,
    #[arg(
        long,
        global = true,
        help = "Stage only paths changed by this native mutation"
    )]
    stage: bool,
    #[arg(
        long,
        global = true,
        help = "Require the noninteractive native annotation interface"
    )]
    no_input: bool,
}
impl HistoryOptions {
    pub(super) fn is_native(&self) -> bool {
        self.request_id.is_some() || self.expected_content.is_some() || self.stage || self.no_input
    }
    fn read(&self) -> Result<()> {
        if self.request_id.is_some() || self.expected_content.is_some() || self.stage {
            return Err(pm_cli::invalid(
                "request-id, expected-content and stage apply only to mutations",
            ));
        }
        Ok(())
    }
}

pub(super) fn run(
    cwd: &Path,
    repository: &Repository,
    source: &Value,
    command: &Command,
) -> Result<()> {
    if let Command::Events {
        command: EventsCommand::List { json },
    } = command
    {
        return pm_cli::emit(
            *json,
            "event_list",
            source,
            &json!({"historical_events":repository.historical_events()?,"mutation_receipts":repository.operation_history()?,"evidence":"historical_annotation"}),
            None,
        );
    }
    let Command::Agent { options, command } = command else {
        return Err(pm_cli::invalid("expected historical annotations command"));
    };
    let json_output = command.wants_json();
    match command {
        AgentCommand::List { .. } => {
            options.read()?;
            return pm_cli::emit(
                json_output,
                "agent_session_list",
                source,
                &repository.recorded_sessions()?,
                None,
            );
        }
        AgentCommand::Show { id, .. } => {
            options.read()?;
            return pm_cli::emit(
                json_output,
                "agent_session",
                source,
                &repository.recorded_session(id)?,
                None,
            );
        }
        _ => {}
    }
    let request = options
        .request_id
        .as_deref()
        .map(str::parse)
        .transpose()?
        .unwrap_or_else(RequestId::new);
    let expected = options
        .expected_content
        .as_deref()
        .map(str::parse)
        .transpose()?;
    let receipt = match command {
        AgentCommand::Record {
            title,
            id,
            agent,
            status,
            goal,
            summary,
            cwd,
            plan_item,
            touched_file,
            command_run,
            test_run,
            handoff_note,
            ..
        } => {
            if expected.is_some() {
                return Err(pm_cli::invalid(
                    "record creates a new annotation and cannot take expected-content",
                ));
            }
            let mut fields = fields(&[
                ("agent", agent),
                ("status", status),
                ("goal", goal),
                ("summary", summary),
            ]);
            if let Some(path) = cwd {
                fields.insert("cwd".into(), json!(path));
            }
            fields.insert("plan".into(), json!(plan_item));
            fields.insert("commands_run".into(), json!(command_run));
            fields.insert("tests_run".into(), json!(test_run));
            fields.insert("handoff_notes".into(), json!(handoff_note));
            fields.insert(
                "touched_files".into(),
                json!(
                    touched_file
                        .iter()
                        .map(|path| json!({"path":path,"change_type":""}))
                        .collect::<Vec<_>>()
                ),
            );
            repository.create_recorded_session(
                &NewRecordedSession {
                    id: id.clone(),
                    title: title.clone(),
                    fields,
                },
                &request,
            )?
        }
        AgentCommand::Import { path, .. } => {
            if expected.is_some() {
                return Err(pm_cli::invalid(
                    "import cannot take a single expected-content token",
                ));
            }
            let text = pm_cli::read_input(cwd, path)?;
            let sessions = decode(&text, path.extension().is_some_and(|ext| ext == "jsonl"))?;
            repository.import_recorded_sessions(&sessions, &request)?
        }
        _ => {
            let (id, mutation) = match command {
                AgentCommand::Update {
                    id,
                    title,
                    agent,
                    status,
                    goal,
                    summary,
                    cwd,
                    ..
                } => {
                    let mut fields = fields(&[
                        ("title", title),
                        ("agent", agent),
                        ("status", status),
                        ("goal", goal),
                        ("summary", summary),
                    ]);
                    if let Some(path) = cwd {
                        fields.insert("cwd".into(), json!(path));
                    }
                    (id, SessionMutation::Update { fields })
                }
                AgentCommand::Finish { id, summary, .. } => (
                    id,
                    SessionMutation::Finish {
                        summary: summary.clone(),
                    },
                ),
                AgentCommand::AppendPlan { id, text, .. } => (
                    id,
                    SessionMutation::Append {
                        field: SessionCollection::Plan,
                        text: text.clone(),
                    },
                ),
                AgentCommand::AddCommand { id, text, .. } => (
                    id,
                    SessionMutation::Append {
                        field: SessionCollection::Commands,
                        text: text.clone(),
                    },
                ),
                AgentCommand::AddTest { id, text, .. } => (
                    id,
                    SessionMutation::Append {
                        field: SessionCollection::Tests,
                        text: text.clone(),
                    },
                ),
                AgentCommand::AddNote { id, text, .. } => (
                    id,
                    SessionMutation::Append {
                        field: SessionCollection::Notes,
                        text: text.clone(),
                    },
                ),
                AgentCommand::AddFile {
                    id,
                    path,
                    change_type,
                    ..
                } => (
                    id,
                    SessionMutation::AddFile {
                        path: path.clone(),
                        change_type: change_type.clone(),
                    },
                ),
                AgentCommand::Delete { id, yes, .. } => {
                    if !yes {
                        return Err(pm_cli::invalid("delete requires --yes"));
                    }
                    (id, SessionMutation::Delete)
                }
                _ => unreachable!(),
            };
            repository.mutate_recorded_session(id, expected.as_ref(), &mutation, &request)?
        }
    };
    let flags = pm_cli::IssueOptions {
        stage: options.stage,
        ..Default::default()
    };
    pm_cli::emit_mutation(
        repository,
        &flags,
        json_output,
        "agent_session",
        source,
        &receipt,
    )
}
fn fields(values: &[(&str, &Option<String>)]) -> BTreeMap<String, Value> {
    values
        .iter()
        .filter_map(|(key, value)| value.as_ref().map(|value| ((*key).into(), json!(value))))
        .collect()
}
fn decode(text: &str, jsonl: bool) -> Result<Vec<RecordedSession>> {
    let parse = |mut value: Value| {
        // Prototype imports accepted direct rows, {session: ...}, and
        // {payload: {session: ...}}. Keep a direct record's unknown fields;
        // wrapper selection never projects fields out of the selected record.
        if value.get("id").is_none() {
            let direct = value.get("session");
            let nested = value.pointer("/payload/session");
            if direct.is_some() && nested.is_some() {
                return Err(pm_cli::invalid("session import has ambiguous wrappers"));
            }
            if let Some(session) = direct.or(nested) {
                value = session.clone();
            }
        }
        serde_json::from_value(value)
            .map_err(|failure| pm_cli::invalid(format!("invalid recorded session: {failure}")))
    };
    if jsonl {
        let mut records = Vec::new();
        for (index, line) in text
            .lines()
            .enumerate()
            .filter(|(_, line)| !line.trim().is_empty())
        {
            if records.len() == 10_000 {
                return Err(pm_cli::invalid("session import exceeds 10,000 records"));
            }
            let value = serde_json::from_str(line).map_err(|failure| {
                let mut error = pm_cli::invalid(format!("invalid session JSONL: {failure}"));
                error.line = Some(index + 1);
                error.column = Some(failure.column());
                error
            })?;
            records.push(parse(value)?);
        }
        return Ok(records);
    }
    let value: Value = serde_json::from_str(text)
        .map_err(|failure| pm_cli::invalid(format!("invalid session JSON: {failure}")))?;
    match value {
        Value::Array(values) if values.len() <= 10_000 => values.into_iter().map(parse).collect(),
        Value::Array(_) => Err(pm_cli::invalid("session import exceeds 10,000 records")),
        Value::Object(_) => Ok(vec![parse(value)?]),
        _ => Err(pm_cli::invalid(
            "session import requires an object or array",
        )),
    }
}
