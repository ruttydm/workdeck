//! Explicit source inspection and refresh; ordinary reads never contact a remote.
use super::pm_claims::Options;
use clap::{Args, Subcommand};
use serde_json::{Value, json};
use std::path::Path;
use workdeck_pm::*;
#[derive(Debug, Args)]
pub(super) struct Refresh {
    #[arg(long, help = "Exact configuration fingerprint from source status")]
    expected_config: String,
    #[arg(long, help = "Exact remote binding fingerprint from source status")]
    expected_binding: String,
    #[arg(long)]
    request_id: String,
}
#[derive(Debug, Subcommand)]
pub(super) enum SourceCommand {
    #[command(about = "Review and publish planning proposals separately from accepted state")]
    Proposal {
        #[command(subcommand)]
        command: super::pm_proposals::ProposalCommand,
    },
    Status,
    Fetch(Refresh),
    Sync(Refresh),
}
impl SourceCommand {
    pub(super) fn is_mutation(&self) -> bool {
        match self {
            Self::Status => false,
            Self::Proposal { command } => command.is_mutation(),
            _ => true,
        }
    }
}
pub(super) fn run(
    cwd: &Path,
    repository: &Repository,
    source: &Value,
    options: &Options,
    command: &SourceCommand,
) -> Result<Option<u8>> {
    options.output.validate()?;
    if let SourceCommand::Proposal { command } = command {
        return super::pm_proposals::run(cwd, repository, source, options, command);
    }
    let (kind, result, exit) = match command {
        SourceCommand::Proposal { .. } => unreachable!(),
        SourceCommand::Status => {
            let status = repository.source_status()?;
            let exit = (!status.errors.is_empty()).then_some(4);
            ("source_status", json!(status), exit)
        }
        SourceCommand::Fetch(input) | SourceCommand::Sync(input) => {
            let request = SourceFetchRequest {
                expected_config: input.expected_config.parse()?,
                expected_binding: input.expected_binding.parse()?,
            };
            let id = input.request_id.parse()?;
            let outcome = match command {
                SourceCommand::Fetch(_) => repository.fetch_sources(&request, &id)?,
                SourceCommand::Sync(_) => repository.sync_sources(&request, &id)?,
                _ => unreachable!(),
            };
            ("source_refresh", json!(outcome), None)
        }
    };
    options.output.emit(options.json, kind, source, &result)?;
    Ok(exit)
}
