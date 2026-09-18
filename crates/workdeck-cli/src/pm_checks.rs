//! Named recipe discovery and explicitly requested local verification.
use super::{pm_cli, pm_context::OutputOptions};
use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use workdeck_pm::*;

#[derive(Debug, Args)]
pub(super) struct Options {
    #[command(flatten)]
    pub native: pm_cli::NativeOptions,
    #[command(flatten)]
    pub output: OutputOptions,
    #[arg(long, global = true)]
    limit: Option<usize>,
    #[arg(long, global = true)]
    cursor: Option<String>,
}

#[derive(Debug, Args)]
pub(super) struct RunOptions {
    #[arg(
        long,
        help = "Actor attribution for the explicitly requested local run"
    )]
    actor: String,
    #[arg(long, help = "Exact fingerprint of the reviewed plan")]
    expected_plan: String,
}

#[derive(Debug, Subcommand)]
pub(super) enum NamedCommand {
    List,
    Show {
        id: String,
    },
    Validate,
    #[command(about = "Capture a named command's inputs without executing its recipe")]
    Plan {
        id: String,
        #[arg(long)]
        arguments_file: Option<PathBuf>,
    },
    #[command(about = "Explicitly execute the exact reviewed local command plan")]
    Run {
        #[arg(long, help = "Saved plan fingerprint or a CheckPlan JSON file")]
        plan: String,
        #[command(flatten)]
        run: RunOptions,
    },
}

#[derive(Debug, Subcommand)]
pub(super) enum ProfileCommand {
    List,
    Show { id: String },
    Validate,
}

#[derive(Debug, Subcommand)]
pub(super) enum CheckCommand {
    List,
    Show {
        id: String,
    },
    Validate,
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    #[command(about = "Select required checks and capture exact local inputs without execution")]
    Plan {
        #[arg(long)]
        issue: Option<String>,
        #[arg(long = "check")]
        checks: Vec<String>,
        #[arg(long = "profile")]
        profiles: Vec<String>,
        #[arg(
            long = "changed-path",
            help = "Advisory changed path; incomplete impact information never removes required checks"
        )]
        changed_paths: Vec<PathBuf>,
        #[arg(long)]
        arguments_file: Option<PathBuf>,
    },
    #[command(about = "Execute an exact reviewed plan as bounded foreground local verification")]
    Run {
        #[arg(long, help = "Saved plan fingerprint or a CheckPlan JSON file")]
        plan: String,
        #[command(flatten)]
        run: RunOptions,
    },
    Status {
        run: String,
    },
    #[command(
        about = "Export a portable terminal report with local-feedback provenance and freshness"
    )]
    Export {
        run: String,
    },
    #[command(about = "Reconcile a retained run without starting another process")]
    Recover {
        run: String,
    },
    Results {
        #[arg(long)]
        issue: Option<String>,
        #[arg(long, value_delimiter = ',')]
        status: Vec<String>,
    },
    Explain {
        run: String,
        #[arg(long)]
        check: Option<String>,
    },
}

impl NamedCommand {
    pub(super) fn is_mutation(&self) -> bool {
        matches!(self, Self::Run { .. })
    }
}
impl CheckCommand {
    pub(super) fn is_mutation(&self) -> bool {
        matches!(self, Self::Run { .. } | Self::Recover { .. })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    fingerprint: ContentHash,
    offset: usize,
}

fn invalid(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::InvalidInput, message)
}

fn read_options(options: &Options, paged: bool) -> Result<()> {
    pm_cli::read_options(&options.native.mutation)?;
    options.output.validate()?;
    if !paged && (options.limit.is_some() || options.cursor.is_some()) {
        return Err(invalid("pagination is supported only for list and results"));
    }
    Ok(())
}

fn page(
    options: &Options,
    source: &Value,
    kind: &str,
    identity: &ContentHash,
    records: Vec<Value>,
) -> Result<()> {
    let limit = options.limit.unwrap_or(20);
    if !(1..=100).contains(&limit) {
        return Err(invalid("limit must be between 1 and 100"));
    }
    let fingerprint = ContentHash::of(&serde_json::to_vec(&json!({"source":source,"kind":kind,"identity":identity,"limit":limit,"records":records})).map_err(|e| invalid(e.to_string()))?);
    let offset = if let Some(raw) = &options.cursor {
        if raw.len() > 4096 {
            return Err(invalid("cursor exceeds 4096 bytes"));
        }
        let cursor: Cursor =
            serde_json::from_str(raw).map_err(|_| invalid("invalid result cursor"))?;
        if cursor.fingerprint != fingerprint {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "catalog or result inputs changed; restart pagination",
            ));
        }
        if cursor.offset > records.len() {
            return Err(invalid("cursor offset exceeds result count"));
        }
        cursor.offset
    } else {
        0
    };
    let end = offset.saturating_add(limit).min(records.len());
    let next = (end < records.len()).then(|| Cursor {
        fingerprint: fingerprint.clone(),
        offset: end,
    });
    options.output.emit(options.native.json, kind, source, &json!({"records":&records[offset..end],"total":records.len(),"offset":offset,"limit":limit,"next_cursor":next,"fingerprint":fingerprint}))
}

fn arguments<T: serde::de::DeserializeOwned + Default>(
    cwd: &Path,
    path: Option<&Path>,
) -> Result<T> {
    path.map(|path| pm_cli::typed_input(cwd, path))
        .unwrap_or_else(|| Ok(T::default()))
}

fn validate(repository: &Repository, source: &Value, options: &Options) -> Result<()> {
    read_options(options, false)?;
    let report = repository.validate_command_catalog()?;
    if !report.valid {
        return Err(PmError::new(
            ErrorCode::InvalidSchema,
            "command/check/profile catalog is invalid",
        )
        .details(json!({"validation":report})));
    }
    options.output.emit(
        options.native.json,
        "check_catalog_validation",
        source,
        &report,
    )
}

fn command_rows(catalog: &CommandCatalogSnapshot) -> Vec<Value> {
    catalog.commands.iter().map(|r|json!({"id":r.definition.id,"name":r.definition.name,"archived":r.definition.archived,"path":r.path,"content":r.content})).collect()
}
fn check_rows(catalog: &CommandCatalogSnapshot) -> Vec<Value> {
    catalog.checks.iter().map(|r|json!({"id":r.definition.id,"name":r.definition.name,"command":r.definition.command,"archived":r.definition.archived,"path":r.path,"content":r.content})).collect()
}
fn profile_rows(catalog: &CommandCatalogSnapshot) -> Vec<Value> {
    catalog.profiles.iter().map(|r|json!({"id":r.definition.id,"name":r.definition.name,"checks":r.definition.checks,"archived":r.definition.archived,"path":r.path,"content":r.content})).collect()
}

fn save_plan(
    repository: &Repository,
    source: &Value,
    options: &Options,
    plan: CheckPlan,
) -> Result<()> {
    let path = repository.save_check_plan(&plan)?;
    options.output.emit(
        options.native.json,
        "check_plan",
        source,
        &json!({"plan":plan,"saved_plan":path}),
    )
}

fn load_plan(cwd: &Path, repository: &Repository, reference: &str) -> Result<CheckPlan> {
    if let Ok(fingerprint) = reference.parse::<ContentHash>() {
        repository.load_check_plan(&fingerprint)
    } else {
        let path = Path::new(reference);
        let bytes = if path == Path::new("-") {
            use std::io::Read;
            let mut bytes = Vec::new();
            std::io::stdin()
                .lock()
                .take((MAX_CHECK_PLAN_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|error| PmError::io(path, error))?;
            if bytes.len() > MAX_CHECK_PLAN_BYTES {
                return Err(invalid("plan input exceeds 4 MiB").at(path));
            }
            bytes
        } else {
            pm_cli::read_regular_input(cwd, path, MAX_CHECK_PLAN_BYTES as u64)?
        };
        let plan: CheckPlan = serde_json::from_slice(&bytes)
            .map_err(|error| invalid(format!("invalid plan JSON: {error}")).at(path))?;
        plan.validate()?;
        Ok(plan)
    }
}

fn emit_run(
    repository: &Repository,
    source: &Value,
    options: &Options,
    outcome: &RunOutcome,
) -> Result<Option<u8>> {
    use std::io::Write;
    let mut value = options.output.project(serde_json::to_value(outcome).map_err(|e| invalid(e.to_string()))?)
        .map_err(|error| error.details(json!({"mutation_committed":true,"run_id":outcome.run.intent.id,"request_id":outcome.run.intent.request_id,
            "receipts":outcome.receipts,"output_projection_failed":true})))?;
    // Projection never removes recovery identity or the durable execution receipts.
    value["run_id"] = json!(outcome.run.intent.id);
    value["request_id"] = json!(outcome.run.intent.request_id);
    value["receipts"] = json!(outcome.receipts);
    if options.native.mutation.stage {
        let staging = outcome.receipts.iter().map(|receipt| pm_cli::stage_receipt(repository,receipt)).collect::<Result<Vec<_>>>()
            .map_err(|error| error.details(json!({"mutation_committed":true,"run_id":outcome.run.intent.id,
                "request_id":outcome.run.intent.request_id,"receipts":outcome.receipts,"staging":{"state":"failed"}})))?;
        value["staging"] = json!(staging);
    }
    if options.native.json || options.output.machine() {
        let envelope = super::pm_context::envelope("local_run", source, value);
        let mut bytes = serde_json::to_vec(&envelope).map_err(|e| invalid(e.to_string()))?;
        bytes.push(b'\n');
        std::io::stdout()
            .lock()
            .write_all(&bytes)
            .map_err(|e| PmError::io("stdout", e))?;
    } else {
        pm_cli::emit(
            false,
            "local_run",
            source,
            &json!({"run":outcome.run.intent.id,"state":outcome.state,
            "basis":outcome.assessment.basis,"reason_codes":outcome.assessment.reason_codes,"request_id":outcome.run.intent.request_id}),
            None,
        )?;
    }
    Ok(match outcome.state {
        RunState::Passed => None,
        RunState::Canceled => Some(130),
        RunState::Failed => Some(1),
        RunState::Stale | RunState::Running => Some(4),
        RunState::Unknown => Some(6),
        RunState::Blocked | RunState::NotRun | RunState::Skipped => Some(5),
    })
}

fn run_plan(
    cwd: &Path,
    repository: &Repository,
    source: &Value,
    options: &Options,
    reference: &str,
    run: &RunOptions,
    command_only: bool,
) -> Result<Option<u8>> {
    options.output.validate()?;
    if options.limit.is_some() || options.cursor.is_some() {
        return Err(invalid("execution does not accept pagination"));
    }
    if options.native.mutation.expected_revision.is_some()
        || options.native.mutation.expected_content.is_some()
    {
        return Err(invalid(
            "execution requires --expected-plan instead of a record source precondition",
        ));
    }
    let request: RequestId = options
        .native
        .mutation
        .request_id
        .as_deref()
        .ok_or_else(|| {
            invalid("execution requires --request-id; retain it for recovery and retries")
        })?
        .parse()?;
    let plan = load_plan(cwd, repository, reference)?;
    if command_only != matches!(plan.request, ExecutionPlanRequest::Command { .. }) {
        return Err(invalid("plan kind does not match this command family"));
    }
    let input = CheckRunRequest {
        plan,
        expected_plan: run.expected_plan.parse()?,
        actor: run.actor.clone(),
    };
    let control = RunControl::default();
    let mut signals = super::register_process_signal_callback({
        let control = control.clone();
        move || {
            if control.cancellation_requested() {
                control.force_cancel();
            } else {
                control.cancel();
            }
        }
    })
    .map_err(|e| PmError::new(ErrorCode::Io, e))?;
    let result = repository.run_check_plan(&input, &request, &control);
    signals.retire();
    emit_run(repository, source, options, &result?)
}

pub(super) fn command(
    cwd: &Path,
    repository: &Repository,
    source: &Value,
    options: &Options,
    command: &NamedCommand,
) -> Result<Option<u8>> {
    match command {
        NamedCommand::Run { plan, run } => {
            return run_plan(cwd, repository, source, options, plan, run, true);
        }
        NamedCommand::Validate => validate(repository, source, options)?,
        NamedCommand::List => {
            read_options(options, true)?;
            let catalog = repository.command_catalog()?;
            page(
                options,
                source,
                "command_list",
                &catalog.fingerprint,
                command_rows(&catalog),
            )?;
        }
        NamedCommand::Show { id } => {
            read_options(options, false)?;
            options.output.emit(
                options.native.json,
                "command",
                source,
                &repository.command(id)?,
            )?;
        }
        NamedCommand::Plan { id, arguments_file } => {
            read_options(options, false)?;
            let input = CommandPlanRequest {
                command: id.clone(),
                arguments: arguments(cwd, arguments_file.as_deref())?,
            };
            save_plan(
                repository,
                source,
                options,
                repository.command_plan(&input)?,
            )?;
        }
    }
    Ok(None)
}

pub(super) fn check(
    cwd: &Path,
    repository: &Repository,
    source: &Value,
    options: &Options,
    command: &CheckCommand,
) -> Result<Option<u8>> {
    match command {
        CheckCommand::Run { plan, run } => {
            return run_plan(cwd, repository, source, options, plan, run, false);
        }
        CheckCommand::Recover { run } => {
            options.output.validate()?;
            if options.native.mutation.request_id.is_some()
                || options.native.mutation.expected_revision.is_some()
                || options.native.mutation.expected_content.is_some()
                || options.limit.is_some()
                || options.cursor.is_some()
            {
                return Err(invalid(
                    "recovery uses the run's original identity; only output and staging options apply",
                ));
            }
            return emit_run(
                repository,
                source,
                options,
                &repository.recover_run(&run.parse()?)?,
            );
        }
        CheckCommand::Validate
        | CheckCommand::Profile {
            command: ProfileCommand::Validate,
        } => validate(repository, source, options)?,
        CheckCommand::List => {
            read_options(options, true)?;
            let catalog = repository.command_catalog()?;
            page(
                options,
                source,
                "check_list",
                &catalog.fingerprint,
                check_rows(&catalog),
            )?;
        }
        CheckCommand::Show { id } => {
            read_options(options, false)?;
            options
                .output
                .emit(options.native.json, "check", source, &repository.check(id)?)?;
        }
        CheckCommand::Profile {
            command: ProfileCommand::List,
        } => {
            read_options(options, true)?;
            let catalog = repository.command_catalog()?;
            page(
                options,
                source,
                "check_profile_list",
                &catalog.fingerprint,
                profile_rows(&catalog),
            )?;
        }
        CheckCommand::Profile {
            command: ProfileCommand::Show { id },
        } => {
            read_options(options, false)?;
            options.output.emit(
                options.native.json,
                "check_profile",
                source,
                &repository.check_profile(id)?,
            )?;
        }
        CheckCommand::Plan {
            issue,
            checks,
            profiles,
            changed_paths,
            arguments_file,
        } => {
            read_options(options, false)?;
            let request = CheckPlanRequest {
                issue: issue.clone(),
                checks: checks.clone(),
                profiles: profiles.clone(),
                arguments: arguments::<BTreeMap<String, ArgumentValues>>(
                    cwd,
                    arguments_file.as_deref(),
                )?,
                changed_paths: changed_paths.clone(),
            };
            save_plan(
                repository,
                source,
                options,
                repository.check_plan(&request)?,
            )?;
        }
        CheckCommand::Export { run } => {
            read_options(options, false)?;
            options.output.emit(
                options.native.json,
                "check_report",
                source,
                &repository.export_check_report(&run.parse()?)?,
            )?;
        }
        CheckCommand::Status { run } => {
            read_options(options, false)?;
            options.output.emit(
                options.native.json,
                "check_status",
                source,
                &repository.check_status(&run.parse()?)?,
            )?;
        }
        CheckCommand::Explain { run, check } => {
            read_options(options, false)?;
            let outcome = repository.check_status(&run.parse()?)?;
            let matching = outcome
                .results
                .as_ref()
                .map(|record| {
                    record
                        .result
                        .checks
                        .iter()
                        .filter(|result| check.as_ref().is_none_or(|id| &result.check.id == id))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if check.is_some() && matching.is_empty() {
                return Err(PmError::new(
                    ErrorCode::NotFound,
                    "check result was not found in this run",
                ));
            }
            let invocations = outcome
                .results
                .as_ref()
                .map(|record| {
                    record
                        .result
                        .invocations
                        .iter()
                        .filter(|invocation| {
                            check.is_none()
                                || matching
                                    .iter()
                                    .any(|item| item.invocation == invocation.index)
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            options.output.emit(options.native.json,"check_explanation",source,&json!({"run":outcome.run.intent.id,"assessment":outcome.assessment,"checks":matching,"invocations":invocations}))?;
        }
        CheckCommand::Results { issue, status } => {
            read_options(options, true)?;
            let states = status
                .iter()
                .map(|value| {
                    serde_json::from_value::<RunState>(json!(value))
                        .map_err(|_| invalid("unknown run status"))
                })
                .collect::<Result<Vec<_>>>()?;
            let query = RunQuery {
                issue: issue.as_deref().map(str::parse).transpose()?,
                ..RunQuery::default()
            };
            let outcomes = repository.check_results(&query)?;
            let identity = ContentHash::of(
                &serde_json::to_vec(&outcomes).map_err(|e| invalid(e.to_string()))?,
            );
            let rows=outcomes.into_iter().filter(|r|states.is_empty()||states.contains(&r.state))
                .map(|r|json!({"run":r.run.intent.id,"recorded_at":r.run.intent.recorded_at,"state":r.state,"assessment":r.assessment,"result":r.results.map(|r|r.content)})).collect();
            page(options, source, "check_results", &identity, rows)?;
        }
    }
    Ok(None)
}
