//! Deterministic protocol documents and explicit source-bound instruction pointers.
use super::{pm_catalog, pm_cli, pm_protocol_install};
use clap::{Args, Subcommand, ValueEnum};
use std::{io::Write, path::Path};
use workdeck_pm::{ContentHash, ErrorCode, PmError, RepositoryId, RequestId, Result};

#[derive(Debug, Default, Args)]
pub(super) struct ProtocolOptions {
    #[arg(long, global = true)]
    pub json: bool,
    #[arg(long, global = true)]
    pub no_input: bool,
}
#[derive(Debug, Clone, Copy, ValueEnum)]
pub(super) enum DocumentKind {
    Skill,
    Commands,
    Schemas,
}
#[derive(Debug, Args)]
pub(super) struct PointerWriteOptions {
    #[arg(long, help = "Repository identity returned by protocol preview")]
    expected_repository: RepositoryId,
    #[arg(
        long,
        required_unless_present = "expect_absent",
        conflicts_with = "expect_absent",
        help = "Exact AGENTS.md SHA-256 from protocol preview"
    )]
    expected_content: Option<ContentHash>,
    #[arg(
        long,
        required_unless_present = "expected_content",
        conflicts_with = "expected_content",
        help = "Require AGENTS.md to be absent"
    )]
    expect_absent: bool,
    #[arg(
        long,
        help = "Stable request identity; reuse the original value and precondition to resume or replay"
    )]
    request_id: RequestId,
}
#[derive(Debug, Subcommand)]
pub(super) enum ProtocolCommand {
    #[command(
        about = "Render an installed PM protocol document without reading or modifying repository instructions"
    )]
    Render {
        #[arg(value_enum)]
        document: DocumentKind,
    },
    #[command(
        about = "Preview a thin AGENTS.md pointer without modifying repository instructions or recovering interrupted writes"
    )]
    Preview {
        #[arg(long, value_enum, default_value = "install")]
        mode: pm_protocol_install::Mode,
    },
    #[command(
        about = "Explicitly install the thin AGENTS.md pointer in an initialized native repository"
    )]
    Install {
        #[command(flatten)]
        input: PointerWriteOptions,
    },
    #[command(
        about = "Explicitly update an existing managed AGENTS.md pointer, preserving surrounding instructions"
    )]
    Update {
        #[command(flatten)]
        input: PointerWriteOptions,
    },
}

pub(super) fn run(cwd: &Path, options: &ProtocolOptions, command: &ProtocolCommand) -> Result<()> {
    match command {
        ProtocolCommand::Render { document } => {
            let (kind, body) = match document {
                DocumentKind::Skill => ("skill", pm_catalog::render_pm_skill()),
                DocumentKind::Commands => ("commands", pm_catalog::render_pm_commands()),
                DocumentKind::Schemas => ("schemas", pm_catalog::render_pm_schemas()?),
            };
            if options.json {
                pm_cli::emit(
                    true,
                    "protocol_document",
                    &serde_json::json!({"repository":null}),
                    &serde_json::json!({"document":kind,"content":body}),
                    None,
                )
            } else {
                std::io::stdout()
                    .lock()
                    .write_all(body.as_bytes())
                    .map_err(|error| PmError::io("stdout", error))
            }
        }
        ProtocolCommand::Preview { mode } => {
            let preview = pm_protocol_install::preview(cwd, *mode)?;
            pm_cli::emit(
                options.json,
                "protocol_preview",
                &serde_json::json!({"repository":preview.repository,"root":preview.project_root.join(".workdeck")}),
                &serde_json::to_value(&preview)
                    .map_err(|error| PmError::new(ErrorCode::InvalidSchema, error.to_string()))?,
                None,
            )
        }
        ProtocolCommand::Install { input } | ProtocolCommand::Update { input } => {
            if input.expect_absent == input.expected_content.is_some() {
                return Err(PmError::new(
                    ErrorCode::InvalidInput,
                    "supply exactly one of --expected-content or --expect-absent",
                ));
            }
            let mode = if matches!(command, ProtocolCommand::Install { .. }) {
                pm_protocol_install::Mode::Install
            } else {
                pm_protocol_install::Mode::Update
            };
            let receipt = pm_protocol_install::apply(
                cwd,
                mode,
                &input.expected_repository,
                input.expected_content.clone(),
                &input.request_id,
            )?;
            pm_cli::emit(
                options.json,
                "protocol_pointer",
                &serde_json::json!({"repository":receipt.repository,"root":receipt.project_root.join(".workdeck")}),
                &serde_json::to_value(&receipt)
                    .map_err(|error| PmError::new(ErrorCode::InvalidSchema, error.to_string()))?,
                None,
            )
        }
    }
}
