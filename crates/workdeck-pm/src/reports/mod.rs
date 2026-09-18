//! Bounded, inert interpretation of explicitly declared report contracts.
//! Process success, artifact identity, freshness and completion trust are separate.
mod cases;
mod junit;
pub use cases::*;
mod sarif;
mod types;
pub use types::*;

use crate::ReportExpectation;

pub const MAX_REPORT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_REPORT_FAILURES: usize = 32;

impl ReportAssessment {
    fn new(state: ReportState, reason: &str) -> Self {
        Self {
            state,
            reason_codes: vec![reason.into()],
            counts: ReportCounts::default(),
            failures: Vec::new(),
            omitted_failures: 0,
        }
    }

    fn failure(&mut self, id: &str, message: &str, path: Option<&str>, line: Option<u64>) {
        if self.failures.len() == MAX_REPORT_FAILURES {
            self.omitted_failures += 1;
            return;
        }
        self.failures.push(ReportFailure {
            id: clipped(id, 256),
            message: clipped(message, 1024),
            path: path.map(|value| clipped(value, 512)),
            line: line.filter(|line| *line > 0),
        });
    }
}

fn clipped(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.into();
    }
    let mut end = max.saturating_sub(3);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    if max < 3 {
        String::new()
    } else {
        format!("{}…", &value[..end])
    }
}

/// Interprets report bytes only. A Passed report does not establish process success,
/// current source identity, artifact availability, or trusted completion evidence.
pub fn assess_check_report(
    expectation: &ReportExpectation,
    artifact: Option<&[u8]>,
) -> ReportAssessment {
    if matches!(expectation, ReportExpectation::Process { .. }) {
        return ReportAssessment::new(ReportState::Passed, "process_only_contract");
    }
    let Some(bytes) = artifact else {
        return ReportAssessment::new(ReportState::Unknown, "report_missing");
    };
    if bytes.len() > MAX_REPORT_BYTES {
        return ReportAssessment::new(ReportState::Unknown, "report_size_limit");
    }
    match expectation {
        ReportExpectation::Process { .. } => unreachable!("handled above"),
        ReportExpectation::JUnit {
            suites,
            minimum_tests,
            maximum_skipped,
            ..
        } => junit::assess(bytes, suites, *minimum_tests, *maximum_skipped),
        ReportExpectation::Sarif {
            tool,
            minimum_invocations,
            failure_levels,
            ..
        } => sarif::assess(bytes, tool, *minimum_invocations, failure_levels),
    }
}
