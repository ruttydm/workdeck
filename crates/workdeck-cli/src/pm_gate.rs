//! Gate declarations expose criterion identity and explicit-subject assessments.
use super::{pm_cli, pm_organization};
use clap::Subcommand;
use serde_json::Value;
use std::path::{Path, PathBuf};
use workdeck_pm::{
    CriterionOwner, GateAssessmentRequest, GateMutation, Repository, RequestId, Result,
};

#[derive(Debug, Subcommand)]
pub(super) enum GateCommand {
    List,
    #[command(
        about = "Verify every committed gate requirement against original retained red/green proof and independent authority"
    )]
    VerifyRedGreen {
        #[arg(
            help = "RedGreenGateRequest JSON file, or '-' for stdin; requires independent authority and exact source/evidence pins"
        )]
        input: PathBuf,
    },
    #[command(
        about = "Verify every committed gate requirement against signed passed checks and independent producer authority"
    )]
    VerifyGreen {
        #[arg(
            help = "VerifiedGateRequest JSON file, or '-' for stdin; requires independent authority and exact source/evidence pins"
        )]
        input: PathBuf,
    },
    #[command(
        about = "Permanently retire a gate while retaining source, history and its reserved identity"
    )]
    Delete {
        id: String,
        #[arg(long)]
        yes: bool,
        #[command(flatten)]
        retirement: pm_cli::RetirementOptions,
    },
    Show {
        id: String,
    },
    #[command(
        about = "Resolve a stable acceptance criterion and its exact semantic definition hash"
    )]
    Criterion {
        #[arg(value_parser = ["issue", "feature", "milestone", "project"])]
        owner_kind: String,
        owner: String,
        criterion: String,
    },
    #[command(about = "Create a nonempty AND gate from CreateGate JSON; '-' reads stdin")]
    Create {
        input: PathBuf,
    },
    #[command(about = "Apply a metadata patch JSON object to the exact inspected gate revision")]
    Update {
        id: String,
        input: PathBuf,
    },
    Archive {
        id: String,
        #[arg(long)]
        restore: bool,
    },
    Custom {
        id: String,
        #[command(flatten)]
        patch: pm_organization::CustomOptions,
    },
    #[command(
        about = "Assess an explicitly declared exact subject; declaration alone never proves verification"
    )]
    Assess {
        id: String,
        #[arg(long, help = "ExactSubject JSON file, or '-' for stdin")]
        subject_file: PathBuf,
        #[arg(
            long,
            value_name = "RFC3339",
            help = "Explicit assessment time used for evidence freshness"
        )]
        as_of: String,
    },
}
impl GateCommand {
    pub(super) fn is_mutation(&self) -> bool {
        if let Self::Delete { retirement, .. } = self {
            return !retirement.dry_run;
        }
        matches!(
            self,
            Self::Create { .. } | Self::Update { .. } | Self::Archive { .. } | Self::Custom { .. }
        )
    }
}
pub(super) fn run(
    cwd: &Path,
    repository: &Repository,
    source: &Value,
    options: &pm_cli::NativeOptions,
    command: &GateCommand,
) -> Result<()> {
    if let GateCommand::Delete {
        id,
        yes,
        retirement,
    } = command
    {
        return pm_cli::retire_native_record(
            repository,
            source,
            options,
            workdeck_pm::RetirementTarget::new(workdeck_pm::RetirementKind::Gate, id)?,
            retirement,
            *yes,
        );
    }
    if !command.is_mutation() {
        pm_cli::read_options(&options.mutation)?;
        return match command {
            GateCommand::VerifyRedGreen { input } => {
                let request: workdeck_pm::RedGreenGateRequest = pm_cli::typed_input(cwd, input)?;
                pm_cli::emit(
                    options.json,
                    "gate.red_green",
                    source,
                    &repository.verify_red_green_gate(&request)?,
                    None,
                )
            }
            GateCommand::VerifyGreen { input } => {
                let request: workdeck_pm::VerifiedGateRequest = pm_cli::typed_input(cwd, input)?;
                pm_cli::emit(
                    options.json,
                    "gate.verified",
                    source,
                    &repository.verify_verified_gate(&request)?,
                    None,
                )
            }
            GateCommand::List => {
                pm_cli::emit(options.json, "gates", source, &repository.gates()?, None)
            }
            GateCommand::Show { id } => pm_cli::emit(
                options.json,
                "gate",
                source,
                &repository.gate(&id.parse()?)?,
                None,
            ),
            GateCommand::Criterion {
                owner_kind,
                owner,
                criterion,
            } => {
                let owner = match owner_kind.as_str() {
                    "issue" => CriterionOwner::Issue(owner.parse()?),
                    "feature" => CriterionOwner::Feature(owner.parse()?),
                    "milestone" => CriterionOwner::Milestone(owner.clone()),
                    "project" => CriterionOwner::Project(owner.clone()),
                    _ => {
                        return Err(pm_cli::invalid(
                            "criterion owner must be issue, feature, milestone or project",
                        ));
                    }
                };
                pm_cli::emit(
                    options.json,
                    "criterion",
                    source,
                    &repository.resolve_criterion(&owner, criterion)?,
                    None,
                )
            }
            GateCommand::Assess {
                id,
                subject_file,
                as_of,
            } => {
                let input = GateAssessmentRequest {
                    gate: id.parse()?,
                    subject: pm_cli::typed_input(cwd, subject_file)?,
                    as_of: as_of.parse().map_err(|error| {
                        pm_cli::invalid(format!("invalid RFC3339 assessment time: {error}"))
                    })?,
                };
                pm_cli::emit(
                    options.json,
                    "gate_assessment",
                    source,
                    &repository.assess_gate(&input)?,
                    None,
                )
            }
            _ => unreachable!(),
        };
    }
    let expected = pm_cli::expected(&options.mutation)?;
    let request = options
        .mutation
        .request_id
        .as_deref()
        .map(str::parse)
        .transpose()?
        .unwrap_or_else(RequestId::new);
    let receipt = if let GateCommand::Create { input } = command {
        if expected.is_some() {
            return Err(pm_cli::invalid(
                "gate creation does not accept expected source flags",
            ));
        }
        repository.create_gate(&pm_cli::typed_input(cwd, input)?, &request)?
    } else {
        let expected = expected.ok_or_else(|| {
            pm_cli::invalid(
                "gate changes require --expected-revision and --expected-content from gate show",
            )
        })?;
        let (id, mutation) = match command {
            GateCommand::Update { id, input } => (
                id,
                GateMutation::Update {
                    fields: pm_cli::typed_input(cwd, input)?,
                },
            ),
            GateCommand::Archive { id, restore } => {
                (id, GateMutation::Archive { archived: !restore })
            }
            GateCommand::Custom { id, patch } => (
                id,
                GateMutation::PatchCustom {
                    patch: patch.patch()?,
                },
            ),
            _ => unreachable!(),
        };
        repository.mutate_gate(&id.parse()?, &expected, &mutation, &request)?
    };
    pm_cli::emit_mutation(
        repository,
        &options.mutation,
        options.json,
        "gate",
        source,
        &receipt,
    )
}
