//! Session-scoped state and side effects for repository-extension trust.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use workdeck_extension_host::TrustDecision;

use crate::next_extension_trust_prompt_root;

const WRITE_FAILURE_NOTICE: &str = "Failed to record the trust decision.";
const RELOAD_FAILURE_NOTICE: &str = "Failed to reload after trusting this repository's extensions.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionTrustWriteError {
    Message(String),
    Unknown,
}

impl ExtensionTrustWriteError {
    pub fn notice(self) -> String {
        match self {
            Self::Message(message) => message,
            Self::Unknown => WRITE_FAILURE_NOTICE.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionTrustHostError {
    Write(ExtensionTrustWriteError),
}

type ExtensionTrustHandlerFn =
    dyn Fn(&Path, TrustDecision) -> Result<(), ExtensionTrustHostError> + Send + Sync;

/// Cloneable composition-root callback for persistence and native-extension loading.
#[derive(Clone)]
pub struct ExtensionTrustHandler(Arc<ExtensionTrustHandlerFn>);

impl ExtensionTrustHandler {
    pub fn new<F>(handler: F) -> Self
    where
        F: Fn(&Path, TrustDecision) -> Result<(), ExtensionTrustHostError> + Send + Sync + 'static,
    {
        Self(Arc::new(handler))
    }

    pub fn run(
        &self,
        repo_root: &Path,
        decision: TrustDecision,
    ) -> Result<(), ExtensionTrustHostError> {
        (self.0)(repo_root, decision)
    }
}

impl fmt::Debug for ExtensionTrustHandler {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ExtensionTrustHandler(..)")
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtensionTrustOutcome {
    pub wrote: Option<(PathBuf, TrustDecision)>,
    pub reloaded_extensions: bool,
    pub notice: Option<String>,
}

/// Tracks the one currently eligible prompt and every repository offered this app session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtensionTrustController {
    prompt_root: Option<PathBuf>,
    offered_repo_roots: BTreeSet<PathBuf>,
}

impl ExtensionTrustController {
    /// Reconcile prompt state after pager mode or the pending repository changes.
    pub fn reconcile(&mut self, pager_mode: bool, pending_repo_root: Option<&Path>) {
        let next = next_extension_trust_prompt_root(
            !pager_mode,
            pending_repo_root,
            &self.offered_repo_roots,
        );
        if let Some(next) = next {
            self.offered_repo_roots.insert(next.clone());
            self.prompt_root = Some(next);
            return;
        }

        let current_is_still_eligible = !pager_mode
            && self
                .prompt_root
                .as_deref()
                .zip(pending_repo_root)
                .is_some_and(|(current, pending)| current == pending);
        if !current_is_still_eligible {
            self.prompt_root = None;
        }
    }

    #[must_use]
    pub fn prompt_open(&self) -> bool {
        self.prompt_root.is_some()
    }

    #[must_use]
    pub fn prompt_root(&self) -> Option<&Path> {
        self.prompt_root.as_deref()
    }

    /// Dismiss this repository for the current session without persisting a decision.
    pub fn close(&mut self) {
        self.prompt_root = None;
    }

    /// Persist trust first, then request one extension-aware refresh when the input supports it.
    pub fn trust_repo_extensions<W, R>(
        &mut self,
        can_refresh_current_input: bool,
        write_trust: W,
        refresh_current_input: R,
    ) -> ExtensionTrustOutcome
    where
        W: FnOnce(&Path, TrustDecision) -> Result<(), ExtensionTrustWriteError>,
        R: FnOnce() -> Result<(), ()>,
    {
        let Some(repo_root) = self.prompt_root.take() else {
            return ExtensionTrustOutcome::default();
        };
        if let Err(error) = write_trust(&repo_root, TrustDecision::Trusted) {
            return ExtensionTrustOutcome {
                notice: Some(error.notice()),
                ..ExtensionTrustOutcome::default()
            };
        }
        let mut outcome = ExtensionTrustOutcome {
            wrote: Some((repo_root, TrustDecision::Trusted)),
            ..ExtensionTrustOutcome::default()
        };
        if !can_refresh_current_input {
            outcome.notice =
                Some("Trusted this repository • restart Workdeck to load its extensions".into());
            return outcome;
        }
        if refresh_current_input().is_err() {
            outcome.notice = Some(RELOAD_FAILURE_NOTICE.into());
        } else {
            outcome.reloaded_extensions = true;
        }
        outcome
    }

    /// Persist denial without refreshing; a denied repository must remain unloaded.
    pub fn deny_repo_extensions<W>(&mut self, write_trust: W) -> ExtensionTrustOutcome
    where
        W: FnOnce(&Path, TrustDecision) -> Result<(), ExtensionTrustWriteError>,
    {
        let Some(repo_root) = self.prompt_root.take() else {
            return ExtensionTrustOutcome::default();
        };
        match write_trust(&repo_root, TrustDecision::Denied) {
            Ok(()) => ExtensionTrustOutcome {
                wrote: Some((repo_root, TrustDecision::Denied)),
                notice: Some("Won't run this repository's extensions".into()),
                ..ExtensionTrustOutcome::default()
            },
            Err(error) => ExtensionTrustOutcome {
                notice: Some(error.notice()),
                ..ExtensionTrustOutcome::default()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn offered() -> ExtensionTrustController {
        let mut controller = ExtensionTrustController::default();
        controller.reconcile(false, Some(Path::new("/repo/alpha")));
        controller
    }

    #[test]
    fn suppresses_in_pager_mode_then_offers_when_pager_ends() {
        let mut controller = ExtensionTrustController::default();
        controller.reconcile(true, Some(Path::new("/repo/alpha")));
        assert!(!controller.prompt_open());
        controller.reconcile(false, Some(Path::new("/repo/alpha")));
        assert_eq!(controller.prompt_root(), Some(Path::new("/repo/alpha")));
    }

    #[test]
    fn dismissed_root_stays_closed_changed_root_opens_and_cleared_pending_closes() {
        let mut controller = offered();
        controller.close();
        controller.reconcile(false, Some(Path::new("/repo/alpha")));
        assert!(!controller.prompt_open());
        controller.reconcile(false, Some(Path::new("/repo/beta")));
        assert_eq!(controller.prompt_root(), Some(Path::new("/repo/beta")));
        controller.reconcile(false, Some(Path::new("/repo/alpha")));
        assert!(!controller.prompt_open());
        controller.reconcile(false, None);
        assert!(!controller.prompt_open());
    }

    #[test]
    fn pager_temporarily_hides_without_reopening_an_offered_root() {
        let mut controller = offered();
        controller.reconcile(true, Some(Path::new("/repo/alpha")));
        assert!(!controller.prompt_open());
        controller.reconcile(false, Some(Path::new("/repo/alpha")));
        assert!(!controller.prompt_open());
    }

    #[test]
    fn repeated_reconciliation_preserves_initial_offer_and_same_root_dismissal() {
        let mut controller = offered();
        controller.reconcile(false, Some(Path::new("/repo/alpha")));
        assert!(controller.prompt_open());
        controller.close();
        controller.reconcile(false, Some(Path::new("/repo/alpha")));
        assert!(!controller.prompt_open());
    }

    #[test]
    fn trust_writes_then_requests_one_extension_aware_refresh() {
        let mut controller = offered();
        let writes = RefCell::new(Vec::new());
        let refreshes = RefCell::new(0);
        let outcome = controller.trust_repo_extensions(
            true,
            |root, decision| {
                writes.borrow_mut().push((root.to_owned(), decision));
                Ok(())
            },
            || {
                *refreshes.borrow_mut() += 1;
                Ok(())
            },
        );
        assert_eq!(
            writes.into_inner(),
            [(PathBuf::from("/repo/alpha"), TrustDecision::Trusted)]
        );
        assert_eq!(refreshes.into_inner(), 1);
        assert!(outcome.reloaded_extensions);
        assert!(outcome.notice.is_none());
        assert!(!controller.prompt_open());
    }

    #[test]
    fn trust_on_nonreloadable_input_defers_loading_until_restart() {
        let mut controller = offered();
        let outcome = controller.trust_repo_extensions(false, |_, _| Ok(()), || panic!());
        assert_eq!(
            outcome.notice.as_deref(),
            Some("Trusted this repository • restart Workdeck to load its extensions")
        );
        assert!(!outcome.reloaded_extensions);
    }

    #[test]
    fn trust_write_failure_is_contained_and_does_not_refresh() {
        let mut controller = offered();
        let outcome = controller.trust_repo_extensions(
            true,
            |_, _| {
                Err(ExtensionTrustWriteError::Message(
                    "state is read-only".into(),
                ))
            },
            || panic!(),
        );
        assert_eq!(outcome.notice.as_deref(), Some("state is read-only"));
        assert!(outcome.wrote.is_none());
        assert!(!controller.prompt_open());
    }

    #[test]
    fn rejected_trust_reload_reports_failure_without_reopening() {
        let mut controller = offered();
        let outcome = controller.trust_repo_extensions(true, |_, _| Ok(()), || Err(()));
        assert_eq!(outcome.notice.as_deref(), Some(RELOAD_FAILURE_NOTICE));
        controller.reconcile(false, Some(Path::new("/repo/alpha")));
        assert!(!controller.prompt_open());
    }

    #[test]
    fn denial_is_persisted_and_reports_confirmation_without_refreshing() {
        let mut controller = offered();
        let writes = RefCell::new(Vec::new());
        let outcome = controller.deny_repo_extensions(|root, decision| {
            writes.borrow_mut().push((root.to_owned(), decision));
            Ok(())
        });
        assert_eq!(
            writes.into_inner(),
            [(PathBuf::from("/repo/alpha"), TrustDecision::Denied)]
        );
        assert_eq!(
            outcome.notice.as_deref(),
            Some("Won't run this repository's extensions")
        );
    }

    #[test]
    fn unknown_denial_failure_uses_safe_fallback_and_closes() {
        let mut controller = offered();
        let outcome =
            controller.deny_repo_extensions(|_, _| Err(ExtensionTrustWriteError::Unknown));
        assert_eq!(outcome.notice.as_deref(), Some(WRITE_FAILURE_NOTICE));
        assert!(!controller.prompt_open());
    }
}
