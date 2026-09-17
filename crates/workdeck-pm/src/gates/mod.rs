//! Flat conjunctions of pinned requirements. Declarations are never evaluator
//! results. Ordinary assessments stay declaration-only; explicit committed gate
//! verification authenticates original evidence against independently pinned authority.
mod assessment;
mod criteria;
mod types;
mod verification;
pub(crate) use assessment::{
    diagnostics, issue_conditions, retirement_blockers, validate_associations,
    validate_issue_associations,
};
pub(crate) mod store;
pub use criteria::criterion_definition_hash;
pub(crate) use criteria::resolve as resolve_criterion;
pub(crate) use store::{load_gate, load_gates, parse, prepare_archive, validate_record};
pub(crate) mod validation;
use crate::{ErrorCode, PmError};
pub use types::*;
fn invalid(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::InvalidSchema, message)
}
