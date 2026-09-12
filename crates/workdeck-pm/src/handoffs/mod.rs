mod types;
pub use types::*;
mod store;
use crate::{ErrorCode, PmError};
pub(crate) use store::{
    inspect, load_handoff, load_handoffs, validate_import, validate_path, validate_receipt,
};
fn invalid(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::InvalidSchema, message)
}
