//! Explicit local foreground execution. Results are local feedback, never CI authority.
pub(crate) mod input_fs;
pub(crate) mod input_types;
pub(crate) mod inputs;
pub(crate) mod local;
mod process;
mod types;
pub use types::*;

mod report;
pub use report::{CheckReport, MAX_CHECK_REPORT_BYTES};
mod assessment;
pub(crate) mod records;
mod store;
pub use records::{RunDocument, RunPublication};
pub(crate) use store::{results_for_context, results_from_snapshot, status_from_snapshot};
fn invalid(message: impl Into<String>) -> crate::PmError {
    crate::PmError::new(crate::ErrorCode::InvalidSchema, message)
}

/// Capacity check shared by discovery/planning and replay-first run admission.
/// Bounds cover retained logs and declared artifacts, not arbitrary recipe effects.
pub fn validate_run_bounds(plan: &crate::CheckPlan) -> crate::Result<()> {
    if plan.invocations.is_empty() {
        return Err(crate::PmError::new(
            crate::ErrorCode::PolicyBlocked,
            "no invocations selected; define or select a command/check before execution",
        ));
    }
    if plan.invocations.len() > MAX_RUN_INVOCATIONS {
        return Err(crate::PmError::new(
            crate::ErrorCode::InvalidInput,
            "run exceeds 256 invocations",
        ));
    }
    let total = plan.invocations.iter().try_fold(0u64, |total, invocation| {
        let logs = (invocation.bounds.stdout_bytes as u64)
            .checked_add(invocation.bounds.stderr_bytes as u64)?;
        invocation
            .artifacts
            .iter()
            .try_fold(total.checked_add(logs)?, |total, artifact| {
                total.checked_add(artifact.max_bytes as u64)
            })
    });
    if total.is_none_or(|total| total > MAX_RUN_OUTPUT_BYTES) {
        return Err(crate::PmError::new(
            crate::ErrorCode::InvalidInput,
            "run declared logs and artifacts exceed the aggregate 64 MiB retention budget",
        ));
    }
    Ok(())
}
