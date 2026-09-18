//! Native PM command adapter. Semantic mutations go directly to the replay-aware
//! application API; adapters never manufacture updates from a current issue read.
use super::{Command, IssueCommand, IssueLabelCommand, MigrateCommand};
use clap::{Args, Subcommand};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use workdeck_pm::{
    ArchiveFilter, AttachmentInput, CreateIssue, ErrorCode, IssueCollection, IssueMutation,
    IssueQuery, IssueSort, IssueSortField, MAX_ATTACHMENT_BYTES, ManualAcceptanceInput, PmError,
    Repository, RequestId, Revision, SortDirection, SourceToken, TargetMatch, TemplateIssueInput,
    UpdateIssue, transactions::MutationReceipt,
};

const MAX_INPUT_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Default, Args)]
pub(super) struct IssueListOptions {
    #[arg(
        long,
        help = "Literal text in issue identity, title, body, labels or assignee (native PM)"
    )]
    query: Option<String>,
    #[arg(long, value_parser = ["active", "all", "archived"], help = "Archive scope; omitted preserves the CLI default of all records (native PM)")]
    archive: Option<String>,
    #[arg(long, help = "Exact milestone identity (native PM)")]
    milestone: Option<String>,
    #[arg(
        long = "target",
        help = "Repeatable target identity; includes direct, project and milestone membership (native PM)"
    )]
    targets: Vec<String>,
    #[arg(long, value_parser = ["all", "any"], requires = "targets", help = "Match all supplied targets (default) or any supplied target")]
    target_match: Option<String>,
    #[arg(
        long,
        value_name = "FIELD:asc|desc",
        help = "Repeatable sort: created_at, updated_at, priority, title or id; defaults to created_at:asc (native PM)"
    )]
    sort: Vec<String>,
}

impl IssueListOptions {
    fn is_native(&self) -> bool {
        self.query.is_some()
            || self.archive.is_some()
            || self.milestone.is_some()
            || !self.sort.is_empty()
            || !self.targets.is_empty()
            || self.target_match.is_some()
    }

    pub(super) fn query(&self) -> workdeck_pm::Result<IssueQuery> {
        let mut query = IssueQuery::all();
        query.query = self.query.clone().unwrap_or_default();
        query.milestone = self.milestone.clone();
        query.targets = self.targets.clone();
        query.target_match = match self.target_match.as_deref() {
            None | Some("all") => TargetMatch::All,
            Some("any") => TargetMatch::Any,
            _ => return Err(invalid("target matching must be all or any")),
        };
        query.archive = match self.archive.as_deref() {
            None | Some("all") => ArchiveFilter::All,
            Some("active") => ArchiveFilter::Active,
            Some("archived") => ArchiveFilter::Archived,
            _ => return Err(invalid("archive scope must be active, all, or archived")),
        };
        if !self.sort.is_empty() {
            query.sort = self
                .sort
                .iter()
                .map(|sort| {
                    let (field, direction) = sort
                        .split_once(':')
                        .ok_or_else(|| invalid("sort requires FIELD:asc or FIELD:desc"))?;
                    let field = match field {
                        "created_at" => IssueSortField::CreatedAt,
                        "updated_at" => IssueSortField::UpdatedAt,
                        "priority" => IssueSortField::Priority,
                        "title" => IssueSortField::Title,
                        "id" => IssueSortField::Id,
                        _ => return Err(invalid(format!("unknown issue sort field {field:?}"))),
                    };
                    let direction = match direction {
                        "asc" => SortDirection::Ascending,
                        "desc" => SortDirection::Descending,
                        _ => return Err(invalid("sort direction must be asc or desc")),
                    };
                    Ok(IssueSort { field, direction })
                })
                .collect::<workdeck_pm::Result<Vec<_>>>()?;
        }
        Ok(query)
    }

    pub(super) fn reject_legacy(&self, source: &Path) -> anyhow::Result<()> {
        if self.is_native() {
            return Err(CommandFailure {
                error: legacy_read_only(source),
                source_identity: json!({"repository":null,"root":source,"state":"legacy"}),
            }
            .into());
        }
        Ok(())
    }
}

#[derive(Debug, Default, Args)]
pub(super) struct IssueAssociationOptions {
    #[arg(
        long = "feature",
        help = "Repeat to replace the issue's native feature associations"
    )]
    features: Vec<String>,
    #[arg(
        long = "gate",
        help = "Repeat to replace the issue's native completion gates"
    )]
    gates: Vec<String>,
    #[arg(long, help = "Milestone identity; must belong to the issue's project")]
    milestone: Option<String>,
    #[arg(
        long = "target",
        help = "Repeat to replace the issue's direct target associations"
    )]
    targets: Vec<String>,
    #[arg(
        long,
        visible_alias = "unset",
        value_name = "FIELD",
        help = "Repeat to clear project, cycle, milestone, targets, features or gates; overrides JSON or template values"
    )]
    clear: Vec<String>,
}

impl IssueAssociationOptions {
    fn is_native(&self) -> bool {
        self.milestone.is_some()
            || !self.targets.is_empty()
            || !self.features.is_empty()
            || !self.gates.is_empty()
            || !self.clear.is_empty()
    }

    fn apply(
        &self,
        fields: &mut BTreeMap<String, Value>,
        project: &Option<String>,
        cycle: &Option<String>,
    ) -> workdeck_pm::Result<()> {
        if let Some(milestone) = &self.milestone {
            fields.insert("milestone".into(), json!(milestone));
        }
        if !self.targets.is_empty() {
            fields.insert("targets".into(), json!(self.targets));
        }
        if !self.features.is_empty() {
            fields.insert("features".into(), json!(self.features));
        }
        if !self.gates.is_empty() {
            fields.insert("gates".into(), json!(self.gates));
        }
        let mut cleared = std::collections::BTreeSet::new();
        for field in &self.clear {
            let set = match field.as_str() {
                "project" => project.is_some(),
                "cycle" => cycle.is_some(),
                "milestone" => self.milestone.is_some(),
                "targets" => !self.targets.is_empty(),
                "features" => !self.features.is_empty(),
                "gates" => !self.gates.is_empty(),
                _ => {
                    return Err(invalid(
                        "issue --clear supports project, cycle, milestone, targets, features and gates",
                    ));
                }
            };
            if set || !cleared.insert(field) {
                return Err(invalid(format!(
                    "association {field:?} cannot be both set and cleared, or cleared twice"
                )));
            }
            // Explicit CLI clearing wins over structured or template defaults,
            // while contradictory flags fail before any mutation is prepared.
            fields.insert(field.clone(), Value::Null);
        }
        Ok(())
    }
}

#[derive(Debug, Default, Args)]
pub(super) struct RetirementOptions {
    #[arg(
        long,
        help = "Preview native retirement and incoming references without writing"
    )]
    pub(super) dry_run: bool,
    #[arg(
        long,
        conflicts_with = "dry_run",
        help = "Require the fingerprint from a reviewed deletion preview"
    )]
    pub(super) expected_preview: Option<String>,
}

impl RetirementOptions {
    pub(super) fn is_native(&self) -> bool {
        self.dry_run || self.expected_preview.is_some()
    }
}

#[derive(Debug, Subcommand)]
pub(super) enum OperationCommand {
    #[command(about = "Inspect pending durable writes without applying them")]
    Pending,
    #[command(about = "Finish interrupted writes when all recorded preconditions still hold")]
    Recover {
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Debug, Default, Args)]
pub(super) struct IssueOptions {
    #[arg(long, global = true, help = "Stable idempotency key for a mutation")]
    pub(super) request_id: Option<String>,
    #[arg(
        long,
        global = true,
        help = "Expected positive record revision; requires expected-content"
    )]
    pub(super) expected_revision: Option<String>,
    #[arg(
        long,
        global = true,
        help = "Expected SHA-256 content hash; requires expected-revision"
    )]
    pub(super) expected_content: Option<String>,
    #[arg(long, global = true, help = "Never prompt for missing input")]
    pub(super) no_input: bool,
    #[arg(
        long,
        global = true,
        help = "Stage only this operation's exact changed files and durable receipt"
    )]
    pub(super) stage: bool,
}

#[derive(Debug, Default, Args)]
pub(super) struct NativeOptions {
    #[command(flatten)]
    pub(super) mutation: IssueOptions,
    #[arg(long, global = true)]
    pub(super) json: bool,
}

pub(super) fn typed_input<T: DeserializeOwned>(cwd: &Path, path: &Path) -> workdeck_pm::Result<T> {
    serde_json::from_str(&read_input(cwd, path)?)
        .map_err(|error| invalid(format!("invalid JSON input: {error}")))
}

pub(super) fn retire_native_record(
    repository: &Repository,
    source: &Value,
    options: &NativeOptions,
    target: workdeck_pm::RetirementTarget,
    retirement: &RetirementOptions,
    yes: bool,
) -> workdeck_pm::Result<()> {
    if retirement.dry_run {
        read_options(&options.mutation)?;
        return emit(
            options.json,
            "retirement_preview",
            source,
            &repository.retirement_preview(&target)?,
            None,
        );
    }
    if !yes {
        return Err(invalid(
            "delete requires --yes; inspect incoming references with --dry-run",
        ));
    }
    let input = workdeck_pm::RetirementInput {
        target,
        expected: expected(&options.mutation)?,
        expected_preview: retirement
            .expected_preview
            .as_deref()
            .map(str::parse)
            .transpose()?,
    };
    let request = options
        .mutation
        .request_id
        .as_deref()
        .map(str::parse)
        .transpose()?
        .unwrap_or_else(workdeck_pm::RequestId::new);
    let receipt = repository.retire_record(&input, &request)?;
    emit_mutation(
        repository,
        &options.mutation,
        options.json,
        "record_retirement",
        source,
        &receipt,
    )
}

#[derive(Debug, Default, Args)]
pub(super) struct AuthoringOptions {
    #[arg(
        long,
        value_name = "PATH",
        help = "Read Markdown body from a file, or '-' for stdin"
    )]
    body_file: Option<PathBuf>,
    #[arg(long)]
    reporter: Option<String>,
    #[arg(long)]
    reviewer: Option<String>,
}

impl AuthoringOptions {
    fn is_native(&self) -> bool {
        self.body_file.is_some() || self.reporter.is_some() || self.reviewer.is_some()
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{error}")]
pub(super) struct CommandFailure {
    pub error: PmError,
    pub(super) source_identity: Value,
}

impl CommandFailure {
    pub fn print(&self, wants_json: bool) {
        let value = super::pm_diagnostics::render(&self.error, &self.source_identity);
        if wants_json {
            let _ = write_json(&value);
        } else {
            let error: PmError = serde_json::from_value(value["error"].clone())
                .expect("bounded diagnostics preserve the error schema");
            eprintln!(
                "workdeck: {}",
                workdeck_diff::sanitize_terminal_line(&error.message)
            );
            let mut location = error
                .path
                .as_deref()
                .map(workdeck_diff::format_terminal_path)
                .unwrap_or_default();
            if let Some(line) = error.line {
                location = if location.is_empty() {
                    format!("line {line}")
                } else {
                    format!("{location}:{line}")
                };
            }
            if let Some(column) = error.column {
                location = if location.is_empty() {
                    format!("column {column}")
                } else if error.line.is_some() {
                    format!("{location}:{column}")
                } else {
                    format!("{location} (column {column})")
                };
            }
            if !location.is_empty() {
                eprintln!("  at {location}");
            }
            if let Some(hint) = &error.hint {
                eprintln!("{}", workdeck_diff::sanitize_terminal_line(hint));
            }
        }
    }
}

/// None admits only read-only compatibility commands on a legacy source.
/// Native mutations and explicit restoration retain their dedicated adapters.
pub(super) fn try_run(cwd: &Path, command: Option<&Command>) -> Option<anyhow::Result<()>> {
    let command = command?;
    if let Command::Ci { options, command } = command {
        return Some(super::pm_ci::run(cwd, options, command));
    }
    if let Command::Hooks { options, command } = command {
        return Some(
            super::pm_hooks::run(cwd, options, command).map_err(|error| {
                CommandFailure {
                    error,
                    source_identity: json!({"repository":null,"root":cwd,"scope":"local_git_hook"}),
                }
                .into()
            }),
        );
    }
    if let Command::Doctor { json, options } = command
        && options.staged
    {
        return Some(super::pm_doctor::run(cwd, options, *json));
    }
    if let Command::Migrate {
        command: MigrateCommand::Legacy { options },
    } = command
    {
        let source_identity = json!({"repository":null,"root":source_root(cwd)});
        return Some(super::pm_migration::run(cwd, options).map_err(|error| {
            CommandFailure {
                error,
                source_identity,
            }
            .into()
        }));
    }
    if let Command::Protocol { options, command } = command {
        return Some(
            super::pm_protocol::run(cwd, options, command).map_err(|error| {
                CommandFailure {
                    error,
                    source_identity: json!({"repository":null,"root":source_root(cwd)}),
                }
                .into()
            }),
        );
    }
    if let Command::Capabilities { json, .. } | Command::Schema { json, .. } = command {
        let source_identity = json!({"repository":null,"root":source_root(cwd)});
        let result = super::pm_catalog::run(cwd, command).and_then(|result| {
            let selected_source = if matches!(command, Command::Capabilities { .. })
                && result["source"]["repository"].is_string()
            {
                json!({"repository":result["source"]["repository"],"root":result["source"]["root"]})
            } else {
                source_identity.clone()
            };
            if let Command::Capabilities { output, .. } = command {
                return output.emit(*json, "capabilities", &selected_source, &result);
            }
            emit(
                *json,
                if matches!(command, Command::Capabilities { .. }) {
                    "capabilities"
                } else {
                    "schema"
                },
                &selected_source,
                &result,
                None,
            )
        });
        return Some(result.map_err(|error| {
            CommandFailure {
                error,
                source_identity,
            }
            .into()
        }));
    }
    if native_domain_command(command) {
        if !has_native_marker(cwd) {
            let native = source_root(cwd);
            let project = native.parent().unwrap_or(cwd);
            let config = match super::Config::load(project) {
                Ok(config) => config,
                Err(error) => return Some(Err(source_config_failure(error, &native))),
            };
            return Some(guard_legacy_consumer(
                project,
                &config.data_dir(project),
                command,
            ));
        }
        let mut source_identity = json!({"repository":null,"root":source_root(cwd)});
        let result = Repository::discover(cwd).and_then(|repository| {
            source_identity = json!({"repository":repository.identity(),"root":repository.root()});
            if let Command::Recipe { options, command } = command {
                return super::pm_checks::command(
                    cwd,
                    &repository,
                    &source_identity,
                    options,
                    command,
                );
            }
            if let Command::Claim { options, command } = command {
                return super::pm_claims::run(cwd, &repository, &source_identity, options, command);
            }
            if let Command::Index {
                options,
                source,
                command,
            } = command
            {
                return super::pm_index::run(
                    cwd,
                    &repository,
                    &source_identity,
                    options,
                    source,
                    command,
                );
            }
            if let Command::Repository { options, command } = command {
                return super::pm_registry::run(
                    cwd,
                    &repository,
                    &source_identity,
                    options,
                    command,
                );
            }
            if let Command::Source { options, command } = command {
                return super::pm_sources::run(
                    cwd,
                    &repository,
                    &source_identity,
                    options,
                    command,
                );
            }
            if let Command::Check { options, command } = command {
                return super::pm_checks::check(
                    cwd,
                    &repository,
                    &source_identity,
                    options,
                    command,
                );
            }
            match command {
                Command::Question { options, command } => super::pm_continuity::question(
                    cwd,
                    &repository,
                    &source_identity,
                    options,
                    command,
                ),
                Command::Handoff { options, command } => super::pm_continuity::handoff(
                    cwd,
                    &repository,
                    &source_identity,
                    options,
                    command,
                ),
                Command::Context { options } => {
                    super::pm_context::context(&repository, &source_identity, options)
                }
                Command::Next { options } => {
                    super::pm_context::next(&repository, &source_identity, options)
                }
                Command::Feature { options, command } => {
                    super::pm_feature::run(cwd, &repository, &source_identity, options, command)
                }
                Command::Gate { options, command } => {
                    super::pm_gate::run(cwd, &repository, &source_identity, options, command)
                }
                Command::Evidence { options, command } => {
                    super::pm_evidence::run(cwd, &repository, &source_identity, options, command)
                }
                _ => unreachable!(),
            }?;
            Ok(None)
        });
        return Some(
            result
                .map_err(|error| -> anyhow::Error {
                    CommandFailure {
                        error,
                        source_identity,
                    }
                    .into()
                })
                .and_then(|exit| match exit {
                    Some(code) => Err(super::CommandExit(i32::from(code)).into()),
                    None => Ok(()),
                }),
        );
    }
    if let Command::Time { options, command } = command {
        let mut source_identity = json!({"repository":null,"root":source_root(cwd)});
        let result = Repository::discover(cwd).and_then(|repository| {
            source_identity = json!({"repository":repository.identity(),"root":repository.root()});
            super::pm_time::run(&repository, &source_identity, options, command)
        });
        return Some(result.map_err(|error| {
            CommandFailure {
                error,
                source_identity,
            }
            .into()
        }));
    }
    if let Command::View { options, command } = command {
        let mut source_identity = json!({"repository":null,"root":source_root(cwd)});
        let result = Repository::discover(cwd).and_then(|repository| {
            source_identity = json!({"repository":repository.identity(),"root":repository.root()});
            super::pm_views::run(cwd, &repository, &source_identity, options, command)
        });
        return Some(result.map_err(|error| {
            CommandFailure {
                error,
                source_identity,
            }
            .into()
        }));
    }
    if let Command::Wiki { options, command } = command {
        let mut source_identity = json!({"repository":null,"root":source_root(cwd)});
        let result = Repository::discover(cwd).and_then(|repository| {
            source_identity = json!({"repository":repository.identity(),"root":repository.root()});
            super::pm_wiki::run(cwd, &repository, &source_identity, options, command)
        });
        return Some(result.map_err(|error| {
            CommandFailure {
                error,
                source_identity,
            }
            .into()
        }));
    }
    if (planning_mutation(command)
        || super::pm_reference::membership_requested(command)
        || matches!(command, Command::User { .. } | Command::Organization { .. }))
        && !has_native_marker(cwd)
        && !matches!(command, Command::Import { options, .. } if options.restores())
    {
        let native = source_root(cwd);
        let project = native.parent().unwrap_or(cwd);
        let config = match super::Config::load(project) {
            Ok(config) => config,
            Err(error) => return Some(Err(source_config_failure(error, &native))),
        };
        return Some(guard_legacy_consumer(
            project,
            &config.data_dir(project),
            command,
        ));
    }
    if matches!(command, Command::User { .. } | Command::Organization { .. }) {
        let mut source_identity = json!({"repository":null,"root":source_root(cwd)});
        let result = Repository::discover(cwd).and_then(|repository| {
            source_identity = json!({"repository":repository.identity(),"root":repository.root()});
            match command {
                Command::User { options, command } => super::pm_organization::run_user(
                    cwd,
                    &repository,
                    &source_identity,
                    options,
                    command,
                ),
                Command::Organization { options, command } => {
                    super::pm_organization::run_organization(
                        cwd,
                        &repository,
                        &source_identity,
                        options,
                        command,
                    )
                }
                _ => unreachable!(),
            }
        });
        return Some(result.map_err(|error| {
            CommandFailure {
                error,
                source_identity,
            }
            .into()
        }));
    }
    if let Command::Search {
        query,
        target,
        json,
    } = command
        && has_native_marker(cwd)
    {
        let mut source_identity = json!({"repository":null,"root":source_root(cwd)});
        let result = Repository::discover(cwd).and_then(|repository| {
            source_identity = json!({"repository":repository.identity(),"root":repository.root()});
            super::pm_read::search(&repository, query, target, *json)
        });
        return Some(result.map_err(|error| {
            CommandFailure {
                error,
                source_identity,
            }
            .into()
        }));
    }
    if (matches!(command, Command::Agent { .. } | Command::Events { .. }) && has_native_marker(cwd))
        || matches!(command, Command::Agent { options, .. } if options.is_native())
    {
        let mut source_identity = json!({"repository":null,"root":source_root(cwd)});
        let result = Repository::discover(cwd).and_then(|repository| {
            source_identity = json!({"repository":repository.identity(),"root":repository.root()});
            super::pm_history::run(cwd, &repository, &source_identity, command)
        });
        return Some(result.map_err(|error| {
            CommandFailure {
                error,
                source_identity,
            }
            .into()
        }));
    }
    if matches!(command, Command::Import { options, .. } if options.restores()) {
        let mut source_identity = json!({"repository":null,"root":source_root(cwd)});
        let result = super::pm_transfer::run_restore(cwd, &mut source_identity, command);
        return Some(result.map_err(|error| {
            CommandFailure {
                error,
                source_identity,
            }
            .into()
        }));
    }
    if (matches!(command, Command::Import { .. } | Command::Export { .. })
        && has_native_marker(cwd))
        || matches!(command, Command::Import { options, .. } if options.is_native())
    {
        let mut source_identity = json!({"repository":null,"root":source_root(cwd)});
        let result = Repository::discover(cwd).and_then(|repository| {
            source_identity = json!({"repository":repository.identity(),"root":repository.root()});
            super::pm_transfer::run(cwd, &repository, &source_identity, command)
        });
        return Some(result.map_err(|error| {
            CommandFailure {
                error,
                source_identity,
            }
            .into()
        }));
    }
    if legacy_only_consumer(command) && has_native_marker(cwd) {
        let native = source_root(cwd);
        let project = native.parent().unwrap_or(cwd);
        return Some(guard_legacy_consumer(project, &native, command));
    }
    if !matches!(
        command,
        Command::Init { .. }
            | Command::Issue { .. }
            | Command::Project { .. }
            | Command::Initiative { .. }
            | Command::Milestone { .. }
            | Command::Target { .. }
            | Command::Cycle { .. }
            | Command::Label { .. }
            | Command::Doctor { .. }
            | Command::Operation { .. }
    ) {
        return None;
    }
    let repository = match command {
        Command::Init { prefix, .. } => Repository::init(cwd, prefix),
        Command::Operation { source, .. } => {
            if let Some(source) = source {
                Repository::open_for_recovery(&cwd.join(source))
            } else {
                Repository::discover(cwd).or_else(|error| {
                    if error.code == ErrorCode::RecoveryRequired {
                        Repository::open_for_recovery(&source_root(cwd))
                    } else {
                        Err(error)
                    }
                })
            }
        }
        _ => Repository::discover(cwd),
    };
    let required = match command {
        Command::Init { .. } | Command::Operation { .. } => true,
        Command::Issue { options, command } => requires_native(options, command),
        Command::Project { .. }
        | Command::Cycle { .. }
        | Command::Label { .. }
        | Command::Initiative { .. }
        | Command::Milestone { .. }
        | Command::Target { .. } => super::pm_reference::requires_native(command),
        _ => false,
    };
    let repository = match repository {
        Ok(repository) => repository,
        Err(error) => {
            if !required
                && !has_native_marker(cwd)
                && matches!(
                    error.code,
                    ErrorCode::NotInitialized | ErrorCode::LegacyStore
                )
            {
                if matches!(command, Command::Doctor { .. }) {
                    return None;
                }
                let native = source_root(cwd);
                let project = native.parent().unwrap_or(cwd);
                let config = match super::Config::load(project) {
                    Ok(config) => config,
                    // This branch admits established read-only prototype commands.
                    Err(error) => return Some(Err(error)),
                };
                return match guard_legacy_consumer(project, &config.data_dir(project), command) {
                    Ok(()) => None,
                    Err(error) => Some(Err(error)),
                };
            }
            let source_identity = json!({"repository":null,"root":source_root(cwd)});
            return Some(Err(CommandFailure {
                error,
                source_identity,
            }
            .into()));
        }
    };
    let source_identity = json!({"repository":repository.identity(),"root":repository.root()});
    let result = (|| match command {
        Command::Operation { json, command, .. } => match command {
            OperationCommand::Pending | OperationCommand::Recover { dry_run: true } => emit(
                *json,
                "operation_pending",
                &source_identity,
                &repository.pending_operations()?,
                None,
            ),
            OperationCommand::Recover { dry_run: false } => emit(
                *json,
                "operation_recover",
                &source_identity,
                &repository.recover_operations()?,
                None,
            ),
        },
        Command::Init { json, .. } => {
            if *json {
                emit(true, "init", &source_identity, &repository.config()?, None)
            } else {
                writeln!(
                    std::io::stdout().lock(),
                    "initialized {}",
                    workdeck_diff::format_terminal_path(&repository.root().to_string_lossy())
                )
                .map_err(|error| PmError::io("stdout", error))
            }
        }
        Command::Doctor { json, .. } => {
            let report = repository.doctor()?;
            if !report.valid {
                return Err(PmError::new(
                    ErrorCode::InvalidSchema,
                    "project management validation failed",
                )
                .at(repository.root())
                .hint(serde_json::to_string(&report.errors).unwrap_or_default()));
            }
            emit(*json, "doctor", &source_identity, &report, None)
        }
        Command::Issue { options, command } => {
            run_issue(cwd, &repository, &source_identity, options, command)
        }
        Command::Project { .. }
        | Command::Cycle { .. }
        | Command::Label { .. }
        | Command::Initiative { .. }
        | Command::Milestone { .. }
        | Command::Target { .. } => {
            super::pm_reference::run(cwd, &repository, &source_identity, command)
        }
        _ => unreachable!(),
    })();
    Some(result.map_err(|error| {
        CommandFailure {
            error,
            source_identity,
        }
        .into()
    }))
}

pub(super) fn source_root(cwd: &Path) -> PathBuf {
    let start = cwd.canonicalize().unwrap_or_else(|_| cwd.to_owned());
    let start = if start.is_file() {
        start.parent().unwrap_or(&start).to_owned()
    } else {
        start
    };
    if let Some(root) = start.ancestors().find(|root| root.join(".git").exists()) {
        return root.join(".workdeck");
    }
    for root in start.ancestors() {
        if fs::symlink_metadata(root.join(".workdeck")).is_ok()
            || root.join(".agents/workdeck/issues").exists()
            || ["projects.toml", "cycles.toml", "labels.toml"]
                .iter()
                .any(|name| root.join(".agents/workdeck").join(name).exists())
        {
            return root.join(".workdeck");
        }
    }
    start.join(".workdeck")
}

fn has_native_marker(cwd: &Path) -> bool {
    let root = source_root(cwd);
    // App preferences alone predate PM. Authoritative PM directories still
    // identify the source when its configuration has been lost or damaged.
    [
        "config.yml",
        "migration.yml",
        "restore.yml",
        "schema.yml",
        "users.yml",
        "labels.yml",
        "issues",
        "comments",
        "operations",
        "tombstones",
        "imported-sessions",
        "imported-history",
        "imported-handoffs",
        "initiatives",
        "projects",
        "milestones",
        "targets",
        "cycles",
        "views",
        "features",
        "gates",
        "relations",
        "evidence",
        "questions",
        "wiki",
        "templates",
        "AGENT-PROTOCOL.md",
        "commands",
        "checks",
        "check-profiles",
        "claims",
        "coordination.yml",
        "runs",
    ]
    .iter()
    .any(|name| fs::symlink_metadata(root.join(name)).is_ok())
        || fs::symlink_metadata(root).is_ok_and(|metadata| !metadata.is_dir())
}

fn legacy_only_consumer(command: &Command) -> bool {
    matches!(
        command,
        Command::Import { .. }
            | Command::Export { .. }
            | Command::Agent { .. }
            | Command::Events { .. }
            | Command::Search { .. }
    )
}

fn source_config_failure(error: anyhow::Error, native: &Path) -> anyhow::Error {
    let code = if error.downcast_ref::<std::io::Error>().is_some() {
        ErrorCode::Io
    } else {
        ErrorCode::InvalidSchema
    };
    CommandFailure {
        error: PmError::new(code, format!("application configuration prevents planning source selection: {error}"))
            .hint("Inspect workdeck config validate and correct the selected application configuration before retrying."),
        source_identity: json!({"repository":null,"root":native}),
    }.into()
}

fn native_domain_command(command: &Command) -> bool {
    matches!(
        command,
        Command::Index { .. }
            | Command::Repository { .. }
            | Command::Claim { .. }
            | Command::Source { .. }
            | Command::Question { .. }
            | Command::Recipe { .. }
            | Command::Check { .. }
            | Command::Handoff { .. }
            | Command::Context { .. }
            | Command::Next { .. }
            | Command::Feature { .. }
            | Command::Gate { .. }
            | Command::Evidence { .. }
    )
}

fn planning_mutation(command: &Command) -> bool {
    match command {
        Command::Repository { command, .. } => command.is_mutation(),
        Command::Index { command, .. } => command.is_mutation(),
        Command::Claim { command, .. } => command.is_mutation(),
        Command::Source { command, .. } => command.is_mutation(),
        Command::Recipe { command, .. } => command.is_mutation(),
        Command::Check { command, .. } => command.is_mutation(),
        Command::Question { command, .. } => command.is_mutation(),
        Command::Handoff { command, .. } => command.is_mutation(),
        Command::Feature { command, .. } => command.is_mutation(),
        Command::Gate { command, .. } => command.is_mutation(),
        Command::Evidence { command, .. } => command.is_mutation(),
        Command::Issue {
            command: IssueCommand::Graph(command),
            ..
        } => command.is_mutation(),
        Command::User { command, .. } => super::pm_organization::user_is_mutation(command),
        Command::Organization { command, .. } => {
            super::pm_organization::organization_is_mutation(command)
        }
        Command::Initiative { command, .. }
        | Command::Milestone { command, .. }
        | Command::Target { command, .. } => command.is_mutation(),
        Command::Issue { command, .. } => !matches!(
            command,
            IssueCommand::List { .. }
                | IssueCommand::Show { .. }
                | IssueCommand::Templates { .. }
                | IssueCommand::Comments { .. }
                | IssueCommand::Attachments { .. }
        ),
        Command::Project { command, .. } => !matches!(
            command,
            super::ProjectCommand::List { .. }
                | super::ProjectCommand::Show { .. }
                | super::ProjectCommand::Assess { .. }
        ),
        Command::Cycle {
            command: super::CycleCommand::Carryover { input, .. },
            ..
        } => input.expected_preview.is_some(),
        Command::Cycle { command, .. } => !matches!(
            command,
            super::CycleCommand::List { .. } | super::CycleCommand::Show { .. }
        ),
        Command::Label { command, .. } => !matches!(
            command,
            super::LabelCommand::List { .. } | super::LabelCommand::Show { .. }
        ),
        Command::Agent { command, .. } => !matches!(
            command,
            super::AgentCommand::List { .. } | super::AgentCommand::Show { .. }
        ),
        Command::Import { dry_run, .. } => !dry_run,
        _ => false,
    }
}

/// Admission check for the remaining prototype consumers after app settings
/// resolve the actual data directory. It never initializes either source.
/// Native adapters must replace this guard when they implement these operations.
pub(super) fn guard_legacy_consumer(
    repo_root: &Path,
    store_root: &Path,
    command: &Command,
) -> anyhow::Result<()> {
    let issue_or_reference = matches!(
        command,
        Command::Index { .. }
            | Command::Repository { .. }
            | Command::Claim { .. }
            | Command::Source { .. }
            | Command::Issue { .. }
            | Command::Recipe { .. }
            | Command::Check { .. }
            | Command::Question { .. }
            | Command::Handoff { .. }
            | Command::Context { .. }
            | Command::Next { .. }
            | Command::Feature { .. }
            | Command::Gate { .. }
            | Command::Evidence { .. }
            | Command::User { .. }
            | Command::Organization { .. }
            | Command::Project { .. }
            | Command::Initiative { .. }
            | Command::Milestone { .. }
            | Command::Target { .. }
            | Command::Cycle { .. }
            | Command::Label { .. }
    );
    if !issue_or_reference && !legacy_only_consumer(command) {
        return Ok(());
    }
    let native = source_root(repo_root);
    let mut source_identity = json!({"repository":null,"root":native});
    let result = (|| -> workdeck_pm::Result<()> {
        if has_native_marker(repo_root) {
            // Preserve malformed, ambiguous, and recovery diagnostics before
            // reporting a command that has not gained a native adapter yet.
            let repository = Repository::discover(repo_root)?;
            source_identity = json!({"repository":repository.identity(),"root":repository.root()});
            return Err(unsupported_legacy_consumer(&native));
        }
        let native_path = normalized_path(&native);
        let selected = normalized_path(store_root);
        let requires_source = issue_or_reference
            || matches!(command, Command::Import { .. })
            || matches!(command, Command::Agent { command, .. } if !matches!(command, super::AgentCommand::List { .. } | super::AgentCommand::Show { .. }));
        if selected != native_path && native_path.starts_with(&selected) {
            return Err(PmError::new(
                ErrorCode::UnsafePath,
                "a legacy data directory cannot contain the repository's native authority",
            )
            .at(store_root));
        }
        if selected.starts_with(&native_path) {
            if selected != native_path {
                return Err(PmError::new(
                    ErrorCode::UnsafePath,
                    "a legacy data directory cannot be inside the native .workdeck authority",
                )
                .at(store_root));
            }
            return if requires_source {
                Err(not_initialized(&native))
            } else {
                Ok(())
            };
        }
        // Explicit custom roots may select legacy data, never reinterpret a
        // native or interrupted source as a collection of old TOML records.
        if [
            "config.yml",
            "migration.yml",
            "restore.yml",
            "schema.yml",
            "users.yml",
            "labels.yml",
            "operations",
            "tombstones",
            "initiatives",
            "projects",
            "milestones",
            "cycles",
            "targets",
            "features",
            "gates",
            "relations",
            "evidence",
            "questions",
            "wiki",
            "views",
        ]
        .iter()
        .any(|name| fs::symlink_metadata(store_root.join(name)).is_ok())
        {
            let repository = Repository::open_source(store_root)?;
            source_identity = json!({"repository":repository.identity(),"root":repository.root()});
            return Err(unsupported_legacy_consumer(store_root));
        }
        let legacy = actual_legacy_source(store_root)?;
        source_identity = json!({"repository":null,"root":store_root,"state":if legacy { "legacy" } else { "uninitialized" }});
        if legacy && planning_mutation(command) {
            Err(legacy_read_only(store_root))
        } else if legacy
            && (super::pm_reference::membership_requested(command)
                || native_domain_command(command)
                || matches!(command, Command::User { .. } | Command::Organization { .. }))
        {
            Err(legacy_native_view(store_root))
        } else if legacy || !requires_source {
            Ok(())
        } else {
            Err(not_initialized(&native))
        }
    })();
    result.map_err(|error| {
        CommandFailure {
            error,
            source_identity,
        }
        .into()
    })
}

fn legacy_read_only(root: &Path) -> PmError {
    PmError::new(ErrorCode::LegacyStore, "legacy project management is read-only; migrate before changing planning records")
        .at(root)
        .hint("Preview with workdeck migrate legacy --source <legacy-root> --plan-out migration-plan.json; review the plan, then run workdeck migrate legacy --apply --plan migration-plan.json --request-id <stable-id>. Use workdeck init for a fresh repository.")
}

pub(super) fn legacy_mutation_failure(root: &Path) -> anyhow::Error {
    CommandFailure {
        error: legacy_read_only(root),
        source_identity: json!({"repository":null,"root":root,"state":"legacy"}),
    }
    .into()
}

fn legacy_native_view(root: &Path) -> PmError {
    let mut error = legacy_read_only(root);
    error.message =
        "this command requires native project management; migrate the selected legacy source"
            .into();
    error
}

pub(super) fn legacy_native_view_failure(root: &Path) -> anyhow::Error {
    CommandFailure {
        error: legacy_native_view(root),
        source_identity: json!({"repository":null,"root":root,"state":"legacy"}),
    }
    .into()
}

fn unsupported_legacy_consumer(root: &Path) -> PmError {
    PmError::new(ErrorCode::Unsupported, "this command has no native project-management adapter; the legacy handler cannot access native authority")
        .at(root)
        .hint("Use supported native issue/project/cycle/label/search commands. Native import, export, agent records, and events require their dedicated adapters.")
}

fn not_initialized(root: &Path) -> PmError {
    PmError::new(ErrorCode::NotInitialized, "project management is not initialized and no existing legacy source was selected")
        .at(root)
        .hint("Run workdeck init explicitly, or configure an existing legacy data directory before migration.")
}

fn normalized_path(path: &Path) -> PathBuf {
    if let Ok(path) = path.canonicalize() {
        return path;
    }
    // Canonicalize the nearest existing parent too, so a missing path under a
    // symlink cannot hide its relationship to the reserved native directory.
    if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
        return normalized_path(parent).join(name);
    }
    path.to_path_buf()
}

fn actual_legacy_source(root: &Path) -> workdeck_pm::Result<bool> {
    match fs::symlink_metadata(root) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => {
            return Err(PmError::new(
                ErrorCode::UnsafePath,
                "legacy source must be a regular directory",
            )
            .at(root));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(PmError::io(root, error)),
    }
    let mut found = false;
    for (name, directory) in [
        ("issues", true),
        ("agents", true),
        ("projects.toml", false),
        ("cycles.toml", false),
        ("labels.toml", false),
        ("events.jsonl", false),
    ] {
        let path = root.join(name);
        match fs::symlink_metadata(&path) {
            Ok(metadata)
                if (directory && metadata.is_dir()) || (!directory && metadata.is_file()) =>
            {
                found = true;
                if name == "issues" {
                    for entry in fs::read_dir(&path).map_err(|error| PmError::io(&path, error))? {
                        let entry = entry.map_err(|error| PmError::io(&path, error))?;
                        if !entry
                            .file_type()
                            .map_err(|error| PmError::io(entry.path(), error))?
                            .is_file()
                        {
                            return Err(PmError::new(ErrorCode::AmbiguousSource, "legacy issue directories cannot contain native item directories or redirected records").at(entry.path()));
                        }
                    }
                }
            }
            Ok(_) => {
                return Err(PmError::new(
                    ErrorCode::UnsafePath,
                    "legacy source marker has an unexpected file type",
                )
                .at(path));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(PmError::io(path, error)),
        }
    }
    Ok(found)
}

fn requires_native(options: &IssueOptions, command: &IssueCommand) -> bool {
    options.no_input
        || options.stage
        || options.request_id.is_some()
        || options.expected_revision.is_some()
        || options.expected_content.is_some()
        || match command {
            IssueCommand::Next { .. } | IssueCommand::Graph(_) => true,
            IssueCommand::List { query_options, .. } => query_options.is_native(),
            IssueCommand::Done { .. }
            | IssueCommand::LinkDocument { .. }
            | IssueCommand::UnlinkDocument { .. }
            | IssueCommand::Cancel { .. }
            | IssueCommand::Templates { .. }
            | IssueCommand::Edit { .. }
            | IssueCommand::Comment { .. }
            | IssueCommand::Comments { .. }
            | IssueCommand::Attach { .. }
            | IssueCommand::Attachments { .. }
            | IssueCommand::Archive { .. } => true,
            IssueCommand::Custom { .. } | IssueCommand::Estimate { .. } => true,
            IssueCommand::Create {
                authoring,
                associations,
                template,
                ..
            } => authoring.is_native() || associations.is_native() || template.is_some(),
            IssueCommand::Update {
                authoring,
                associations,
                from_json,
                ..
            } => authoring.is_native() || associations.is_native() || from_json.is_some(),
            IssueCommand::Delete { retirement, .. } => retirement.is_native(),
            _ => false,
        }
}

pub(super) fn invalid(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::InvalidInput, message)
}

pub(super) fn expected(options: &IssueOptions) -> workdeck_pm::Result<Option<SourceToken>> {
    match (&options.expected_revision, &options.expected_content) {
        (None, None) => Ok(None),
        (Some(revision), Some(content)) => Ok(Some(SourceToken {
            revision: Revision::new(
                revision
                    .parse()
                    .map_err(|_| invalid("expected-revision must be a positive integer"))?,
            )?,
            content: content.parse()?,
        })),
        _ => Err(invalid(
            "expected-revision and expected-content must be provided together",
        )),
    }
}

pub(super) fn read_options(options: &IssueOptions) -> workdeck_pm::Result<()> {
    if options.stage || options.request_id.is_some() || expected(options)?.is_some() {
        return Err(invalid(
            "stage, request-id and expected source flags apply only to mutations",
        ));
    }
    Ok(())
}

fn run_issue(
    cwd: &Path,
    repository: &Repository,
    source: &Value,
    options: &IssueOptions,
    command: &IssueCommand,
) -> workdeck_pm::Result<()> {
    if let IssueCommand::Next { options: next } = command {
        return super::pm_context::next_issue(repository, source, options, next);
    }
    if let IssueCommand::Graph(command) = command {
        return super::pm_graph::run(repository, source, options, command);
    }
    let json_output = command.wants_json();
    let expected = expected(options)?;
    if let IssueCommand::Done {
        key,
        dry_run,
        verification_file: Some(path),
        ..
    } = command
    {
        enum CompletionInput {
            RedGreen(workdeck_pm::CompleteRedGreenIssue),
            Verified(workdeck_pm::CompleteVerifiedIssue),
        }
        let document = read_input(cwd, path)?;
        let input = match serde_json::from_str::<workdeck_pm::CompleteRedGreenIssue>(&document) {
            Ok(input) => CompletionInput::RedGreen(input),
            Err(legacy_error) => serde_json::from_str::<workdeck_pm::CompleteVerifiedIssue>(
                &document,
            )
            .map(CompletionInput::Verified)
            .map_err(|error| {
                invalid(format!(
                    "invalid completion input (red/green: {legacy_error}; verified: {error})"
                ))
            })?,
        };
        let (issue_id, issue_source) = match &input {
            CompletionInput::RedGreen(input) => (&input.issue, &input.expected_issue),
            CompletionInput::Verified(input) => (&input.issue, &input.expected_issue),
        };
        if repository.show_issue(key)?.metadata.id != *issue_id
            || expected.as_ref().is_some_and(|pin| pin != issue_source)
        {
            return Err(invalid(
                "completion file must match the selected issue and any command-line source pin",
            ));
        }
        if *dry_run {
            read_options(options)?;
            return match input {
                CompletionInput::RedGreen(input) => emit(
                    json_output,
                    "issue_completion",
                    source,
                    &repository.red_green_completion_report(&input)?,
                    None,
                ),
                CompletionInput::Verified(input) => emit(
                    json_output,
                    "issue_completion",
                    source,
                    &repository.verified_completion_report(&input)?,
                    None,
                ),
            };
        }
        let request = options
            .request_id
            .as_deref()
            .map(str::parse)
            .transpose()?
            .unwrap_or_else(RequestId::new);
        let receipt = match input {
            CompletionInput::RedGreen(input) => {
                repository.complete_red_green_issue(&input, &request)?
            }
            CompletionInput::Verified(input) => {
                repository.complete_verified_issue(&input, &request)?
            }
        };
        return emit_mutation(
            repository,
            options,
            json_output,
            "issue_completion_verified",
            source,
            &receipt,
        );
    }
    match command {
        IssueCommand::List {
            status,
            priority,
            project,
            cycle,
            label,
            assignee,
            due_at,
            query_options,
            ..
        } => {
            read_options(options)?;
            let mut query = query_options.query()?;
            query.status = status.clone();
            query.priority = priority
                .as_ref()
                .map(|value| workdeck_pm::Priority::parse_input(value))
                .transpose()?;
            query.project = project.clone();
            query.cycle = cycle.clone();
            query.label = label.clone();
            query.assignee = assignee.clone();
            query.due_at = due_at.clone();
            let issues = repository.query_issues(&query)?;
            return emit(json_output, "issue_list", source, &issues, None);
        }
        IssueCommand::Templates { .. } => {
            read_options(options)?;
            return emit(
                json_output,
                "issue_templates",
                source,
                &repository.list_issue_templates()?,
                None,
            );
        }
        IssueCommand::Show { key, .. } => {
            read_options(options)?;
            return emit(
                json_output,
                "issue_show",
                source,
                &repository.show_issue(key)?,
                None,
            );
        }
        IssueCommand::Attachments { key, .. } => {
            read_options(options)?;
            return emit(
                json_output,
                "issue_attachments",
                source,
                &repository.list_attachments(key)?,
                None,
            );
        }
        IssueCommand::Comments { key, .. } => {
            read_options(options)?;
            return emit(
                json_output,
                "issue_comments",
                source,
                &repository.comments(key)?,
                None,
            );
        }
        IssueCommand::Done {
            key,
            dry_run: true,
            manual_actor,
            manual_reason,
            ..
        } => {
            read_options(options)?;
            if manual_actor.is_some() || manual_reason.is_some() {
                return Err(invalid(
                    "manual acceptance is a mutation and cannot be recorded by dry-run",
                ));
            }
            return emit(
                json_output,
                "issue_completion",
                source,
                &repository.completion_report(key)?,
                None,
            );
        }
        IssueCommand::Edit { key, from_file, .. } => {
            let receipt = edit_issue(
                cwd,
                repository,
                options,
                key,
                from_file.as_deref(),
                expected.as_ref(),
            )?;
            return emit_mutation(
                repository,
                options,
                json_output,
                "issue_edit",
                source,
                &receipt,
            );
        }
        IssueCommand::Delete {
            key, retirement, ..
        } if retirement.dry_run => {
            read_options(options)?;
            return emit(
                json_output,
                "issue_retirement_preview",
                source,
                &repository.retirement_preview_issue(key)?,
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
    let (action, receipt) = match command {
        IssueCommand::Delete {
            key,
            yes,
            retirement,
            ..
        } => {
            if !yes {
                return Err(invalid(
                    "delete requires --yes; inspect incoming references with --dry-run",
                ));
            }
            let expected_preview = retirement
                .expected_preview
                .as_deref()
                .map(str::parse)
                .transpose()?;
            (
                "issue_delete",
                repository.retire_issue(
                    key,
                    expected.as_ref(),
                    expected_preview.as_ref(),
                    &request,
                )?,
            )
        }
        IssueCommand::Attach {
            key,
            path,
            author,
            name,
            media_type,
            ..
        } => {
            let name = name
                .clone()
                .or_else(|| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .map(str::to_owned)
                })
                .ok_or_else(|| invalid("attachment requires a UTF-8 filename or explicit name"))?;
            let input = AttachmentInput {
                name,
                content: read_regular_input(cwd, path, MAX_ATTACHMENT_BYTES as u64)?,
                actor: author.clone(),
                media_type: media_type.clone(),
            };
            (
                "issue_attach",
                repository.attach_issue(key, expected.as_ref(), &input, &request)?,
            )
        }
        IssueCommand::Create {
            title,
            template,
            from_json,
            description,
            status,
            priority,
            project,
            cycle,
            assignee,
            due_at,
            label,
            linked_commit,
            linked_file,
            authoring,
            associations,
            ..
        } => {
            if expected.is_some() {
                return Err(invalid(
                    "create does not accept an existing issue source token",
                ));
            }
            let (mut input, mut body_provided) = if let Some(path) = from_json {
                create_from_json(read_json(cwd, path)?)?
            } else {
                (CreateIssue::new("", ""), false)
            };
            if let Some(title) = title {
                input.title.clone_from(title);
            }
            if input.title.trim().is_empty() {
                return Err(invalid("a title or a from-json title is required"));
            }
            if let Some(body) = authoring_body(cwd, description, authoring)? {
                input.body = body;
                body_provided = true;
            }
            apply_options(
                &mut input.fields,
                [
                    ("status", status),
                    ("priority", priority),
                    ("project", project),
                    ("cycle", cycle),
                    ("assignee", assignee),
                    ("due_at", due_at),
                    ("reporter", &authoring.reporter),
                    ("reviewer", &authoring.reviewer),
                ],
            );
            associations.apply(&mut input.fields, project, cycle)?;
            if !label.is_empty() {
                input.fields.insert("labels".into(), json!(label));
            }
            if !linked_commit.is_empty() {
                input.fields.insert("commits".into(), json!(linked_commit));
            }
            if !linked_file.is_empty() {
                input.fields.insert(
                    "files".into(),
                    json!(
                        linked_file
                            .iter()
                            .map(|path| json!({"path":path}))
                            .collect::<Vec<_>>()
                    ),
                );
            }
            normalize_priority(&mut input.fields)?;
            let receipt = if let Some(template) = template {
                repository.create_issue_from_template(
                    &TemplateIssueInput {
                        template: template.clone(),
                        title: input.title,
                        body: body_provided.then_some(input.body),
                        fields: input.fields,
                    },
                    &request,
                )?
            } else {
                repository.create_issue(&input, &request)?
            };
            ("issue_create", receipt)
        }
        _ => {
            let (key, mutation, action) = mutation(cwd, command)?;
            (
                action,
                repository.mutate_issue(key, expected.as_ref(), &mutation, &request)?,
            )
        }
    };
    emit_mutation(repository, options, json_output, action, source, &receipt)
}

/// File mutation and Git staging are separate outcomes. Never report a staging
/// failure as an absent mutation or hide the receipt needed for a safe retry.
pub(super) fn emit_mutation(
    repository: &Repository,
    options: &IssueOptions,
    json_output: bool,
    kind: &str,
    source: &Value,
    receipt: &MutationReceipt,
) -> workdeck_pm::Result<()> {
    if !options.stage {
        return emit(json_output, kind, source, &receipt.result, Some(receipt));
    }
    let staging = stage_receipt(repository, receipt)?;
    if json_output {
        write_json(
            &json!({"api_version":1,"ok":true,"kind":kind,"source":source,
            "result":receipt.result,"receipt":receipt,"staging":staging}),
        )
    } else {
        emit(false, kind, source, &receipt.result, Some(receipt))?;
        writeln!(
            std::io::stdout().lock(),
            "staged {} operation paths",
            staging.paths.len()
        )
        .map_err(|error| PmError::io("stdout", error))
    }
}

/// Preserve committed planning receipt details when the separate Git staging step fails.
pub(super) fn stage_receipt(
    repository: &Repository,
    receipt: &MutationReceipt,
) -> workdeck_pm::Result<workdeck_pm::StagingReport> {
    repository.stage_operation(receipt).map_err(|mut error| {
        let hint = format!("Mutation {} is committed to planning files; staging failed. Preserve its receipt and retry the same request {} after resolving the staging error.", receipt.operation_id, receipt.request_id);
        error.hint = Some(match error.hint.take() {
            Some(previous) => format!("{hint}\n{previous}"),
            None => hint,
        });
        error.details(json!({"mutation_committed":true,"receipt":receipt,"staging":{"state":"failed"}}))
    })
}

/// Interactive edits have a newly generated request because their complete
/// payload does not exist until the editor exits. Deterministic retries use
/// --from-file, preserving both the supplied payload and expected source token.
fn edit_issue(
    cwd: &Path,
    repository: &Repository,
    options: &IssueOptions,
    key: &str,
    from_file: Option<&Path>,
    expected: Option<&SourceToken>,
) -> workdeck_pm::Result<MutationReceipt> {
    if let Some(path) = from_file {
        let request = options
            .request_id
            .as_deref()
            .map(str::parse)
            .transpose()?
            .unwrap_or_else(RequestId::new);
        let result = (|| {
            let markdown = read_markdown_file(cwd, path)?;
            repository.mutate_issue(
                key,
                expected,
                &IssueMutation::EditDocument { markdown },
                &request,
            )
        })();
        return result.map_err(|error| retain_draft_hint(error, &cwd.join(path)));
    }
    if options.no_input {
        return Err(invalid("issue edit --no-input requires --from-file")
            .hint("Supply a complete Markdown draft with --from-file PATH."));
    }
    if options.request_id.is_some() {
        return Err(
            invalid("interactive editing does not accept a caller-supplied request-id")
                .hint("Use issue edit --from-file PATH --request-id ID for deterministic retries."),
        );
    }
    let editor = editor_argv()?;
    let original = repository.show_issue(key)?;
    if expected.is_some_and(|expected| expected != &original.source) {
        return Err(PmError::new(
            ErrorCode::StaleSource,
            "issue changed before the editor was opened",
        )
        .at(&original.path));
    }
    let markdown = repository.issue_markdown(key)?;
    // The public record and raw document reads use separate snapshots. Verify
    // their content identity before handing a draft to an external process.
    if workdeck_pm::ContentHash::of(markdown.as_bytes()) != original.source.content {
        return Err(PmError::new(
            ErrorCode::StaleSource,
            "issue changed while preparing the editor draft",
        )
        .at(&original.path));
    }
    let directory = tempfile::Builder::new()
        .prefix("workdeck-edit-")
        .tempdir()
        .map_err(|error| PmError::io(std::env::temp_dir(), error))?;
    let draft_path = directory.path().join("issue.md");
    let result = (|| {
        fs::write(&draft_path, markdown).map_err(|error| PmError::io(&draft_path, error))?;
        let status = std::process::Command::new(&editor[0])
            .args(&editor[1..])
            .arg(&draft_path)
            .current_dir(cwd)
            .stdin(std::process::Stdio::inherit())
            // Duplicating stderr keeps a terminal descriptor for interactive
            // editors while reserving stdout for the command's JSON response.
            .stdout(std::io::stderr())
            .stderr(std::process::Stdio::inherit())
            .status()
            .map_err(|error| PmError::io(&draft_path, error))?;
        if !status.success() {
            return Err(PmError::new(
                ErrorCode::Canceled,
                format!("editor exited with {status}; issue was not changed"),
            ));
        }
        let markdown = read_markdown_file(cwd, &draft_path)?;
        let request = RequestId::new();
        repository.mutate_issue(
            key,
            Some(&original.source),
            &IssueMutation::EditDocument { markdown },
            &request,
        )
    })();
    match result {
        Ok(receipt) => Ok(receipt),
        Err(error) => {
            let retained_path = directory.keep().join("issue.md");
            Err(retain_draft_hint(error, &retained_path))
        }
    }
}

fn editor_argv() -> workdeck_pm::Result<Vec<String>> {
    for variable in ["EDITOR", "VISUAL"] {
        let value = match std::env::var(variable) {
            Ok(value) if !value.trim().is_empty() => value,
            Ok(_) | Err(std::env::VarError::NotPresent) => continue,
            Err(_) => return Err(invalid(format!("{variable} must be UTF-8"))),
        };
        let arguments = shell_words::split(&value)
            .map_err(|error| invalid(format!("invalid {variable} command: {error}")))?;
        if arguments.first().is_some_and(|program| !program.is_empty()) {
            return Ok(arguments);
        }
        return Err(invalid(format!(
            "{variable} must name an editor executable"
        )));
    }
    Err(invalid("no editor is configured")
        .hint("Set EDITOR or VISUAL, or supply --from-file PATH."))
}

fn read_markdown_file(cwd: &Path, path: &Path) -> workdeck_pm::Result<String> {
    String::from_utf8(read_regular_input(cwd, path, MAX_INPUT_BYTES)?)
        .map_err(|_| invalid("Markdown draft must be UTF-8").at(cwd.join(path)))
}

fn retain_draft_hint(mut error: PmError, path: &Path) -> PmError {
    if fs::symlink_metadata(path).is_err() {
        return error;
    }
    let path = path.canonicalize().unwrap_or_else(|_| path.to_owned());
    let recovery = format!("Draft retained at: {}", path.display());
    error.hint = Some(match error.hint {
        Some(hint) => format!("{hint}\n{recovery}"),
        None => recovery,
    });
    error
}

fn mutation<'a>(
    cwd: &Path,
    command: &'a IssueCommand,
) -> workdeck_pm::Result<(&'a str, IssueMutation, &'static str)> {
    use IssueCollection::{Commits, Documents, Files, Labels};
    let result = match command {
        IssueCommand::Custom { key, input, .. } => (
            key.as_str(),
            IssueMutation::PatchCustom {
                patch: input.patch()?,
            },
            "issue_custom",
        ),
        IssueCommand::Estimate {
            key,
            value,
            unit,
            clear,
            ..
        } => {
            let value = if *clear {
                Value::Null
            } else {
                let estimate = workdeck_pm::Estimate {
                    value: value
                        .as_deref()
                        .ok_or_else(|| invalid("estimate value is required"))?
                        .parse()?,
                    unit: unit
                        .clone()
                        .ok_or_else(|| invalid("estimate unit is required"))?,
                };
                estimate.validate()?;
                json!(estimate)
            };
            (
                key.as_str(),
                IssueMutation::Update {
                    input: UpdateIssue {
                        fields: BTreeMap::from([("estimate".into(), value)]),
                        body: None,
                    },
                },
                "issue_estimate",
            )
        }
        IssueCommand::Update {
            key,
            title,
            description,
            status,
            priority,
            project,
            cycle,
            assignee,
            due_at,
            label,
            linked_commit,
            authoring,
            associations,
            from_json,
            ..
        } => {
            let mut input = if let Some(path) = from_json {
                update_from_json(read_json(cwd, path)?)?
            } else {
                UpdateIssue::default()
            };
            apply_options(
                &mut input.fields,
                [
                    ("title", title),
                    ("status", status),
                    ("priority", priority),
                    ("project", project),
                    ("cycle", cycle),
                    ("assignee", assignee),
                    ("due_at", due_at),
                    ("reporter", &authoring.reporter),
                    ("reviewer", &authoring.reviewer),
                ],
            );
            associations.apply(&mut input.fields, project, cycle)?;
            if let Some(body) = authoring_body(cwd, description, authoring)? {
                input.body = Some(body);
            }
            if !label.is_empty() {
                input.fields.insert("labels".into(), json!(label));
            }
            normalize_priority(&mut input.fields)?;
            let mutation = if linked_commit.is_empty() {
                IssueMutation::Update { input }
            } else {
                IssueMutation::UpdateAndAdd {
                    input,
                    field: Commits,
                    values: linked_commit.iter().map(|sha| json!(sha)).collect(),
                }
            };
            (key.as_str(), mutation, "issue_update")
        }
        IssueCommand::Link { key, path, .. } | IssueCommand::LinkFile { key, path, .. } => (
            key.as_str(),
            IssueMutation::Add {
                field: Files,
                value: json!({"path":path}),
            },
            "issue_link_file",
        ),
        IssueCommand::LinkDocument { key, reference, .. } => (
            key.as_str(),
            IssueMutation::Add {
                field: Documents,
                value: json!(reference),
            },
            "issue_link_document",
        ),
        IssueCommand::UnlinkDocument { key, reference, .. } => (
            key.as_str(),
            IssueMutation::Remove {
                field: Documents,
                value: json!(reference),
            },
            "issue_unlink_document",
        ),
        IssueCommand::Cancel { key, .. } => (key.as_str(), IssueMutation::Cancel, "issue_cancel"),
        IssueCommand::UnlinkFile { key, path, .. } => (
            key.as_str(),
            IssueMutation::Remove {
                field: Files,
                value: json!({"path":path}),
            },
            "issue_unlink_file",
        ),
        IssueCommand::LinkCommit { key, sha, .. } => (
            key.as_str(),
            IssueMutation::Add {
                field: Commits,
                value: json!(sha),
            },
            "issue_link_commit",
        ),
        IssueCommand::UnlinkCommit { key, sha, .. } => (
            key.as_str(),
            IssueMutation::Remove {
                field: Commits,
                value: json!(sha),
            },
            "issue_unlink_commit",
        ),
        IssueCommand::Close { key, .. } => (
            key.as_str(),
            IssueMutation::Complete { manual: None },
            "issue_done",
        ),
        IssueCommand::Done {
            key,
            manual_actor,
            manual_reason,
            ..
        } => {
            let manual = match (manual_actor, manual_reason) {
                (None, None) => None,
                (Some(actor), Some(reason)) => Some(ManualAcceptanceInput {
                    actor: actor.clone(),
                    reason: reason.clone(),
                }),
                _ => {
                    return Err(invalid(
                        "manual-actor and manual-reason must be provided together",
                    ));
                }
            };
            (
                key.as_str(),
                IssueMutation::Complete { manual },
                "issue_done",
            )
        }
        IssueCommand::Reopen { key, .. } => (key.as_str(), IssueMutation::Reopen, "issue_reopen"),
        IssueCommand::Archive { key, restore, .. } => (
            key.as_str(),
            IssueMutation::Archive { archived: !restore },
            "issue_archive",
        ),
        IssueCommand::Comment {
            key,
            body,
            body_file,
            author,
            ..
        } => {
            let body = match (body, body_file) {
                (Some(body), None) => body.clone(),
                (None, Some(path)) => read_input(cwd, path)?,
                _ => return Err(invalid("provide one comment body or body-file")),
            };
            (
                key.as_str(),
                IssueMutation::Comment {
                    author: author.clone(),
                    body,
                },
                "issue_comment",
            )
        }
        IssueCommand::Move { key, status, .. } => (
            key.as_str(),
            update_field("status", json!(status)),
            "issue_move",
        ),
        IssueCommand::Assign { key, assignee, .. } => (
            key.as_str(),
            update_field("assignee", json!(assignee)),
            "issue_assign",
        ),
        IssueCommand::Unassign { key, .. } => (
            key.as_str(),
            update_field("assignee", Value::Null),
            "issue_unassign",
        ),
        IssueCommand::Label {
            command: IssueLabelCommand::Add { key, label, .. },
        } => (
            key.as_str(),
            IssueMutation::Add {
                field: Labels,
                value: json!(label),
            },
            "issue_label_add",
        ),
        IssueCommand::Label {
            command: IssueLabelCommand::Remove { key, label, .. },
        } => (
            key.as_str(),
            IssueMutation::Remove {
                field: Labels,
                value: json!(label),
            },
            "issue_label_remove",
        ),
        _ => return Err(invalid("expected an issue mutation")),
    };
    Ok(result)
}

fn update_field(field: &str, value: Value) -> IssueMutation {
    IssueMutation::Update {
        input: UpdateIssue {
            fields: BTreeMap::from([(field.into(), value)]),
            body: None,
        },
    }
}

fn normalize_priority(fields: &mut BTreeMap<String, Value>) -> workdeck_pm::Result<()> {
    if let Some(Value::String(value)) = fields.get("priority") {
        fields.insert(
            "priority".into(),
            json!(workdeck_pm::Priority::parse_input(value)?),
        );
    }
    Ok(())
}

fn apply_options<const N: usize>(
    fields: &mut BTreeMap<String, Value>,
    options: [(&str, &Option<String>); N],
) {
    for (field, value) in options {
        if let Some(value) = value {
            fields.insert(field.into(), json!(value));
        }
    }
}

fn authoring_body(
    cwd: &Path,
    description: &Option<String>,
    authoring: &AuthoringOptions,
) -> workdeck_pm::Result<Option<String>> {
    match (description, &authoring.body_file) {
        (Some(_), Some(_)) => Err(invalid("description and body-file cannot be combined")),
        (_, Some(path)) => read_input(cwd, path).map(Some),
        (body, None) => Ok(body.clone()),
    }
}

pub(super) fn read_input(cwd: &Path, path: &Path) -> workdeck_pm::Result<String> {
    let bytes = if path == Path::new("-") {
        let mut bytes = Vec::new();
        std::io::stdin()
            .lock()
            .take(MAX_INPUT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| PmError::io(path, error))?;
        if bytes.len() as u64 > MAX_INPUT_BYTES {
            return Err(invalid("input exceeds 2 MiB").at(path));
        }
        bytes
    } else {
        read_regular_input(cwd, path, MAX_INPUT_BYTES)?
    };
    String::from_utf8(bytes).map_err(|_| invalid("input must be UTF-8").at(path))
}

pub(super) fn read_regular_input(
    cwd: &Path,
    path: &Path,
    limit: u64,
) -> workdeck_pm::Result<Vec<u8>> {
    let path = cwd.join(path);
    let metadata = fs::symlink_metadata(&path).map_err(|error| PmError::io(&path, error))?;
    if !metadata.is_file() {
        return Err(PmError::new(ErrorCode::UnsafePath, "input must be a regular file").at(&path));
    }
    if metadata.len() > limit {
        return Err(invalid(format!("input exceeds {limit} bytes")).at(&path));
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    // Prevent a concurrent replacement with a FIFO or symlink from blocking
    // before the opened descriptor can be checked as well.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW);
    }
    let file = options
        .open(&path)
        .map_err(|error| PmError::io(&path, error))?;
    if !file
        .metadata()
        .map_err(|error| PmError::io(&path, error))?
        .is_file()
    {
        return Err(PmError::new(ErrorCode::UnsafePath, "input must be a regular file").at(&path));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| PmError::io(&path, error))?;
    if bytes.len() as u64 > limit {
        return Err(invalid(format!("input exceeds {limit} bytes")).at(&path));
    }
    Ok(bytes)
}

fn read_json(cwd: &Path, path: &Path) -> workdeck_pm::Result<Value> {
    serde_json::from_str(&read_input(cwd, path)?).map_err(|error| {
        let mut diagnostic = invalid(error.to_string()).at(path);
        diagnostic.line = Some(error.line());
        diagnostic.column = Some(error.column());
        diagnostic
    })
}

fn decode<T: DeserializeOwned>(value: Value) -> workdeck_pm::Result<T> {
    serde_json::from_value(value).map_err(|error| invalid(error.to_string()))
}

fn create_from_json(mut value: Value) -> workdeck_pm::Result<(CreateIssue, bool)> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| invalid("issue input must be a JSON object"))?;
    let body_provided = object.contains_key("body") || object.contains_key("description");
    if object.contains_key("fields") {
        // The positional title may supply the required title after decoding.
        object.entry("title").or_insert_with(|| json!(""));
        return decode(value).map(|input| (input, body_provided));
    }
    let title = object.remove("title").unwrap_or(json!(""));
    let body = take_body(object)?;
    normalize_legacy_links(object)?;
    decode(json!({"title":title,"body":body.unwrap_or(json!("")),"fields":object}))
        .map(|input| (input, body_provided))
}

fn update_from_json(mut value: Value) -> workdeck_pm::Result<UpdateIssue> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| invalid("issue update must be a JSON object"))?;
    if object.contains_key("fields") {
        return decode(value);
    }
    let body = take_body(object)?;
    normalize_legacy_links(object)?;
    decode(json!({"body":body,"fields":object}))
}

fn take_body(object: &mut serde_json::Map<String, Value>) -> workdeck_pm::Result<Option<Value>> {
    if object.contains_key("body") && object.contains_key("description") {
        return Err(invalid("body and description cannot be combined"));
    }
    Ok(object
        .remove("body")
        .or_else(|| object.remove("description")))
}

fn normalize_legacy_links(object: &mut serde_json::Map<String, Value>) -> workdeck_pm::Result<()> {
    for (old, new) in [("linked_files", "files"), ("linked_commits", "commits")] {
        if let Some(mut value) = object.remove(old) {
            if object.contains_key(new) {
                return Err(invalid(format!("{old} and {new} cannot be combined")));
            }
            if old == "linked_files" {
                let files: Vec<String> = decode(value)?;
                value = json!(
                    files
                        .into_iter()
                        .map(|path| json!({"path":path}))
                        .collect::<Vec<_>>()
                );
            }
            object.insert(new.into(), value);
        }
    }
    Ok(())
}

pub(super) fn emit(
    json_output: bool,
    kind: &str,
    source: &Value,
    result: &impl Serialize,
    receipt: Option<&MutationReceipt>,
) -> workdeck_pm::Result<()> {
    let result = serde_json::to_value(result).map_err(|error| invalid(error.to_string()))?;
    if json_output {
        let mut value =
            json!({"api_version":1,"ok":true,"kind":kind,"source":source,"result":result});
        if let Some(receipt) = receipt {
            value["receipt"] = json!(receipt);
        }
        write_json(&value)
    } else {
        let mut stdout = std::io::stdout().lock();
        if let Some(issues) = result.as_array() {
            for issue in issues {
                writeln!(stdout, "{}", human_result(issue))
                    .map_err(|error| PmError::io("stdout", error))?;
            }
            if issues.is_empty() {
                writeln!(stdout, "no records").map_err(|error| PmError::io("stdout", error))?;
            }
        } else {
            writeln!(stdout, "{}", human_result(&result))
                .map_err(|error| PmError::io("stdout", error))?;
        }
        Ok(())
    }
}

fn human_result(value: &Value) -> String {
    let rendered = if let Some(metadata) = value.get("metadata") {
        format!(
            "{}  {}  {}{}",
            metadata["id"].as_str().unwrap_or(""),
            metadata["status"].as_str().unwrap_or(""),
            metadata["title"]
                .as_str()
                .or_else(|| metadata["name"].as_str())
                .unwrap_or(""),
            value["body"]
                .as_str()
                .filter(|body| !body.is_empty())
                .map(|body| format!("\n\n{body}"))
                .unwrap_or_default()
        )
    } else {
        serde_json::to_string_pretty(value).unwrap_or_default()
    };
    rendered
        .lines()
        .map(workdeck_diff::sanitize_terminal_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn write_json(value: &Value) -> workdeck_pm::Result<()> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, value).map_err(|error| PmError::io("stdout", error))?;
    writeln!(stdout).map_err(|error| PmError::io("stdout", error))
}
