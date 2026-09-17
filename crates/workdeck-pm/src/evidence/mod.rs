//! Immutable declared evidence references. Supersession preserves every source
//! record; verified producers and execution verdicts require the later evaluator.
pub(crate) mod store;
mod types;
mod validation;
mod verification;
pub use types::*;
