//! In-process navigation admission pinned to an explicit registry owner and mapping.
use super::{RegisteredCheckout, RegistryStore};
use crate::{ErrorCode, PmError, Repository, Result};

#[derive(Debug, Clone)]
pub struct RegistryNavigation {
    store: RegistryStore,
    checkout: RegisteredCheckout,
}
impl RegistryNavigation {
    pub fn checkout(&self) -> &RegisteredCheckout {
        &self.checkout
    }

    /// Recheck just before adopting a prepared checkout. The original owner
    /// directory remains pinned even when its files are copied to a replacement.
    /// This handle is navigation admission, not a write token or a claim.
    pub fn revalidate(&self) -> Result<Repository> {
        let (current, repository) = self.store.resolve(&self.checkout.alias)?;
        if current != self.checkout || !self.store.snapshot()?.entries.contains(&self.checkout) {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "Registered checkout changed after navigation was prepared",
            ));
        }
        Ok(repository)
    }
}
impl RegistryStore {
    pub fn prepare_navigation(&self, checkout: &RegisteredCheckout) -> Result<RegistryNavigation> {
        let navigation = RegistryNavigation {
            store: self.clone(),
            checkout: checkout.clone(),
        };
        navigation.revalidate()?;
        Ok(navigation)
    }
}
