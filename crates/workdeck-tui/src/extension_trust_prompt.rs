//! Session-scoped selection for the repository-extension trust prompt.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Decide which repository root, if any, should receive a native-extension trust prompt.
///
/// The pending root is live review state rather than a launch-time constant: reloads may move the
/// review to another repository. Callers add a returned root to `offered_repo_roots` before opening
/// the prompt so dismissing it remains an answer for that repository for the rest of the session.
#[must_use]
pub fn next_extension_trust_prompt_root(
    enabled: bool,
    pending_repo_root: Option<&Path>,
    offered_repo_roots: &BTreeSet<PathBuf>,
) -> Option<PathBuf> {
    let pending_repo_root = pending_repo_root.filter(|root| !root.as_os_str().is_empty())?;
    if !enabled || offered_repo_roots.contains(pending_repo_root) {
        return None;
    }
    Some(pending_repo_root.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_returns_a_nonempty_pending_root_when_prompts_are_enabled() {
        let offered = BTreeSet::new();
        assert_eq!(
            next_extension_trust_prompt_root(true, Some(Path::new("/repo/alpha")), &offered),
            Some(PathBuf::from("/repo/alpha"))
        );
        assert_eq!(
            next_extension_trust_prompt_root(false, Some(Path::new("/repo/alpha")), &offered),
            None
        );
        assert_eq!(next_extension_trust_prompt_root(true, None, &offered), None);
        assert_eq!(
            next_extension_trust_prompt_root(true, Some(Path::new("")), &offered),
            None
        );
    }

    #[test]
    fn a_root_already_offered_this_session_stays_dismissed() {
        let offered = BTreeSet::from([PathBuf::from("/repo/alpha")]);
        assert_eq!(
            next_extension_trust_prompt_root(true, Some(Path::new("/repo/alpha")), &offered),
            None
        );
    }

    #[test]
    fn changing_repository_reopens_the_question_for_the_new_root() {
        let offered = BTreeSet::from([PathBuf::from("/repo/alpha")]);
        assert_eq!(
            next_extension_trust_prompt_root(true, Some(Path::new("/repo/beta")), &offered),
            Some(PathBuf::from("/repo/beta"))
        );
    }
}
