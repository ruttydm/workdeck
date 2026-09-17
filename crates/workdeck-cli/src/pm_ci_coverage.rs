use super::{
    pm_ci::CiOptions,
    pm_cli::{self, CommandFailure},
};
use clap::Args;
use serde_json::json;
use std::path::{Path, PathBuf};
use workdeck_pm::*;
#[derive(Debug, Args)]
pub(super) struct CoverageArgs {
    #[arg(long)]
    revision: String,
    #[arg(
        long,
        help = "Compare live planning contracts and declared evaluator files with the selected commit"
    )]
    working_tree: bool,
    #[arg(long, help = "issue:ID, feature:ID, gate:ID or planning-kind:ID")]
    subject: Option<String>,
    #[arg(
        long,
        requires = "subject",
        help = "Exact selected document hash, to detect dirty or mismatched selection"
    )]
    expected_subject: Option<ContentHash>,
    #[arg(long, requires_all = ["expected_policy", "expected_base_commit", "expected_base_contract"])]
    policy_file: Option<PathBuf>,
    #[arg(long, requires = "policy_file")]
    expected_policy: Option<ContentHash>,
    #[arg(long, requires = "policy_file")]
    expected_base_commit: Option<GitOid>,
    #[arg(long, requires = "policy_file")]
    expected_base_contract: Option<ContentHash>,
}
fn subject(value: &str) -> workdeck_pm::Result<CiSubjectIdentity> {
    if value.len() > 256 {
        return Err(pm_cli::invalid("review subject exceeds 256 bytes"));
    }
    let (kind, id) = value
        .split_once(':')
        .ok_or_else(|| pm_cli::invalid("review subject requires kind:ID"))?;
    Ok(match kind {
        "issue" => CiSubjectIdentity::Issue { id: id.parse()? },
        "feature" => CiSubjectIdentity::Feature { id: id.parse()? },
        "gate" => CiSubjectIdentity::Gate { id: id.parse()? },
        _ => CiSubjectIdentity::Planning {
            record_kind: serde_json::from_value(json!(kind))
                .map_err(|_| pm_cli::invalid("unknown review subject kind"))?,
            id: id.into(),
        },
    })
}
pub(super) fn run(cwd: &Path, options: &CiOptions, args: &CoverageArgs) -> anyhow::Result<()> {
    let mut source = json!({"repository":null,"root":cwd,"role":"review_coverage"});
    (|| -> workdeck_pm::Result<()> {
        let repo = Repository::open_source(&cwd.join(".workdeck"))?;
        source["repository"] = json!(repo.identity());
        let authority = if let Some(path) = &args.policy_file {
            Some(ReviewCoverageAuthority {
                policy:ContractReviewPolicy::from_json(&pm_cli::read_regular_input(cwd, path, MAX_CONTRACT_REVIEW_POLICY_BYTES as u64)?)?,
                expected_policy:args.expected_policy.clone().ok_or_else(|| pm_cli::invalid("expected policy is required"))?,
                baseline:CiBaselinePin {
                    commit:args.expected_base_commit.clone().ok_or_else(|| pm_cli::invalid("baseline commit is required"))?,
                    contract:args.expected_base_contract.clone().ok_or_else(|| pm_cli::invalid("baseline contract is required"))?,
                },
            })
        } else { None };
        let request = ReviewCoverageRequest { revision:super::pm_ci::revision(&args.revision)?,
            working_tree:args.working_tree, subject:args.subject.as_deref().map(subject).transpose()?, expected_subject:args.expected_subject.clone(), authority };
        let report = repo.contract_review_coverage(&request)?;
        source["revision"] = json!(report.source);
        if request.authority.is_some() && !report.authenticated {
            return Err(PmError::new(ErrorCode::PolicyBlocked, "no retained review authenticates the selected revision and subject under current policy")
                .details(json!({"report":report})));
        }
        pm_cli::emit(options.json, "ci.review_coverage", &source, &report, None)
    })().map_err(|error| CommandFailure { error, source_identity:source })?;
    Ok(())
}
