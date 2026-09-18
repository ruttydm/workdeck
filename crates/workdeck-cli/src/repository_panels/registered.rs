//! Explicit registry admission for a second native file/panel provider.
use super::{Config, PanelError, PlanningSource, RepositoryPanels, error, pm_error};
use workdeck_pm::{
    SourceSelector,
    registry::{RegisteredCheckout, RegistryNavigation, RegistryStore},
};

impl RepositoryPanels {
    /// Resolve a reload's explicit root through the launch provider's registry.
    /// Equal-root reloads preserve the provider's original physical identity.
    pub fn prepare_review_root(
        &self,
        root: &std::path::Path,
    ) -> Result<(Self, Option<RegistryNavigation>), PanelError> {
        self.verify_root()?;
        let root = root.canonicalize().map_err(error)?;
        if root == self.root {
            return Ok((self.clone(), None));
        }
        let PlanningSource::Native(owner) = self.planning()? else {
            return Err(PanelError::new(
                "Checkout reload requires an explicit native registry mapping",
            ));
        };
        let registry = RegistryStore::open(&owner).map_err(pm_error)?;
        let snapshot = registry.snapshot().map_err(pm_error)?;
        let mapping = snapshot
            .entries
            .iter()
            .find(|entry| entry.checkout == root && entry.source == SourceSelector::WorkingTree)
            .ok_or_else(|| {
                PanelError::new("The requested reload root has no registered working-tree mapping")
            })?;
        let navigation = registry.prepare_navigation(mapping).map_err(pm_error)?;
        let provider = self.open_registered_worktree(mapping)?;
        navigation.revalidate().map_err(pm_error)?;
        Ok((provider, Some(navigation)))
    }

    /// Open a separate provider for an exact reviewed working-tree mapping. The
    /// original provider retains its root and cannot read files in the target.
    /// Immutable planning refs require their indexed source reader, not native files.
    pub fn open_registered_worktree(
        &self,
        expected: &RegisteredCheckout,
    ) -> Result<Self, PanelError> {
        self.verify_root()?;
        if expected.source != SourceSelector::WorkingTree {
            return Err(PanelError::new(
                "Native checkout navigation requires an explicit working-tree mapping; immutable refs retain their source reader",
            ));
        }
        let PlanningSource::Native(owner) = self.planning()? else {
            return Err(PanelError::new(
                "Register checkout mappings in a native planning repository before switching",
            ));
        };
        let registry = RegistryStore::open(&owner).map_err(pm_error)?;
        let verify = || {
            let (current, _) = registry.resolve(&expected.alias).map_err(pm_error)?;
            if &current != expected {
                return Err(PanelError::new(
                    "Registered checkout changed after inspection",
                ));
            }
            Ok(())
        };
        verify()?;
        let config = Config::load(&expected.checkout).map_err(error)?;
        let base = (!config.git.base_branch.is_empty()).then_some(config.git.base_branch);
        let provider = Self::new(&expected.checkout, base, config.git.recent_commits)?
            .with_git_base_key(Some(config.keys.base.clone()));
        verify()?;
        self.planning()?;
        self.verify_root()?;
        Ok(provider)
    }
}
