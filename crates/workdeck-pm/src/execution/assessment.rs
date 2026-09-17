use super::*;
use crate::*;
use std::{collections::BTreeMap, path::Path};

pub(super) fn artifacts(
    root: &Path,
    intent: &RunIntent,
    index: usize,
) -> (Vec<ArtifactObservation>, BTreeMap<String, Vec<u8>>) {
    let mut observations = Vec::new();
    let mut contents = BTreeMap::new();
    let mut remaining = MAX_RUN_OUTPUT_BYTES as usize;
    for spec in &intent.input.plan.invocations[index].artifacts {
        let path = records::artifact_relative(intent, index, &spec.name);
        let absolute = root.join(&path);
        let bound = spec.max_bytes.min(remaining);
        let (availability, content, bytes) = match local::read(root, &absolute, bound) {
            Ok(Some(bytes)) => {
                remaining = remaining.saturating_sub(bytes.len());
                let content = ContentHash::of(&bytes);
                let size = bytes.len() as u64;
                contents.insert(spec.id.clone(), bytes);
                (ArtifactAvailability::Present, Some(content), size)
            }
            Ok(None) => (ArtifactAvailability::Missing, None, 0),
            Err(error) if error.code == ErrorCode::InvalidInput => {
                (ArtifactAvailability::TooLarge, None, 0)
            }
            Err(_) => (ArtifactAvailability::Unsafe, None, 0),
        };
        observations.push(ArtifactObservation {
            id: spec.id.clone(),
            path,
            availability,
            content,
            bytes,
        });
    }
    (observations, contents)
}
pub(super) fn process_state(process: &ProcessObservation, allowed: &[i32]) -> RunState {
    if !process.cleanup_complete {
        return RunState::Unknown;
    }
    match process.termination {
        ProcessTermination::Exited => {
            if process
                .exit_code
                .is_some_and(|code| allowed.contains(&code))
            {
                RunState::Passed
            } else {
                RunState::Failed
            }
        }
        ProcessTermination::SpawnFailed => RunState::Blocked,
        ProcessTermination::TimedOut | ProcessTermination::CleanupFailed => RunState::Unknown,
        ProcessTermination::Canceled => RunState::Canceled,
        ProcessTermination::NotRun => RunState::NotRun,
    }
}
pub(super) fn allowed(expectation: &ReportExpectation) -> &[i32] {
    match expectation {
        ReportExpectation::Process { allowed_exit_codes }
        | ReportExpectation::JUnit {
            allowed_exit_codes, ..
        }
        | ReportExpectation::Sarif {
            allowed_exit_codes, ..
        } => allowed_exit_codes,
    }
}
pub(super) fn report_artifact(expectation: &ReportExpectation) -> Option<&str> {
    match expectation {
        ReportExpectation::Process { .. } => None,
        ReportExpectation::JUnit { artifact, .. } | ReportExpectation::Sarif { artifact, .. } => {
            Some(artifact)
        }
    }
}
pub(super) fn invocation_state(
    plan: &PlannedInvocation,
    process: &ProcessObservation,
    artifacts: &[ArtifactObservation],
    unchanged: bool,
    allowed: &[i32],
) -> RunState {
    let process = process_state(process, allowed);
    if process != RunState::Passed {
        return process;
    }
    if !unchanged {
        return RunState::Stale;
    }
    if plan
        .artifacts
        .iter()
        .zip(artifacts)
        .any(|(spec, record)| spec.required && record.availability != ArtifactAvailability::Present)
    {
        return RunState::Unknown;
    }
    RunState::Passed
}
pub(super) fn check_state(invocation: RunState, report: ReportState) -> RunState {
    if invocation != RunState::Passed {
        return invocation;
    }
    match report {
        ReportState::Passed => RunState::Passed,
        ReportState::Failed => RunState::Failed,
        ReportState::Skipped => RunState::Skipped,
        ReportState::Unknown => RunState::Unknown,
    }
}
pub(super) fn aggregate(states: impl IntoIterator<Item = RunState>) -> RunState {
    let states: Vec<_> = states.into_iter().collect();
    if states.is_empty() {
        return RunState::NotRun;
    }
    for candidate in [
        RunState::Unknown,
        RunState::Canceled,
        RunState::Stale,
        RunState::Blocked,
        RunState::Failed,
        RunState::NotRun,
        RunState::Skipped,
        RunState::Running,
    ] {
        if states.contains(&candidate) {
            return candidate;
        }
    }
    RunState::Passed
}
pub(super) fn result_states(intent: &RunIntent, result: &RunResult) -> Result<()> {
    for (index, outcome) in result.invocations.iter().enumerate() {
        let plan = &intent.input.plan.invocations[index];
        let codes = intent
            .input
            .plan
            .checks
            .iter()
            .find(|check| check.invocation == plan.id)
            .map(|check| allowed(&check.expectation))
            .unwrap_or(&[0]);
        let expected = invocation_state(
            plan,
            &outcome.process,
            &outcome.artifacts,
            outcome.inputs_unchanged,
            codes,
        );
        if outcome.state != expected {
            return Err(invalid(
                "invocation verdict contradicts process, input or artifact observations",
            ));
        }
    }
    for check in &result.checks {
        if check.state
            != check_state(
                result.invocations[check.invocation].state,
                check.report.state,
            )
        {
            return Err(invalid(
                "check verdict contradicts process or report observations",
            ));
        }
        if check.report.failures.len() > crate::MAX_REPORT_FAILURES {
            return Err(invalid("check report exceeds failure excerpt bound"));
        }
    }
    let expected = aggregate(
        result
            .invocations
            .iter()
            .map(|i| i.state)
            .chain(result.checks.iter().map(|check| check.state)),
    );
    if result.state != expected {
        return Err(invalid("run verdict contradicts retained observations"));
    }
    Ok(())
}
