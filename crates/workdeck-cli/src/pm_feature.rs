//! Native feature declarations and derived coverage share the PM application engine.
use super::{pm_cli, pm_organization};
use clap::Subcommand;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use workdeck_pm::{
    CreateFeature, FeatureCoverageQuery, FeatureMaturity, FeatureMutation, PolicyAcceptance,
    Repository, RequestId, Result,
};

fn maturity(value: &str) -> Result<FeatureMaturity> {
    match value {
        "draft" => Ok(FeatureMaturity::Draft),
        "specified" => Ok(FeatureMaturity::Specified),
        "implemented" => Ok(FeatureMaturity::Implemented),
        _ => Err(pm_cli::invalid(
            "feature maturity must be draft, specified or implemented",
        )),
    }
}

#[derive(Debug, Subcommand)]
pub(super) enum FeatureCommand {
    List,
    #[command(
        about = "Permanently retire a feature while retaining its source and reserving its identity"
    )]
    Delete {
        id: String,
        #[arg(long)]
        yes: bool,
        #[command(flatten)]
        retirement: pm_cli::RetirementOptions,
    },
    #[command(
        about = "Add a single symmetric related-feature link without changing declared maturity"
    )]
    Relate {
        id: String,
        other: String,
        #[arg(long)]
        expected_other_revision: Option<String>,
        #[arg(long)]
        expected_other_content: Option<String>,
    },
    Unrelate {
        id: String,
        other: String,
        #[arg(long)]
        expected_other_revision: Option<String>,
        #[arg(long)]
        expected_other_content: Option<String>,
    },
    Show {
        id: String,
    },
    #[command(about = "Create a capability declaration with an immutable feature identity")]
    Create {
        #[arg(required_unless_present = "from_json")]
        name: Option<String>,
        #[arg(
            long,
            help = "Read CreateFeature JSON, or '-' for stdin; explicit flags override it"
        )]
        from_json: Option<PathBuf>,
        #[arg(long)]
        body_file: Option<PathBuf>,
        #[arg(long, help = "Optional grouping directory beneath .workdeck/features")]
        directory: Option<String>,
    },
    Update {
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, help = "Read a metadata patch JSON object, or '-' for stdin")]
        fields_file: Option<PathBuf>,
        #[arg(long)]
        body_file: Option<PathBuf>,
    },
    #[command(about = "Change logical hierarchy while preserving feature identity and placement")]
    Parent {
        id: String,
        #[arg(required_unless_present = "clear", conflicts_with = "clear")]
        parent: Option<String>,
        #[arg(long)]
        clear: bool,
    },
    #[command(
        about = "Move the feature file beneath a grouping directory, retaining its immutable ID"
    )]
    Move {
        id: String,
        directory: String,
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
        about = "Inspect declared feature coverage and unresolved references; issue completion does not promote maturity"
    )]
    Coverage {
        id: String,
        #[command(flatten)]
        query: Box<pm_cli::IssueListOptions>,
    },
    #[command(
        about = "Assess a feature maturity transition without writing or treating declarations as evidence"
    )]
    Assess {
        id: String,
        #[arg(long, default_value = "implemented")]
        to: String,
    },
    #[command(about = "Promote one feature maturity stage after explicit attributed acceptance")]
    Promote {
        id: String,
        #[arg(long, value_parser = ["specified", "implemented"])]
        to: String,
        #[arg(long)]
        actor: String,
        #[arg(long)]
        reason: String,
    },
}

impl FeatureCommand {
    pub(super) fn is_mutation(&self) -> bool {
        if let Self::Delete { retirement, .. } = self {
            return !retirement.dry_run;
        }
        !matches!(
            self,
            Self::List | Self::Show { .. } | Self::Coverage { .. } | Self::Assess { .. }
        )
    }
}

pub(super) fn run(
    cwd: &Path,
    repository: &Repository,
    source: &Value,
    options: &pm_cli::NativeOptions,
    command: &FeatureCommand,
) -> Result<()> {
    if let FeatureCommand::Delete {
        id,
        yes,
        retirement,
    } = command
    {
        return pm_cli::retire_native_record(
            repository,
            source,
            options,
            workdeck_pm::RetirementTarget::new(workdeck_pm::RetirementKind::Feature, id)?,
            retirement,
            *yes,
        );
    }
    if !command.is_mutation() {
        pm_cli::read_options(&options.mutation)?;
        return match command {
            FeatureCommand::List => pm_cli::emit(
                options.json,
                "features",
                source,
                &repository.list_features()?,
                None,
            ),
            FeatureCommand::Show { id } => pm_cli::emit(
                options.json,
                "feature",
                source,
                &repository.feature(id)?,
                None,
            ),
            FeatureCommand::Coverage { id, query } => pm_cli::emit(
                options.json,
                "feature_coverage",
                source,
                &repository.feature_coverage(&FeatureCoverageQuery {
                    feature: id.clone(),
                    issues: query.query()?,
                })?,
                None,
            ),
            FeatureCommand::Assess { id, to } => pm_cli::emit(
                options.json,
                "feature_maturity_assessment",
                source,
                &repository.assess_feature_maturity(id, maturity(to)?)?,
                None,
            ),
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
    if let FeatureCommand::Relate {
        id,
        other,
        expected_other_revision,
        expected_other_content,
    }
    | FeatureCommand::Unrelate {
        id,
        other,
        expected_other_revision,
        expected_other_content,
    } = command
    {
        let expected_other = pm_cli::expected(&pm_cli::IssueOptions {
            expected_revision: expected_other_revision.clone(),
            expected_content: expected_other_content.clone(),
            ..Default::default()
        })?;
        let receipt = repository.set_feature_related(
            id,
            other,
            expected.as_ref(),
            expected_other.as_ref(),
            matches!(command, FeatureCommand::Relate { .. }),
            &request,
        )?;
        return pm_cli::emit_mutation(
            repository,
            &options.mutation,
            options.json,
            "feature_related",
            source,
            &receipt,
        );
    }
    let receipt = if let FeatureCommand::Create {
        name,
        from_json,
        body_file,
        directory,
    } = command
    {
        if expected.is_some() {
            return Err(pm_cli::invalid(
                "feature creation does not accept expected source flags",
            ));
        }
        if from_json.as_deref() == Some(Path::new("-"))
            && body_file.as_deref() == Some(Path::new("-"))
        {
            return Err(pm_cli::invalid("only one input may read stdin"));
        }
        let mut input: CreateFeature = match from_json {
            Some(path) => pm_cli::typed_input(cwd, path)?,
            None => CreateFeature::new(name.as_deref().unwrap_or_default()),
        };
        if let Some(name) = name {
            input.name = name.clone();
        }
        if let Some(path) = body_file {
            input.body = pm_cli::read_input(cwd, path)?;
        }
        if let Some(directory) = directory {
            input.directory = Some(directory.clone());
        }
        repository.create_feature(&input, &request)?
    } else if let FeatureCommand::Promote {
        id,
        to,
        actor,
        reason,
    } = command
    {
        let expected = expected.ok_or_else(|| {
            pm_cli::invalid("feature promotion requires --expected-revision and --expected-content")
        })?;
        let target = maturity(to)?;
        repository.promote_feature(
            id,
            &expected,
            target,
            &PolicyAcceptance {
                actor: actor.clone(),
                reason: reason.clone(),
            },
            &request,
        )?
    } else {
        let (id, mutation) = match command {
            FeatureCommand::Update {
                id,
                name,
                fields_file,
                body_file,
            } => {
                if name.is_none() && fields_file.is_none() && body_file.is_none() {
                    return Err(pm_cli::invalid(
                        "feature update requires --name, --fields-file or --body-file",
                    ));
                }
                if fields_file.as_deref() == Some(Path::new("-"))
                    && body_file.as_deref() == Some(Path::new("-"))
                {
                    return Err(pm_cli::invalid("only one input may read stdin"));
                }
                let mut fields: BTreeMap<String, Value> = fields_file
                    .as_ref()
                    .map(|path| pm_cli::typed_input(cwd, path))
                    .transpose()?
                    .unwrap_or_default();
                if let Some(name) = name {
                    fields.insert("name".into(), json!(name));
                }
                (
                    id,
                    FeatureMutation::Update {
                        fields,
                        body: body_file
                            .as_ref()
                            .map(|path| pm_cli::read_input(cwd, path))
                            .transpose()?,
                    },
                )
            }
            FeatureCommand::Parent { id, parent, .. } => (
                id,
                FeatureMutation::Reparent {
                    parent: parent.as_deref().map(str::parse).transpose()?,
                },
            ),
            FeatureCommand::Move { id, directory } => (
                id,
                FeatureMutation::Relocate {
                    directory: directory.clone(),
                },
            ),
            FeatureCommand::Archive { id, restore } => {
                (id, FeatureMutation::Archive { archived: !restore })
            }
            FeatureCommand::Custom { id, patch } => (
                id,
                FeatureMutation::PatchCustom {
                    patch: patch.patch()?,
                },
            ),
            _ => unreachable!(),
        };
        repository.mutate_feature(id, expected.as_ref(), &mutation, &request)?
    };
    pm_cli::emit_mutation(
        repository,
        &options.mutation,
        options.json,
        "feature",
        source,
        &receipt,
    )
}
