//! Immutable declarations and explicit verification of original attested check links.
use super::pm_cli;
use clap::Subcommand;
use serde_json::Value;
use std::path::{Path, PathBuf};
use workdeck_pm::{
    DeclareEvidence, EvidenceQuery, EvidenceSupersession, Repository, RequestId, Result,
};

#[derive(Debug, Subcommand)]
pub(super) enum EvidenceCommand {
    #[command(about = "List active declared evidence; query JSON can include superseded history")]
    List {
        #[arg(long)]
        query_file: Option<PathBuf>,
    },
    Show {
        id: String,
    },
    #[command(
        about = "Reverify a pinned criterion-to-attestation link with independent current authority; does not complete work"
    )]
    VerifyRedGreen {
        id: String,
        #[arg(long)]
        expected_evidence_content: workdeck_pm::ContentHash,
        #[arg(long)]
        authority_file: PathBuf,
    },
    #[command(
        about = "Append DeclareEvidence JSON with exact source and provenance; '-' reads stdin"
    )]
    Declare {
        input: PathBuf,
    },
    #[command(
        about = "Append a correction of the exact inspected record, retaining immutable history"
    )]
    Supersede {
        id: String,
        input: PathBuf,
        #[arg(long)]
        expected_evidence_content: String,
        #[arg(long)]
        reason: String,
    },
}
impl EvidenceCommand {
    pub(super) fn is_mutation(&self) -> bool {
        matches!(self, Self::Declare { .. } | Self::Supersede { .. })
    }
}
pub(super) fn run(
    cwd: &Path,
    repository: &Repository,
    source: &Value,
    options: &pm_cli::NativeOptions,
    command: &EvidenceCommand,
) -> Result<()> {
    if !command.is_mutation() {
        pm_cli::read_options(&options.mutation)?;
        return match command {
            EvidenceCommand::List { query_file } => {
                let query: EvidenceQuery = query_file
                    .as_ref()
                    .map(|path| pm_cli::typed_input(cwd, path))
                    .transpose()?
                    .unwrap_or_default();
                pm_cli::emit(
                    options.json,
                    "evidence_references",
                    source,
                    &repository.evidence_references(&query)?,
                    None,
                )
            }
            EvidenceCommand::VerifyRedGreen {
                id,
                expected_evidence_content,
                authority_file,
            } => {
                let bytes = pm_cli::read_regular_input(cwd, authority_file, 1024 * 1024)?;
                let authority =
                    serde_json::from_slice::<workdeck_pm::RetainedRedGreenAuthority>(&bytes)
                        .map_err(|error| {
                            pm_cli::invalid(format!("invalid current evidence authority: {error}"))
                        })?;
                let assessment = repository.verify_red_green_evidence(
                    &workdeck_pm::RedGreenEvidenceRequest {
                        evidence: id.parse()?,
                        expected_evidence: expected_evidence_content.clone(),
                        authority,
                    },
                )?;
                pm_cli::emit(
                    options.json,
                    "evidence.red_green",
                    source,
                    &assessment,
                    None,
                )
            }
            EvidenceCommand::Show { id } => pm_cli::emit(
                options.json,
                "evidence_reference",
                source,
                &repository.evidence(&id.parse()?)?,
                None,
            ),
            _ => unreachable!(),
        };
    }
    if pm_cli::expected(&options.mutation)?.is_some() {
        return Err(pm_cli::invalid(
            "immutable evidence has no record revision; supersede uses --expected-evidence-content",
        ));
    }
    let request = options
        .mutation
        .request_id
        .as_deref()
        .map(str::parse)
        .transpose()?
        .unwrap_or_else(RequestId::new);
    let input = match command {
        EvidenceCommand::Declare { input } => pm_cli::typed_input(cwd, input)?,
        EvidenceCommand::Supersede {
            id,
            input,
            expected_evidence_content,
            reason,
        } => {
            let mut input: DeclareEvidence = pm_cli::typed_input(cwd, input)?;
            let supersedes = EvidenceSupersession {
                id: id.parse()?,
                content: expected_evidence_content.parse()?,
                reason: reason.clone(),
            };
            if input
                .supersedes
                .as_ref()
                .is_some_and(|value| value != &supersedes)
            {
                return Err(pm_cli::invalid(
                    "input supersedes conflicts with the explicit correction target",
                ));
            }
            input.supersedes = Some(supersedes);
            input
        }
        _ => unreachable!(),
    };
    let receipt = repository.declare_evidence(&input, &request)?;
    pm_cli::emit_mutation(
        repository,
        &options.mutation,
        options.json,
        "evidence_reference",
        source,
        &receipt,
    )
}
