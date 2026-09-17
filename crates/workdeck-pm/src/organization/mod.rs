//! Optional repository organization policy. Structural readers retain history;
//! ordinary writers validate their final candidate in the transaction snapshot.
mod policy;
#[cfg(test)]
mod scale_tests;
mod store;
mod types;

use crate::{ErrorCode, PmError, Result};
pub(crate) use policy::{
    inspect, inspect_documents, validate_actor, validate_evidence_change, validate_feature_change,
    validate_gate_change, validate_import, validate_issue_change, validate_planning_change,
    validate_template,
};
pub(crate) use store::{schema as capture_schema, users as capture_users};
pub use types::*;
fn invalid(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::InvalidSchema, message)
}
fn blocked(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::PolicyBlocked, message)
}
