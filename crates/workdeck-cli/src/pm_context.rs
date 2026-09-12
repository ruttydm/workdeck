//! Bounded agent reads use the shared captured context and action operations.
use super::pm_cli;
use clap::Args;
use serde_json::{Value, json};
use std::io::Write;
use workdeck_pm::{ContextRequest, NextActionRequest, NextIssueRequest, Repository, Result};

#[derive(Debug, Default, Args)]
pub(super) struct OutputOptions {
    #[arg(long, global = true, help = "Emit compact JSON, also for human output")]
    pub compact: bool,
    #[arg(
        long,
        global = true,
        value_delimiter = ',',
        help = "Select result fields by dotted path; envelope identity is retained"
    )]
    pub fields: Vec<String>,
}

#[derive(Debug, Args)]
pub(super) struct ContextOptions {
    #[command(flatten)]
    pub native: pm_cli::NativeOptions,
    #[command(flatten)]
    pub output: OutputOptions,
    #[arg(long)]
    pub issue: String,
    #[arg(
        long,
        default_value_t = 16384,
        help = "Maximum UTF-8 stdout bytes, including compact JSON envelope and newline"
    )]
    pub budget: usize,
    #[arg(
        long,
        help = "Explicit RFC3339 time for evidence freshness; omitted means unknown"
    )]
    pub as_of: Option<String>,
    #[arg(long, help = "Reject a changed captured context fingerprint")]
    pub expected_context: Option<String>,
}

#[derive(Debug, Args)]
pub(super) struct NextOptions {
    #[command(flatten)]
    pub native: pm_cli::NativeOptions,
    #[command(flatten)]
    pub output: OutputOptions,
    #[arg(long)]
    pub issue: String,
    #[arg(long)]
    pub expected_context: Option<String>,
}

#[derive(Debug, Args)]
pub(super) struct NextIssueOptions {
    #[arg(long)]
    pub json: bool,
    #[command(flatten)]
    pub output: OutputOptions,
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
    #[arg(
        long,
        help = "JSON next_cursor from a prior page; changed query/source is rejected"
    )]
    pub cursor: Option<String>,
    #[arg(long)]
    pub query: Option<String>,
    #[arg(long)]
    pub status: Option<String>,
    #[arg(long)]
    pub priority: Option<String>,
    #[arg(long)]
    pub assignee: Option<String>,
    #[arg(long)]
    pub project: Option<String>,
    #[arg(long)]
    pub label: Option<String>,
}

pub(super) fn context(
    repository: &Repository,
    source: &Value,
    options: &ContextOptions,
) -> Result<()> {
    pm_cli::read_options(&options.native.mutation)?;
    options.output.validate()?;
    // This is the exact envelope emitted below; null contributes four bytes.
    let overhead = serde_json::to_vec(&envelope("context", source, Value::Null))
        .map_err(|error| pm_cli::invalid(error.to_string()))?
        .len()
        - 4
        + 1;
    let available = options.budget.checked_sub(overhead).ok_or_else(|| {
        pm_cli::invalid("context budget is smaller than its response envelope")
            .details(json!({"minimum_envelope_bytes":overhead}))
    })?;
    let mut request = ContextRequest::new(&options.issue, available);
    request.as_of = options
        .as_of
        .as_deref()
        .map(|value| {
            value.parse().map_err(|error| {
                pm_cli::invalid(format!("invalid RFC3339 assessment time: {error}"))
            })
        })
        .transpose()?;
    request.expected_context = options
        .expected_context
        .as_deref()
        .map(str::parse)
        .transpose()?;
    let packet = repository.context(&request).map_err(|mut error| {
        if let Some(details) = error.details.as_mut()
            && let Some(object) = details.as_object_mut()
        {
            object.insert("transport_overhead_bytes".into(), json!(overhead));
            if let Some(minimum) = object.get("minimum_required_bytes").and_then(Value::as_u64) {
                object.insert(
                    "minimum_stdout_bytes".into(),
                    json!(minimum.saturating_add(overhead as u64)),
                );
            }
        }
        error
    })?;
    let result = options.output.project(
        serde_json::to_value(packet).map_err(|error| pm_cli::invalid(error.to_string()))?,
    )?;
    // Context always uses compact JSON so formatting cannot exceed the inspected budget.
    let mut bytes = serde_json::to_vec(&envelope("context", source, result))
        .map_err(|error| pm_cli::invalid(error.to_string()))?;
    bytes.push(b'\n');
    if bytes.len() > options.budget {
        return Err(
            pm_cli::invalid("context response exceeds the requested stdout budget")
                .details(json!({"budget_bytes":options.budget,"required_bytes":bytes.len()})),
        );
    }
    std::io::stdout()
        .lock()
        .write_all(&bytes)
        .map_err(|error| workdeck_pm::PmError::io("stdout", error))
}

pub(super) fn next(repository: &Repository, source: &Value, options: &NextOptions) -> Result<()> {
    pm_cli::read_options(&options.native.mutation)?;
    options.output.validate()?;
    let mut request = NextActionRequest::new(&options.issue);
    request.expected_context = options
        .expected_context
        .as_deref()
        .map(str::parse)
        .transpose()?;
    let result = repository.next_actions(&request)?;
    options
        .output
        .emit(options.native.json, "next_actions", source, &result)
}

pub(super) fn next_issue(
    repository: &Repository,
    source: &Value,
    mutation: &pm_cli::IssueOptions,
    options: &NextIssueOptions,
) -> Result<()> {
    pm_cli::read_options(mutation)?;
    options.output.validate()?;
    let mut request = NextIssueRequest {
        limit: options.limit,
        cursor: options
            .cursor
            .as_deref()
            .map(|value| {
                if value.len() > 4096 {
                    return Err(pm_cli::invalid("cursor exceeds 4096 bytes"));
                }
                serde_json::from_str(value)
                    .map_err(|error| pm_cli::invalid(format!("invalid cursor JSON: {error}")))
            })
            .transpose()?,
        ..NextIssueRequest::default()
    };
    request.query.query = options.query.clone().unwrap_or_default();
    request.query.status = options.status.clone();
    request.query.priority = options
        .priority
        .as_deref()
        .map(workdeck_pm::Priority::parse_input)
        .transpose()?;
    request.query.assignee = options.assignee.clone();
    request.query.project = options.project.clone();
    request.query.label = options.label.clone();
    let result = repository.next_issue(&request)?;
    options
        .output
        .emit(options.json, "next_issue", source, &result)
}

pub(super) fn envelope(kind: &str, source: &Value, result: Value) -> Value {
    json!({"api_version":1,"ok":true,"kind":kind,"source":source,"result":result})
}

impl OutputOptions {
    pub fn machine(&self) -> bool {
        self.compact || !self.fields.is_empty()
    }
    pub fn mutation(
        &self,
        repository: &Repository,
        options: &pm_cli::NativeOptions,
        kind: &str,
        source: &Value,
        receipt: &workdeck_pm::transactions::MutationReceipt,
    ) -> Result<()> {
        if self.fields.is_empty() {
            return pm_cli::emit_mutation(
                repository,
                &options.mutation,
                options.json || self.machine(),
                kind,
                source,
                receipt,
            );
        }
        let result = self.project(receipt.result.clone()).map_err(|error| {
            error.details(json!({
                "mutation_committed":true,"receipt":receipt,"output_projection_failed":true
            }))
        })?;
        let staging = options
            .mutation
            .stage
            .then(|| pm_cli::stage_receipt(repository, receipt))
            .transpose()?;
        let mut value = envelope(kind, source, result);
        value["receipt"] =
            serde_json::to_value(receipt).map_err(|error| pm_cli::invalid(error.to_string()))?;
        if let Some(staging) = staging {
            value["staging"] = json!(staging);
        }
        let mut bytes =
            serde_json::to_vec(&value).map_err(|error| pm_cli::invalid(error.to_string()))?;
        bytes.push(b'\n');
        std::io::stdout()
            .lock()
            .write_all(&bytes)
            .map_err(|error| workdeck_pm::PmError::io("stdout", error))
    }
    pub fn validate(&self) -> Result<()> {
        if self.fields.len() > 32 {
            return Err(pm_cli::invalid("select at most 32 fields"));
        }
        for field in &self.fields {
            if field.len() > 256
                || field.split('.').count() > 8
                || field.split('.').any(|part| {
                    part.is_empty()
                        || !part
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                })
            {
                return Err(pm_cli::invalid(
                    "fields must be bounded dotted object paths",
                ));
            }
        }
        Ok(())
    }
    pub fn project(&self, value: Value) -> Result<Value> {
        self.validate()?;
        if self.fields.is_empty() {
            return Ok(value);
        }
        let mut projected = json!({});
        for field in &self.fields {
            let parts: Vec<_> = field.split('.').collect();
            let mut selected = &value;
            for part in &parts {
                selected = selected
                    .get(*part)
                    .ok_or_else(|| pm_cli::invalid(format!("unknown result field {field:?}")))?;
            }
            let mut destination = &mut projected;
            for part in &parts[..parts.len() - 1] {
                if destination.get(*part).is_none() {
                    destination[*part] = json!({});
                }
                destination = &mut destination[*part];
                if !destination.is_object() {
                    return Err(pm_cli::invalid("selected field paths overlap"));
                }
            }
            destination[parts[parts.len() - 1]] = selected.clone();
        }
        Ok(projected)
    }
    pub fn emit(
        &self,
        json_output: bool,
        kind: &str,
        source: &Value,
        result: &impl serde::Serialize,
    ) -> Result<()> {
        let result = self.project(
            serde_json::to_value(result).map_err(|error| pm_cli::invalid(error.to_string()))?,
        )?;
        pm_cli::emit(json_output || self.machine(), kind, source, &result, None)
    }
}
