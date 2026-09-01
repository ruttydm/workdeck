//! Product-owned catalog ordering and nearest project-boundary discovery.

use std::path::{Path, PathBuf};

pub const DEFAULT_VCS_PROVIDER_ID: &str = "git";
pub const BUNDLED_VCS_PROVIDER_IDS: &[&str] = &["jj", "sl", "git"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundledVcsCatalog {
    pub default_provider_id: &'static str,
    pub provider_ids: &'static [&'static str],
}

pub const fn bundled_vcs_catalog() -> BundledVcsCatalog {
    BundledVcsCatalog {
        default_provider_id: DEFAULT_VCS_PROVIDER_ID,
        provider_ids: BUNDLED_VCS_PROVIDER_IDS,
    }
}

/// Find the nearest Workdeck or bundled-VCS project boundary.
///
/// Workdeck branding replaces Hunk's `.hunk` bootstrap marker. Directory
/// symlinks are followed by `is_dir`, while marker files are accepted for VCS
/// metadata because `.git` may be a worktree indirection file.
pub fn find_project_root_candidate(cwd: &Path) -> Option<PathBuf> {
    let mut current = cwd.canonicalize().ok()?;
    loop {
        if current.join(".agents/workdeck").is_dir()
            || [".jj", ".sl", ".git"]
                .iter()
                .any(|marker| current.join(marker).exists())
        {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn catalog_owns_bundled_order_fallback_and_reserved_ids() {
        let catalog = bundled_vcs_catalog();
        assert_eq!(catalog.default_provider_id, "git");
        assert_eq!(catalog.provider_ids, ["jj", "sl", "git"]);
    }

    #[test]
    fn workdeck_marker_is_provider_independent_and_must_be_a_directory() {
        let temporary = tempdir().unwrap();
        let nested = temporary.path().join("src/deep");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir_all(temporary.path().join(".agents/workdeck")).unwrap();
        assert_eq!(
            find_project_root_candidate(&nested),
            Some(temporary.path().canonicalize().unwrap())
        );

        let other = tempdir().unwrap();
        let nested = other.path().join("src");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir_all(other.path().join(".agents")).unwrap();
        fs::write(other.path().join(".agents/workdeck"), "not a directory\n").unwrap();
        assert_eq!(find_project_root_candidate(&nested), None);
    }

    #[test]
    fn nearest_nested_checkout_wins_over_an_outer_checkout() {
        let temporary = tempdir().unwrap();
        let inner = temporary.path().join("vendor/nested");
        let source = inner.join("src");
        fs::create_dir_all(temporary.path().join(".git")).unwrap();
        fs::create_dir_all(inner.join(".git")).unwrap();
        fs::create_dir_all(&source).unwrap();
        assert_eq!(
            find_project_root_candidate(&source),
            Some(inner.canonicalize().unwrap())
        );
    }
}
