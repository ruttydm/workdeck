mod types;
pub use types::*;
pub(crate) mod records;
mod store;
use crate::*;
pub(crate) use store::load;
fn invalid(message: &str) -> PmError {
    PmError::new(ErrorCode::InvalidSchema, message)
}
