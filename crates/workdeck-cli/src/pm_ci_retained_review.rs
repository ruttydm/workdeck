use super::{
    pm_ci::{CiCommand, CiOptions},
    pm_cli::{self, CommandFailure},
};
use clap::Args;
use serde_json::json;
use std::path::{Path, PathBuf};
use workdeck_pm::*;

#[derive(Debug, Args)]
pub(super) struct AdmissionArgs {
    #[arg(long)]
    policy_file: PathBuf,
    #[arg(long)]
    expected_policy: ContentHash,
    #[arg(long)]
    expected_base_commit: GitOid,
    #[arg(long)]
    expected_base_contract: ContentHash,
    #[arg(long)]
    expected_commit: GitOid,
}
impl AdmissionArgs {
    fn baseline(&self) -> CiBaselinePin {
        CiBaselinePin {
            commit: self.expected_base_commit.clone(),
            contract: self.expected_base_contract.clone(),
        }
    }
    fn policy(&self, cwd: &Path) -> workdeck_pm::Result<ContractReviewPolicy> {
        ContractReviewPolicy::from_json(&pm_cli::read_regular_input(
            cwd,
            &self.policy_file,
            MAX_CONTRACT_REVIEW_POLICY_BYTES as u64,
        )?)
    }
}
#[derive(Debug, Args)]
pub(super) struct ImportArgs {
    #[command(flatten)]
    admission: AdmissionArgs,
    #[arg(long)]
    review_file: PathBuf,
    #[arg(long)]
    actor: String,
    #[arg(long)]
    request_id: RequestId,
}
#[derive(Debug, Args)]
pub(super) struct ReauthenticateArgs {
    id: ContractReviewId,
    #[command(flatten)]
    admission: AdmissionArgs,
}
pub(super) fn run(cwd: &Path, options: &CiOptions, command: &CiCommand) -> anyhow::Result<()> {
    let mut source = json!({"repository":null,"root":cwd,"role":"retained_contract_review"});
    (|| -> workdeck_pm::Result<()> {
        let repo = Repository::open_source(&cwd.join(".workdeck"))?;
        source["repository"] = json!(repo.identity());
        match command {
            CiCommand::ImportReview(args) => {
                let envelope = String::from_utf8(pm_cli::read_regular_input(cwd, &args.review_file, MAX_CONTRACT_REVIEW_BYTES as u64)?)
                    .map_err(|_| pm_cli::invalid("signed review must be UTF-8"))?;
                let input = ImportContractReviewRequest {
                    envelope, policy:args.admission.policy(cwd)?, expected_policy:args.admission.expected_policy.clone(),
                    baseline:args.admission.baseline(), expected_commit:args.admission.expected_commit.clone(), actor:args.actor.clone(),
                };
                let receipt = repo.import_contract_review(&input, &args.request_id)?;
                pm_cli::emit(options.json, "ci.import_review", &source, &receipt.result, Some(&receipt))
                    .map_err(|error| error.details(json!({"mutation_committed":true,"request_id":args.request_id,"receipt":receipt})))
            }
            CiCommand::Reviews => pm_cli::emit(options.json, "ci.reviews", &source, &repo.imported_contract_review_summaries()?, None),
            CiCommand::Review { id } => pm_cli::emit(options.json, "ci.review", &source, &repo.imported_contract_review(id)?, None),
            CiCommand::ReauthenticateReview(args) => {
                let report = repo.reauthenticate_contract_review(&args.id, &args.admission.policy(cwd)?,
                    &args.admission.expected_policy, &args.admission.baseline(), &args.admission.expected_commit)?;
                if !report.valid {
                    return Err(PmError::new(ErrorCode::PolicyBlocked, "reviewed candidate fails current planning validation").details(json!({"report":report})));
                }
                pm_cli::emit(options.json, "ci.reauthenticate_review", &source, &report, None)
            }
            _ => unreachable!("only retained review commands use this dispatch"),
        }
    })().map_err(|error| CommandFailure { error, source_identity:source })?;
    Ok(())
}
