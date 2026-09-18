//! Explicit local repository/checkout mappings. Reports never gain write authority.
mod navigation;
mod work_evidence;
pub use work_evidence::MyWorkEvidence;
mod store;
pub use navigation::RegistryNavigation;
mod types;
mod work;
pub use store::RegistryStore;
pub use types::*;
pub use work::{
    MyWorkFacet, MyWorkFaultPoint, MyWorkReport, MyWorkRequest, MyWorkSource, RegisteredWorkRow,
};

use crate::{ContentHash, ErrorCode, PmError, Repository, Result, SourceSelector};
use std::path::Path;

pub(crate) fn invalid(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::InvalidInput, message)
}

fn validate_alias(alias: &str) -> Result<()> {
    if alias.is_empty()
        || alias.len() > 64
        || !alias
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err(invalid(
            "checkout alias must contain 1–64 ASCII letters, digits, '-' or '_'",
        ));
    }
    Ok(())
}

/// Inspect only the explicitly supplied checkout; no network or external writes.
pub fn inspect_checkout(
    alias: &str,
    checkout: &Path,
    source: SourceSelector,
) -> Result<RegisteredCheckout> {
    validate_alias(alias)?;
    let checkout = checkout
        .canonicalize()
        .map_err(|e| PmError::io(checkout, e))?;
    let planning = checkout.join(".workdeck");
    let outer = crate::sources::fs::directory(&checkout)?;
    let inner = crate::sources::fs::directory(&planning)?;
    let repository = Repository::open_source(&planning)?;
    if crate::sources::fs::directory(&checkout)? != outer
        || crate::sources::fs::directory(&planning)? != inner
    {
        return Err(PmError::new(
            ErrorCode::StaleSource,
            "checkout changed during registration inspection",
        ));
    }
    let checkout_binding = ContentHash::of(
        &serde_json::to_vec(&(
            &checkout,
            repository.identity(),
            outer.0,
            outer.1,
            inner.0,
            inner.1,
        ))
        .map_err(|e| invalid(e.to_string()))?,
    );
    Ok(RegisteredCheckout {
        alias: alias.to_owned(),
        repository: repository.identity().clone(),
        checkout,
        checkout_binding,
        source,
    })
}

impl RegisteredCheckout {
    /// Revalidate an explicit mapping before opening its source. No fallback by ID.
    pub fn resolve(&self) -> Result<Repository> {
        let current = inspect_checkout(&self.alias, &self.checkout, self.source.clone())?;
        if current != *self {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "registered checkout identity changed; inspect and register it again",
            ));
        }
        Repository::open_source(&self.checkout.join(".workdeck"))
    }
}
