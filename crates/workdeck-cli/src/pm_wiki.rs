//! Plain Markdown wiki commands backed by the shared file transaction engine.
use super::pm_cli;
use clap::{Args, Subcommand};
use serde_json::Value;
use std::path::{Path, PathBuf};
use workdeck_pm::{ErrorCode, PmError, Repository, RequestId, Result, WriteWiki};

#[derive(Debug, Default, Args)]
pub(super) struct WikiOptions {
    #[arg(long, global = true)]
    pub json: bool,
    #[arg(long, global = true)]
    request_id: Option<String>,
    #[arg(
        long,
        global = true,
        help = "Stage this exact operation and its receipt"
    )]
    stage: bool,
}

#[derive(Debug, Args)]
pub(super) struct Body {
    #[arg(
        long,
        required_unless_present = "body_file",
        conflicts_with = "body_file"
    )]
    body: Option<String>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Read exact Markdown from a file or '-' for stdin"
    )]
    body_file: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub(super) enum WikiCommand {
    #[command(about = "List plain Markdown wiki documents and exact content identities")]
    List,
    #[command(about = "Read inert Markdown without opening links or executing content")]
    Show { path: String },
    #[command(about = "Create a new wiki document without replacing existing content")]
    Create {
        path: String,
        #[command(flatten)]
        body: Body,
    },
    #[command(about = "Replace a wiki document only at its expected content hash")]
    Update {
        path: String,
        #[command(flatten)]
        body: Body,
        #[arg(long, value_name = "SHA256")]
        expected_content: String,
    },
}

pub(super) fn run(
    cwd: &Path,
    repository: &Repository,
    source: &Value,
    options: &WikiOptions,
    command: &WikiCommand,
) -> Result<()> {
    match command {
        WikiCommand::List | WikiCommand::Show { .. } => {
            if options.request_id.is_some() || options.stage {
                return Err(PmError::new(
                    ErrorCode::InvalidInput,
                    "read-only wiki commands do not accept mutation options",
                ));
            }
            match command {
                WikiCommand::List => pm_cli::emit(
                    options.json,
                    "wiki_documents",
                    source,
                    &repository.wiki_documents()?,
                    None,
                ),
                WikiCommand::Show { path } => pm_cli::emit(
                    options.json,
                    "wiki_document",
                    source,
                    &repository.wiki_document(path)?,
                    None,
                ),
                _ => unreachable!(),
            }
        }
        WikiCommand::Create { path, body } | WikiCommand::Update { path, body, .. } => {
            let request = options
                .request_id
                .as_deref()
                .map(str::parse)
                .transpose()?
                .unwrap_or_else(RequestId::new);
            let expected = match command {
                WikiCommand::Update {
                    expected_content, ..
                } => Some(expected_content.parse()?),
                _ => None,
            };
            let body = match &body.body_file {
                Some(path) => pm_cli::read_input(cwd, path)?,
                None => body.body.clone().ok_or_else(|| {
                    PmError::new(
                        ErrorCode::InvalidInput,
                        "wiki authoring requires --body or --body-file",
                    )
                })?,
            };
            let receipt = repository.write_wiki(
                &WriteWiki {
                    path: path.clone(),
                    body,
                    expected,
                },
                &request,
            )?;
            let staging_options = pm_cli::IssueOptions {
                stage: options.stage,
                ..Default::default()
            };
            pm_cli::emit_mutation(
                repository,
                &staging_options,
                options.json,
                "wiki_document",
                source,
                &receipt,
            )
        }
    }
}
