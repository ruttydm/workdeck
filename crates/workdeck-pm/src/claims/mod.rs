mod saved;
mod shared;
pub(crate) mod store;
mod transition;
mod types;
pub(crate) mod validation;
use crate::*;
pub use types::*;
fn invalid(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::InvalidSchema, message)
}
pub(crate) fn validate_receipt(receipt: &crate::transactions::MutationReceipt) -> Result<()> {
    validation::validate_receipt(receipt)?;
    completion::validate_receipt(receipt)
}
pub(crate) use store::load_claims;

mod completion;
pub use completion::{
    ClaimedCompletionConfirmation, ClaimedCompletionOutcome, ClaimedCompletionProof,
    ClaimedCompletionVerification, CompleteClaimedIssue, CompleteClaimedVerifiedIssue,
};
