//! Statically linked VCS adapters composed through the public catalog boundary.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::OnceLock;

use crate::{
    GitVcsAdapterOptions, JujutsuVcsAdapterOptions, SaplingVcsAdapterOptions, VcsAdapter,
    VcsCatalog, create_base_vcs_catalog, create_git_vcs_adapter, create_jujutsu_vcs_adapter,
    create_sapling_vcs_adapter,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BundledBackend {
    Git,
    Jujutsu,
    Sapling,
}

/// Stable metadata for one statically linked bundled extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundledExtensionMetadata {
    pub id: &'static str,
    pub source_path: &'static str,
    pub origin: &'static str,
}

/// Memoized bundled-extension result. Compiled Rust registration cannot fail,
/// but issues remain explicit so it retains the same host contract as native
/// user extension loading.
pub struct BundledExtensionLoad {
    pub extensions: Vec<BundledExtensionMetadata>,
    pub catalog: VcsCatalog,
    pub issues: Vec<String>,
}

static BUNDLED_LOAD: OnceLock<BundledExtensionLoad> = OnceLock::new();

/// Load every shipped backend once, in registration order.
#[must_use]
pub fn load_bundled_vcs_extensions() -> &'static BundledExtensionLoad {
    BUNDLED_LOAD.get_or_init(|| {
        let definitions = [
            (
                bundled_metadata("jj", "workdeck:bundled/jj"),
                BundledBackend::Jujutsu,
            ),
            (
                bundled_metadata("sl", "workdeck:bundled/sl"),
                BundledBackend::Sapling,
            ),
            (
                bundled_metadata("git", "workdeck:bundled/git"),
                BundledBackend::Git,
            ),
        ];
        let mut extensions = Vec::new();
        let mut adapters = Vec::new();
        let mut issues = Vec::new();
        for (metadata, backend) in definitions {
            match catch_unwind(AssertUnwindSafe(|| bundled_adapter(backend))) {
                Ok(adapter) => {
                    extensions.push(metadata);
                    adapters.push(adapter);
                }
                Err(_) => issues.push(format!(
                    "bundled extension {} failed during registration",
                    metadata.id
                )),
            }
        }
        BundledExtensionLoad {
            extensions,
            catalog: create_base_vcs_catalog(adapters, "git"),
            issues,
        }
    })
}

fn bundled_metadata(id: &'static str, source_path: &'static str) -> BundledExtensionMetadata {
    BundledExtensionMetadata {
        id,
        source_path,
        origin: "bundled",
    }
}

/// Complete catalog registered by the bundled extension tier.
#[must_use]
pub fn bundled_vcs_catalog() -> &'static VcsCatalog {
    &load_bundled_vcs_extensions().catalog
}

/// Backends in resolved registration order.
#[must_use]
pub fn get_bundled_vcs_adapters() -> &'static [VcsAdapter] {
    &bundled_vcs_catalog().adapters
}

fn bundled_adapter(backend: BundledBackend) -> VcsAdapter {
    match backend {
        BundledBackend::Git => create_git_vcs_adapter(GitVcsAdapterOptions::default()),
        BundledBackend::Jujutsu => create_jujutsu_vcs_adapter(JujutsuVcsAdapterOptions::default()),
        BundledBackend::Sapling => create_sapling_vcs_adapter(SaplingVcsAdapterOptions::default()),
    }
}

#[cfg(test)]
mod tests;
