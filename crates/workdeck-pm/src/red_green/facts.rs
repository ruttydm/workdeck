use super::blocked;
use crate::*;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub(super) fn source(root: &Path, report: &AuthenticatedCheckReport) -> Result<()> {
    let intent = &report.report.publication.intent.intent;
    let binding = intent
        .revision
        .as_ref()
        .ok_or_else(|| blocked("red/green report has no committed input binding"))?;
    let captured = crate::sources::bind_check_inputs(root, &report.source, &intent.input.plan)?;
    if captured != binding.inputs {
        return Err(blocked(
            "red/green input proof differs from freshly captured committed inputs",
        ));
    }
    Ok(())
}
pub(super) fn check<'a>(
    report: &'a AuthenticatedCheckReport,
    definition: &CheckRecord,
    artifact_bytes: &[u8],
    red: bool,
) -> Result<(
    &'a PlannedInvocation,
    &'a CiInvocationInputManifest,
    JUnitCaseReport,
)> {
    let publication = &report.report.publication;
    let result = &publication.result.result;
    let expected_state = if red {
        RunState::Failed
    } else {
        RunState::Passed
    };
    if result.state != expected_state || report.report.observation.state != expected_state {
        return Err(blocked(
            "red/green requires fresh failed red and passed green results",
        ));
    }
    let check = result
        .checks
        .iter()
        .find(|c| c.check.id == definition.definition.id)
        .ok_or_else(|| blocked("required red/green check is missing from the run"))?;
    if check.check.definition != definition.content || check.state != expected_state {
        return Err(blocked(
            "red/green check identity, definition or outcome differs from the accepted contract",
        ));
    }
    let invocation = result
        .invocations
        .get(check.invocation)
        .ok_or_else(|| blocked("red/green invocation is unavailable"))?;
    let process = &invocation.process;
    if process.termination != ProcessTermination::Exited
        || !process.cleanup_complete
        || process.signal.is_some()
        || process.started_at.is_none()
        || !invocation.inputs_unchanged
    {
        return Err(blocked(
            "interrupted, unavailable or stale execution cannot qualify red/green",
        ));
    }
    let ReportExpectation::JUnit {
        artifact,
        allowed_exit_codes,
        ..
    } = &definition.definition.expectation
    else {
        return Err(blocked("red/green requires JUnit artifacts"));
    };
    let requirement = definition
        .definition
        .red_green
        .as_ref()
        .ok_or_else(|| blocked("accepted check has no red/green requirement"))?;
    let allowed = if red {
        &requirement.red_exit_codes
    } else {
        allowed_exit_codes
    };
    if !process
        .exit_code
        .is_some_and(|code| allowed.contains(&code))
    {
        return Err(blocked(
            "red/green process exit is outside the accepted check contract",
        ));
    }
    let descriptor = invocation
        .artifacts
        .iter()
        .find(|a| &a.id == artifact)
        .ok_or_else(|| blocked("red/green artifact descriptor is missing"))?;
    if descriptor.availability != ArtifactAvailability::Present
        || descriptor.bytes != artifact_bytes.len() as u64
        || descriptor.content.as_ref() != Some(&ContentHash::of(artifact_bytes))
    {
        return Err(blocked(
            "red/green artifact bytes differ from authenticated execution proof",
        ));
    }
    let parsed = assess_check_report(&definition.definition.expectation, Some(artifact_bytes));
    if parsed != check.report
        || parsed.counts.errors != 0
        || parsed.state
            != if red {
                ReportState::Failed
            } else {
                ReportState::Passed
            }
    {
        return Err(blocked(
            "red/green report is inconsistent or contains infrastructure errors",
        ));
    }
    let intent = &publication.intent.intent;
    let planned = intent
        .input
        .plan
        .invocations
        .get(check.invocation)
        .ok_or_else(|| blocked("red/green invocation plan is unavailable"))?;
    let binding = intent
        .revision
        .as_ref()
        .and_then(|b| b.inputs.get(check.invocation))
        .ok_or_else(|| blocked("red/green invocation input binding is unavailable"))?;
    Ok((planned, binding, junit_test_cases(artifact_bytes)?))
}
pub(super) fn comparable(red: &PlannedInvocation, green: &PlannedInvocation) -> Result<()> {
    if red.id != green.id
        || red.command != green.command
        || red.definition != green.definition
        || red.tool != green.tool
        || red.args != green.args
        || red.cwd != green.cwd
        || red.environment != green.environment
        || red.tools != green.tools
        || red.input_selection != green.input_selection
        || red.bounds != green.bounds
        || red.artifacts != green.artifacts
        || red.effects != green.effects
        || red.inputs.environment != green.inputs.environment
        || red.inputs.tools != green.inputs.tools
    {
        return Err(blocked(
            "red/green execution parameters, environment or tools changed",
        ));
    }
    Ok(())
}
pub(super) fn changed(
    red: &CiInvocationInputManifest,
    green: &CiInvocationInputManifest,
) -> Vec<PathBuf> {
    let entries = |manifest: &CiInvocationInputManifest| {
        manifest
            .entries
            .iter()
            .map(|e| (e.path().to_owned(), e.clone()))
            .collect::<BTreeMap<_, _>>()
    };
    let red = entries(red);
    let green = entries(green);
    red.keys()
        .chain(green.keys())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter(|path| red.get(*path) != green.get(*path))
        .cloned()
        .collect()
}

pub(super) fn pair(
    request: &RedGreenRequest,
    red: &AuthenticatedCheckReport,
    green: &AuthenticatedCheckReport,
    definition: &CheckRecord,
) -> Result<Vec<PathBuf>> {
    let requirement = definition
        .definition
        .red_green
        .as_ref()
        .ok_or_else(|| blocked("accepted check has no red/green requirement"))?;
    requirement.validate(&definition.definition.expectation)?;
    if red.report.publication.result.result.finished_at
        > green.report.publication.intent.intent.recorded_at
    {
        return Err(blocked(
            "red execution must finish before the green run is admitted",
        ));
    }
    let (red_invocation, red_inputs, red_cases) =
        check(red, definition, request.red_artifact.as_bytes(), true)?;
    let (green_invocation, green_inputs, green_cases) =
        check(green, definition, request.green_artifact.as_bytes(), false)?;
    comparable(red_invocation, green_invocation)?;
    let changed_inputs = changed(red_inputs, green_inputs);
    if changed_inputs.is_empty() {
        return Err(blocked(
            "red/green selected committed inputs did not change",
        ));
    }
    let red_cases = red_cases
        .cases
        .iter()
        .map(|c| (&c.identity, c.outcome))
        .collect::<BTreeMap<_, _>>();
    let green_cases = green_cases
        .cases
        .iter()
        .map(|c| (&c.identity, c.outcome))
        .collect::<BTreeMap<_, _>>();
    if red_cases.keys().any(|case| !green_cases.contains_key(case)) {
        return Err(blocked(
            "red/green case inventory changed; removed or replaced cases cannot qualify",
        ));
    }
    for case in &requirement.cases {
        if red_cases.get(case) != Some(&JUnitCaseOutcome::Failed)
            || green_cases.get(case) != Some(&JUnitCaseOutcome::Passed)
        {
            return Err(blocked(
                "required red/green case did not change from assertion failure to pass",
            ));
        }
    }
    for (case, state) in &red_cases {
        let next = green_cases.get(case);
        if (*state == JUnitCaseOutcome::Failed && next != Some(&JUnitCaseOutcome::Passed))
            || (*state != JUnitCaseOutcome::Skipped && next == Some(&JUnitCaseOutcome::Skipped))
        {
            return Err(blocked(
                "red/green cannot hide failed or executed cases with skips",
            ));
        }
    }
    Ok(changed_inputs)
}
