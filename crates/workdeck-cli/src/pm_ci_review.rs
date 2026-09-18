//! Exact candidate review admission using externally pinned reviewer authority.
use super::{
    pm_ci::CiOptions,
    pm_cli::{self, CommandFailure},
};
use clap::Args;
use serde_json::json;
use std::path::{Path, PathBuf};
use workdeck_pm::*;

#[derive(Debug, Args)]
pub(super) struct ReviewArgs {
    #[arg(long)]
    base: String,
    #[arg(long)]
    head: String,
    #[arg(long)]
    expected_base_commit: GitOid,
    #[arg(long)]
    expected_base_contract: ContentHash,
    #[arg(long)]
    policy_file: PathBuf,
    #[arg(
        long,
        help = "Reviewer policy fingerprint obtained independently of candidate files"
    )]
    expected_policy: ContentHash,
    #[arg(
        long,
        help = "Original DSSE envelope signed by every required reviewer"
    )]
    review_file: PathBuf,
}

pub(super) fn run(cwd: &Path, options: &CiOptions, args: &ReviewArgs) -> anyhow::Result<()> {
    let mut source = json!({"repository":null,"root":cwd,"role":"contract_review_validation"});
    (|| -> workdeck_pm::Result<()> {
        let policy = ContractReviewPolicy::from_json(&pm_cli::read_regular_input(
            cwd,
            &args.policy_file,
            MAX_CONTRACT_REVIEW_POLICY_BYTES as u64,
        )?)?;
        let envelope = SignedContractReview::from_json(&pm_cli::read_regular_input(
            cwd,
            &args.review_file,
            MAX_CONTRACT_REVIEW_BYTES as u64,
        )?)?;
        let request = CiValidateRequest {
            base: super::pm_ci::revision(&args.base)?,
            head: super::pm_ci::revision(&args.head)?,
        };
        let baseline = CiBaselinePin {
            commit: args.expected_base_commit.clone(),
            contract: args.expected_base_contract.clone(),
        };
        let report = ci_validate_reviewed(
            cwd,
            &request,
            &baseline,
            &policy,
            &args.expected_policy,
            &envelope,
            chrono::Utc::now(),
        )?;
        source["repository"] = json!(report.validation.head.repository);
        source["head"] = json!(report.validation.head);
        if !report.valid {
            return Err(PmError::new(
                ErrorCode::PolicyBlocked,
                "reviewed candidate fails planning or organization validation",
            )
            .details(json!({"report":report})));
        }
        pm_cli::emit(options.json, "ci.validate_reviewed", &source, &report, None)
    })()
    .map_err(|error| CommandFailure {
        error,
        source_identity: source,
    })?;
    Ok(())
}

pub(super) fn policy(cwd: &Path, options: &CiOptions, path: &Path) -> anyhow::Result<()> {
    let source = json!({"repository":null,"root":cwd,"role":"contract_review_policy"});
    (|| -> workdeck_pm::Result<()> {
        let policy = ContractReviewPolicy::from_json(&pm_cli::read_regular_input(
            cwd,
            path,
            MAX_CONTRACT_REVIEW_POLICY_BYTES as u64,
        )?)?;
        pm_cli::emit(
            options.json,
            "ci.review_policy",
            &source,
            &json!({"fingerprint":policy.fingerprint()?,"policy":policy}),
            None,
        )
    })()
    .map_err(|error| CommandFailure {
        error,
        source_identity: source,
    })?;
    Ok(())
}
