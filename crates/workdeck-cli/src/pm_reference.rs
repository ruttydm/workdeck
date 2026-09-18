//! Native planning adapters. Compatibility saves remain one shared
//! operation, so retries do not reinterpret a previous create as a new update.
use super::{Command, CycleCommand, LabelCommand, ProjectCommand, pm_cli};
use clap::{Args, Subcommand};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use workdeck_pm::{
    ArchiveFilter, CreatePlanning, PlanningKind, PlanningMembershipQuery, PlanningMutation,
    PolicyAcceptance, Repository, RequestId, Result, RetirementInput, RetirementTarget,
    SavePlanning,
};

#[derive(Debug, Default, Args)]
pub(super) struct MembershipOptions {
    #[arg(
        long,
        help = "Show a snapshot of the record and its issue/planning membership (native PM)"
    )]
    members: bool,
    #[arg(long, requires = "members", value_parser = ["active", "all", "archived"], help = "Member issue archive scope; defaults to active")]
    archive: Option<String>,
}

impl MembershipOptions {
    fn is_native(&self) -> bool {
        self.members || self.archive.is_some()
    }

    fn query(&self, kind: PlanningKind, id: &str) -> Result<PlanningMembershipQuery> {
        let mut query = PlanningMembershipQuery::new(kind, id);
        query.issues.archive = match self.archive.as_deref() {
            None | Some("active") => ArchiveFilter::Active,
            Some("all") => ArchiveFilter::All,
            Some("archived") => ArchiveFilter::Archived,
            _ => {
                return Err(pm_cli::invalid(
                    "archive scope must be active, all or archived",
                ));
            }
        };
        Ok(query)
    }

    pub(super) fn reject_legacy(&self, source: &Path) -> anyhow::Result<()> {
        if self.is_native() {
            return Err(pm_cli::legacy_native_view_failure(source));
        }
        Ok(())
    }
}

pub(super) fn membership_requested(command: &Command) -> bool {
    match command {
        Command::Project {
            command: ProjectCommand::Show { membership, .. },
            ..
        }
        | Command::Cycle {
            command: CycleCommand::Show { membership, .. },
            ..
        }
        | Command::Label {
            command: LabelCommand::Show { membership, .. },
            ..
        }
        | Command::Initiative {
            command: HierarchyCommand::Show { membership, .. },
            ..
        }
        | Command::Milestone {
            command: HierarchyCommand::Show { membership, .. },
            ..
        }
        | Command::Target {
            command: HierarchyCommand::Show { membership, .. },
            ..
        } => membership.is_native(),
        _ => false,
    }
}

#[derive(Debug, Default, Args)]
pub(super) struct Fields {
    #[arg(long, conflicts_with = "body_file")]
    description: Option<String>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Read Markdown body from a file, or '-' for stdin"
    )]
    body_file: Option<PathBuf>,
    #[arg(long)]
    status: Option<String>,
    #[arg(long)]
    starts_at: Option<String>,
    #[arg(long)]
    ends_at: Option<String>,
    #[arg(long)]
    color: Option<String>,
    #[arg(long)]
    initiative: Option<String>,
    #[arg(long, help = "Owning project for a milestone")]
    project: Option<String>,
    #[arg(long)]
    lead: Option<String>,
    #[arg(long)]
    scope: Option<String>,
    #[arg(long)]
    goal: Option<String>,
    #[arg(
        long,
        value_name = "JSON_OBJECT",
        help = "Explicitly replace the complete custom object; use the custom subcommand for per-key updates"
    )]
    custom: Option<String>,
    #[arg(
        long = "target",
        help = "Repeat to replace project or milestone target membership"
    )]
    targets: Vec<String>,
    #[arg(
        long = "exit-criterion",
        value_name = "ID=DESCRIPTION",
        help = "Repeat to replace declared project exit criteria; declarations are not verification evidence"
    )]
    exit_criteria: Vec<String>,
    #[arg(
        long = "outcome",
        value_name = "ID=DESCRIPTION",
        help = "Repeat to replace declared initiative, milestone or target outcomes"
    )]
    outcomes: Vec<String>,
    #[arg(
        long,
        value_name = "FIELD",
        help = "Explicitly clear an optional field; cannot be combined with setting the same field"
    )]
    clear: Vec<String>,
}

impl Fields {
    fn body(&self, cwd: &Path) -> Result<Option<String>> {
        self.body_file
            .as_ref()
            .map(|path| pm_cli::read_input(cwd, path))
            .transpose()
            .map(|body| body.or_else(|| self.description.clone()))
    }
    fn metadata(&self) -> Result<BTreeMap<String, Value>> {
        let mut fields = fields(&[
            ("status", &self.status),
            ("starts_at", &self.starts_at),
            ("ends_at", &self.ends_at),
            ("color", &self.color),
            ("initiative", &self.initiative),
            ("project", &self.project),
            ("lead", &self.lead),
            ("scope", &self.scope),
            ("goal", &self.goal),
        ]);
        if let Some(custom) = &self.custom {
            let value: Value = serde_json::from_str(custom)
                .map_err(|error| pm_cli::invalid(format!("invalid custom JSON: {error}")))?;
            if !value.is_object() {
                return Err(pm_cli::invalid("--custom requires a JSON object"));
            }
            fields.insert("custom".into(), value);
        }
        if !self.targets.is_empty() {
            fields.insert("targets".into(), json!(self.targets));
        }
        for (name, values) in [
            ("exit_criteria", &self.exit_criteria),
            ("outcomes", &self.outcomes),
        ] {
            if !values.is_empty() {
                let criteria = values
                    .iter()
                    .map(|value| {
                        let (id, description) = value.split_once('=').ok_or_else(|| {
                            pm_cli::invalid(format!("{name} requires ID=DESCRIPTION"))
                        })?;
                        if id.trim().is_empty() || description.trim().is_empty() {
                            return Err(pm_cli::invalid(format!(
                                "{name} requires nonempty ID and description"
                            )));
                        }
                        Ok(json!({"id":id,"description":description}))
                    })
                    .collect::<Result<Vec<_>>>()?;
                fields.insert(name.into(), json!(criteria));
            }
        }
        for field in &self.clear {
            if ![
                "status",
                "starts_at",
                "ends_at",
                "color",
                "initiative",
                "project",
                "lead",
                "scope",
                "goal",
                "targets",
                "exit_criteria",
                "outcomes",
                "custom",
            ]
            .contains(&field.as_str())
            {
                return Err(pm_cli::invalid(format!(
                    "field {field:?} cannot be cleared through planning update"
                )));
            }
            if fields.insert(field.clone(), Value::Null).is_some() {
                return Err(pm_cli::invalid(format!(
                    "field {field:?} cannot be both set and cleared, or cleared twice"
                )));
            }
        }
        Ok(fields)
    }
}

#[derive(Debug, Subcommand)]
pub(super) enum HierarchyCommand {
    #[command(about = "Patch individual custom fields without replacing unmentioned values")]
    Custom {
        id: String,
        #[command(flatten)]
        input: super::pm_organization::CustomOptions,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "List native planning records")]
    List {
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Show a native planning record")]
    Show {
        id: String,
        #[command(flatten)]
        membership: MembershipOptions,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Create a planning record with a stable identity")]
    Create {
        name: String,
        #[arg(long)]
        id: Option<String>,
        #[command(flatten)]
        input: Fields,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Update a planning record without changing its identity")]
    Update {
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[command(flatten)]
        input: Fields,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Archive a record while retaining its references and history")]
    Archive {
        id: String,
        #[arg(long)]
        restore: bool,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Assess project or milestone exit policy without writing")]
    Assess {
        id: String,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Complete a project or milestone after attributed manual acceptance")]
    Complete {
        id: String,
        #[arg(long)]
        actor: String,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        json: bool,
    },
}

impl HierarchyCommand {
    pub(super) fn wants_json(&self) -> bool {
        match self {
            Self::List { json, .. }
            | Self::Custom { json, .. }
            | Self::Show { json, .. }
            | Self::Create { json, .. }
            | Self::Update { json, .. }
            | Self::Archive { json, .. }
            | Self::Assess { json, .. }
            | Self::Complete { json, .. } => *json,
        }
    }
    pub(super) fn is_mutation(&self) -> bool {
        !matches!(
            self,
            Self::List { .. } | Self::Show { .. } | Self::Assess { .. }
        )
    }
    fn action(&self, cwd: &Path) -> Result<Action<'_>> {
        Ok(match self {
            Self::Custom { id, input, .. } => Action::Mutate(
                id,
                PlanningMutation::PatchCustom {
                    patch: input.patch()?,
                },
            ),
            Self::List { status, .. } => {
                Action::List(status.as_deref().map(|status| ("status", status)))
            }
            Self::Show { id, membership, .. } => Action::Show(id, membership),
            Self::Create {
                name, id, input, ..
            } => Action::Create(CreatePlanning {
                id: id.clone(),
                name: name.clone(),
                body: input.body(cwd)?.unwrap_or_default(),
                fields: input.metadata()?,
            }),
            Self::Update {
                id, name, input, ..
            } => {
                let mut fields = input.metadata()?;
                if let Some(name) = name {
                    fields.insert("name".into(), json!(name));
                }
                Action::Mutate(
                    id,
                    PlanningMutation::Update {
                        fields,
                        body: input.body(cwd)?,
                    },
                )
            }
            Self::Archive { id, restore, .. } => {
                Action::Mutate(id, PlanningMutation::Archive { archived: !restore })
            }
            Self::Assess { .. } | Self::Complete { .. } => {
                return Err(pm_cli::invalid(
                    "policy commands are dispatched before ordinary planning actions",
                ));
            }
        })
    }
}

enum Action<'a> {
    List(Option<(&'static str, &'a str)>),
    Show(&'a str, &'a MembershipOptions),
    Save(SavePlanning),
    Create(CreatePlanning),
    Mutate(&'a str, PlanningMutation),
    Delete {
        id: &'a str,
        yes: bool,
        force: bool,
        retirement: &'a pm_cli::RetirementOptions,
    },
}

fn fields(values: &[(&str, &Option<String>)]) -> BTreeMap<String, Value> {
    values
        .iter()
        .filter_map(|(key, value)| value.as_ref().map(|value| ((*key).into(), json!(value))))
        .collect()
}

fn save(
    id: &Option<String>,
    name: &str,
    body: Option<String>,
    fields: BTreeMap<String, Value>,
) -> Action<'static> {
    // Preserve the established save identity when --id is omitted. New `create`
    // uses a full kind-prefixed ULID through the library.
    let id = id.clone().unwrap_or_else(|| {
        let mut slug = String::new();
        let mut last_dash = false;
        for ch in name.chars() {
            if ch.is_ascii_alphanumeric() {
                slug.push(ch.to_ascii_lowercase());
                last_dash = false;
            } else if !last_dash {
                slug.push('-');
                last_dash = true;
            }
        }
        let slug = slug.trim_matches('-');
        if slug.is_empty() {
            "session".into()
        } else {
            slug.into()
        }
    });
    Action::Save(SavePlanning {
        id,
        name: name.into(),
        body,
        fields,
    })
}

macro_rules! native_command {
    ($enum:ident, $command:expr) => {
        matches!(
            $command,
            $enum::Create { .. } | $enum::Update { .. } | $enum::Archive { .. } | $enum::Custom { .. }
        ) || matches!($command, $enum::Delete { retirement, .. } if retirement.is_native())
            || matches!($command, $enum::Show { membership, .. } if membership.is_native())
    };
}

pub(super) fn requires_native(command: &Command) -> bool {
    let (options, native) = match command {
        Command::Project { options, command } => (
            options,
            native_command!(ProjectCommand, command)
                || matches!(
                    command,
                    ProjectCommand::Assess { .. } | ProjectCommand::Complete { .. }
                ),
        ),
        Command::Cycle { options, command } => (
            options,
            matches!(command, CycleCommand::Carryover { .. })
                || native_command!(CycleCommand, command),
        ),
        Command::Label { options, command } => (options, native_command!(LabelCommand, command)),
        Command::Initiative { .. } | Command::Milestone { .. } | Command::Target { .. } => {
            return true;
        }
        _ => return false,
    };
    native
        || options.no_input
        || options.stage
        || options.request_id.is_some()
        || options.expected_revision.is_some()
        || options.expected_content.is_some()
}

macro_rules! common_action {
    ($enum:ident, $command:expr, $cwd:expr, $filter:ident $(, $extra:pat => $result:expr)*) => {
        match $command {
            $enum::Custom { id, input, .. } => Some(Action::Mutate(
                id,
                PlanningMutation::PatchCustom {
                    patch: input.patch()?,
                },
            )),
            $enum::List { $filter, .. } => Some(Action::List(
                $filter.as_deref().map(|value| (stringify!($filter), value)),
            )),
            $enum::Show { id, membership, .. } => Some(Action::Show(id, membership)),
            $enum::Create {
                name, id, input, ..
            } => Some(Action::Create(CreatePlanning {
                id: id.clone(),
                name: name.clone(),
                body: input.body($cwd)?.unwrap_or_default(),
                fields: input.metadata()?,
            })),
            $enum::Update {
                id, name, input, ..
            } => {
                let mut fields = input.metadata()?;
                if let Some(name) = name {
                    fields.insert("name".into(), json!(name));
                }
                Some(Action::Mutate(
                    id,
                    PlanningMutation::Update {
                        fields,
                        body: input.body($cwd)?,
                    },
                ))
            }
            $enum::Archive { id, restore, .. } => Some(Action::Mutate(
                id,
                PlanningMutation::Archive { archived: !restore },
            )),
            $enum::Delete {
                id,
                yes,
                force,
                retirement,
                ..
            } => Some(Action::Delete {
                id,
                yes: *yes,
                force: *force,
                retirement,
            }),
            $enum::Save { .. } => None,
            $( $extra => $result, )*
        }
    };
}

pub(super) fn run(
    cwd: &Path,
    repository: &Repository,
    source: &Value,
    command: &Command,
) -> Result<()> {
    if let Command::Cycle {
        options,
        command: CycleCommand::Carryover { input, json },
    } = command
    {
        return super::pm_carryover::run(repository, source, options, input, *json);
    }
    match command {
        Command::Project {
            options,
            command: ProjectCommand::Assess { id, json },
        } => {
            pm_cli::read_options(options)?;
            return pm_cli::emit(
                *json,
                "planning_policy_assessment",
                source,
                &repository.assess_planning_policy(PlanningKind::Project, id)?,
                None,
            );
        }
        Command::Project {
            options,
            command:
                ProjectCommand::Complete {
                    id,
                    actor,
                    reason,
                    json,
                },
        } => {
            let expected = pm_cli::expected(options)?.ok_or_else(|| {
                pm_cli::invalid(
                    "planning completion requires --expected-revision and --expected-content",
                )
            })?;
            let request = options
                .request_id
                .as_deref()
                .map(str::parse)
                .transpose()?
                .unwrap_or_else(RequestId::new);
            let receipt = repository.complete_planning(
                PlanningKind::Project,
                id,
                &expected,
                &PolicyAcceptance {
                    actor: actor.clone(),
                    reason: reason.clone(),
                },
                &request,
            )?;
            return pm_cli::emit_mutation(
                repository,
                options,
                *json,
                "planning_completion",
                source,
                &receipt,
            );
        }
        Command::Milestone {
            options,
            command: HierarchyCommand::Assess { id, json },
        } => {
            pm_cli::read_options(options)?;
            return pm_cli::emit(
                *json,
                "planning_policy_assessment",
                source,
                &repository.assess_planning_policy(PlanningKind::Milestone, id)?,
                None,
            );
        }
        Command::Milestone {
            options,
            command:
                HierarchyCommand::Complete {
                    id,
                    actor,
                    reason,
                    json,
                },
        } => {
            let expected = pm_cli::expected(options)?.ok_or_else(|| {
                pm_cli::invalid(
                    "planning completion requires --expected-revision and --expected-content",
                )
            })?;
            let request = options
                .request_id
                .as_deref()
                .map(str::parse)
                .transpose()?
                .unwrap_or_else(RequestId::new);
            let receipt = repository.complete_planning(
                PlanningKind::Milestone,
                id,
                &expected,
                &PolicyAcceptance {
                    actor: actor.clone(),
                    reason: reason.clone(),
                },
                &request,
            )?;
            return pm_cli::emit_mutation(
                repository,
                options,
                *json,
                "planning_completion",
                source,
                &receipt,
            );
        }
        Command::Initiative {
            command: HierarchyCommand::Assess { .. } | HierarchyCommand::Complete { .. },
            ..
        }
        | Command::Target {
            command: HierarchyCommand::Assess { .. } | HierarchyCommand::Complete { .. },
            ..
        } => {
            return Err(pm_cli::invalid(
                "completion policy is defined for projects and milestones only",
            ));
        }
        _ => {}
    }
    let (kind, options, json_output, common) = match command {
        Command::Initiative { options, command } => (
            PlanningKind::Initiative,
            options,
            command.wants_json(),
            Some(command.action(cwd)?),
        ),
        Command::Milestone { options, command } => (
            PlanningKind::Milestone,
            options,
            command.wants_json(),
            Some(command.action(cwd)?),
        ),
        Command::Target { options, command } => (
            PlanningKind::Target,
            options,
            command.wants_json(),
            Some(command.action(cwd)?),
        ),
        Command::Project { options, command } => (
            PlanningKind::Project,
            options,
            command.wants_json(),
            common_action!(
                ProjectCommand,
                command,
                cwd,
                status,
                ProjectCommand::Assess { .. } => None,
                ProjectCommand::Complete { .. } => None
            ),
        ),
        Command::Cycle { options, command } => (
            PlanningKind::Cycle,
            options,
            command.wants_json(),
            common_action!(CycleCommand, command, cwd, status, CycleCommand::Carryover { .. } => None),
        ),
        Command::Label { options, command } => (
            PlanningKind::Label,
            options,
            command.wants_json(),
            common_action!(LabelCommand, command, cwd, color),
        ),
        _ => return Err(pm_cli::invalid("expected a native planning command")),
    };
    let action = match common {
        Some(action) => action,
        None => match command {
            Command::Project {
                command:
                    ProjectCommand::Save {
                        id,
                        name,
                        description,
                        status,
                        ..
                    },
                ..
            } => save(id, name, description.clone(), fields(&[("status", status)])),
            Command::Cycle {
                command:
                    CycleCommand::Save {
                        id,
                        name,
                        starts_at,
                        ends_at,
                        status,
                        ..
                    },
                ..
            } => save(
                id,
                name,
                None,
                fields(&[
                    ("starts_at", starts_at),
                    ("ends_at", ends_at),
                    ("status", status),
                ]),
            ),
            Command::Label {
                command:
                    LabelCommand::Save {
                        id, name, color, ..
                    },
                ..
            } => save(id, name, None, fields(&[("color", color)])),
            _ => return Err(pm_cli::invalid("unsupported planning command")),
        },
    };
    let expected = pm_cli::expected(options)?;
    match &action {
        Action::List(filter) => {
            pm_cli::read_options(options)?;
            let mut records = repository.list_planning(kind)?;
            if let Some((field, value)) = filter {
                records.retain(|record| match *field {
                    "status" => record.metadata.status.as_deref() == Some(*value),
                    "color" => record.metadata.color.as_deref() == Some(*value),
                    _ => false,
                });
            }
            return pm_cli::emit(json_output, "planning_list", source, &records, None);
        }
        Action::Show(id, membership) => {
            pm_cli::read_options(options)?;
            if membership.members {
                return pm_cli::emit(
                    json_output,
                    "planning_membership",
                    source,
                    &repository.planning_membership(&membership.query(kind, id)?)?,
                    None,
                );
            }
            return pm_cli::emit(
                json_output,
                "planning_show",
                source,
                &repository.planning_record(kind, id)?,
                None,
            );
        }
        Action::Delete {
            id,
            yes,
            force,
            retirement,
        } => {
            if retirement.dry_run {
                pm_cli::read_options(options)?;
                let target = RetirementTarget::new(kind.into(), *id)?;
                if *force {
                    return pm_cli::emit(
                        json_output,
                        "planning_resolution_preview",
                        source,
                        &repository.reference_retirement_preview(&target)?,
                        None,
                    );
                }
                return pm_cli::emit(
                    json_output,
                    "planning_retirement_preview",
                    source,
                    &repository.retirement_preview(&target)?,
                    None,
                );
            }
            if !yes {
                return Err(pm_cli::invalid(
                    "delete requires --yes; inspect incoming references with --dry-run",
                ));
            }
            if *force && retirement.expected_preview.is_none() {
                return Err(pm_cli::invalid(
                    "--force requires a reviewed resolution: run delete with --force --dry-run, then apply with --yes --expected-preview HASH",
                ));
            }
        }
        _ => {}
    }
    let request = options
        .request_id
        .as_deref()
        .map(str::parse)
        .transpose()?
        .unwrap_or_else(RequestId::new);
    let (operation, receipt) = match action {
        Action::Delete {
            id,
            force,
            retirement,
            ..
        } => {
            let input = RetirementInput {
                target: RetirementTarget::new(kind.into(), id)?,
                expected,
                expected_preview: retirement
                    .expected_preview
                    .as_deref()
                    .map(str::parse)
                    .transpose()?,
            };
            (
                "planning_delete",
                if force {
                    repository.retire_reference(&input, &request)?
                } else {
                    repository.retire_record(&input, &request)?
                },
            )
        }
        Action::Save(input) => (
            "planning_save",
            repository.save_planning(kind, &input, expected.as_ref(), &request)?,
        ),
        Action::Create(input) => {
            if expected.is_some() {
                return Err(pm_cli::invalid(
                    "create cannot take an expected source token",
                ));
            }
            (
                "planning_create",
                repository.create_planning(kind, &input, &request)?,
            )
        }
        Action::Mutate(id, mutation) => (
            "planning_update",
            repository.mutate_planning(kind, id, expected.as_ref(), &mutation, &request)?,
        ),
        _ => unreachable!(),
    };
    pm_cli::emit_mutation(
        repository,
        options,
        json_output,
        operation,
        source,
        &receipt,
    )
}
