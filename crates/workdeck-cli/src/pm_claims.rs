//! Claim commands retain original request material and distinguish receipts from ownership.
use super::{pm_cli, pm_context::OutputOptions};
use clap::{Args, Subcommand};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use workdeck_pm::*;

#[derive(Debug, Default, Args)]
pub(super) struct Options {
    #[arg(long, global = true)]
    pub json: bool,
    #[arg(long, global = true)]
    pub no_input: bool,
    #[command(flatten)]
    pub output: OutputOptions,
}
#[derive(Debug, Args)]
pub(super) struct ContractInput {
    #[arg(
        long,
        help = "Saved contract fingerprint or exact ClaimWorkContract JSON file"
    )]
    contract: String,
    #[arg(long)]
    expected_contract: String,
}
#[derive(Debug, Args)]
pub(super) struct MutationInput {
    #[arg(long)]
    actor: String,
    #[arg(long)]
    request_id: String,
    #[arg(
        long,
        help = "Required for shared actions: inspected binding from source status"
    )]
    expected_binding: Option<String>,
}
impl MutationInput {
    fn binding(&self, repository: &Repository) -> Result<Option<ContentHash>> {
        let binding = self
            .expected_binding
            .as_deref()
            .map(str::parse)
            .transpose()?;
        if binding.is_none() && repository.config()?.sources.is_some() {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "shared claim actions require --expected-binding from the inspected source status",
            ));
        }
        Ok(binding)
    }
}
#[derive(Debug, Args)]
pub(super) struct Expected {
    #[arg(long)]
    token: String,
    #[arg(long)]
    generation: u64,
    #[arg(long)]
    expected_content: String,
}
impl Expected {
    fn parse(&self) -> Result<ClaimPrecondition> {
        Ok(ClaimPrecondition {
            token: self.token.parse()?,
            generation: self.generation,
            content: self.expected_content.parse()?,
        })
    }
}
#[derive(Debug, Args)]
pub(super) struct CurrentInput {
    issue: String,
    #[command(flatten)]
    expected: Expected,
    #[command(flatten)]
    input: MutationInput,
}
#[derive(Debug, Subcommand)]
pub(super) enum ClaimCommand {
    Contract {
        issue: String,
    },
    List {
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        cursor: Option<String>,
    },
    Status {
        issue: String,
    },
    Explain {
        issue: String,
    },
    Validate,
    #[command(
        about = "Complete an issue using its current claim; release remains a separate operation"
    )]
    Complete {
        #[command(flatten)]
        current: CurrentInput,
        #[command(flatten)]
        contract: ContractInput,
        #[arg(long)]
        expected_issue_revision: u64,
        #[arg(long)]
        expected_issue_content: String,
        #[arg(long, requires = "release_reason")]
        release_request_id: Option<String>,
        #[arg(long, requires = "release_request_id")]
        release_reason: Option<String>,
        #[arg(
            long,
            value_name = "FILE",
            help = "Authenticated CompleteVerifiedIssue JSON; keeps claim ownership and CI proof in one receipt"
        )]
        verification_file: Option<PathBuf>,
    },
    Acquire {
        #[command(flatten)]
        contract: ContractInput,
        #[command(flatten)]
        input: MutationInput,
        #[arg(long)]
        ttl_seconds: Option<u64>,
    },
    Recover {
        #[command(flatten)]
        contract: ContractInput,
        #[command(flatten)]
        input: MutationInput,
        #[command(flatten)]
        expected: Expected,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        ttl_seconds: Option<u64>,
    },
    Renew {
        #[command(flatten)]
        current: CurrentInput,
        #[arg(long)]
        ttl_seconds: Option<u64>,
    },
    Revalidate {
        #[command(flatten)]
        current: CurrentInput,
        #[command(flatten)]
        contract: ContractInput,
        #[arg(long)]
        ttl_seconds: Option<u64>,
    },
    Release {
        #[command(flatten)]
        current: CurrentInput,
        #[arg(long)]
        reason: String,
    },
    Cancel {
        #[command(flatten)]
        current: CurrentInput,
        #[arg(long)]
        reason: String,
    },
    Supersede {
        #[command(flatten)]
        current: CurrentInput,
        #[arg(long)]
        reason: String,
    },
}
impl ClaimCommand {
    pub(super) fn is_mutation(&self) -> bool {
        !matches!(
            self,
            Self::Contract { .. }
                | Self::List { .. }
                | Self::Status { .. }
                | Self::Explain { .. }
                | Self::Validate
        )
    }
}
fn contract(
    cwd: &Path,
    repository: &Repository,
    input: &ContractInput,
) -> Result<ClaimWorkContract> {
    let contract = if let Ok(hash) = input.contract.parse::<ContentHash>() {
        repository.load_claim_contract(&hash)?
    } else {
        let bytes =
            pm_cli::read_regular_input(cwd, Path::new(&input.contract), MAX_CLAIM_BYTES as u64)?;
        serde_json::from_slice::<ClaimWorkContract>(&bytes)
            .map_err(|e| PmError::new(ErrorCode::InvalidInput, e.to_string()))?
    };
    contract.validate()?;
    if contract.fingerprint()? != input.expected_contract.parse()? {
        return Err(PmError::new(
            ErrorCode::StaleSource,
            "claim contract differs from the reviewed fingerprint",
        ));
    }
    Ok(contract)
}
pub(super) fn run(
    cwd: &Path,
    repository: &Repository,
    source: &Value,
    options: &Options,
    command: &ClaimCommand,
) -> Result<Option<u8>> {
    options.output.validate()?;
    if let ClaimCommand::Complete {
        current,
        contract: selected,
        expected_issue_revision,
        expected_issue_content,
        release_request_id,
        release_reason,
        verification_file,
    } = command
    {
        let input = CompleteClaimedIssue {
            issue: current.issue.parse()?,
            actor: current.input.actor.clone(),
            expected_claim: current.expected.parse()?,
            expected_issue: SourceToken {
                revision: Revision::new(*expected_issue_revision)?,
                content: expected_issue_content.parse()?,
            },
            contract: contract(cwd, repository, selected)?,
            expected_binding: current.input.binding(repository)?,
        };
        let request = current.input.request_id.parse()?;
        if let Some(path) = verification_file {
            if release_request_id.is_some() || release_reason.is_some() {
                return Err(pm_cli::invalid(
                    "--verification-file cannot be combined with the separate claim release flags",
                ));
            }
            let document = pm_cli::read_input(cwd, path)?;
            let verification: CompleteVerifiedIssue =
                serde_json::from_str(&document).map_err(|error| {
                    pm_cli::invalid(format!("invalid verified completion input: {error}"))
                })?;
            if verification.issue != input.issue
                || verification.expected_issue != input.expected_issue
                || verification.actor != input.actor
            {
                return Err(pm_cli::invalid(
                    "verified completion file must match the claim issue, actor and source token",
                ));
            }
            let claimed = CompleteClaimedVerifiedIssue {
                claim: input,
                verification,
            };
            let receipt = repository.complete_claimed_verified_issue(&claimed, &request)?;
            options.output.emit(
                options.json,
                "claimed_verified_completion_receipt",
                source,
                &receipt,
            )?;
            return Ok(None);
        }
        if let (Some(release), Some(reason)) = (release_request_id, release_reason) {
            let outcome = repository.complete_claimed_issue_and_release(
                &input,
                &request,
                &release.parse()?,
                reason,
            )?;
            options
                .output
                .emit(options.json, "claimed_completion_outcome", source, &outcome)?;
            return Ok(outcome
                .release_error
                .as_ref()
                .map(|error| error.code.exit_code())
                .or_else(|| {
                    outcome
                        .release_publication
                        .as_ref()
                        .and_then(|release| release.publication.as_ref())
                        .and_then(|publication| match publication.state {
                            PublicationState::Confirmed => None,
                            PublicationState::Uncertain => Some(6),
                            PublicationState::Prepared | PublicationState::Rejected => Some(4),
                        })
                }));
        }
        let receipt = repository.complete_claimed_issue(&input, &request)?;
        options
            .output
            .emit(options.json, "claimed_completion_receipt", source, &receipt)?;
        return Ok(None);
    }
    let mut requires_continuation = false;
    let request = match command {
        ClaimCommand::Complete { .. } => unreachable!(),
        ClaimCommand::Contract { issue } => {
            let contract = repository.claim_contract(&issue.parse()?)?;
            let path = repository.save_claim_contract(&contract)?;
            options.output.emit(options.json,"claim_contract",source,&json!({"fingerprint":contract.fingerprint()?,"contract":contract,"saved_contract":path}))?;
            return Ok(None);
        }
        ClaimCommand::List { limit, cursor } => {
            page(repository, source, options, *limit, cursor.as_deref())?;
            return Ok(None);
        }
        ClaimCommand::Status { issue } | ClaimCommand::Explain { issue } => {
            let issue: IssueId = issue.parse()?;
            let status = repository
                .claims()?
                .into_iter()
                .find(|s| s.claim.metadata.issue == issue)
                .ok_or_else(|| PmError::new(ErrorCode::NotFound, "claim not found"))?;
            options
                .output
                .emit(options.json, "claim_status", source, &status)?;
            return Ok(None);
        }
        ClaimCommand::Validate => {
            let claims = repository.claims()?;
            options.output.emit(options.json,"claim_validation",source,&json!({"valid":true,"count":claims.len(),"ownership_confirmed":false,"reason":"validation checks retained authority; use mutation outcomes for permission to continue"}))?;
            return Ok(None);
        }
        ClaimCommand::Acquire {
            contract: selected,
            input,
            ttl_seconds,
        }
        | ClaimCommand::Recover {
            contract: selected,
            input,
            ttl_seconds,
            ..
        } => {
            requires_continuation = true;
            let recovery = if let ClaimCommand::Recover {
                expected, reason, ..
            } = command
            {
                Some(ClaimRecovery {
                    expected: expected.parse()?,
                    reason: reason.clone(),
                })
            } else {
                None
            };
            (
                ClaimRequest::Acquire {
                    input: Box::new(AcquireClaim {
                        actor: input.actor.clone(),
                        contract: contract(cwd, repository, selected)?,
                        ttl_seconds: *ttl_seconds,
                        recovery,
                    }),
                },
                input.request_id.parse::<RequestId>()?,
            )
        }
        ClaimCommand::Renew { current, .. }
        | ClaimCommand::Revalidate { current, .. }
        | ClaimCommand::Release { current, .. }
        | ClaimCommand::Cancel { current, .. }
        | ClaimCommand::Supersede { current, .. } => {
            let actor = current.input.actor.clone();
            let mutation = match command {
                ClaimCommand::Renew { ttl_seconds, .. } => {
                    requires_continuation = true;
                    ClaimMutation::Renew {
                        actor,
                        ttl_seconds: *ttl_seconds,
                    }
                }
                ClaimCommand::Revalidate {
                    contract: selected,
                    ttl_seconds,
                    ..
                } => {
                    requires_continuation = true;
                    ClaimMutation::Revalidate {
                        actor,
                        contract: Box::new(contract(cwd, repository, selected)?),
                        ttl_seconds: *ttl_seconds,
                    }
                }
                ClaimCommand::Release { reason, .. } => ClaimMutation::Release {
                    actor,
                    reason: reason.clone(),
                },
                ClaimCommand::Cancel { reason, .. } => ClaimMutation::Cancel {
                    actor,
                    reason: reason.clone(),
                },
                ClaimCommand::Supersede { reason, .. } => ClaimMutation::Supersede {
                    actor,
                    reason: reason.clone(),
                },
                _ => unreachable!(),
            };
            (
                ClaimRequest::Mutate {
                    issue: current.issue.parse()?,
                    expected: current.expected.parse()?,
                    mutation,
                },
                current.input.request_id.parse::<RequestId>()?,
            )
        }
    };
    let input = match command {
        ClaimCommand::Acquire { input, .. } | ClaimCommand::Recover { input, .. } => input,
        ClaimCommand::Renew { current, .. }
        | ClaimCommand::Revalidate { current, .. }
        | ClaimCommand::Release { current, .. }
        | ClaimCommand::Cancel { current, .. }
        | ClaimCommand::Supersede { current, .. } => &current.input,
        _ => unreachable!("read and completion commands returned before claim publication"),
    };
    let outcome = if let Some(binding) = input.binding(repository)? {
        repository.mutate_claim_reviewed(&request.0, &request.1, &binding)?
    } else {
        repository.mutate_local_claim_outcome(&request.0, &request.1)?
    };
    options
        .output
        .emit(options.json, "claim_operation", source, &outcome)?;
    Ok(
        if outcome
            .publication
            .as_ref()
            .is_some_and(|p| p.state == PublicationState::Uncertain)
        {
            Some(6)
        } else if outcome
            .publication
            .as_ref()
            .is_some_and(|p| p.state != PublicationState::Confirmed)
            || (requires_continuation && !outcome.may_continue)
        {
            Some(4)
        } else {
            None
        },
    )
}
fn page(
    repository: &Repository,
    source: &Value,
    options: &Options,
    limit: usize,
    cursor: Option<&str>,
) -> Result<()> {
    if !(1..=100).contains(&limit) {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "limit must be between 1 and 100",
        ));
    }
    let claims = repository.claims()?;
    // Clock-derived disposition is included; wall-clock observation timestamps are not.
    let members: Vec<_> = claims.iter().map(|s| json!({"claim":s.claim,"source":s.source.identity,"freshness":s.source.freshness,
        "disposition":s.assessment.disposition,"guarantee":s.assessment.guarantee,"may_continue":s.assessment.may_continue})).collect();
    let fingerprint = ContentHash::of(
        &serde_json::to_vec(&json!({"source":source,"limit":limit,"members":members}))
            .map_err(|error| PmError::new(ErrorCode::InvalidInput, error.to_string()))?,
    );
    let offset = if let Some(cursor) = cursor {
        if cursor.len() > 4096 {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "claim cursor exceeds 4096 bytes",
            ));
        }
        let (hash, offset) = cursor
            .split_once(':')
            .ok_or_else(|| PmError::new(ErrorCode::InvalidInput, "invalid claim cursor"))?;
        if hash != fingerprint.to_string() {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "claim membership or eligibility changed; restart pagination",
            ));
        }
        offset
            .parse::<usize>()
            .map_err(|_| PmError::new(ErrorCode::InvalidInput, "invalid claim cursor offset"))?
    } else {
        0
    };
    if offset > claims.len() {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "claim cursor exceeds membership",
        ));
    }
    let end = offset.saturating_add(limit).min(claims.len());
    options.output.emit(options.json,"claims",source,&json!({"records":&claims[offset..end],"total":claims.len(),"limit":limit,"offset":offset,
        "fingerprint":fingerprint,"next_cursor":(end<claims.len()).then(||format!("{fingerprint}:{end}"))}))
}
