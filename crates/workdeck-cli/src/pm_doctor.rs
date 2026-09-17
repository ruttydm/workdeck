//! Candidate validation is dispatched before any working-tree source discovery.
use super::pm_cli::{self, CommandFailure};
use clap::Args;
use serde_json::json;
use std::path::{Path, PathBuf};
use workdeck_pm::{IndexSelection, StagedDoctorRequest};

#[derive(Debug, Default, Args)]
pub(super) struct DoctorOptions {
    #[arg(
        long,
        help = "Validate the actual Git candidate index and its planning relation closure"
    )]
    pub staged: bool,
    #[arg(
        long,
        requires = "staged",
        help = "Explicit candidate index; otherwise honor the hook's GIT_INDEX_FILE"
    )]
    pub index: Option<PathBuf>,
    #[arg(
        long,
        requires = "staged",
        help = "Reject an index different from this reviewed content hash"
    )]
    pub expected_index: Option<String>,
}

pub(super) fn run(cwd: &Path, options: &DoctorOptions, json_output: bool) -> anyhow::Result<()> {
    let initial = json!({"repository":null,"root":cwd,"role":"staged"});
    let request = (|| -> workdeck_pm::Result<StagedDoctorRequest> {
        Ok(StagedDoctorRequest {
            index: options
                .index
                .as_ref()
                .map(|path| IndexSelection::Explicit { path: path.clone() })
                .unwrap_or(IndexSelection::EffectiveHook),
            expected_index: options
                .expected_index
                .as_deref()
                .map(str::parse)
                .transpose()?,
        })
    })()
    .map_err(|error| CommandFailure {
        error,
        source_identity: initial.clone(),
    })?;
    let outcome = workdeck_pm::doctor_staged(cwd, &request).map_err(|error| CommandFailure {
        error,
        source_identity: initial,
    })?;
    let source = json!({"repository":outcome.source.identity.repository,"root":cwd,
        "role":"staged","observation":outcome.source});
    if !outcome.report.valid {
        let hints = outcome
            .report
            .errors
            .iter()
            .take(8)
            .map(|error| error.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        return Err(CommandFailure {
            error: pm_cli::invalid(
                "staged planning validation found invalid authoritative records",
            )
            .hint(hints)
            .details(json!({"report":outcome.report,"source":outcome.source})),
            source_identity: source,
        }
        .into());
    }
    pm_cli::emit(json_output, "doctor.staged", &source, &outcome.report, None).map_err(|error| {
        CommandFailure {
            error,
            source_identity: source,
        }
        .into()
    })
}
