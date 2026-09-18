//! Read-only CI validation dispatch precedes working-tree config discovery.
use super::pm_cli::{self, CommandFailure};
use clap::{Args, Subcommand};
use serde_json::json;
use std::path::{Path, PathBuf};
use workdeck_pm::{CiRevision, CiValidateRequest, ErrorCode, GitOid, PmError};

#[derive(Debug, Default, Args)]
pub(super) struct CiOptions {
    #[arg(long, global = true, help = "Print source-bound CI output as JSON")]
    pub json: bool,
}

#[derive(Debug, Subcommand)]
pub(super) enum CiCommand {
    #[command(
        about = "Verify signed assertion failure/pass evidence under an accepted red baseline; does not complete work"
    )]
    RedGreen(super::pm_ci_red_green::RedGreenArgs),
    #[command(about = "Assess retained review coverage for an exact revision and optional subject")]
    ReviewCoverage(super::pm_ci_coverage::CoverageArgs),
    #[command(about = "Retain an authenticated contract review with durable replay")]
    ImportReview(super::pm_ci_retained_review::ImportArgs),
    #[command(about = "List historical contract-review summaries without renewing trust")]
    Reviews,
    #[command(about = "Read the original retained contract-review proof")]
    Review { id: workdeck_pm::ContractReviewId },
    #[command(
        about = "Revalidate retained review proof against independently pinned current authority"
    )]
    ReauthenticateReview(super::pm_ci_retained_review::ReauthenticateArgs),
    #[command(
        about = "Validate an exact candidate with authenticated contract review; does not qualify checks or completion"
    )]
    ValidateReviewed(super::pm_ci_review::ReviewArgs),
    #[command(about = "Inspect a required reviewer policy without establishing trust")]
    ReviewPolicy {
        #[arg(long)]
        policy_file: PathBuf,
    },
    #[command(about = "Import a signed report with durable replay; does not qualify completion")]
    ImportReport {
        #[arg(
            long,
            help = "RetainedRedGreenProof JSON; original red report, artifacts and optional signed baseline review"
        )]
        red_green_file: Option<PathBuf>,
        #[arg(long)]
        report_file: PathBuf,
        #[arg(long)]
        policy_file: PathBuf,
        #[arg(long)]
        expected_policy: String,
        #[arg(long)]
        expected_commit: String,
        #[arg(long)]
        actor: String,
        #[arg(long)]
        request_id: String,
    },
    #[command(about = "List retained historical report imports without renewing trust")]
    Reports,
    #[command(about = "Read a retained historical report import")]
    Report { id: String },
    #[command(
        about = "Reauthenticate retained signed bytes under an independently pinned current policy"
    )]
    Reauthenticate {
        id: String,
        #[arg(long)]
        policy_file: PathBuf,
        #[arg(long)]
        expected_policy: String,
        #[arg(long)]
        expected_commit: String,
    },
    #[command(
        about = "Reverify retained red/green proof with independent current baseline and producer/reviewer authority"
    )]
    ReauthenticateRedGreen {
        id: workdeck_pm::AttestationId,
        #[arg(
            long,
            help = "RetainedRedGreenAuthority JSON obtained independently of the retained import"
        )]
        authority_file: PathBuf,
    },
    #[command(
        about = "Qualify a pinned imported check against current HEAD, inputs and independent authority"
    )]
    VerifyImportedCheck {
        /// VerifyImportedCheck JSON with current authority and exact attestation selection.
        input: PathBuf,
    },
    #[command(about = "Inspect a producer policy and its fingerprint without establishing trust")]
    Policy {
        #[arg(long)]
        policy_file: PathBuf,
    },
    #[command(
        about = "Authenticate a DSSE report against an explicit producer policy and commit; does not qualify completion"
    )]
    Authenticate {
        #[arg(long)]
        report_file: PathBuf,
        #[arg(long)]
        policy_file: PathBuf,
        #[arg(
            long,
            help = "Policy fingerprint supplied independently of the candidate and signed report"
        )]
        expected_policy: String,
        #[arg(
            long,
            help = "Exact expected commit ID; revision expressions are not accepted"
        )]
        expected_commit: String,
    },
    #[command(
        about = "Validate committed planning and compare required check and subject acceptance contracts; does not execute checks or qualify completion"
    )]
    Validate {
        #[arg(
            long,
            help = "Baseline exact commit ID, full ref, local branch name, or HEAD; selection does not establish trust"
        )]
        base: String,
        #[arg(
            long,
            help = "Candidate exact commit ID, full ref, local branch name, or HEAD"
        )]
        head: String,
        #[arg(
            long,
            requires = "expected_base_contract",
            help = "Accepted baseline commit supplied independently of the candidate"
        )]
        expected_base_commit: Option<GitOid>,
        #[arg(
            long,
            requires = "expected_base_commit",
            help = "Accepted baseline contract fingerprint supplied independently of the candidate"
        )]
        expected_base_contract: Option<workdeck_pm::ContentHash>,
    },
    #[command(
        about = "Prepare revision-bound check feedback in the supplied checkout; does not execute or grant CI trust"
    )]
    Plan {
        #[arg(long)]
        revision: String,
        #[arg(long)]
        profile: String,
        #[arg(long)]
        issue: Option<String>,
    },
    #[command(
        about = "Execute an exact prepared revision-bound check plan with durable replay; results remain local feedback"
    )]
    Check {
        #[arg(
            long,
            help = "CiPreparedCheck JSON or the complete successful ci plan JSON output"
        )]
        plan_file: PathBuf,
        #[arg(
            long,
            help = "Exact binding.fingerprint of the reviewed revision-bound plan"
        )]
        expected_plan: String,
        #[arg(long)]
        actor: String,
        #[arg(long, help = "Stable request identity; retain for replay and recovery")]
        request_id: String,
    },
}

pub(super) fn revision(value: &str) -> workdeck_pm::Result<CiRevision> {
    if value == "HEAD" {
        return Ok(CiRevision::Head {});
    }
    if let Ok(oid) = value.parse::<GitOid>() {
        return Ok(CiRevision::Commit { oid });
    }
    if value.is_empty() || value.starts_with('-') {
        return Err(pm_cli::invalid(
            "CI revision must be an exact commit ID or a bounded ref/branch name",
        ));
    }
    let reference = if value.starts_with("refs/") {
        value.to_owned()
    } else {
        format!("refs/heads/{value}")
    };
    Ok(CiRevision::Reference {
        reference: reference.parse()?,
    })
}

pub(super) fn run(cwd: &Path, options: &CiOptions, command: &CiCommand) -> anyhow::Result<()> {
    if let CiCommand::RedGreen(args) = command {
        return super::pm_ci_red_green::run(cwd, options, args);
    }
    if let CiCommand::ReviewCoverage(args) = command {
        return super::pm_ci_coverage::run(cwd, options, args);
    }
    if matches!(
        command,
        CiCommand::ImportReview(_)
            | CiCommand::Reviews
            | CiCommand::Review { .. }
            | CiCommand::ReauthenticateReview(_)
    ) {
        return super::pm_ci_retained_review::run(cwd, options, command);
    }
    if let CiCommand::ValidateReviewed(args) = command {
        return super::pm_ci_review::run(cwd, options, args);
    }
    if let CiCommand::ReviewPolicy { policy_file } = command {
        return super::pm_ci_review::policy(cwd, options, policy_file);
    }
    if matches!(
        command,
        CiCommand::ImportReport { .. }
            | CiCommand::Reports
            | CiCommand::Report { .. }
            | CiCommand::Reauthenticate { .. }
            | CiCommand::ReauthenticateRedGreen { .. }
            | CiCommand::VerifyImportedCheck { .. }
    ) {
        return super::pm_ci_import::run(cwd, options, command);
    }
    if let CiCommand::Policy { policy_file } = command {
        return super::pm_ci_auth::inspect(cwd, options, policy_file);
    }
    if let CiCommand::Authenticate {
        report_file,
        policy_file,
        expected_policy,
        expected_commit,
    } = command
    {
        return super::pm_ci_auth::run(
            cwd,
            options,
            report_file,
            policy_file,
            expected_policy,
            expected_commit,
        );
    }
    let initial = json!({"repository":null,"root":cwd,"role":"ci_validation"});
    let CiCommand::Validate {
        base,
        head,
        expected_base_commit,
        expected_base_contract,
    } = command
    else {
        return super::pm_ci_checks::run(cwd, options, command);
    };
    let request = (|| -> workdeck_pm::Result<_> {
        Ok(CiValidateRequest {
            base: revision(base)?,
            head: revision(head)?,
        })
    })()
    .map_err(|error| CommandFailure {
        error,
        source_identity: initial.clone(),
    })?;
    let validation = match (expected_base_commit, expected_base_contract) {
        (Some(commit), Some(contract)) => workdeck_pm::ci_validate_pinned(
            cwd,
            &request,
            &workdeck_pm::CiBaselinePin {
                commit: commit.clone(),
                contract: contract.clone(),
            },
        ),
        (None, None) => workdeck_pm::ci_validate(cwd, &request),
        _ => Err(pm_cli::invalid("Both baseline pins are required together")),
    };
    let report = validation.map_err(|error| CommandFailure {
        error,
        source_identity: initial,
    })?;
    let source = json!({"repository":report.head.repository,"root":cwd,"role":"ci_validation",
        "base":report.base,"head":report.head,"basis":report.basis});
    if !report.valid {
        return Err(CommandFailure {
            error: PmError::new(ErrorCode::PolicyBlocked,
                "CI planning validation failed or evaluation contracts require review")
                .hint("Inspect base/head diagnostics and contract changes. This command cannot approve contract changes or establish trusted CI evidence.")
                .details(json!({"report":report})),
            source_identity: source,
        }.into());
    }
    pm_cli::emit(options.json, "ci.validate", &source, &report, None).map_err(|error| {
        CommandFailure {
            error,
            source_identity: source,
        }
        .into()
    })
}
