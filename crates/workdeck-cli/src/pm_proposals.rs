//! Reviewed planning proposals publish separately from coordination claims.
use super::{pm_claims::Options, pm_cli};
use clap::Subcommand;
use serde_json::{Value, json};
use std::path::Path;
use workdeck_pm::{sources::ProposalPlan, *};
#[derive(Debug, Subcommand)]
pub(super) enum ProposalCommand {
    Preview {
        #[arg(
            long = "ref",
            help = "Full ref inside the configured proposal namespace"
        )]
        reference: String,
        #[arg(long)]
        title: String,
    },
    Publish {
        #[arg(
            long,
            help = "Saved proposal fingerprint or exact ProposalPlan JSON file"
        )]
        plan: String,
        #[arg(long)]
        expected_plan: String,
        #[arg(long)]
        request_id: String,
    },
    Status {
        #[arg(long)]
        request_id: String,
    },
    Resume {
        #[arg(long, help = "Resume only this request's retained original proposal")]
        request_id: String,
    },
}
impl ProposalCommand {
    pub(super) fn is_mutation(&self) -> bool {
        matches!(self, Self::Publish { .. } | Self::Resume { .. })
    }
}
pub(super) fn run(
    cwd: &Path,
    repository: &Repository,
    source: &Value,
    options: &Options,
    command: &ProposalCommand,
) -> Result<Option<u8>> {
    options.output.validate()?;
    if let ProposalCommand::Preview { reference, title } = command {
        let plan = repository.preview_proposal(&sources::ProposalRequest {
            reference: reference.parse()?,
            title: title.clone(),
        })?;
        let saved = repository.save_proposal_plan(&plan)?;
        options.output.emit(
            options.json,
            "proposal_plan",
            source,
            &json!({"plan":plan,"saved_plan":saved}),
        )?;
        return Ok(None);
    }
    let outcome = match command {
        ProposalCommand::Publish {
            plan,
            expected_plan,
            request_id,
        } => {
            let plan = load(cwd, repository, plan)?;
            if plan.fingerprint != expected_plan.parse()? {
                return Err(PmError::new(
                    ErrorCode::StaleSource,
                    "proposal differs from the reviewed plan fingerprint",
                ));
            }
            repository.publish_proposal(&plan, &request_id.parse()?)?
        }
        ProposalCommand::Status { request_id } => {
            repository.proposal_status(&request_id.parse()?)?
        }
        ProposalCommand::Resume { request_id } => {
            repository.resume_proposal(&request_id.parse()?)?
        }
        ProposalCommand::Preview { .. } => unreachable!(),
    };
    options
        .output
        .emit(options.json, "proposal_outcome", source, &outcome)?;
    Ok(match outcome.state {
        PublicationState::Confirmed => None,
        PublicationState::Uncertain => Some(6),
        PublicationState::Prepared | PublicationState::Rejected => Some(4),
    })
}
fn load(cwd: &Path, repository: &Repository, reference: &str) -> Result<ProposalPlan> {
    let plan = if let Ok(hash) = reference.parse::<ContentHash>() {
        repository.load_proposal_plan(&hash)?
    } else {
        serde_json::from_slice(&pm_cli::read_regular_input(
            cwd,
            Path::new(reference),
            sources::MAX_PROPOSAL_PLAN_BYTES as u64,
        )?)
        .map_err(|e| PmError::new(ErrorCode::InvalidInput, e.to_string()))?
    };
    plan.validate()?;
    Ok(plan)
}
