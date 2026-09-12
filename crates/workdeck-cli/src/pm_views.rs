//! Saved-view authoring and live evaluation through shared PM queries.
use super::pm_cli;
use clap::{Args, Subcommand};
use serde_json::Value;
use std::path::{Path, PathBuf};
use workdeck_pm::{ErrorCode, PmError, Repository, RequestId, Result, WriteSavedView};
#[derive(Debug, Default, Args)]
pub(super) struct ViewOptions {
    #[arg(long, global = true)]
    pub json: bool,
    #[arg(long, global = true)]
    request_id: Option<String>,
    #[arg(long, global = true)]
    stage: bool,
}
#[derive(Debug, Args)]
pub(super) struct Definition {
    #[arg(long)]
    name: String,
    #[arg(
        long,
        required_unless_present = "query_file",
        conflicts_with = "query_file",
        help = "Exact IssueQuery JSON; {} selects active issues"
    )]
    query: Option<String>,
    #[arg(long, help = "Read IssueQuery JSON from a file or '-' for stdin")]
    query_file: Option<PathBuf>,
    #[arg(
        long,
        help = "Archive this definition without changing its issue predicate"
    )]
    archived: bool,
}
#[derive(Debug, Subcommand)]
pub(super) enum ViewCommand {
    List,
    Show {
        id: String,
    },
    #[command(about = "Evaluate the saved query and issue records from one source snapshot")]
    Run {
        id: String,
    },
    Create {
        id: String,
        #[command(flatten)]
        definition: Definition,
    },
    Update {
        id: String,
        #[command(flatten)]
        definition: Definition,
        #[arg(long)]
        expected_content: String,
    },
}
pub(super) fn run(
    cwd: &Path,
    repository: &Repository,
    source: &Value,
    options: &ViewOptions,
    command: &ViewCommand,
) -> Result<()> {
    match command {
        ViewCommand::List | ViewCommand::Show { .. } | ViewCommand::Run { .. } => {
            if options.request_id.is_some() || options.stage {
                return Err(PmError::new(
                    ErrorCode::InvalidInput,
                    "read-only view commands do not accept mutation options",
                ));
            }
            match command {
                ViewCommand::List => pm_cli::emit(
                    options.json,
                    "saved_views",
                    source,
                    &repository.saved_views()?,
                    None,
                ),
                ViewCommand::Show { id } => pm_cli::emit(
                    options.json,
                    "saved_view",
                    source,
                    &repository.saved_view(id)?,
                    None,
                ),
                ViewCommand::Run { id } => pm_cli::emit(
                    options.json,
                    "saved_view_result",
                    source,
                    &repository.query_saved_view(id)?,
                    None,
                ),
                _ => unreachable!(),
            }
        }
        ViewCommand::Create { id, definition } | ViewCommand::Update { id, definition, .. } => {
            let query = match &definition.query_file {
                Some(path) => pm_cli::read_input(cwd, path)?,
                None => definition.query.clone().ok_or_else(|| {
                    PmError::new(
                        ErrorCode::InvalidInput,
                        "view authoring requires --query or --query-file",
                    )
                })?,
            };
            let query = serde_json::from_str(&query).map_err(|e| {
                PmError::new(
                    ErrorCode::InvalidInput,
                    format!("invalid saved IssueQuery: {e}"),
                )
            })?;
            let expected = match command {
                ViewCommand::Update {
                    expected_content, ..
                } => Some(expected_content.parse()?),
                _ => None,
            };
            let request = options
                .request_id
                .as_deref()
                .map(str::parse)
                .transpose()?
                .unwrap_or_else(RequestId::new);
            let receipt = repository.write_saved_view(
                &WriteSavedView {
                    id: id.clone(),
                    name: definition.name.clone(),
                    query,
                    archived: definition.archived,
                    expected,
                },
                &request,
            )?;
            pm_cli::emit_mutation(
                repository,
                &pm_cli::IssueOptions {
                    stage: options.stage,
                    ..Default::default()
                },
                options.json,
                "saved_view",
                source,
                &receipt,
            )
        }
    }
}
