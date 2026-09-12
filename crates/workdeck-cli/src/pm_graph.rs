//! Issue relationship commands use the shared snapshot and semantic mutation APIs.
use super::pm_cli;
use clap::Subcommand;
use serde_json::Value;
use workdeck_pm::{IssueGraphMutation, Repository, RequestId, Result};

#[derive(Debug, Subcommand)]
pub(super) enum PrerequisiteCommand {
    Add {
        prerequisite: String,
    },
    #[command(
        about = "Resolve a requirement explicitly, retaining the reason in operation history"
    )]
    Remove {
        prerequisite: String,
        #[arg(long)]
        reason: String,
    },
    Replace {
        prerequisite: String,
        replacement: String,
        #[arg(long)]
        reason: String,
    },
    #[command(about = "Record a source-bound waiver when repository acceptance policy permits it")]
    Waive {
        prerequisite: String,
        #[arg(long)]
        actor: String,
        #[arg(long)]
        reason: String,
    },
    RevokeWaiver {
        prerequisite: String,
        #[arg(long)]
        reason: String,
    },
}

#[derive(Debug, Subcommand)]
pub(super) enum GraphCommand {
    #[command(
        about = "Set or clear an issue's parent without weakening completed-parent requirements"
    )]
    Parent {
        key: String,
        #[arg(required_unless_present = "clear", conflicts_with = "clear")]
        parent: Option<String>,
        #[arg(long)]
        clear: bool,
        #[arg(long)]
        expected_graph: Option<String>,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Add, resolve, replace or explicitly waive a hard prerequisite")]
    Prerequisite {
        key: String,
        #[command(subcommand)]
        command: PrerequisiteCommand,
        #[arg(long, global = true)]
        expected_graph: Option<String>,
        #[arg(long, global = true)]
        json: bool,
    },
    #[command(about = "Add one canonical symmetric related link; this does not block readiness")]
    Relate {
        key: String,
        other: String,
        #[arg(long)]
        expected_graph: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Unrelate {
        key: String,
        other: String,
        #[arg(long)]
        expected_graph: Option<String>,
        #[arg(long)]
        json: bool,
    },
    #[command(
        about = "Inspect parents, children, prerequisites, dependents and related issues from one snapshot"
    )]
    Relations {
        key: String,
        #[arg(long)]
        json: bool,
    },
    #[command(
        about = "Explain hard-prerequisite readiness, including unresolved and canceled requirements"
    )]
    Ready {
        key: String,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Find a directed hard-prerequisite path; this is not a calendar forecast")]
    DependencyPath {
        from: String,
        to: String,
        #[arg(long)]
        json: bool,
    },
}

impl GraphCommand {
    pub(super) fn wants_json(&self) -> bool {
        match self {
            Self::Parent { json, .. }
            | Self::Prerequisite { json, .. }
            | Self::Relate { json, .. }
            | Self::Unrelate { json, .. }
            | Self::Relations { json, .. }
            | Self::Ready { json, .. }
            | Self::DependencyPath { json, .. } => *json,
        }
    }

    pub(super) fn is_mutation(&self) -> bool {
        !matches!(
            self,
            Self::Relations { .. } | Self::Ready { .. } | Self::DependencyPath { .. }
        )
    }
}

pub(super) fn run(
    repository: &Repository,
    source: &Value,
    options: &pm_cli::IssueOptions,
    command: &GraphCommand,
) -> Result<()> {
    let json = command.wants_json();
    if !command.is_mutation() {
        pm_cli::read_options(options)?;
        return match command {
            GraphCommand::Relations { key, .. } => pm_cli::emit(
                json,
                "issue_relations",
                source,
                &repository.issue_relations(key)?,
                None,
            ),
            GraphCommand::Ready { key, .. } => pm_cli::emit(
                json,
                "issue_readiness",
                source,
                &repository.issue_readiness(key)?,
                None,
            ),
            GraphCommand::DependencyPath { from, to, .. } => pm_cli::emit(
                json,
                "issue_dependency_path",
                source,
                &repository.issue_dependency_path(from, to)?,
                None,
            ),
            _ => unreachable!(),
        };
    }
    let (key, expected_graph, mutation) = match command {
        GraphCommand::Parent {
            key,
            parent,
            expected_graph,
            ..
        } => (
            key,
            expected_graph,
            IssueGraphMutation::SetParent {
                parent: parent.clone(),
            },
        ),
        GraphCommand::Prerequisite {
            key,
            command,
            expected_graph,
            ..
        } => {
            let mutation = match command {
                PrerequisiteCommand::Add { prerequisite } => IssueGraphMutation::AddPrerequisite {
                    prerequisite: prerequisite.clone(),
                },
                PrerequisiteCommand::Remove {
                    prerequisite,
                    reason,
                } => IssueGraphMutation::RemovePrerequisite {
                    prerequisite: prerequisite.clone(),
                    reason: reason.clone(),
                },
                PrerequisiteCommand::Replace {
                    prerequisite,
                    replacement,
                    reason,
                } => IssueGraphMutation::ReplacePrerequisite {
                    prerequisite: prerequisite.clone(),
                    replacement: replacement.clone(),
                    reason: reason.clone(),
                },
                PrerequisiteCommand::Waive {
                    prerequisite,
                    actor,
                    reason,
                } => IssueGraphMutation::WaivePrerequisite {
                    prerequisite: prerequisite.clone(),
                    actor: actor.clone(),
                    reason: reason.clone(),
                },
                PrerequisiteCommand::RevokeWaiver {
                    prerequisite,
                    reason,
                } => IssueGraphMutation::RevokeWaiver {
                    prerequisite: prerequisite.clone(),
                    reason: reason.clone(),
                },
            };
            (key, expected_graph, mutation)
        }
        GraphCommand::Relate {
            key,
            other,
            expected_graph,
            ..
        } => (
            key,
            expected_graph,
            IssueGraphMutation::SetRelated {
                other: other.clone(),
                related: true,
            },
        ),
        GraphCommand::Unrelate {
            key,
            other,
            expected_graph,
            ..
        } => (
            key,
            expected_graph,
            IssueGraphMutation::SetRelated {
                other: other.clone(),
                related: false,
            },
        ),
        _ => unreachable!(),
    };
    let expected = pm_cli::expected(options)?;
    let expected_graph = expected_graph.as_deref().map(str::parse).transpose()?;
    let request = options
        .request_id
        .as_deref()
        .map(str::parse)
        .transpose()?
        .unwrap_or_else(RequestId::new);
    let receipt = repository.mutate_issue_graph(
        key,
        expected.as_ref(),
        expected_graph.as_ref(),
        &mutation,
        &request,
    )?;
    pm_cli::emit_mutation(repository, options, json, "issue_graph", source, &receipt)
}
