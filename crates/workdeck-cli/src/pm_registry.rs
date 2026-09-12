//! Explicit local mappings; cross-repository reads never select a write destination.
use super::{pm_claims::Options, pm_cli};
use clap::{Args, Subcommand, ValueEnum};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use workdeck_pm::{registry::*, *};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(super) enum Role {
    WorkingTree,
    Accepted,
    Proposal,
    Coordination,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(super) enum Facet {
    Assigned,
    ReviewRequested,
    Overdue,
    Blocked,
    Claimed,
}
impl From<Facet> for MyWorkFacet {
    fn from(value: Facet) -> Self {
        match value {
            Facet::Assigned => Self::Assigned,
            Facet::ReviewRequested => Self::ReviewRequested,
            Facet::Overdue => Self::Overdue,
            Facet::Blocked => Self::Blocked,
            Facet::Claimed => Self::Claimed,
        }
    }
}

#[derive(Debug, Args)]
pub(super) struct Inspect {
    alias: String,
    checkout: PathBuf,
    #[arg(long, value_enum, default_value = "working-tree")]
    source: Role,
    #[arg(long, help = "Full proposal ref; required only with --source proposal")]
    reference: Option<String>,
}

#[derive(Debug, Subcommand)]
pub(super) enum RegistryCommand {
    #[command(about = "List explicit checkout mappings and the registry revision")]
    List,
    #[command(
        about = "List work across explicit mappings with assignment, review, overdue, blocker and claim facets"
    )]
    MyWork {
        #[arg(long)]
        assignee: String,
        #[arg(long, value_enum, default_value = "assigned")]
        facet: Facet,
        #[arg(
            long,
            help = "RFC3339 evaluation instant; required for overdue/claimed and retained across pages"
        )]
        as_of: Option<Timestamp>,
        #[arg(
            long = "repository",
            help = "Registered alias; repeat to select a subset"
        )]
        aliases: Vec<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        cursor: Option<String>,
        #[arg(long, default_value_t = 30_000)]
        timeout_ms: u64,
    },
    #[command(
        about = "Inspect a checkout and prepare an exact registration request without writing"
    )]
    Inspect(Inspect),
    #[command(about = "Register the exact reviewed RegistryRequest JSON locally")]
    Register {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        request_id: String,
    },
    #[command(about = "Remove a mapping without modifying or requiring its target")]
    Remove {
        alias: String,
        #[arg(long)]
        expected_revision: u64,
        #[arg(long)]
        expected_content: String,
        #[arg(long)]
        request_id: String,
    },
    #[command(about = "Resolve one exact mapping and verify its current checkout identity")]
    Show { alias: String },
}

impl RegistryCommand {
    pub(super) fn is_mutation(&self) -> bool {
        matches!(self, Self::Register { .. } | Self::Remove { .. })
    }
}

pub(super) fn run(
    cwd: &Path,
    repository: &Repository,
    source: &Value,
    options: &Options,
    command: &RegistryCommand,
) -> Result<Option<u8>> {
    options.output.validate()?;
    let store = RegistryStore::open(repository)?;
    if let RegistryCommand::MyWork {
        assignee,
        facet,
        as_of,
        aliases,
        limit,
        cursor,
        timeout_ms,
    } = command
    {
        let report = store.my_work(&MyWorkRequest {
            assignee: assignee.clone(),
            facet: (*facet).into(),
            as_of: *as_of,
            aliases: aliases.clone(),
            limit: *limit,
            cursor: cursor.clone(),
            timeout_ms: *timeout_ms,
        })?;
        options
            .output
            .emit(options.json, "my_work", source, &json!(report))?;
        return Ok((!report.all_sources_available).then_some(4));
    }
    let (kind, result) = match command {
        RegistryCommand::MyWork { .. } => unreachable!(),
        RegistryCommand::List => ("repository_registry", json!(store.snapshot()?)),
        RegistryCommand::Show { alias } => {
            let (entry, _) = store.resolve(alias)?;
            ("registered_checkout", json!(entry))
        }
        RegistryCommand::Inspect(input) => {
            let selector = match (input.source, &input.reference) {
                (Role::WorkingTree, None) => SourceSelector::WorkingTree,
                (Role::Accepted, None) => SourceSelector::Accepted,
                (Role::Coordination, None) => SourceSelector::Coordination,
                (Role::Proposal, Some(reference)) => SourceSelector::Proposal {
                    reference: reference.parse()?,
                },
                _ => {
                    return Err(PmError::new(
                        ErrorCode::InvalidInput,
                        "--reference is required exactly when --source proposal is selected",
                    ));
                }
            };
            let checkout = inspect_checkout(&input.alias, &cwd.join(&input.checkout), selector)?;
            let request = RegistryRequest {
                expected: store.snapshot()?.source,
                mutation: RegistryMutation::Register { checkout },
            };
            ("repository_registration_plan", json!(request))
        }
        RegistryCommand::Register { input, request_id } => {
            let request: RegistryRequest = pm_cli::typed_input(cwd, input)?;
            if !matches!(request.mutation, RegistryMutation::Register { .. }) {
                return Err(PmError::new(
                    ErrorCode::InvalidInput,
                    "repository register requires a registration request",
                ));
            }
            (
                "repository_registration",
                json!(store.mutate(&request, &request_id.parse()?)?),
            )
        }
        RegistryCommand::Remove {
            alias,
            expected_revision,
            expected_content,
            request_id,
        } => {
            let request = RegistryRequest {
                expected: SourceToken {
                    revision: Revision::new(*expected_revision)?,
                    content: expected_content.parse()?,
                },
                mutation: RegistryMutation::Remove {
                    alias: alias.clone(),
                },
            };
            (
                "repository_registration",
                json!(store.mutate(&request, &request_id.parse()?)?),
            )
        }
    };
    options.output.emit(options.json, kind, source, &result)?;
    Ok(None)
}
