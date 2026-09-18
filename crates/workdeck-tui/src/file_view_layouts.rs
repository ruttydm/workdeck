//! Bounded native file-view preparation, cache identity, and refresh lifetime.
//!
//! This is the explicit-controller Rust port of Hunk's
//! `src/ui/fileViews/useFileViews.ts` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. React effect cleanup becomes
//! cancellation attached to a preparation pass, while render-time projection
//! remains synchronous so stale width, selection, file, and registration
//! geometry can never reach Ratatui's cell buffer.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use workdeck_extension_api::ValidatedFileViewLayout;
use workdeck_extension_host::ExtensionRequestCancellation;

use crate::file_presentation_rendering::ResolvedFileViewLayout;

/// Bound one third-party layout request so raw diff cannot wait indefinitely.
pub const FILE_VIEW_LAYOUT_TIMEOUT: Duration = Duration::from_millis(1_500);
/// Keep extension preparation parallel but bounded across a large changeset.
pub const FILE_VIEW_LAYOUT_CONCURRENCY: usize = 4;
/// Coalesce rapid width changes without ever painting geometry for a stale width.
pub const FILE_VIEW_LAYOUT_RESIZE_DEBOUNCE: Duration = Duration::from_millis(50);
/// Retain a bounded set of prepared trees across file, view, and resize churn.
pub const FILE_VIEW_LAYOUT_CACHE_MAX_ENTRIES: usize = 64;
/// Bound warning-deduplication metadata for one controller lifetime.
pub const FILE_VIEW_LAYOUT_ISSUE_MAX_ENTRIES: usize = 256;

/// Complete semantic identity of one prepared native file-view tree.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FileViewLayoutIdentity {
    pub file_id: String,
    pub file_path: String,
    pub content_identity: String,
    pub view_key: String,
    pub extension_id: String,
    pub view_id: String,
    pub registration_identity: u64,
    pub width: usize,
    pub epoch: u64,
}

impl FileViewLayoutIdentity {
    fn same_prepared_tree(&self, other: &Self) -> bool {
        self.file_id == other.file_id
            && self.file_path == other.file_path
            && self.content_identity == other.content_identity
            && self.view_key == other.view_key
            && self.registration_identity == other.registration_identity
            && self.width == other.width
    }

    fn same_variant_family(&self, other: &Self) -> bool {
        self.file_id == other.file_id
            && self.view_key == other.view_key
            && self.registration_identity == other.registration_identity
    }
}

/// One request authorized by the current pass and ready for a native worker.
#[derive(Debug, Clone)]
pub struct FileViewLayoutTask {
    pub request_id: u64,
    pub identity: FileViewLayoutIdentity,
    pub cancellation: ExtensionRequestCancellation,
}

/// A worker result. Request identity prevents late superseded work from committing.
#[derive(Debug)]
pub struct FileViewLayoutWorkerResult {
    pub request_id: u64,
    pub outcome: FileViewLayoutOutcome,
}

/// Result of matching and preparing one selected presentation.
#[derive(Debug)]
pub enum FileViewLayoutOutcome {
    Prepared(ValidatedFileViewLayout),
    Declined,
    Retry,
    Failed { category: String, warning: String },
}

#[derive(Debug, Clone)]
enum CachedLayoutOutcome {
    Prepared(ResolvedFileViewLayout),
    Empty,
}

#[derive(Debug, Clone)]
struct DisplayedLayout {
    identity: FileViewLayoutIdentity,
    resolved: ResolvedFileViewLayout,
}

#[derive(Debug, Clone)]
struct ActiveLayoutRequest {
    identity: FileViewLayoutIdentity,
    cancellation: ExtensionRequestCancellation,
}

#[derive(Debug)]
struct PreparationPass {
    identities: Vec<FileViewLayoutIdentity>,
    queued: VecDeque<FileViewLayoutIdentity>,
    active: BTreeMap<u64, ActiveLayoutRequest>,
    next: BTreeMap<String, DisplayedLayout>,
}

impl PreparationPass {
    fn complete(&self) -> bool {
        self.queued.is_empty() && self.active.is_empty()
    }
}

/// Host-owned counterpart of Hunk's file-view preparation hook.
#[derive(Debug)]
pub struct FileViewLayoutController {
    cache: BTreeMap<FileViewLayoutIdentity, CachedLayoutOutcome>,
    cache_order: VecDeque<FileViewLayoutIdentity>,
    displayed: BTreeMap<String, DisplayedLayout>,
    pass: Option<PreparationPass>,
    pending_resize: Option<(Instant, Vec<FileViewLayoutIdentity>)>,
    previous_width: Option<usize>,
    next_request_id: u64,
    next_layout_generation: u64,
    reported_issues: BTreeSet<String>,
    issue_order: VecDeque<String>,
    results_tx: mpsc::Sender<FileViewLayoutWorkerResult>,
    results_rx: mpsc::Receiver<FileViewLayoutWorkerResult>,
}

impl Default for FileViewLayoutController {
    fn default() -> Self {
        let (results_tx, results_rx) = mpsc::channel();
        Self {
            cache: BTreeMap::new(),
            cache_order: VecDeque::new(),
            displayed: BTreeMap::new(),
            pass: None,
            pending_resize: None,
            previous_width: None,
            next_request_id: 1,
            next_layout_generation: 1,
            reported_issues: BTreeSet::new(),
            issue_order: VecDeque::new(),
            results_tx,
            results_rx,
        }
    }
}

impl Drop for FileViewLayoutController {
    fn drop(&mut self) {
        self.cancel_pass();
    }
}

impl FileViewLayoutController {
    /// Clone the completion endpoint passed to bounded background workers.
    #[must_use]
    pub fn result_sender(&self) -> mpsc::Sender<FileViewLayoutWorkerResult> {
        self.results_tx.clone()
    }

    /// Cancel every in-flight request and retire all displayed geometry.
    pub fn clear(&mut self) {
        self.cancel_pass();
        self.pending_resize = None;
        self.displayed.clear();
        self.cache.clear();
        self.cache_order.clear();
    }

    /// Invalidate a selected view while retaining its compatible tree until replacement settles.
    pub fn invalidate(&mut self, view_key: &str, file_id: Option<&str>) {
        self.cache.retain(|identity, _| {
            identity.view_key != view_key
                || file_id.is_some_and(|file_id| identity.file_id != file_id)
        });
        self.cache_order
            .retain(|identity| self.cache.contains_key(identity));
        self.cancel_pass();
    }

    /// Poll every completed worker and return newly reportable warnings.
    pub fn poll_results(&mut self) -> Vec<String> {
        let mut warnings = Vec::new();
        while let Ok(result) = self.results_rx.try_recv() {
            if let Some(warning) = self.accept_result(result) {
                warnings.push(warning);
            }
        }
        warnings
    }

    /// Reconcile the exact selected file/view set and return only paint-safe layouts.
    pub fn reconcile(
        &mut self,
        now: Instant,
        identities: Vec<FileViewLayoutIdentity>,
    ) -> BTreeMap<String, ResolvedFileViewLayout> {
        let projection_identities = identities.clone();
        self.suppress_incompatible_displayed(&identities);
        if identities.is_empty() {
            self.cancel_pass();
            self.pending_resize = None;
            self.displayed.clear();
            return BTreeMap::new();
        }

        let width = identities[0].width;
        let width_changed = self
            .previous_width
            .is_some_and(|previous| previous != width);
        self.previous_width = Some(width);
        if width_changed {
            self.cancel_pass();
            self.pending_resize = Some((now + FILE_VIEW_LAYOUT_RESIZE_DEBOUNCE, identities));
        } else if let Some((_, pending)) = &self.pending_resize {
            if pending != &identities {
                self.cancel_pass();
                self.pending_resize = Some((now + FILE_VIEW_LAYOUT_RESIZE_DEBOUNCE, identities));
            }
        } else if self
            .pass
            .as_ref()
            .is_none_or(|pass| pass.identities != identities)
        {
            self.cancel_pass();
            self.start_pass(identities);
        }

        if self
            .pending_resize
            .as_ref()
            .is_some_and(|(deadline, _)| now >= *deadline)
        {
            let (_, identities) = self.pending_resize.take().expect("checked above");
            self.start_pass(identities);
        }
        self.commit_completed_pass();
        self.project_displayed(&projection_identities)
    }

    /// Claim up to the global preparation limit for native dispatch.
    pub fn take_startable(&mut self) -> Vec<FileViewLayoutTask> {
        let Some(pass) = self.pass.as_mut() else {
            return Vec::new();
        };
        let slots = FILE_VIEW_LAYOUT_CONCURRENCY.saturating_sub(pass.active.len());
        let mut tasks = Vec::with_capacity(slots);
        for _ in 0..slots {
            let Some(identity) = pass.queued.pop_front() else {
                break;
            };
            let request_id = self.next_request_id;
            self.next_request_id = self.next_request_id.saturating_add(1);
            let cancellation = ExtensionRequestCancellation::default();
            pass.active.insert(
                request_id,
                ActiveLayoutRequest {
                    identity: identity.clone(),
                    cancellation: cancellation.clone(),
                },
            );
            tasks.push(FileViewLayoutTask {
                request_id,
                identity,
                cancellation,
            });
        }
        tasks
    }

    fn start_pass(&mut self, identities: Vec<FileViewLayoutIdentity>) {
        let mut queued = VecDeque::new();
        let mut next = BTreeMap::new();
        for identity in &identities {
            self.remove_superseded_variants(identity);
            match self.cache.get(identity).cloned() {
                Some(CachedLayoutOutcome::Prepared(resolved)) => {
                    self.touch_cache(identity);
                    next.insert(
                        identity.file_id.clone(),
                        DisplayedLayout {
                            identity: identity.clone(),
                            resolved,
                        },
                    );
                }
                Some(CachedLayoutOutcome::Empty) => self.touch_cache(identity),
                None => queued.push_back(identity.clone()),
            }
        }
        self.pass = Some(PreparationPass {
            identities,
            queued,
            active: BTreeMap::new(),
            next,
        });
    }

    fn accept_result(&mut self, result: FileViewLayoutWorkerResult) -> Option<String> {
        let active = {
            let pass = self.pass.as_mut()?;
            let active = pass.active.remove(&result.request_id)?;
            if !pass.identities.contains(&active.identity) {
                return None;
            }
            active
        };

        let mut warning = None;
        match result.outcome {
            FileViewLayoutOutcome::Prepared(validated) => {
                let resolved = ResolvedFileViewLayout {
                    key: active.identity.view_key.clone(),
                    extension_id: active.identity.extension_id.clone(),
                    view_id: active.identity.view_id.clone(),
                    registration_identity: active.identity.registration_identity,
                    layout_generation: self.next_layout_generation,
                    validated,
                };
                self.next_layout_generation = self.next_layout_generation.saturating_add(1);
                self.insert_cache(
                    active.identity.clone(),
                    CachedLayoutOutcome::Prepared(resolved.clone()),
                );
                if let Some(pass) = self.pass.as_mut() {
                    pass.next.insert(
                        active.identity.file_id.clone(),
                        DisplayedLayout {
                            identity: active.identity,
                            resolved,
                        },
                    );
                }
            }
            FileViewLayoutOutcome::Declined => {
                self.insert_cache(active.identity.clone(), CachedLayoutOutcome::Empty);
                if let Some(pass) = self.pass.as_mut() {
                    pass.next.remove(&active.identity.file_id);
                }
            }
            FileViewLayoutOutcome::Retry => {
                if let Some(pass) = self.pass.as_mut() {
                    pass.queued.push_front(active.identity);
                }
            }
            FileViewLayoutOutcome::Failed {
                category,
                warning: message,
            } => {
                let issue_key = format!(
                    "{}\0{}\0{}\0{}",
                    active.identity.registration_identity,
                    active.identity.file_id,
                    active.identity.view_key,
                    category
                );
                if self.record_issue(issue_key) {
                    warning = Some(message);
                }
                self.insert_cache(active.identity.clone(), CachedLayoutOutcome::Empty);
                if let Some(pass) = self.pass.as_mut() {
                    pass.next.remove(&active.identity.file_id);
                }
            }
        }
        self.commit_completed_pass();
        warning
    }

    fn suppress_incompatible_displayed(&mut self, identities: &[FileViewLayoutIdentity]) {
        let by_file = identities
            .iter()
            .map(|identity| (identity.file_id.as_str(), identity))
            .collect::<BTreeMap<_, _>>();
        self.displayed.retain(|file_id, displayed| {
            by_file
                .get(file_id.as_str())
                .is_some_and(|identity| displayed.identity.same_prepared_tree(identity))
        });
    }

    fn project_displayed(
        &self,
        identities: &[FileViewLayoutIdentity],
    ) -> BTreeMap<String, ResolvedFileViewLayout> {
        let by_file = identities
            .iter()
            .map(|identity| (identity.file_id.as_str(), identity))
            .collect::<BTreeMap<_, _>>();
        self.displayed
            .iter()
            .filter(|(file_id, displayed)| {
                by_file
                    .get(file_id.as_str())
                    .is_some_and(|identity| displayed.identity.same_prepared_tree(identity))
            })
            .map(|(file_id, displayed)| (file_id.clone(), displayed.resolved.clone()))
            .collect()
    }

    fn cancel_pass(&mut self) {
        if let Some(pass) = self.pass.take() {
            for active in pass.active.into_values() {
                active.cancellation.cancel();
            }
        }
    }

    fn commit_completed_pass(&mut self) {
        if self.pass.as_ref().is_some_and(PreparationPass::complete) {
            let pass = self.pass.take().expect("checked above");
            self.displayed = pass.next;
        }
    }

    fn remove_superseded_variants(&mut self, identity: &FileViewLayoutIdentity) {
        self.cache.retain(|candidate, _| {
            candidate == identity || !candidate.same_variant_family(identity)
        });
        self.cache_order
            .retain(|candidate| self.cache.contains_key(candidate));
    }

    fn touch_cache(&mut self, identity: &FileViewLayoutIdentity) {
        self.cache_order.retain(|candidate| candidate != identity);
        self.cache_order.push_back(identity.clone());
    }

    fn insert_cache(&mut self, identity: FileViewLayoutIdentity, outcome: CachedLayoutOutcome) {
        self.cache.insert(identity.clone(), outcome);
        self.touch_cache(&identity);
        while self.cache.len() > FILE_VIEW_LAYOUT_CACHE_MAX_ENTRIES {
            let Some(oldest) = self.cache_order.pop_front() else {
                break;
            };
            self.cache.remove(&oldest);
        }
    }

    fn record_issue(&mut self, key: String) -> bool {
        if self.reported_issues.contains(&key) {
            return false;
        }
        while self.reported_issues.len() >= FILE_VIEW_LAYOUT_ISSUE_MAX_ENTRIES {
            let Some(oldest) = self.issue_order.pop_front() else {
                break;
            };
            self.reported_issues.remove(&oldest);
        }
        self.reported_issues.insert(key.clone());
        self.issue_order.push_back(key);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_extension_api::ExtensionFileViewLayout;

    #[test]
    fn frozen_hunk_layout_oracle_records_both_pins_and_every_source_test() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/file-view-layouts.json"
        ))
        .unwrap();
        assert_eq!(oracle["baselineOracle"]["passed"], 18);
        assert_eq!(oracle["baselineOracle"]["failed"], 0);
        assert_eq!(oracle["baselineOracle"]["assertions"], 70);
        assert_eq!(oracle["stableOracle"]["passed"], 18);
        assert_eq!(oracle["stableOracle"]["failed"], 0);
        assert_eq!(oracle["stableOracle"]["assertions"], 70);
        let mappings = oracle["testMappings"].as_array().unwrap();
        assert_eq!(mappings.len(), 18);
        assert!(mappings.iter().all(|mapping| {
            mapping["source"]
                .as_str()
                .is_some_and(|name| !name.is_empty())
                && mapping["rust"]
                    .as_array()
                    .is_some_and(|tests| !tests.is_empty())
        }));
    }

    fn identity(
        file_id: &str,
        width: usize,
        epoch: u64,
        registration: u64,
    ) -> FileViewLayoutIdentity {
        FileViewLayoutIdentity {
            file_id: file_id.into(),
            file_path: format!("{file_id}.rs"),
            content_identity: format!("content:{file_id}"),
            view_key: "test-extension:test-view".into(),
            extension_id: "test-extension".into(),
            view_id: "test-view".into(),
            registration_identity: registration,
            width,
            epoch,
        }
    }

    fn layout() -> ValidatedFileViewLayout {
        ValidatedFileViewLayout {
            layout: ExtensionFileViewLayout {
                rows: Vec::new(),
                hunk_rows: Vec::new(),
            },
            row_heights: Vec::new(),
        }
    }

    fn prepare(
        controller: &mut FileViewLayoutController,
        now: Instant,
        identities: Vec<FileViewLayoutIdentity>,
    ) -> Vec<FileViewLayoutTask> {
        controller.reconcile(now, identities);
        controller.take_startable()
    }

    fn resolve(controller: &mut FileViewLayoutController, task: &FileViewLayoutTask) {
        controller
            .result_sender()
            .send(FileViewLayoutWorkerResult {
                request_id: task.request_id,
                outcome: FileViewLayoutOutcome::Prepared(layout()),
            })
            .unwrap();
        assert!(controller.poll_results().is_empty());
    }

    #[test]
    fn inactive_raw_state_schedules_no_preparation() {
        let mut controller = FileViewLayoutController::default();
        assert!(prepare(&mut controller, Instant::now(), Vec::new()).is_empty());
        assert!(controller.displayed.is_empty());
    }

    #[test]
    fn exact_unaffected_file_survives_another_files_selection_change() {
        let now = Instant::now();
        let mut controller = FileViewLayoutController::default();
        let first = identity("first", 80, 0, 1);
        let second = identity("second", 80, 0, 1);
        let tasks = prepare(&mut controller, now, vec![first.clone(), second.clone()]);
        for task in &tasks {
            resolve(&mut controller, task);
        }
        let projected = controller.reconcile(now, vec![first.clone(), second.clone()]);
        assert_eq!(projected.len(), 2);

        let mut changed_second = second;
        changed_second.view_key = "test-extension:delayed".into();
        changed_second.view_id = "delayed".into();
        let projected = controller.reconcile(now, vec![first, changed_second]);
        assert_eq!(projected.keys().cloned().collect::<Vec<_>>(), ["first"]);
    }

    #[test]
    fn width_change_is_hidden_and_rapid_changes_are_debounced() {
        let now = Instant::now();
        let mut controller = FileViewLayoutController::default();
        let first = identity("first", 80, 0, 1);
        let task = prepare(&mut controller, now, vec![first.clone()]).remove(0);
        resolve(&mut controller, &task);
        assert_eq!(controller.reconcile(now, vec![first]).len(), 1);

        for (offset, width) in [(0, 79), (10, 78), (20, 77)] {
            let current = identity("first", width, 0, 1);
            assert!(
                controller
                    .reconcile(now + Duration::from_millis(offset), vec![current])
                    .is_empty()
            );
            assert!(controller.take_startable().is_empty());
        }
        let current = identity("first", 77, 0, 1);
        controller.reconcile(now + Duration::from_millis(69), vec![current.clone()]);
        assert!(controller.take_startable().is_empty());
        controller.reconcile(now + Duration::from_millis(70), vec![current]);
        let tasks = controller.take_startable();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].identity.width, 77);
    }

    #[test]
    fn width_variants_replace_each_other_instead_of_becoming_reusable() {
        let now = Instant::now();
        let mut controller = FileViewLayoutController::default();
        let first = identity("first", 80, 0, 1);
        let task = prepare(&mut controller, now, vec![first.clone()]).remove(0);
        resolve(&mut controller, &task);

        let narrow = identity("first", 40, 0, 1);
        controller.reconcile(now, vec![narrow.clone()]);
        controller.reconcile(now + FILE_VIEW_LAYOUT_RESIZE_DEBOUNCE, vec![narrow]);
        let task = controller.take_startable().remove(0);
        resolve(&mut controller, &task);

        controller.reconcile(now + FILE_VIEW_LAYOUT_RESIZE_DEBOUNCE, vec![first.clone()]);
        controller.reconcile(now + FILE_VIEW_LAYOUT_RESIZE_DEBOUNCE * 2, vec![first]);
        assert_eq!(controller.take_startable().len(), 1);
    }

    #[test]
    fn oldest_prepared_tree_is_evicted_at_the_fixed_limit() {
        let now = Instant::now();
        let mut controller = FileViewLayoutController::default();
        for index in 0..=FILE_VIEW_LAYOUT_CACHE_MAX_ENTRIES {
            let current = identity(&format!("cache-{index}"), 80, 0, 1);
            let task = prepare(&mut controller, now, vec![current]).remove(0);
            resolve(&mut controller, &task);
        }
        assert_eq!(controller.cache.len(), FILE_VIEW_LAYOUT_CACHE_MAX_ENTRIES);
        let first = identity("cache-0", 80, 0, 1);
        assert_eq!(prepare(&mut controller, now, vec![first]).len(), 1);
    }

    #[test]
    fn failures_dedupe_across_widths_but_not_replacement_registrations() {
        let now = Instant::now();
        let mut controller = FileViewLayoutController::default();
        for (index, width) in [80, 79, 78].into_iter().enumerate() {
            let current = identity("first", width, 0, 1);
            controller.reconcile(now, vec![current.clone()]);
            if index > 0 {
                controller.reconcile(now + FILE_VIEW_LAYOUT_RESIZE_DEBOUNCE, vec![current]);
            }
            let task = controller.take_startable().remove(0);
            controller
                .result_sender()
                .send(FileViewLayoutWorkerResult {
                    request_id: task.request_id,
                    outcome: FileViewLayoutOutcome::Failed {
                        category: "layout".into(),
                        warning: "failed".into(),
                    },
                })
                .unwrap();
            assert_eq!(controller.poll_results().len(), usize::from(index == 0));
        }
        let replacement = identity("first", 78, 0, 2);
        let task = prepare(&mut controller, now, vec![replacement]).remove(0);
        controller
            .result_sender()
            .send(FileViewLayoutWorkerResult {
                request_id: task.request_id,
                outcome: FileViewLayoutOutcome::Failed {
                    category: "layout".into(),
                    warning: "failed again".into(),
                },
            })
            .unwrap();
        assert_eq!(controller.poll_results(), ["failed again"]);
    }

    #[test]
    fn replacement_registration_synchronously_suppresses_old_geometry() {
        let now = Instant::now();
        let mut controller = FileViewLayoutController::default();
        let first = identity("first", 80, 0, 1);
        let task = prepare(&mut controller, now, vec![first.clone()]).remove(0);
        resolve(&mut controller, &task);
        assert_eq!(controller.reconcile(now, vec![first]).len(), 1);
        assert!(
            controller
                .reconcile(now, vec![identity("first", 80, 0, 2)])
                .is_empty()
        );
    }

    #[test]
    fn exact_cache_bypasses_work_but_replacement_registration_reprepares() {
        let now = Instant::now();
        let mut controller = FileViewLayoutController::default();
        let first = identity("first", 80, 0, 1);
        let task = prepare(&mut controller, now, vec![first.clone()]).remove(0);
        resolve(&mut controller, &task);
        assert!(prepare(&mut controller, now, vec![first]).is_empty());
        assert_eq!(
            prepare(&mut controller, now, vec![identity("first", 80, 0, 2)]).len(),
            1
        );
    }

    #[test]
    fn epoch_refresh_reprepares_but_keeps_previous_tree_until_commit() {
        let now = Instant::now();
        let mut controller = FileViewLayoutController::default();
        let first = identity("first", 80, 0, 1);
        let task = prepare(&mut controller, now, vec![first]).remove(0);
        resolve(&mut controller, &task);

        let refreshed = identity("first", 80, 1, 1);
        assert_eq!(controller.reconcile(now, vec![refreshed.clone()]).len(), 1);
        let task = controller.take_startable().remove(0);
        assert_eq!(controller.reconcile(now, vec![refreshed.clone()]).len(), 1);
        resolve(&mut controller, &task);
        let next = controller.reconcile(now, vec![refreshed]);
        assert_eq!(next.len(), 1);
        assert_eq!(next["first"].layout_generation, 2);
    }

    #[test]
    fn unselected_refresh_does_no_work_and_selection_reprepares() {
        let now = Instant::now();
        let mut controller = FileViewLayoutController::default();
        let first = identity("first", 80, 0, 1);
        let task = prepare(&mut controller, now, vec![first]).remove(0);
        resolve(&mut controller, &task);
        assert!(prepare(&mut controller, now, Vec::new()).is_empty());
        let refreshed = identity("first", 80, 1, 1);
        assert_eq!(prepare(&mut controller, now, vec![refreshed]).len(), 1);
    }

    #[test]
    fn scoped_and_view_wide_epoch_changes_reprepare_only_affected_identities() {
        let now = Instant::now();
        let mut controller = FileViewLayoutController::default();
        let first = identity("first", 80, 0, 1);
        let second = identity("second", 80, 0, 1);
        let tasks = prepare(&mut controller, now, vec![first.clone(), second.clone()]);
        for task in &tasks {
            resolve(&mut controller, task);
        }

        let first_refreshed = identity("first", 80, 1, 1);
        let tasks = prepare(
            &mut controller,
            now,
            vec![first_refreshed.clone(), second.clone()],
        );
        assert_eq!(
            tasks
                .iter()
                .map(|task| task.identity.file_id.as_str())
                .collect::<Vec<_>>(),
            ["first"]
        );
        resolve(&mut controller, &tasks[0]);

        let second_refreshed = identity("second", 80, 1, 1);
        let tasks = prepare(
            &mut controller,
            now,
            vec![first_refreshed, second_refreshed],
        );
        assert_eq!(
            tasks
                .iter()
                .map(|task| task.identity.file_id.as_str())
                .collect::<Vec<_>>(),
            ["second"]
        );
    }

    #[test]
    fn supersession_cancels_children_and_late_results_are_ignored() {
        let now = Instant::now();
        let mut controller = FileViewLayoutController::default();
        let task = prepare(&mut controller, now, vec![identity("first", 80, 0, 1)]).remove(0);
        controller.reconcile(now, vec![identity("first", 80, 0, 2)]);
        assert!(task.cancellation.is_cancelled());
        controller
            .result_sender()
            .send(FileViewLayoutWorkerResult {
                request_id: task.request_id,
                outcome: FileViewLayoutOutcome::Prepared(layout()),
            })
            .unwrap();
        assert!(controller.poll_results().is_empty());
        assert!(controller.displayed.is_empty());
    }

    #[test]
    fn preparation_is_bounded_to_four_active_requests() {
        let now = Instant::now();
        let mut controller = FileViewLayoutController::default();
        let identities = (0..10)
            .map(|index| identity(&format!("file-{index}"), 80, 0, 1))
            .collect();
        controller.reconcile(now, identities);
        assert_eq!(
            controller.take_startable().len(),
            FILE_VIEW_LAYOUT_CONCURRENCY
        );
        assert!(controller.take_startable().is_empty());
    }

    #[test]
    fn a_preparation_pass_commits_atomically_after_its_last_worker() {
        let now = Instant::now();
        let mut controller = FileViewLayoutController::default();
        let first = identity("first", 80, 0, 1);
        let second = identity("second", 80, 0, 1);
        let tasks = prepare(&mut controller, now, vec![first.clone(), second.clone()]);
        resolve(&mut controller, &tasks[0]);
        assert!(
            controller
                .reconcile(now, vec![first.clone(), second.clone()])
                .is_empty()
        );
        resolve(&mut controller, &tasks[1]);
        assert_eq!(controller.reconcile(now, vec![first, second]).len(), 2);
    }

    #[test]
    fn warning_identity_retention_never_exceeds_the_fixed_limit() {
        let now = Instant::now();
        let mut controller = FileViewLayoutController::default();
        for index in 0..=FILE_VIEW_LAYOUT_ISSUE_MAX_ENTRIES {
            let current = identity(&format!("failed-{index}"), 80, 0, 1);
            let task = prepare(&mut controller, now, vec![current]).remove(0);
            controller
                .result_sender()
                .send(FileViewLayoutWorkerResult {
                    request_id: task.request_id,
                    outcome: FileViewLayoutOutcome::Failed {
                        category: "layout".into(),
                        warning: format!("failed {index}"),
                    },
                })
                .unwrap();
            assert_eq!(controller.poll_results(), [format!("failed {index}")]);
        }
        assert_eq!(
            controller.reported_issues.len(),
            FILE_VIEW_LAYOUT_ISSUE_MAX_ENTRIES
        );
    }
}
