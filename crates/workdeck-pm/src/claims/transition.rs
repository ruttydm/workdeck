use super::*;
use chrono::Duration;

pub(crate) fn text(value: &str, name: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 2048 || value.chars().any(char::is_control) {
        return Err(invalid(format!(
            "{name} must be bounded nonempty text without control characters"
        )));
    }
    Ok(())
}

pub(crate) fn same_requirements(a: &ClaimWorkContract, b: &ClaimWorkContract) -> bool {
    a.issue == b.issue
        && a.issue_source == b.issue_source
        && a.requirements == b.requirements
        && a.accepted_source.repository == b.accepted_source.repository
        && a.accepted_source.role == b.accepted_source.role
        && a.accepted_source.ref_name == b.accepted_source.ref_name
}

fn time_add(now: Timestamp, seconds: u64) -> Result<Timestamp> {
    let duration = i64::try_from(seconds)
        .ok()
        .and_then(Duration::try_seconds)
        .ok_or_else(|| invalid("claim time interval overflow"))?;
    now.checked_add_signed(duration)
        .ok_or_else(|| invalid("claim expiry overflow"))
}

pub(crate) fn assess(
    record: &ClaimRecord,
    current: Option<&ClaimWorkContract>,
    policy: &ClaimPolicy,
    now: Timestamp,
    guarantee: ClaimGuarantee,
) -> ClaimAssessment {
    let metadata = &record.metadata;
    let mut reasons = Vec::new();
    let disposition = match metadata.state {
        ClaimState::Released => ClaimDisposition::Released,
        ClaimState::Canceled => ClaimDisposition::Canceled,
        ClaimState::Superseded => ClaimDisposition::Superseded,
        ClaimState::Active => {
            let skew = Duration::seconds(policy.max_clock_skew_seconds.min(3600) as i64);
            if now
                .checked_sub_signed(skew)
                .is_some_and(|latest| latest > metadata.expires_at)
            {
                reasons.push("expired_requires_explicit_recovery".into());
                ClaimDisposition::Expired
            } else if now
                .checked_add_signed(skew)
                .is_none_or(|latest| latest >= metadata.expires_at)
                || now
                    .checked_add_signed(skew)
                    .is_none_or(|latest| latest < metadata.updated_at)
            {
                reasons.push("clock_skew_window".into());
                ClaimDisposition::ClockUncertain
            } else if let Some(current) = current {
                if same_requirements(&metadata.contract, current) {
                    ClaimDisposition::Usable
                } else {
                    reasons.push("accepted_requirements_changed".into());
                    ClaimDisposition::NeedsRevalidation
                }
            } else {
                reasons.push("accepted_requirements_unavailable".into());
                ClaimDisposition::Unknown
            }
        }
    };
    if guarantee == ClaimGuarantee::Unconfirmed {
        reasons.push("coordination_not_confirmed".into());
    }
    ClaimAssessment {
        disposition,
        guarantee,
        assessed_at: now,
        may_continue: disposition == ClaimDisposition::Usable
            && guarantee != ClaimGuarantee::Unconfirmed,
        recovery_required: disposition == ClaimDisposition::Expired,
        reason_codes: reasons,
    }
}

pub(crate) fn check_expected(record: &ClaimRecord, expected: &ClaimPrecondition) -> Result<()> {
    if record.precondition() != *expected {
        return Err(PmError::new(ErrorCode::ClaimLost, "claim token, generation or content is no longer current")
            .details(serde_json::json!({"issue":record.metadata.issue,"current_generation":record.metadata.generation})));
    }
    Ok(())
}

pub(crate) fn apply(
    request: &ClaimRequest,
    previous: Option<&ClaimRecord>,
    current: &ClaimWorkContract,
    policy: &ClaimPolicy,
    now: Timestamp,
    request_id: &RequestId,
    token: ClaimToken,
) -> Result<ClaimRecord> {
    policy.validate()?;
    if request.issue() != &current.issue {
        return Err(invalid("claim targets a different accepted issue"));
    }
    let generation = previous.map_or(Ok(1), |r| {
        r.metadata
            .generation
            .checked_add(1)
            .ok_or_else(|| PmError::new(ErrorCode::Conflict, "claim generation exhausted"))
    })?;
    let lease = |ttl: Option<u64>| -> Result<Timestamp> {
        let seconds = ttl.unwrap_or(policy.default_ttl_seconds);
        if seconds == 0
            || seconds > policy.max_ttl_seconds
            || seconds <= policy.max_clock_skew_seconds.saturating_mul(2)
        {
            return Err(invalid(
                "claim lease must exceed twice the clock-skew allowance and stay within policy maximum",
            ));
        }
        time_add(now, seconds)
    };
    let metadata = match request {
        ClaimRequest::Acquire { input } => {
            text(&input.actor, "claim actor")?;
            if !same_requirements(&input.contract, current) {
                return Err(PmError::new(
                    ErrorCode::StaleSource,
                    "accepted claim requirements changed; inspect a fresh work contract",
                ));
            }
            match (previous, &input.recovery) {
                (Some(old), recovery) if old.metadata.state == ClaimState::Active => {
                    let recovery = recovery.as_ref().ok_or_else(|| PmError::new(ErrorCode::ClaimLost,
                        "an active or expired claim already exists; recovery requires its exact token and explicit acknowledgement"))?;
                    check_expected(old, &recovery.expected)?;
                    text(&recovery.reason, "recovery acknowledgement")?;
                    if assess(
                        old,
                        Some(current),
                        policy,
                        now,
                        ClaimGuarantee::LocalSourceOnly,
                    )
                    .disposition
                        != ClaimDisposition::Expired
                    {
                        return Err(PmError::new(
                            ErrorCode::ClaimLost,
                            "claim cannot be recovered before expiry plus the clock-skew allowance",
                        ));
                    }
                }
                (_, Some(_)) => {
                    return Err(invalid(
                        "recovery requires an existing expired active claim",
                    ));
                }
                _ => {}
            }
            ClaimMetadata {
                schema: SchemaVersion::CURRENT,
                repository: current.accepted_source.repository.clone(),
                issue: current.issue.clone(),
                token,
                generation,
                actor: input.actor.clone(),
                contract: current.clone(),
                state: ClaimState::Active,
                acquired_at: now,
                updated_at: now,
                expires_at: lease(input.ttl_seconds)?,
                last_request: request_id.clone(),
                last_operation: request.operation().into(),
                reason: input.recovery.as_ref().map(|r| r.reason.clone()),
            }
        }
        ClaimRequest::Mutate {
            expected, mutation, ..
        } => {
            let old = previous
                .ok_or_else(|| PmError::new(ErrorCode::ClaimLost, "claim no longer exists"))?;
            check_expected(old, expected)?;
            text(mutation.actor(), "claim actor")?;
            if old.metadata.actor != mutation.actor() || old.metadata.state != ClaimState::Active {
                return Err(PmError::new(
                    ErrorCode::ClaimLost,
                    "only the current active claim actor may change this token",
                ));
            }
            if now < old.metadata.updated_at {
                return Err(PmError::new(
                    ErrorCode::Conflict,
                    "clock precedes the last claim mutation; wait or reconcile clock skew",
                ));
            }
            let mut next = old.metadata.clone();
            next.generation = generation;
            next.updated_at = now;
            next.last_request = request_id.clone();
            next.last_operation = request.operation().into();
            next.reason = None;
            match mutation {
                ClaimMutation::Renew { ttl_seconds, .. } => {
                    if !assess(
                        old,
                        Some(current),
                        policy,
                        now,
                        ClaimGuarantee::LocalSourceOnly,
                    )
                    .may_continue
                    {
                        return Err(PmError::new(
                            ErrorCode::ClaimLost,
                            "renewal requires an unexpired current accepted work contract; revalidate or recover explicitly",
                        ));
                    }
                    next.expires_at = lease(*ttl_seconds)?;
                }
                ClaimMutation::Revalidate {
                    contract,
                    ttl_seconds,
                    ..
                } => {
                    if !same_requirements(contract, current) {
                        return Err(PmError::new(
                            ErrorCode::StaleSource,
                            "replacement claim contract is stale",
                        ));
                    }
                    if matches!(
                        assess(
                            old,
                            Some(current),
                            policy,
                            now,
                            ClaimGuarantee::LocalSourceOnly
                        )
                        .disposition,
                        ClaimDisposition::Expired | ClaimDisposition::ClockUncertain
                    ) {
                        return Err(PmError::new(
                            ErrorCode::ClaimLost,
                            "expired or clock-uncertain claims require recovery, not revalidation",
                        ));
                    }
                    next.contract = current.clone();
                    next.expires_at = lease(*ttl_seconds)?;
                }
                ClaimMutation::Release { reason, .. }
                | ClaimMutation::Cancel { reason, .. }
                | ClaimMutation::Supersede { reason, .. } => {
                    text(reason, "claim termination reason")?;
                    next.reason = Some(reason.clone());
                    next.state = match mutation {
                        ClaimMutation::Release { .. } => ClaimState::Released,
                        ClaimMutation::Cancel { .. } => ClaimState::Canceled,
                        _ => ClaimState::Superseded,
                    };
                }
            }
            next
        }
    };
    validation::serialize(metadata)
}
