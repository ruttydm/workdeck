//! Source-qualified issue graph; only hard prerequisites affect readiness.
mod capture;
mod evaluation;
mod mutations;
mod proof;
mod types;
use crate::*;
pub(crate) use capture::{capture, preflight, validate_metadata, validate_path};
pub(crate) use evaluation::{completion_conditions, inspect, retirement_blockers};
pub(crate) use mutations::{invalidate_waivers, validate_issue_change, validate_related_import};
pub(crate) use proof::validate_receipt;
use serde_json::json;
use std::path::PathBuf;
pub use types::*;
const MAX_NODES: usize = 10_000;
const MAX_EDGES: usize = 100_000;
const MAX_ENTRIES: usize = 100_000;
const MAX_BYTES: usize = 64 * 1024 * 1024;
fn invalid(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::InvalidSchema, message)
}
fn unsupported(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::Unsupported, message)
}
fn blocked(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::PolicyBlocked, message)
}
fn text_value(value: &str, name: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            format!("{name} must be nonempty text up to 4096 bytes without control characters"),
        ));
    }
    Ok(())
}
