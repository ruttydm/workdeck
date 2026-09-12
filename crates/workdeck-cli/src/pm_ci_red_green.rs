use super::{
    pm_ci::CiOptions,
    pm_cli::{self, CommandFailure},
};
use clap::Args;
use serde_json::json;
use std::path::{Path, PathBuf};
use workdeck_pm::*;
#[derive(Debug, Args)]
pub(super) struct RedGreenArgs {
    #[arg(long)]
    red_report_file: PathBuf,
    #[arg(long)]
    green_report_file: PathBuf,
    #[arg(long)]
    red_artifact_file: PathBuf,
    #[arg(long)]
    green_artifact_file: PathBuf,
    #[arg(long)]
    policy_file: PathBuf,
    #[arg(long)]
    expected_policy: ContentHash,
    #[arg(long)]
    expected_base_commit: GitOid,
    #[arg(long)]
    expected_base_contract: ContentHash,
    #[arg(long)]
    revision: GitOid,
    #[arg(long)]
    check: String,
    /// Original signed review admitting the red baseline from prior acceptance.
    #[arg(long, requires_all = ["review_policy_file", "expected_review_policy", "accepted_commit", "accepted_contract"])]
    baseline_review_file: Option<PathBuf>,
    #[arg(long, requires = "baseline_review_file")]
    review_policy_file: Option<PathBuf>,
    #[arg(long, requires = "baseline_review_file")]
    expected_review_policy: Option<ContentHash>,
    #[arg(long, requires = "baseline_review_file")]
    accepted_commit: Option<GitOid>,
    #[arg(long, requires = "baseline_review_file")]
    accepted_contract: Option<ContentHash>,
}
pub(super) fn run(cwd: &Path, options: &CiOptions, args: &RedGreenArgs) -> anyhow::Result<()> {
    let mut source = json!({"repository":null,"root":cwd,"role":"red_green_verification"});
    (|| -> workdeck_pm::Result<()> {
        let read = |path: &Path, max: usize| pm_cli::read_regular_input(cwd, path, max as u64);
        let xml = |path: &Path| {
            String::from_utf8(read(path, MAX_REPORT_BYTES)?)
                .map_err(|_| pm_cli::invalid("JUnit artifacts must use UTF-8"))
        };
        let request = RedGreenRequest {
            baseline: CiBaselinePin {
                commit: args.expected_base_commit.clone(),
                contract: args.expected_base_contract.clone(),
            },
            candidate: args.revision.clone(),
            check: args.check.clone(),
            producer_policy: ProducerTrustPolicy::from_json(&read(
                &args.policy_file,
                MAX_PRODUCER_POLICY_BYTES,
            )?)?,
            expected_producer_policy: args.expected_policy.clone(),
            red: SignedCheckReport::from_json(&read(
                &args.red_report_file,
                MAX_SIGNED_REPORT_BYTES,
            )?)?,
            green: SignedCheckReport::from_json(&read(
                &args.green_report_file,
                MAX_SIGNED_REPORT_BYTES,
            )?)?,
            red_artifact: xml(&args.red_artifact_file)?,
            green_artifact: xml(&args.green_artifact_file)?,
        };
        if let Some(envelope_path) = &args.baseline_review_file {
            let missing = || {
                pm_cli::invalid(
                    "reviewed red/green requires all independent reviewer and prior-baseline pins",
                )
            };
            let authority = RedGreenBaselineReview {
                accepted: CiBaselinePin {
                    commit: args.accepted_commit.clone().ok_or_else(missing)?,
                    contract: args.accepted_contract.clone().ok_or_else(missing)?,
                },
                policy: ContractReviewPolicy::from_json(&read(
                    args.review_policy_file.as_deref().ok_or_else(missing)?,
                    MAX_CONTRACT_REVIEW_POLICY_BYTES,
                )?)?,
                expected_policy: args.expected_review_policy.clone().ok_or_else(missing)?,
                envelope: SignedContractReview::from_json(&read(
                    envelope_path,
                    MAX_CONTRACT_REVIEW_BYTES,
                )?)?,
            };
            let assessment = verify_reviewed_red_green(cwd, &request, &authority)?;
            source["repository"] = json!(assessment.pair.repository);
            source["revision"] = json!(assessment.pair.candidate);
            return pm_cli::emit(
                options.json,
                "ci.red_green_reviewed",
                &source,
                &assessment,
                None,
            );
        }
        let assessment = verify_red_green(cwd, &request)?;
        source["repository"] = json!(assessment.repository);
        source["revision"] = json!(assessment.candidate);
        pm_cli::emit(options.json, "ci.red_green", &source, &assessment, None)
    })()
    .map_err(|error| CommandFailure {
        error,
        source_identity: source,
    })?;
    Ok(())
}
