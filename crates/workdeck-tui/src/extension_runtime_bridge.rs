use std::sync::{Arc, Mutex, Weak};

use workdeck_core::Changeset;
use workdeck_core::ReviewSnapshot;
use workdeck_extension_api::{
    ExtensionCommandAvailability, ExtensionDiffFile, ExtensionFileSide, ExtensionHostAction,
    ExtensionReviewSelection, ExtensionReviewSnapshot,
};

/// One render's extension-visible facts after the Ratatui app commits them.
#[derive(Debug, Clone)]
pub struct ExtensionRuntimeCommit {
    pub registry_generation: u64,
    pub review_generation: u64,
    pub snapshot: SharedRuntimeSnapshot,
    pub review: ExtensionReviewSnapshot,
    pub files: Arc<DeferredFileProjections>,
    pub selection: ExtensionReviewSelection,
    pub selected_file_id: Option<String>,
    pub commands: ExtensionCommandAvailability,
}

/// Internal immutable storage; public snapshot readers still receive owned values.
#[derive(Debug, Clone)]
pub struct SharedRuntimeSnapshot {
    pub generation: u64,
    pub changeset: Arc<Changeset>,
    pub selection: workdeck_core::ReviewSelection,
}

impl SharedRuntimeSnapshot {
    fn to_owned_snapshot(&self) -> ReviewSnapshot {
        ReviewSnapshot {
            generation: self.generation,
            changeset: self.changeset.as_ref().clone(),
            selection: self.selection,
        }
    }
}

impl From<ReviewSnapshot> for SharedRuntimeSnapshot {
    fn from(snapshot: ReviewSnapshot) -> Self {
        Self {
            generation: snapshot.generation,
            changeset: Arc::new(snapshot.changeset),
            selection: snapshot.selection,
        }
    }
}

/// One immutable document's projections; replacing the Arc retires the cache.
#[derive(Debug, Default)]
pub(crate) struct ExtensionFileProjectionCache {
    document: Option<Arc<Changeset>>,
    files: Option<Arc<DeferredFileProjections>>,
}

#[derive(Debug)]
enum FileProjectionState {
    Pending(Arc<Changeset>),
    Ready(Arc<[ExtensionDiffFile]>),
}

/// Retain exact document authority without eagerly allocating opaque JSON metadata.
/// Once materialized, only detached projections are retained by this handle.
#[derive(Debug)]
pub struct DeferredFileProjections(Mutex<FileProjectionState>);

impl DeferredFileProjections {
    #[cfg(test)]
    pub(crate) fn is_materialized(&self) -> bool {
        matches!(*self.0.lock().unwrap(), FileProjectionState::Ready(_))
    }

    fn new(document: Arc<Changeset>) -> Self {
        Self(Mutex::new(FileProjectionState::Pending(document)))
    }

    pub(crate) fn resolve(&self) -> Arc<[ExtensionDiffFile]> {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let FileProjectionState::Pending(document) = &*state {
            let files = document
                .files
                .iter()
                .map(workdeck_extension_host::project_extension_diff_file)
                .collect::<Vec<_>>()
                .into();
            *state = FileProjectionState::Ready(files);
        }
        let FileProjectionState::Ready(files) = &*state else {
            unreachable!()
        };
        Arc::clone(files)
    }

    fn contains_target(&self, file_id: &str, hunk_index: Option<usize>) -> bool {
        let state = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let hunk_count = match &*state {
            FileProjectionState::Pending(document) => document
                .files
                .iter()
                .find(|file| file.runtime_id == file_id)
                .map(|file| file.hunks.len()),
            FileProjectionState::Ready(files) => files
                .iter()
                .find(|file| file.id == file_id)
                .map(|file| file.hunks.len()),
        };
        hunk_count.is_some_and(|count| hunk_index.is_none_or(|index| index < count))
    }
}

#[cfg(test)]
impl From<Vec<ExtensionDiffFile>> for DeferredFileProjections {
    fn from(files: Vec<ExtensionDiffFile>) -> Self {
        Self(Mutex::new(FileProjectionState::Ready(files.into())))
    }
}

impl ExtensionFileProjectionCache {
    pub(crate) fn get(&mut self, document: Arc<Changeset>) -> Arc<DeferredFileProjections> {
        if self
            .document
            .as_ref()
            .is_none_or(|previous| !Arc::ptr_eq(previous, &document))
        {
            self.files = Some(Arc::new(DeferredFileProjections::new(Arc::clone(
                &document,
            ))));
            self.document = Some(document);
        }
        Arc::clone(
            self.files
                .as_ref()
                .expect("document projection initialized"),
        )
    }
}

#[derive(Debug)]
struct ExtensionRuntimeState {
    app_alive: bool,
    binding_revision: u64,
    committed: ExtensionRuntimeCommit,
}

/// Committed extension authority shared by commands, review readers, and navigation.
///
/// React layout effects are not part of Ratatui's synchronous renderer. The native equivalent is
/// an owned commit boundary: `ReviewApp` publishes a complete immutable projection before any
/// lifecycle callback, and retained controls validate their captured registry/review generation
/// against this state each time they are used.
#[derive(Debug, Clone)]
pub struct ExtensionRuntimeBridge {
    state: Arc<Mutex<ExtensionRuntimeState>>,
}

impl ExtensionRuntimeBridge {
    #[must_use]
    pub fn new(committed: ExtensionRuntimeCommit) -> Self {
        Self {
            state: Arc::new(Mutex::new(ExtensionRuntimeState {
                app_alive: true,
                binding_revision: 1,
                committed,
            })),
        }
    }

    /// Restore authority after a mount replay and atomically replace every committed fact.
    pub fn commit(&self, committed: ExtensionRuntimeCommit) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.app_alive = true;
        state.binding_revision = state.binding_revision.saturating_add(1);
        state.committed = committed;
    }

    /// Revoke every control minted by this mounted app before successor lifecycle publication.
    pub fn retire_mount(&self) {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .app_alive = false;
    }

    #[must_use]
    pub fn command_controls(&self) -> ExtensionRuntimeCommandControls {
        let registry_generation = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .committed
            .registry_generation;
        ExtensionRuntimeCommandControls {
            lease: ExtensionRuntimeCapabilityLease {
                state: Arc::downgrade(&self.state),
                registry_generation,
                review_generation: None,
            },
        }
    }

    #[must_use]
    pub fn create_review_capability_lease(&self) -> ExtensionRuntimeCapabilityLease {
        let committed = &self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .committed;
        ExtensionRuntimeCapabilityLease {
            state: Arc::downgrade(&self.state),
            registry_generation: committed.registry_generation,
            review_generation: Some(committed.review_generation),
        }
    }

    #[must_use]
    pub fn create_navigation(&self, extension_id: impl Into<String>) -> ExtensionRuntimeNavigation {
        ExtensionRuntimeNavigation {
            extension_id: extension_id.into(),
            lease: self.create_review_capability_lease(),
        }
    }

    #[must_use]
    pub fn create_review_controls(&self) -> ExtensionRuntimeReviewControls {
        ExtensionRuntimeReviewControls {
            lease: self.create_review_capability_lease(),
        }
    }

    #[must_use]
    pub fn get_committed_file_views(&self) -> Vec<ExtensionDiffFile> {
        let files = Arc::clone(
            &self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .committed
                .files,
        );
        files.resolve().to_vec()
    }

    /// Project the immutable file values belonging to a render before that render is committed.
    #[must_use]
    pub fn get_render_file_views(&self, render: &ExtensionRuntimeCommit) -> Vec<ExtensionDiffFile> {
        render.files.resolve().to_vec()
    }

    #[must_use]
    pub fn get_selection(&self) -> ExtensionReviewSelection {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .committed
            .selection
            .clone()
    }

    /// Project selection from a render-in-progress without mutating committed command facts.
    #[must_use]
    pub fn get_render_selection(
        &self,
        render: &ExtensionRuntimeCommit,
    ) -> ExtensionReviewSelection {
        render.selection.clone()
    }

    #[must_use]
    pub fn get_selected_file_id(&self) -> Option<String> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .committed
            .selected_file_id
            .clone()
    }

    #[must_use]
    pub fn committed_review(&self) -> ExtensionRuntimeCommittedReview {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        ExtensionRuntimeCommittedReview {
            snapshot: state.committed.snapshot.to_owned_snapshot(),
            review: state.committed.review.clone(),
        }
    }
}

/// Runtime or review-scoped authority retained across asynchronous native callbacks.
#[derive(Debug, Clone)]
pub struct ExtensionRuntimeCapabilityLease {
    state: Weak<Mutex<ExtensionRuntimeState>>,
    registry_generation: u64,
    review_generation: Option<u64>,
}

impl ExtensionRuntimeCapabilityLease {
    #[must_use]
    pub fn is_live(&self) -> bool {
        let Some(state) = self.state.upgrade() else {
            return false;
        };
        let state = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.app_alive
            && state.committed.registry_generation == self.registry_generation
            && self
                .review_generation
                .is_none_or(|generation| state.committed.review_generation == generation)
    }
}

#[derive(Debug, Clone)]
pub struct ExtensionRuntimeCommandControls {
    lease: ExtensionRuntimeCapabilityLease,
}

impl ExtensionRuntimeCommandControls {
    #[must_use]
    pub fn availability(&self) -> ExtensionCommandAvailability {
        let Some(state) = self.lease.state.upgrade() else {
            return ExtensionCommandAvailability::default();
        };
        let state = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !self.lease.is_live_against(&state) {
            return ExtensionCommandAvailability::default();
        }
        state.committed.commands.clone()
    }

    #[must_use]
    pub fn is_enabled(&self, command_id: &str) -> bool {
        self.availability().is_enabled(command_id)
    }

    pub fn execute(
        &self,
        command_id: &str,
        count: Option<u16>,
    ) -> Result<Option<ExtensionHostAction>, workdeck_extension_api::ExtensionCommandExecutionError>
    {
        self.availability().execute(command_id, count)
    }
}

impl ExtensionRuntimeCapabilityLease {
    fn is_live_against(&self, state: &ExtensionRuntimeState) -> bool {
        state.app_alive
            && state.committed.registry_generation == self.registry_generation
            && self
                .review_generation
                .is_none_or(|generation| state.committed.review_generation == generation)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionRuntimeNavigationTarget {
    File {
        file_id: String,
    },
    Hunk {
        file_id: String,
        hunk_index: usize,
    },
    Line {
        file_id: String,
        side: ExtensionFileSide,
        line: u32,
    },
}

/// A navigation request resolved from the latest committed bindings, never invocation-time data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedExtensionRuntimeNavigation {
    pub extension_id: String,
    pub binding_revision: u64,
    pub selected_file_id: Option<String>,
    pub target: ExtensionRuntimeNavigationTarget,
}

#[derive(Debug, Clone)]
pub struct ExtensionRuntimeNavigation {
    extension_id: String,
    lease: ExtensionRuntimeCapabilityLease,
}

impl ExtensionRuntimeNavigation {
    #[must_use]
    pub fn is_live(&self) -> bool {
        self.lease.is_live()
    }

    #[must_use]
    pub fn select_file(&self, file_id: &str) -> Option<ResolvedExtensionRuntimeNavigation> {
        self.resolve(
            file_id,
            None,
            ExtensionRuntimeNavigationTarget::File {
                file_id: file_id.to_owned(),
            },
        )
    }

    #[must_use]
    pub fn select_hunk(
        &self,
        file_id: &str,
        hunk_index: usize,
    ) -> Option<ResolvedExtensionRuntimeNavigation> {
        self.resolve(
            file_id,
            Some(hunk_index),
            ExtensionRuntimeNavigationTarget::Hunk {
                file_id: file_id.to_owned(),
                hunk_index,
            },
        )
    }

    #[must_use]
    pub fn reveal_line(
        &self,
        file_id: &str,
        side: ExtensionFileSide,
        line: u32,
    ) -> Option<ResolvedExtensionRuntimeNavigation> {
        self.resolve(
            file_id,
            None,
            ExtensionRuntimeNavigationTarget::Line {
                file_id: file_id.to_owned(),
                side,
                line,
            },
        )
    }

    fn resolve(
        &self,
        file_id: &str,
        hunk_index: Option<usize>,
        target: ExtensionRuntimeNavigationTarget,
    ) -> Option<ResolvedExtensionRuntimeNavigation> {
        let state = self.lease.state.upgrade()?;
        let state = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !self.lease.is_live_against(&state) {
            return None;
        }
        if !state.committed.files.contains_target(file_id, hunk_index) {
            return None;
        }
        Some(ResolvedExtensionRuntimeNavigation {
            extension_id: self.extension_id.clone(),
            binding_revision: state.binding_revision,
            selected_file_id: state.committed.selected_file_id.clone(),
            target,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ExtensionRuntimeReviewControls {
    lease: ExtensionRuntimeCapabilityLease,
}

impl ExtensionRuntimeReviewControls {
    #[must_use]
    pub fn snapshot(&self) -> Option<ExtensionReviewSnapshot> {
        let state = self.lease.state.upgrade()?;
        let state = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.lease
            .is_live_against(&state)
            .then(|| state.committed.review.clone())
    }
}

#[derive(Debug, Clone)]
pub struct ExtensionRuntimeCommittedReview {
    pub snapshot: ReviewSnapshot,
    pub review: ExtensionReviewSnapshot,
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_core::{Changeset, ChangesetSource, ReviewSelection, ReviewSnapshot};
    use workdeck_extension_api::{ExtensionDiffHunk, ExtensionDiffStats};

    fn file(id: &str, hunk_count: usize) -> ExtensionDiffFile {
        ExtensionDiffFile {
            id: id.into(),
            path: format!("{id}.ts"),
            previous_path: None,
            patch: String::new(),
            language: Some("typescript".into()),
            stats: ExtensionDiffStats {
                additions: hunk_count,
                deletions: hunk_count,
            },
            metadata: serde_json::json!({ "hunks": [] }),
            change_type: Some(workdeck_extension_api::ExtensionVcsFileChangeType::Change),
            stats_truncated: false,
            hunks: (0..hunk_count)
                .map(|index| ExtensionDiffHunk {
                    index,
                    header: format!("@@ hunk {index} @@"),
                    old_range: Some([1, 1]),
                    new_range: Some([1, 1]),
                })
                .collect(),
            agent: None,
            is_untracked: false,
            is_binary: false,
            is_too_large: false,
        }
    }

    fn core_snapshot(id: &str) -> ReviewSnapshot {
        ReviewSnapshot {
            generation: 1,
            changeset: Changeset {
                id: id.into(),
                source_label: id.into(),
                title: id.into(),
                summary: None,
                agent_summary: None,
                source: ChangesetSource::Patch { label: id.into() },
                files: Vec::new(),
            },
            selection: ReviewSelection::default(),
        }
    }

    #[test]
    fn deferred_projection_keeps_navigation_cold_and_materializes_once_for_readers() {
        let mut document = workdeck_diff::parse_patch(
            "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n",
            "test",
            "test",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap();
        // Duplicate IDs preserve the existing first-match navigation contract.
        let mut duplicate = document.files[0].clone();
        duplicate.hunks.push(duplicate.hunks[0].clone());
        document.files.push(duplicate);
        let id = document.files[0].runtime_id.clone();
        let expected = document
            .files
            .iter()
            .map(workdeck_extension_host::project_extension_diff_file)
            .collect::<Vec<_>>();
        let document = Arc::new(document);
        let retained = Arc::downgrade(&document);
        let projection = Arc::new(DeferredFileProjections::new(document));
        assert!(!projection.is_materialized());
        assert!(projection.contains_target(&id, None));
        assert!(projection.contains_target(&id, Some(0)));
        assert!(!projection.contains_target(&id, Some(1)));
        assert!(!projection.contains_target("missing", None));
        assert!(!projection.is_materialized());
        assert!(retained.upgrade().is_some());
        let readers = (0..4)
            .map(|_| {
                let projection = Arc::clone(&projection);
                std::thread::spawn(move || projection.resolve())
            })
            .collect::<Vec<_>>();
        let resolved = readers
            .into_iter()
            .map(|reader| reader.join().unwrap())
            .collect::<Vec<_>>();
        assert!(projection.is_materialized());
        assert!(
            retained.upgrade().is_none(),
            "materialized handle retires its source document"
        );
        for files in &resolved {
            assert_eq!(files.as_ref(), expected.as_slice());
            assert!(Arc::ptr_eq(files, &resolved[0]));
        }
        assert!(projection.contains_target(&id, Some(0)));
        assert!(!projection.contains_target(&id, Some(1)));
    }

    #[test]
    fn shared_runtime_snapshot_retains_document_but_public_reads_are_detached() {
        let shared: SharedRuntimeSnapshot = core_snapshot("original").into();
        let retained = shared.clone();
        assert!(Arc::ptr_eq(&shared.changeset, &retained.changeset));
        let mut owned = shared.to_owned_snapshot();
        owned.changeset.id = "reader mutation".into();
        owned.generation += 1;
        assert_eq!(shared.changeset.id, "original");
        assert_eq!(retained.to_owned_snapshot().changeset.id, "original");
        assert_ne!(owned.generation, shared.generation);
    }

    fn commands() -> ExtensionCommandAvailability {
        ExtensionCommandAvailability {
            enabled: vec!["workdeck.test.run".into()],
        }
    }

    fn commit(
        registry_generation: u64,
        review_generation: u64,
        file_id: &str,
    ) -> ExtensionRuntimeCommit {
        let selected = file(file_id, 1);
        ExtensionRuntimeCommit {
            registry_generation,
            review_generation,
            snapshot: core_snapshot(&format!("runtime:{review_generation}")).into(),
            review: ExtensionReviewSnapshot {
                generation: format!("runtime:{review_generation}"),
                ..ExtensionReviewSnapshot::default()
            },
            files: Arc::new(vec![selected.clone()].into()),
            selection: ExtensionReviewSelection {
                file: Some(selected),
                hunk_index: Some(0),
                current_line: None,
                files: Vec::new(),
            },
            selected_file_id: Some(file_id.into()),
            commands: commands(),
        }
    }

    #[test]
    fn frozen_hunk_runtime_bridge_oracle_records_executed_source_and_test_coverage() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/extension-runtime-bridge.json"
        ))
        .unwrap();
        assert_eq!(
            oracle["baseline"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(oracle["stable"], "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd");
        assert_eq!(oracle["stableOracle"]["status"], "absent");
        assert_eq!(oracle["baselineOracle"]["passed"], 5);
        assert_eq!(oracle["baselineOracle"]["assertions"], 25);
        assert_eq!(oracle["testMappings"].as_array().unwrap().len(), 5);
    }

    #[test]
    fn restores_authority_after_mount_replay_and_revokes_it_during_cleanup() {
        let bridge = ExtensionRuntimeBridge::new(commit(1, 1, "alpha"));
        let controls = bridge.command_controls();
        bridge.retire_mount();
        assert!(!controls.is_enabled("workdeck.test.run"));

        bridge.commit(commit(1, 1, "alpha"));
        assert!(controls.is_enabled("workdeck.test.run"));
        assert!(matches!(
            controls.execute("workdeck.test.run", None).unwrap(),
            Some(ExtensionHostAction::ExecuteReviewCommand { .. })
        ));

        bridge.retire_mount();
        assert!(!controls.is_enabled("workdeck.test.run"));
        assert_eq!(controls.execute("workdeck.test.run", None).unwrap(), None);
    }

    #[test]
    fn hard_remount_retires_predecessor_while_same_registry_successor_stays_live() {
        let predecessor = ExtensionRuntimeBridge::new(commit(1, 1, "alpha"));
        let predecessor_controls = predecessor.command_controls();
        predecessor.retire_mount();

        let successor = ExtensionRuntimeBridge::new(commit(1, 2, "alpha"));
        let successor_controls = successor.command_controls();
        assert!(!predecessor_controls.is_enabled("workdeck.test.run"));
        assert!(successor_controls.is_enabled("workdeck.test.run"));
    }

    #[test]
    fn runtime_commands_survive_content_reload_while_review_controls_expire() {
        let bridge = ExtensionRuntimeBridge::new(commit(1, 1, "alpha"));
        let command_controls = bridge.command_controls();
        let predecessor_lease = bridge.create_review_capability_lease();
        let predecessor_navigation = bridge.create_navigation("probe");
        let predecessor_review = bridge.create_review_controls();
        assert_eq!(
            predecessor_review.snapshot().unwrap().generation,
            "runtime:1"
        );

        bridge.commit(commit(1, 2, "alpha"));

        assert!(command_controls.is_enabled("workdeck.test.run"));
        assert!(!predecessor_lease.is_live());
        assert!(predecessor_review.snapshot().is_none());
        assert!(predecessor_navigation.select_file("alpha").is_none());
        assert!(bridge.create_review_capability_lease().is_live());
        assert_eq!(
            bridge
                .create_review_controls()
                .snapshot()
                .unwrap()
                .generation,
            "runtime:2"
        );
        assert!(
            bridge
                .create_navigation("probe")
                .select_file("alpha")
                .is_some()
        );
    }

    #[test]
    fn replacing_registry_retires_predecessor_controls_without_driving_successor() {
        let bridge = ExtensionRuntimeBridge::new(commit(1, 1, "alpha"));
        let stale = bridge.command_controls();
        bridge.commit(commit(2, 1, "alpha"));

        assert!(!stale.is_enabled("workdeck.test.run"));
        assert!(bridge.command_controls().is_enabled("workdeck.test.run"));
    }

    #[test]
    fn shared_file_projections_keep_public_getters_deeply_owned() {
        let initial = commit(1, 1, "alpha");
        let shared = initial.files.resolve();
        let bridge = ExtensionRuntimeBridge::new(initial);
        let mut exposed = bridge.get_committed_file_views();
        exposed[0].path = "edited.rs".into();
        exposed[0].metadata["hunks"] = serde_json::json!([{"changed": true}]);
        exposed[0].hunks[0].header = "edited header".into();
        assert_eq!(bridge.get_committed_file_views(), shared.as_ref());
        let next = commit(1, 1, "beta");
        let mut preview = bridge.get_render_file_views(&next);
        preview[0].metadata["hunks"] = serde_json::json!([42]);
        assert_ne!(preview[0].metadata, next.files.resolve()[0].metadata);
        bridge.commit(next);
        assert_eq!(shared[0].id, "alpha");
        assert_eq!(bridge.get_committed_file_views()[0].id, "beta");
    }

    #[test]
    fn invocation_selection_is_frozen_while_navigation_reads_latest_commit() {
        let bridge = ExtensionRuntimeBridge::new(commit(1, 1, "alpha"));
        let selection = bridge.get_selection();
        let navigation = bridge.create_navigation("probe");
        let first_revision = navigation.select_file("alpha").unwrap().binding_revision;

        let render = commit(1, 1, "beta");
        assert_eq!(bridge.get_render_file_views(&render)[0].id, "beta");
        assert_eq!(
            bridge.get_render_selection(&render).file.unwrap().id,
            "beta"
        );
        assert_eq!(bridge.get_committed_file_views()[0].id, "alpha");

        bridge.commit(render);

        assert_eq!(selection.file.unwrap().id, "alpha");
        let resolved = navigation.select_file("beta").unwrap();
        assert_eq!(resolved.selected_file_id.as_deref(), Some("beta"));
        assert!(resolved.binding_revision > first_revision);
        assert_eq!(bridge.get_selected_file_id().as_deref(), Some("beta"));
        assert_eq!(bridge.get_committed_file_views()[0].id, "beta");
        assert!(matches!(
            navigation.select_hunk("beta", 0).unwrap().target,
            ExtensionRuntimeNavigationTarget::Hunk { hunk_index: 0, .. }
        ));
        assert!(navigation.select_hunk("beta", 1).is_none());
        assert!(matches!(
            navigation
                .reveal_line("beta", ExtensionFileSide::New, 7)
                .unwrap()
                .target,
            ExtensionRuntimeNavigationTarget::Line { line: 7, .. }
        ));
        let committed = bridge.committed_review();
        assert_eq!(committed.review.generation, "runtime:1");
        assert_eq!(committed.snapshot.changeset.id, "runtime:1");
    }
}
