//! Organization adapters pass semantic intent to the shared repository engine.
use super::pm_cli;
use clap::{Args, Subcommand};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use workdeck_pm::{
    CustomPatch, ErrorCode, IdentityMode, PmError, Repository, RequestId, Result, SchemaChange,
    UserDefinition, UserKind, UserMutation,
};

#[derive(Debug, Default, Args)]
pub(super) struct Options {
    #[command(flatten)]
    mutation: pm_cli::IssueOptions,
    #[arg(long, global = true)]
    pub(super) json: bool,
}

#[derive(Debug, Default, Args)]
pub(super) struct CustomOptions {
    #[arg(
        long,
        value_name = "KEY=JSON",
        help = "Repeat to set individual custom keys; strings require JSON quotes"
    )]
    set: Vec<String>,
    #[arg(
        long,
        value_name = "KEY",
        help = "Repeat to remove individual custom keys without changing others"
    )]
    unset: Vec<String>,
}
impl CustomOptions {
    pub(super) fn patch(&self) -> Result<CustomPatch> {
        let mut set = BTreeMap::new();
        for entry in &self.set {
            let (key, value) = entry
                .split_once('=')
                .ok_or_else(|| pm_cli::invalid("custom set requires KEY=JSON"))?;
            let value: Value = serde_json::from_str(value).map_err(|error| {
                pm_cli::invalid(format!("invalid JSON for custom key {key:?}: {error}"))
            })?;
            if set.insert(key.to_owned(), value).is_some() {
                return Err(pm_cli::invalid(format!(
                    "custom key {key:?} was set more than once"
                )));
            }
        }
        let patch = CustomPatch {
            set,
            unset: self.unset.clone(),
        };
        patch.apply(&BTreeMap::new())?;
        Ok(patch)
    }
    fn is_empty(&self) -> bool {
        self.set.is_empty() && self.unset.is_empty()
    }
}

#[derive(Debug, Subcommand)]
pub(super) enum UserCommand {
    #[command(about = "List declared identities and the aggregate source token")]
    List,
    #[command(about = "Show one declared identity")]
    Show { id: String },
    #[command(about = "Declare a stable user, agent or service identity")]
    Create {
        id: String,
        name: Option<String>,
        #[arg(long,value_parser=["human","agent","service"])]
        kind: Option<String>,
        #[arg(
            long,
            value_name = "PATH",
            help = "Read a complete UserDefinition JSON, or '-' for stdin"
        )]
        from_json: Option<PathBuf>,
    },
    #[command(about = "Patch an identity while retaining unmentioned metadata")]
    Update {
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long,value_parser=["human","agent","service"])]
        kind: Option<String>,
        #[command(flatten)]
        custom: CustomOptions,
        #[arg(
            long,
            value_name = "PATH",
            help = "Explicitly replace the complete UserDefinition; cannot be combined with patch flags"
        )]
        from_json: Option<PathBuf>,
    },
    #[command(about = "Archive an identity, retaining historical references")]
    Archive {
        id: String,
        #[arg(long)]
        restore: bool,
    },
    #[command(about = "Choose open identities or require registered active identities")]
    Mode {
        #[arg(value_parser=["open","registered"])]
        mode: String,
    },
}

#[derive(Debug, Subcommand)]
pub(super) enum SchemaCommand {
    #[command(about = "Inspect custom-field declarations and estimate units")]
    Show,
    #[command(about = "Preview a SchemaChange JSON against current records without writing")]
    Preview { input: PathBuf },
    #[command(about = "Apply the exact reviewed SchemaChange when its sources still match")]
    Apply {
        input: PathBuf,
        #[arg(long, value_name = "SHA256")]
        expected_preview: String,
    },
}

#[derive(Debug, Subcommand)]
pub(super) enum OrganizationCommand {
    #[command(about = "Inspect and change repository custom-field and estimate policy")]
    Schema {
        #[command(subcommand)]
        command: SchemaCommand,
    },
    #[command(
        about = "Inspect current policy violations; --check returns nonzero when noncompliant"
    )]
    Compliance {
        #[arg(long)]
        check: bool,
    },
    #[command(
        about = "Report exact estimate totals per unit, without combining incompatible units"
    )]
    EstimateReport {
        #[command(flatten)]
        query: Box<pm_cli::IssueListOptions>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        cycle: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        priority: Option<String>,
        #[arg(long)]
        assignee: Option<String>,
        #[arg(long)]
        label: Option<String>,
        #[arg(long)]
        due_at: Option<String>,
    },
}

pub(super) fn user_is_mutation(command: &UserCommand) -> bool {
    !matches!(command, UserCommand::List | UserCommand::Show { .. })
}
pub(super) fn organization_is_mutation(command: &OrganizationCommand) -> bool {
    matches!(
        command,
        OrganizationCommand::Schema {
            command: SchemaCommand::Apply { .. }
        }
    )
}
fn request(options: &Options) -> Result<RequestId> {
    options
        .mutation
        .request_id
        .as_deref()
        .map(str::parse)
        .transpose()
        .map(|id| id.unwrap_or_else(RequestId::new))
}
fn read<T: DeserializeOwned>(cwd: &Path, path: &Path) -> Result<T> {
    serde_json::from_str(&pm_cli::read_input(cwd, path)?)
        .map_err(|error| pm_cli::invalid(format!("invalid organization JSON: {error}")).at(path))
}
fn user_kind(value: &str) -> Result<UserKind> {
    match value {
        "human" => Ok(UserKind::Human),
        "agent" => Ok(UserKind::Agent),
        "service" => Ok(UserKind::Service),
        _ => Err(pm_cli::invalid("user kind must be human, agent or service")),
    }
}

pub(super) fn run_user(
    cwd: &Path,
    repository: &Repository,
    source: &Value,
    options: &Options,
    command: &UserCommand,
) -> Result<()> {
    match command {
        UserCommand::List => {
            pm_cli::read_options(&options.mutation)?;
            return pm_cli::emit(options.json, "users", source, &repository.users()?, None);
        }
        UserCommand::Show { id } => {
            pm_cli::read_options(&options.mutation)?;
            return pm_cli::emit(options.json, "user", source, &repository.user(id)?, None);
        }
        _ => {}
    }
    let request = request(options)?;
    let expected = pm_cli::expected(&options.mutation)?;
    let receipt = match command {
        UserCommand::Mode { mode } => repository.set_identity_mode(
            match mode.as_str() {
                "open" => IdentityMode::Open,
                "registered" => IdentityMode::Registered,
                _ => return Err(pm_cli::invalid("identity mode must be open or registered")),
            },
            expected.as_ref(),
            &request,
        )?,
        UserCommand::Create {
            id,
            name,
            kind,
            from_json,
        } => {
            let mut user =
                if let Some(path) = from_json {
                    read(cwd, path)?
                } else {
                    UserDefinition::new(name.as_deref().ok_or_else(|| {
                        pm_cli::invalid("user create requires a name or --from-json")
                    })?)
                };
            if let Some(name) = name {
                user.name.clone_from(name);
            }
            if let Some(kind) = kind {
                user.kind = user_kind(kind)?;
            }
            repository.mutate_user(
                id,
                expected.as_ref(),
                &UserMutation::Create { user },
                &request,
            )?
        }
        UserCommand::Update {
            id,
            name,
            kind,
            custom,
            from_json,
        } => {
            let mutation = if let Some(path) = from_json {
                if name.is_some() || kind.is_some() || !custom.is_empty() {
                    return Err(pm_cli::invalid(
                        "user --from-json replacement cannot be combined with patch flags",
                    ));
                }
                UserMutation::Update {
                    user: read(cwd, path)?,
                }
            } else {
                if name.is_none() && kind.is_none() && custom.is_empty() {
                    return Err(pm_cli::invalid(
                        "user update requires at least one patch flag or --from-json",
                    ));
                }
                UserMutation::Patch {
                    name: name.clone(),
                    kind: kind.as_deref().map(user_kind).transpose()?,
                    custom: custom.patch()?,
                }
            };
            repository.mutate_user(id, expected.as_ref(), &mutation, &request)?
        }
        UserCommand::Archive { id, restore } => repository.mutate_user(
            id,
            expected.as_ref(),
            &UserMutation::Archive {
                archived: !*restore,
            },
            &request,
        )?,
        _ => unreachable!(),
    };
    pm_cli::emit_mutation(
        repository,
        &options.mutation,
        options.json,
        "user_update",
        source,
        &receipt,
    )
}

pub(super) fn run_organization(
    cwd: &Path,
    repository: &Repository,
    source: &Value,
    options: &Options,
    command: &OrganizationCommand,
) -> Result<()> {
    if let OrganizationCommand::Schema {
        command: SchemaCommand::Apply {
            input,
            expected_preview,
        },
    } = command
    {
        if pm_cli::expected(&options.mutation)?.is_some() {
            return Err(pm_cli::invalid(
                "schema apply uses --expected-preview rather than record source tokens",
            ));
        }
        let change: SchemaChange = read(cwd, input)?;
        let expected = expected_preview.parse()?;
        let receipt =
            repository.apply_schema_change(&change, Some(&expected), &request(options)?)?;
        return pm_cli::emit_mutation(
            repository,
            &options.mutation,
            options.json,
            "organization_schema_apply",
            source,
            &receipt,
        );
    }
    pm_cli::read_options(&options.mutation)?;
    match command {
        OrganizationCommand::Schema {
            command: SchemaCommand::Show,
        } => pm_cli::emit(
            options.json,
            "organization_schema",
            source,
            &repository.organization_schema()?,
            None,
        ),
        OrganizationCommand::Schema {
            command: SchemaCommand::Preview { input },
        } => {
            let change: SchemaChange = read(cwd, input)?;
            pm_cli::emit(
                options.json,
                "organization_schema_preview",
                source,
                &repository.preview_schema_change(&change)?,
                None,
            )
        }
        OrganizationCommand::Compliance { check } => {
            let report = repository.organization_compliance()?;
            if *check && !report.compliant {
                return Err(PmError::new(
                    ErrorCode::PolicyBlocked,
                    "repository organization policy is not satisfied",
                )
                .details(json!(report)));
            }
            pm_cli::emit(
                options.json,
                "organization_compliance",
                source,
                &report,
                None,
            )
        }
        OrganizationCommand::EstimateReport {
            query,
            project,
            cycle,
            status,
            priority,
            assignee,
            label,
            due_at,
        } => {
            let mut query = query.query()?;
            query.project = project.clone();
            query.cycle = cycle.clone();
            query.status = status.clone();
            query.priority = priority
                .as_deref()
                .map(workdeck_pm::Priority::parse_input)
                .transpose()?;
            query.assignee = assignee.clone();
            query.label = label.clone();
            query.due_at = due_at.clone();
            pm_cli::emit(
                options.json,
                "estimate_report",
                source,
                &repository.estimate_report(&query)?,
                None,
            )
        }
        OrganizationCommand::Schema {
            command: SchemaCommand::Apply { .. },
        } => unreachable!(),
    }
}
