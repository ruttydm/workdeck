//! Immutable line-highlight maps consumed by the review painter.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;

use serde_json::Value;
use workdeck_core::{Changeset, DiffFile};
use workdeck_extension_api::ValidatedLineHighlight;
use workdeck_extension_host::{
    HostError, LineHighlightValidation, LoadedExtension, MAX_MERGED_LINE_HIGHLIGHTS_PER_FILE,
    RegisteredLineHighlighter, scoped_epoch, validate_line_highlights,
};

pub const LINE_HIGHLIGHT_CONCURRENCY: usize = 4;
const LINE_HIGHLIGHT_ISSUE_MAX_ENTRIES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineHighlightRuntimeError {
    Retry,
    Failed(String),
}

/// Process boundary used by the background coordinator and deterministic tests.
pub trait LineHighlightRuntime: Send + Sync {
    fn source_generation(&self, _file: &DiffFile) -> Option<u64> {
        None
    }
    fn request_pending(&self) -> bool;
    fn highlight_file(
        &self,
        highlighter_id: &str,
        file: &DiffFile,
        cancelled: &AtomicBool,
    ) -> Result<Value, LineHighlightRuntimeError>;
    fn notify_warning(&self, message: String);
}

/// Capture source authority with the worker generation, not with public file metadata.
pub(crate) struct SourceBoundLineHighlightRuntime {
    pub runtime: Arc<dyn LineHighlightRuntime>,
    pub sources: Option<workdeck_vcs::VcsSourceCapabilities>,
}

impl LineHighlightRuntime for SourceBoundLineHighlightRuntime {
    fn source_generation(&self, file: &DiffFile) -> Option<u64> {
        if file.source_attested {
            None
        } else {
            self.sources
                .as_ref()?
                .get(file)
                .map(|source| source.runtime_identity())
        }
    }
    fn request_pending(&self) -> bool {
        self.runtime.request_pending()
    }

    fn highlight_file(
        &self,
        highlighter_id: &str,
        file: &DiffFile,
        cancelled: &AtomicBool,
    ) -> Result<Value, LineHighlightRuntimeError> {
        if cancelled.load(Ordering::Acquire) {
            return Err(LineHighlightRuntimeError::Retry);
        }
        let loaded = self
            .sources
            .as_ref()
            .map(|sources| sources.with_source_snapshots(file))
            .transpose()
            .map_err(|error| LineHighlightRuntimeError::Failed(error.to_string()))?;
        if cancelled.load(Ordering::Acquire) {
            return Err(LineHighlightRuntimeError::Retry);
        }
        self.runtime
            .highlight_file(highlighter_id, loaded.as_ref().unwrap_or(file), cancelled)
    }

    fn notify_warning(&self, message: String) {
        self.runtime.notify_warning(message);
    }
}

impl LineHighlightRuntime for LoadedExtension {
    fn request_pending(&self) -> bool {
        LoadedExtension::request_pending(self)
    }

    fn highlight_file(
        &self,
        highlighter_id: &str,
        file: &DiffFile,
        cancelled: &AtomicBool,
    ) -> Result<Value, LineHighlightRuntimeError> {
        let mut runtime = self.clone();
        LoadedExtension::highlight_file_cancellable(&mut runtime, highlighter_id, file, cancelled)
            .map_err(|error| match error {
                HostError::Busy(_) | HostError::Cancelled(_) => LineHighlightRuntimeError::Retry,
                error => LineHighlightRuntimeError::Failed(error.to_string()),
            })
    }

    fn notify_warning(&self, message: String) {
        LoadedExtension::notify_warning(self, message);
    }
}

#[derive(Debug, Clone, Default)]
pub struct LineHighlightMap(Arc<BTreeMap<String, Arc<[ValidatedLineHighlight]>>>);

impl LineHighlightMap {
    #[must_use]
    pub fn from_entries(
        entries: impl IntoIterator<Item = (String, Vec<ValidatedLineHighlight>)>,
    ) -> Self {
        Self(Arc::new(
            entries
                .into_iter()
                .map(|(file_id, marks)| (file_id, Arc::from(marks)))
                .collect(),
        ))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    pub fn get(&self, file_id: &str) -> Option<&[ValidatedLineHighlight]> {
        self.0.get(file_id).map(AsRef::as_ref)
    }

    #[must_use]
    pub fn get_shared(&self, file_id: &str) -> Option<&Arc<[ValidatedLineHighlight]>> {
        self.0.get(file_id)
    }

    /// Count every mark across the immutable file map.
    #[must_use]
    pub fn mark_count(&self) -> usize {
        self.0.values().map(|marks| marks.len()).sum()
    }

    /// Return a new map with one file replaced or removed.
    #[must_use]
    pub fn with_file_marks(
        &self,
        file_id: impl Into<String>,
        marks: Vec<ValidatedLineHighlight>,
    ) -> Self {
        let file_id = file_id.into();
        let mut entries = (*self.0).clone();
        if marks.is_empty() {
            entries.remove(&file_id);
        } else {
            entries.insert(file_id, Arc::from(marks));
        }
        Self(Arc::new(entries))
    }

    #[must_use]
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    fn has_same_entries(&self, entries: &BTreeMap<String, Arc<[ValidatedLineHighlight]>>) -> bool {
        self.0.len() == entries.len()
            && self.0.iter().all(|(file_id, marks)| {
                entries
                    .get(file_id)
                    .is_some_and(|other| Arc::ptr_eq(marks, other))
            })
    }
}

impl LineHighlightMap {
    fn from_shared_entries(
        entries: impl IntoIterator<Item = (String, Arc<[ValidatedLineHighlight]>)>,
    ) -> Self {
        Self(Arc::new(entries.into_iter().collect()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct LineHighlightTaskKey {
    file_id: String,
    content_identity: String,
    source_identity: Option<String>,
    source_generation: Option<u64>,
    highlighter_key: String,
    registration_identity: u64,
    epoch: u64,
}

#[derive(Debug, Clone)]
struct LineHighlightTask {
    key: LineHighlightTaskKey,
    extension_index: usize,
    extension_id: String,
    highlighter_id: String,
    file: DiffFile,
}

#[derive(Debug)]
enum LineHighlightTaskOutcome {
    Value(Value),
    Retry,
    Failed(String),
}

#[derive(Debug)]
struct LineHighlightCompletion {
    task: LineHighlightTask,
    outcome: LineHighlightTaskOutcome,
}

#[derive(Debug, Clone)]
struct MergedLineHighlights {
    content_identity: String,
    registrations: Vec<(String, u64)>,
    parts: Vec<Option<Arc<[ValidatedLineHighlight]>>>,
    merged: Arc<[ValidatedLineHighlight]>,
}

/// Background preparation, caching, containment, and publication for native line highlighters.
#[derive(Debug)]
pub struct LineHighlightPreparationController {
    cache: BTreeMap<LineHighlightTaskKey, Option<Arc<[ValidatedLineHighlight]>>>,
    pending: BTreeMap<LineHighlightTaskKey, Arc<AtomicBool>>,
    sender: mpsc::Sender<LineHighlightCompletion>,
    receiver: mpsc::Receiver<LineHighlightCompletion>,
    merged: BTreeMap<String, MergedLineHighlights>,
    resolved: LineHighlightMap,
    reported_issues: BTreeSet<String>,
    issue_order: VecDeque<String>,
}

impl Default for LineHighlightPreparationController {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            cache: BTreeMap::new(),
            pending: BTreeMap::new(),
            sender,
            receiver,
            merged: BTreeMap::new(),
            resolved: LineHighlightMap::default(),
            reported_issues: BTreeSet::new(),
            issue_order: VecDeque::new(),
        }
    }
}

impl LineHighlightPreparationController {
    #[must_use]
    pub fn resolved(&self) -> &LineHighlightMap {
        &self.resolved
    }

    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Poll completed work, retire stale derivations, and start at most four requests.
    ///
    /// The method never waits for extension code. Calling it from successive
    /// Ratatui frames publishes each file as soon as all of that file's
    /// registration-ordered parts have settled.
    pub fn reconcile(
        &mut self,
        extensions: &[Arc<dyn LineHighlightRuntime>],
        registrations: &[RegisteredLineHighlighter],
        epochs: &workdeck_extension_host::LineHighlightEpochState,
        files: &[DiffFile],
    ) {
        let tasks = desired_line_highlight_tasks(extensions, registrations, epochs, files);
        let desired = tasks
            .iter()
            .map(|task| task.key.clone())
            .collect::<BTreeSet<_>>();
        for (key, cancellation) in &self.pending {
            if !desired.contains(key) {
                cancellation.store(true, Ordering::Release);
            }
        }
        self.poll_completions(&desired, extensions);
        self.cache.retain(|key, _| desired.contains(key));
        self.merged.retain(|file_id, entry| {
            files.iter().any(|file| {
                file.runtime_id == *file_id && file.content_identity == entry.content_identity
            })
        });

        for task in &tasks {
            if self.pending.len() >= LINE_HIGHLIGHT_CONCURRENCY {
                break;
            }
            if self.cache.contains_key(&task.key) || self.pending.contains_key(&task.key) {
                continue;
            }
            let Some(extension) = extensions.get(task.extension_index) else {
                continue;
            };
            if extension.request_pending() {
                continue;
            }
            let cancelled = Arc::new(AtomicBool::new(false));
            self.pending
                .insert(task.key.clone(), Arc::clone(&cancelled));
            let extension = Arc::clone(extension);
            let task = task.clone();
            let sender = self.sender.clone();
            thread::spawn(move || {
                let outcome =
                    match extension.highlight_file(&task.highlighter_id, &task.file, &cancelled) {
                        Ok(value) => LineHighlightTaskOutcome::Value(value),
                        Err(LineHighlightRuntimeError::Retry) => LineHighlightTaskOutcome::Retry,
                        Err(LineHighlightRuntimeError::Failed(error)) => {
                            LineHighlightTaskOutcome::Failed(error)
                        }
                    };
                let _ = sender.send(LineHighlightCompletion { task, outcome });
            });
        }
        self.publish_complete_files(extensions, registrations, epochs, files);
    }

    fn poll_completions(
        &mut self,
        desired: &BTreeSet<LineHighlightTaskKey>,
        extensions: &[Arc<dyn LineHighlightRuntime>],
    ) {
        while let Ok(completion) = self.receiver.try_recv() {
            self.pending.remove(&completion.task.key);
            if !desired.contains(&completion.task.key) {
                continue;
            }
            match completion.outcome {
                LineHighlightTaskOutcome::Retry => {}
                LineHighlightTaskOutcome::Failed(error) => {
                    self.cache.insert(completion.task.key.clone(), None);
                    self.report_once(
                        extensions,
                        &completion.task,
                        "highlight",
                        format!(
                            "Extension {} line highlighter {:?} failed highlighting {} ({error}) • marks dropped",
                            completion.task.extension_id,
                            completion.task.highlighter_id,
                            completion.task.file.path,
                        ),
                    );
                }
                LineHighlightTaskOutcome::Value(value) => {
                    match validate_line_highlights(Some(&value)) {
                        LineHighlightValidation::Invalid { issue } => {
                            self.cache.insert(completion.task.key.clone(), None);
                            self.report_once(
                                extensions,
                                &completion.task,
                                &issue,
                                format!(
                                    "Extension {} line highlighter {:?} {issue} for {} • marks dropped",
                                    completion.task.extension_id,
                                    completion.task.highlighter_id,
                                    completion.task.file.path,
                                ),
                            );
                        }
                        LineHighlightValidation::Valid {
                            marks,
                            dropped_invalid,
                        } => {
                            if dropped_invalid > 0 {
                                self.report_once(
                                    extensions,
                                    &completion.task,
                                    "invalid-entries",
                                    format!(
                                        "Extension {} line highlighter {:?} returned {dropped_invalid} invalid range{} for {} • dropped",
                                        completion.task.extension_id,
                                        completion.task.highlighter_id,
                                        if dropped_invalid == 1 { "" } else { "s" },
                                        completion.task.file.path,
                                    ),
                                );
                            }
                            self.cache.insert(
                                completion.task.key,
                                (!marks.is_empty()).then(|| Arc::from(marks)),
                            );
                        }
                    }
                }
            }
        }
    }

    fn report_once(
        &mut self,
        extensions: &[Arc<dyn LineHighlightRuntime>],
        task: &LineHighlightTask,
        issue: &str,
        message: String,
    ) {
        let key = format!(
            "{}:{}:{}:{issue}",
            task.key.registration_identity, task.highlighter_id, task.file.runtime_id
        );
        if self.reported_issues.contains(&key) {
            return;
        }
        if self.reported_issues.len() >= LINE_HIGHLIGHT_ISSUE_MAX_ENTRIES
            && let Some(oldest) = self.issue_order.pop_front()
        {
            self.reported_issues.remove(&oldest);
        }
        self.reported_issues.insert(key.clone());
        self.issue_order.push_back(key);
        if let Some(extension) = extensions.get(task.extension_index) {
            extension.notify_warning(message);
        }
    }

    fn publish_complete_files(
        &mut self,
        extensions: &[Arc<dyn LineHighlightRuntime>],
        registrations: &[RegisteredLineHighlighter],
        epochs: &workdeck_extension_host::LineHighlightEpochState,
        files: &[DiffFile],
    ) {
        let mut resolved = BTreeMap::new();
        for file in files {
            if file.flags.binary || file.flags.too_large || file.hunks.is_empty() {
                self.merged.remove(&file.runtime_id);
                continue;
            }
            let keyed = registrations
                .iter()
                .map(|registration| {
                    let highlighter_key =
                        workdeck_extension_host::registered_line_highlighter_key(registration);
                    let epoch = scoped_epoch(epochs, &highlighter_key, &file.runtime_id);
                    let task_key = LineHighlightTaskKey {
                        file_id: file.runtime_id.clone(),
                        content_identity: file.content_identity.clone(),
                        source_identity: file.source_identity.clone(),
                        source_generation: extensions
                            .get(registration.extension_index)
                            .and_then(|runtime| runtime.source_generation(file)),
                        highlighter_key: highlighter_key.clone(),
                        registration_identity: registration.registration_identity(),
                        epoch,
                    };
                    (registration, highlighter_key, epoch, task_key)
                })
                .collect::<Vec<_>>();
            if keyed
                .iter()
                .any(|(_, _, _, key)| !self.cache.contains_key(key))
            {
                continue;
            }
            let mut accepted_parts = Vec::with_capacity(keyed.len());
            let mut merged_count = 0_usize;
            for (registration, _, _, key) in &keyed {
                let mut part = self.cache.get(key).cloned().flatten();
                if part.as_ref().is_some_and(|marks| {
                    merged_count.saturating_add(marks.len()) > MAX_MERGED_LINE_HIGHLIGHTS_PER_FILE
                }) {
                    let task = LineHighlightTask {
                        key: (*key).clone(),
                        extension_index: registration.extension_index,
                        extension_id: registration.extension_id.clone(),
                        highlighter_id: registration.highlighter_id.clone(),
                        file: file.clone(),
                    };
                    self.report_once(
                        extensions,
                        &task,
                        "merged-cap",
                        format!(
                            "Extension {} line highlighter {:?} pushed {} past {} merged ranges • marks dropped",
                            registration.extension_id,
                            registration.highlighter_id,
                            file.path,
                            MAX_MERGED_LINE_HIGHLIGHTS_PER_FILE,
                        ),
                    );
                    part = None;
                }
                merged_count = merged_count.saturating_add(part.as_ref().map_or(0, |p| p.len()));
                accepted_parts.push(part);
            }
            if merged_count == 0 {
                self.merged.remove(&file.runtime_id);
                continue;
            }
            let signature = keyed
                .iter()
                .map(|(_, key, epoch, _)| (key.clone(), *epoch))
                .collect::<Vec<_>>();
            let reuse = self.merged.get(&file.runtime_id).filter(|previous| {
                previous.content_identity == file.content_identity
                    && previous.registrations == signature
                    && same_line_highlight_parts(&previous.parts, &accepted_parts)
            });
            let merged = reuse.map_or_else(
                || {
                    Arc::from(
                        accepted_parts
                            .iter()
                            .filter_map(Option::as_deref)
                            .flatten()
                            .cloned()
                            .collect::<Vec<_>>(),
                    )
                },
                |previous| Arc::clone(&previous.merged),
            );
            self.merged.insert(
                file.runtime_id.clone(),
                MergedLineHighlights {
                    content_identity: file.content_identity.clone(),
                    registrations: signature,
                    parts: accepted_parts,
                    merged: Arc::clone(&merged),
                },
            );
            resolved.insert(file.runtime_id.clone(), merged);
        }
        if !self.resolved.has_same_entries(&resolved) {
            self.resolved = LineHighlightMap::from_shared_entries(resolved);
        }
    }
}

fn desired_line_highlight_tasks(
    extensions: &[Arc<dyn LineHighlightRuntime>],
    registrations: &[RegisteredLineHighlighter],
    epochs: &workdeck_extension_host::LineHighlightEpochState,
    files: &[DiffFile],
) -> Vec<LineHighlightTask> {
    files
        .iter()
        .filter(|file| !file.flags.binary && !file.flags.too_large && !file.hunks.is_empty())
        .flat_map(|file| {
            registrations.iter().map(move |registration| {
                let highlighter_key =
                    workdeck_extension_host::registered_line_highlighter_key(registration);
                LineHighlightTask {
                    key: LineHighlightTaskKey {
                        file_id: file.runtime_id.clone(),
                        content_identity: file.content_identity.clone(),
                        source_identity: file.source_identity.clone(),
                        source_generation: extensions
                            .get(registration.extension_index)
                            .and_then(|runtime| runtime.source_generation(file)),
                        epoch: scoped_epoch(epochs, &highlighter_key, &file.runtime_id),
                        highlighter_key,
                        registration_identity: registration.registration_identity(),
                    },
                    extension_index: registration.extension_index,
                    extension_id: registration.extension_id.clone(),
                    highlighter_id: registration.highlighter_id.clone(),
                    file: file.clone(),
                }
            })
        })
        .collect()
}

fn same_line_highlight_parts(
    left: &[Option<Arc<[ValidatedLineHighlight]>>],
    right: &[Option<Arc<[ValidatedLineHighlight]>>],
) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| match (left, right) {
                (Some(left), Some(right)) => Arc::ptr_eq(left, right),
                (None, None) => true,
                _ => false,
            })
}

/// Merge base and overlay marks in paint order without mutating either input.
///
/// Overlay marks append after base marks, so overlaps paint last. If either
/// side is empty the other map allocation is retained unchanged, preserving
/// downstream row-memoization identity.
#[must_use]
pub fn merge_line_highlight_maps(
    base: &LineHighlightMap,
    overlay: &LineHighlightMap,
) -> LineHighlightMap {
    if overlay.is_empty() {
        return base.clone();
    }
    if base.is_empty() {
        return overlay.clone();
    }

    let mut merged = (*base.0).clone();
    for (file_id, marks) in overlay.0.iter() {
        if let Some(existing) = merged.get(file_id) {
            let mut combined = Vec::with_capacity(existing.len() + marks.len());
            combined.extend_from_slice(existing);
            combined.extend_from_slice(marks);
            merged.insert(file_id.clone(), Arc::from(combined));
        } else {
            merged.insert(file_id.clone(), Arc::clone(marks));
        }
    }
    LineHighlightMap(Arc::new(merged))
}

/// Carry agent attention marks across one immutable review reload.
///
/// Stable file keys locate replacements. Exact content identity is required
/// before the existing mark allocation is re-keyed to the new runtime ID.
/// Removed or changed files lose their marks instead of painting stale text.
#[must_use]
pub fn carry_over_line_highlights(
    marks_by_file_id: &LineHighlightMap,
    previous: &Changeset,
    next: &Changeset,
) -> LineHighlightMap {
    let mut carried = BTreeMap::new();
    if marks_by_file_id.is_empty() {
        return LineHighlightMap(Arc::new(carried));
    }

    let next_by_key = next
        .files
        .iter()
        .map(|file| (file.key.as_str(), file))
        .collect::<BTreeMap<_, _>>();
    for file in &previous.files {
        let Some(marks) = marks_by_file_id.0.get(&file.runtime_id) else {
            continue;
        };
        if marks.is_empty() {
            continue;
        }
        let Some(replacement) = next_by_key.get(file.key.as_str()) else {
            continue;
        };
        if replacement.content_identity != file.content_identity {
            continue;
        }
        carried.insert(replacement.runtime_id.clone(), Arc::clone(marks));
    }
    LineHighlightMap(Arc::new(carried))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::{Duration, Instant};
    use workdeck_core::{ChangesetSource, ReviewSide};
    use workdeck_diff::parse_patch;
    use workdeck_extension_api::HighlightTone;

    type HighlightHandler = dyn Fn(&str, &DiffFile, &AtomicBool) -> Result<Value, LineHighlightRuntimeError>
        + Send
        + Sync;

    #[test]
    fn bound_highlighter_loads_sources_only_in_worker_and_preserves_review_data() {
        let reads = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&reads);
        let (changeset, sources) = workdeck_vcs::materialize_vcs_patch_result_deferred(
            workdeck_vcs::VcsPatchResult {
                repo_root: ".".into(),
                source_label: "highlight-source".into(),
                title: "highlight-source".into(),
                patch_text: "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-old\n+new\n".into(),
                untracked_paths: vec![],
                extra_files: vec![],
                source_cache_key: Some("pinned".into()),
                source_reader: Some(Arc::new(move |request| {
                    observed.fetch_add(1, Ordering::SeqCst);
                    Ok(workdeck_vcs::VcsFileSourceResult::Source(workdeck_core::SourceSnapshot::new(
                        match request.side { ReviewSide::Old => "old\n", ReviewSide::New => "new\n" }.into(),
                        workdeck_core::SourceOrigin::WorkingTree,
                        true,
                    )))
                })),
            },
            "review",
            ChangesetSource::WorkingTree { staged: false },
        ).unwrap();
        let original = serde_json::to_value(&changeset).unwrap();
        let runtime = FakeLineHighlightRuntime::new(|_, file, _| {
            assert_eq!(file.sources.old.as_ref().unwrap().content, "old\n");
            assert_eq!(file.sources.new.as_ref().unwrap().content, "new\n");
            Ok(json!([]))
        });
        let bound = SourceBoundLineHighlightRuntime {
            runtime: runtime.clone(),
            sources: Some(sources),
        };
        assert_eq!(reads.load(Ordering::SeqCst), 0);
        assert_eq!(
            bound.highlight_file("test", &changeset.files[0], &AtomicBool::new(true)),
            Err(LineHighlightRuntimeError::Retry)
        );
        assert_eq!(reads.load(Ordering::SeqCst), 0);
        assert!(runtime.calls().is_empty());
        for _ in 0..2 {
            bound
                .highlight_file("test", &changeset.files[0], &AtomicBool::new(false))
                .unwrap();
        }
        assert_eq!(reads.load(Ordering::SeqCst), 2);
        assert_eq!(runtime.calls().len(), 2);
        assert_eq!(serde_json::to_value(&changeset).unwrap(), original);
    }

    struct FakeLineHighlightRuntime {
        source_generation: std::sync::atomic::AtomicU64,
        pending: AtomicBool,
        calls: Mutex<Vec<(String, String)>>,
        warnings: Mutex<Vec<String>>,
        handler: Arc<HighlightHandler>,
    }

    #[test]
    fn replacement_source_handles_rederive_unattested_but_reuse_attested_highlights() {
        for attested in [false, true] {
            let load = |text: &'static str| {
                workdeck_vcs::materialize_vcs_patch_result_deferred(
                    workdeck_vcs::VcsPatchResult {
                        repo_root: ".".into(),
                        source_label: "source".into(),
                        title: "source".into(),
                        patch_text: "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-old\n+new\n".into(),
                        untracked_paths: vec![],
                        extra_files: vec![],
                        source_cache_key: attested.then(|| "immutable".into()),
                        source_reader: Some(Arc::new(move |_| {
                            Ok(workdeck_vcs::VcsFileSourceResult::Source(workdeck_core::SourceSnapshot::new(
                                text.into(), workdeck_core::SourceOrigin::WorkingTree, attested,
                            )))
                        })),
                    }, "review", ChangesetSource::WorkingTree { staged: false },
                ).unwrap()
            };
            let (first, first_sources) = load("first\n");
            let (second, second_sources) = load(if attested { "first\n" } else { "second\n" });
            assert_eq!(
                first.files[0].source_identity,
                second.files[0].source_identity
            );
            assert_eq!(
                first.files[0].content_identity,
                second.files[0].content_identity
            );
            let observed = Arc::new(Mutex::new(Vec::new()));
            let captured = Arc::clone(&observed);
            let runtime = FakeLineHighlightRuntime::new(move |_, file, _| {
                captured
                    .lock()
                    .unwrap()
                    .push(file.sources.new.as_ref().unwrap().content.clone());
                Ok(one_mark("match"))
            });
            let registrations = [registration("source-aware")];
            let epochs = workdeck_extension_host::LineHighlightEpochState::default();
            let mut controller = LineHighlightPreparationController::default();
            for (changeset, sources) in [(first, first_sources), (second, second_sources)] {
                let extensions: Vec<Arc<dyn LineHighlightRuntime>> =
                    vec![Arc::new(SourceBoundLineHighlightRuntime {
                        runtime: runtime.clone(),
                        sources: Some(sources),
                    })];
                reconcile_until(
                    &mut controller,
                    &extensions,
                    &registrations,
                    &epochs,
                    &changeset.files,
                    |controller| controller.pending_count() == 0,
                );
            }
            let expected = if attested {
                vec!["first\n"]
            } else {
                vec!["first\n", "second\n"]
            };
            assert_eq!(*observed.lock().unwrap(), expected);
        }
    }

    impl FakeLineHighlightRuntime {
        fn new(
            handler: impl Fn(&str, &DiffFile, &AtomicBool) -> Result<Value, LineHighlightRuntimeError>
            + Send
            + Sync
            + 'static,
        ) -> Arc<Self> {
            Arc::new(Self {
                source_generation: std::sync::atomic::AtomicU64::new(0),
                pending: AtomicBool::new(false),
                calls: Mutex::new(Vec::new()),
                warnings: Mutex::new(Vec::new()),
                handler: Arc::new(handler),
            })
        }

        fn calls(&self) -> Vec<(String, String)> {
            self.calls.lock().unwrap().clone()
        }

        fn clear_calls(&self) {
            self.calls.lock().unwrap().clear();
        }

        fn warnings(&self) -> Vec<String> {
            self.warnings.lock().unwrap().clone()
        }
    }

    impl LineHighlightRuntime for FakeLineHighlightRuntime {
        fn source_generation(&self, _: &DiffFile) -> Option<u64> {
            let generation = self.source_generation.load(Ordering::Acquire);
            (generation != 0).then_some(generation)
        }
        fn request_pending(&self) -> bool {
            self.pending.load(Ordering::Acquire)
        }

        fn highlight_file(
            &self,
            highlighter_id: &str,
            file: &DiffFile,
            cancelled: &AtomicBool,
        ) -> Result<Value, LineHighlightRuntimeError> {
            self.calls
                .lock()
                .unwrap()
                .push((highlighter_id.to_owned(), file.runtime_id.clone()));
            (self.handler)(highlighter_id, file, cancelled)
        }

        fn notify_warning(&self, message: String) {
            self.warnings.lock().unwrap().push(message);
        }
    }

    fn runtime_list(runtime: &Arc<FakeLineHighlightRuntime>) -> Vec<Arc<dyn LineHighlightRuntime>> {
        vec![Arc::clone(runtime) as Arc<dyn LineHighlightRuntime>]
    }

    fn registration(id: &str) -> RegisteredLineHighlighter {
        RegisteredLineHighlighter::new(0, "test-extension", id)
    }

    fn test_file(id: &str, content_identity: &str) -> DiffFile {
        let mut changeset = reloaded_document(&[(id, Some(content_identity))], "test");
        let mut file = changeset.files.remove(0);
        file.runtime_id = id.into();
        file.key = id.into();
        file.path = format!("{id}.rs");
        file
    }

    fn one_mark(tone: &str) -> Value {
        json!([{ "side": "new", "line": 1, "range": [0, 3], "tone": tone }])
    }

    fn reconcile_until(
        controller: &mut LineHighlightPreparationController,
        extensions: &[Arc<dyn LineHighlightRuntime>],
        registrations: &[RegisteredLineHighlighter],
        epochs: &workdeck_extension_host::LineHighlightEpochState,
        files: &[DiffFile],
        mut complete: impl FnMut(&LineHighlightPreparationController) -> bool,
    ) {
        // The 600-file stress case shares a debug test process with renderer, subprocess, and
        // highlighter workloads. Keep this as a deadlock deadline with scheduler headroom; exact
        // throughput belongs to the same-host release benchmarks rather than a wall-clock unit.
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            controller.reconcile(extensions, registrations, epochs, files);
            if complete(controller) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "line highlight preparation did not settle before the test deadline"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn mark(line: u64, start: u64, end: u64) -> ValidatedLineHighlight {
        ValidatedLineHighlight {
            side: ReviewSide::New,
            line,
            start,
            end,
            tone: HighlightTone::Match,
        }
    }

    fn reloaded_document(files: &[(&str, Option<&str>)], generation: &str) -> Changeset {
        let mut parsed = parse_patch(
            "diff --git a/sample.rs b/sample.rs\n--- a/sample.rs\n+++ b/sample.rs\n@@ -1 +1 @@\n-old\n+new\n",
            format!("reload-{generation}"),
            "reload",
            ChangesetSource::Patch {
                label: "reload".into(),
            },
        )
        .unwrap();
        let template = parsed.files.remove(0);
        parsed.files = files
            .iter()
            .map(|(key, content_identity)| {
                let mut file = template.clone();
                file.key = (*key).into();
                file.runtime_id = format!("{key}:{generation}");
                file.content_identity = content_identity
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("content:{key}"));
                file
            })
            .collect();
        parsed
    }

    #[test]
    fn returns_either_side_unchanged_when_the_other_is_empty() {
        let base = LineHighlightMap::from_entries([("file-1".into(), vec![mark(1, 0, 4)])]);
        let empty = LineHighlightMap::default();

        assert!(merge_line_highlight_maps(&base, &empty).ptr_eq(&base));
        assert!(merge_line_highlight_maps(&empty, &base).ptr_eq(&base));
    }

    #[test]
    fn appends_overlay_marks_after_base_without_mutating_inputs() {
        let base = LineHighlightMap::from_entries([("file-1".into(), vec![mark(1, 0, 4)])]);
        let overlay = LineHighlightMap::from_entries([
            ("file-1".into(), vec![mark(1, 2, 6)]),
            ("file-2".into(), vec![mark(3, 0, 2)]),
        ]);

        let merged = merge_line_highlight_maps(&base, &overlay);
        assert_eq!(
            merged.get("file-1"),
            Some([mark(1, 0, 4), mark(1, 2, 6)].as_slice())
        );
        assert_eq!(merged.get("file-2"), Some([mark(3, 0, 2)].as_slice()));
        assert_eq!(base.get("file-1").map(<[_]>::len), Some(1));
        assert_eq!(overlay.get("file-1").map(<[_]>::len), Some(1));
    }

    #[test]
    fn reload_rekeys_marks_when_content_is_unchanged_and_preserves_mark_identity() {
        let previous = reloaded_document(&[("alpha", None), ("beta", None)], "1");
        let next = reloaded_document(&[("alpha", None), ("beta", None)], "2");
        let marks = LineHighlightMap::from_entries([("alpha:1".into(), vec![mark(1, 0, 4)])]);

        let carried = carry_over_line_highlights(&marks, &previous, &next);
        assert_eq!(carried.get("alpha:2"), marks.get("alpha:1"));
        assert!(Arc::ptr_eq(
            carried.0.get("alpha:2").unwrap(),
            marks.0.get("alpha:1").unwrap(),
        ));
    }

    #[test]
    fn reload_drops_marks_when_content_changes() {
        let previous = reloaded_document(&[("alpha", None)], "1");
        let next = reloaded_document(&[("alpha", Some("content:changed"))], "2");
        let marks = LineHighlightMap::from_entries([("alpha:1".into(), vec![mark(1, 0, 4)])]);

        assert!(carry_over_line_highlights(&marks, &previous, &next).is_empty());
    }

    #[test]
    fn reload_drops_removed_files_and_keeps_surviving_files() {
        let previous = reloaded_document(&[("alpha", None), ("beta", None)], "1");
        let next = reloaded_document(&[("beta", None)], "2");
        let marks = LineHighlightMap::from_entries([
            ("alpha:1".into(), vec![mark(1, 0, 4)]),
            ("beta:1".into(), vec![mark(1, 0, 4)]),
        ]);

        let carried = carry_over_line_highlights(&marks, &previous, &next);
        assert_eq!(carried.len(), 1);
        assert_eq!(carried.get("beta:2"), Some([mark(1, 0, 4)].as_slice()));
    }

    #[test]
    fn reload_returns_an_empty_map_when_there_is_nothing_to_carry() {
        let previous = reloaded_document(&[("alpha", None)], "1");
        let next = reloaded_document(&[("alpha", None)], "2");
        assert!(
            carry_over_line_highlights(&LineHighlightMap::default(), &previous, &next).is_empty()
        );
    }

    #[test]
    fn coordinator_validates_caches_and_preserves_identity_across_unrelated_frames() {
        let runtime = FakeLineHighlightRuntime::new(|_, _, _| Ok(one_mark("match")));
        let extensions = runtime_list(&runtime);
        let registrations = [registration("test-highlighter")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let files = [test_file("request", "content:request")];
        let mut controller = LineHighlightPreparationController::default();

        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &epochs,
            &files,
            |controller| controller.resolved().get("request").is_some(),
        );
        assert_eq!(runtime.calls().len(), 1);
        assert_eq!(
            controller.resolved().get("request"),
            Some([mark(1, 0, 3)].as_slice())
        );
        let first_map = controller.resolved().clone();
        let first_marks = first_map.get_shared("request").unwrap().clone();

        for _ in 0..3 {
            controller.reconcile(&extensions, &registrations, &epochs, &files);
        }
        assert_eq!(runtime.calls().len(), 1);
        assert!(controller.resolved().ptr_eq(&first_map));
        assert!(Arc::ptr_eq(
            controller.resolved().get_shared("request").unwrap(),
            &first_marks
        ));
    }

    #[test]
    fn coordinator_rederives_only_the_file_with_a_bumped_epoch() {
        let runtime = FakeLineHighlightRuntime::new(|_, _, _| Ok(one_mark("match")));
        let extensions = runtime_list(&runtime);
        let registrations = [registration("test-highlighter")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let files = [
            test_file("first", "content:first"),
            test_file("second", "content:second"),
        ];
        let mut controller = LineHighlightPreparationController::default();
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &epochs,
            &files,
            |controller| controller.resolved().len() == 2,
        );
        let first_marks = controller.resolved().get_shared("first").unwrap().clone();

        runtime.clear_calls();
        let bumped = workdeck_extension_host::bump_scoped_epoch(
            &epochs,
            "test-extension:test-highlighter",
            Some("second"),
        );
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &bumped,
            &files,
            |controller| runtime.calls().len() == 1 && controller.pending_count() == 0,
        );
        assert_eq!(
            runtime.calls(),
            vec![("test-highlighter".into(), "second".into())]
        );
        assert!(Arc::ptr_eq(
            controller.resolved().get_shared("first").unwrap(),
            &first_marks
        ));
    }

    #[test]
    fn changed_source_identity_invalidates_unchanged_patch_highlights() {
        let runtime = FakeLineHighlightRuntime::new(|_, _, _| Ok(one_mark("match")));
        let extensions = runtime_list(&runtime);
        let registrations = [registration("source-aware")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let mut file = test_file("file", "same-patch");
        file.source_identity = Some("before".into());
        let mut controller = LineHighlightPreparationController::default();
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &epochs,
            std::slice::from_ref(&file),
            |controller| controller.pending_count() == 0,
        );
        assert_eq!(runtime.calls().len(), 1);
        file.source_identity = Some("after".into());
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &epochs,
            std::slice::from_ref(&file),
            |controller| controller.pending_count() == 0,
        );
        assert_eq!(runtime.calls().len(), 2);
    }

    #[test]
    fn replacement_registration_with_the_same_public_name_rederives_highlights() {
        let first = FakeLineHighlightRuntime::new(|_, _, _| Ok(one_mark("match")));
        let second = FakeLineHighlightRuntime::new(|_, _, _| Ok(one_mark("match")));
        let files = [test_file("file", "same-patch")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let mut controller = LineHighlightPreparationController::default();
        let registered = registration("same-name");
        for _ in 0..2 {
            reconcile_until(
                &mut controller,
                &runtime_list(&first),
                std::slice::from_ref(&registered),
                &epochs,
                &files,
                |controller| controller.pending_count() == 0,
            );
        }
        assert_eq!(first.calls().len(), 1);
        let replacement = registration("same-name");
        reconcile_until(
            &mut controller,
            &runtime_list(&second),
            &[replacement],
            &epochs,
            &files,
            |controller| controller.pending_count() == 0,
        );
        assert_eq!(second.calls().len(), 1);
    }

    #[test]
    fn replacement_registration_has_its_own_warning_deduplication() {
        let runtime = FakeLineHighlightRuntime::new(|_, _, _| {
            Err(LineHighlightRuntimeError::Failed("failure".into()))
        });
        let files = [test_file("file", "same-patch")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let mut controller = LineHighlightPreparationController::default();
        for count in 1..=2 {
            let registered = registration("same-name");
            for _ in 0..2 {
                reconcile_until(
                    &mut controller,
                    &runtime_list(&runtime),
                    std::slice::from_ref(&registered),
                    &epochs,
                    &files,
                    |controller| controller.pending_count() == 0,
                );
            }
            assert_eq!(runtime.calls().len(), count);
            assert_eq!(runtime.warnings().len(), count);
        }
    }

    #[test]
    fn replaced_source_generation_discards_a_late_worker_result() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let calls = AtomicUsize::new(0);
        let runtime = FakeLineHighlightRuntime::new(move |_, _, _| {
            let first = calls.fetch_add(1, Ordering::SeqCst) == 0;
            if first {
                started_tx.send(()).unwrap();
                release_rx
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(10))
                    .unwrap();
            }
            Ok(
                json!([{ "side": "new", "line": 1, "range": [0, if first { 1 } else { 2 }], "tone": "match" }]),
            )
        });
        let extensions = runtime_list(&runtime);
        let registrations = [registration("source-aware")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let files = [test_file("file", "same-patch")];
        let mut controller = LineHighlightPreparationController::default();
        runtime.source_generation.store(1, Ordering::Release);
        controller.reconcile(&extensions, &registrations, &epochs, &files);
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        runtime.source_generation.store(2, Ordering::Release);
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &epochs,
            &files,
            |controller| {
                controller
                    .resolved()
                    .get("file")
                    .is_some_and(|marks| marks[0].end == 2)
            },
        );
        release_tx.send(()).unwrap();
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &epochs,
            &files,
            |controller| controller.pending_count() == 0,
        );
        assert_eq!(controller.resolved().get("file").unwrap()[0].end, 2);
        assert_eq!(runtime.calls().len(), 2);
    }

    #[test]
    fn coordinator_contains_a_failure_to_one_highlighter_and_warns_once() {
        let runtime = FakeLineHighlightRuntime::new(|id, _, _| match id {
            "failing" => Err(LineHighlightRuntimeError::Failed("boom".into())),
            _ => Ok(one_mark("info")),
        });
        let extensions = runtime_list(&runtime);
        let registrations = [registration("failing"), registration("working")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let files = [test_file("request", "content:request")];
        let mut controller = LineHighlightPreparationController::default();
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &epochs,
            &files,
            |controller| controller.pending_count() == 0 && runtime.calls().len() == 2,
        );
        assert_eq!(controller.resolved().get("request").unwrap().len(), 1);
        assert_eq!(
            controller.resolved().get("request").unwrap()[0].tone,
            HighlightTone::Info
        );
        assert_eq!(runtime.warnings().len(), 1);
        assert!(runtime.warnings()[0].contains("failing"));
        for _ in 0..3 {
            controller.reconcile(&extensions, &registrations, &epochs, &files);
        }
        assert_eq!(runtime.warnings().len(), 1);
    }

    #[test]
    fn coordinator_rejects_an_over_cap_result_whole_with_one_warning() {
        let over_cap = Arc::new(Value::Array(
            (0..=workdeck_extension_host::MAX_LINE_HIGHLIGHTS_PER_FILE)
                .map(|index| json!({ "side": "new", "line": index + 1, "range": [0, 1] }))
                .collect(),
        ));
        let runtime = FakeLineHighlightRuntime::new(move |_, _, _| Ok((*over_cap).clone()));
        let extensions = runtime_list(&runtime);
        let registrations = [registration("test-highlighter")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let files = [test_file("request", "content:request")];
        let mut controller = LineHighlightPreparationController::default();
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &epochs,
            &files,
            |controller| controller.pending_count() == 0 && runtime.calls().len() == 1,
        );
        assert!(controller.resolved().get("request").is_none());
        assert_eq!(runtime.warnings().len(), 1);
        assert!(runtime.warnings()[0].contains("marks dropped"));
    }

    #[test]
    fn coordinator_publishes_a_quick_file_without_waiting_for_a_slow_file() {
        let runtime = FakeLineHighlightRuntime::new(|_, file, _| {
            if file.runtime_id == "slow" {
                thread::sleep(Duration::from_millis(200));
            }
            Ok(one_mark("match"))
        });
        let extensions = runtime_list(&runtime);
        let registrations = [registration("test-highlighter")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let files = [
            test_file("slow", "content:slow"),
            test_file("quick", "content:quick"),
        ];
        let mut controller = LineHighlightPreparationController::default();
        let started = Instant::now();
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &epochs,
            &files,
            |controller| controller.resolved().get("quick").is_some(),
        );
        assert!(started.elapsed() < Duration::from_millis(150));
        assert!(controller.resolved().get("slow").is_none());
    }

    #[test]
    fn coordinator_stops_publishing_marks_as_soon_as_content_is_replaced() {
        let requests = Arc::new(AtomicUsize::new(0));
        let runtime = FakeLineHighlightRuntime::new({
            let requests = Arc::clone(&requests);
            move |_, _, cancelled| {
                if requests.fetch_add(1, Ordering::AcqRel) > 0 {
                    while !cancelled.load(Ordering::Acquire) {
                        thread::sleep(Duration::from_millis(1));
                    }
                    return Err(LineHighlightRuntimeError::Retry);
                }
                Ok(one_mark("match"))
            }
        });
        let extensions = runtime_list(&runtime);
        let registrations = [registration("test-highlighter")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let initial = [test_file("request", "content:initial")];
        let replacement = [test_file("request", "content:replacement")];
        let mut controller = LineHighlightPreparationController::default();
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &epochs,
            &initial,
            |controller| controller.resolved().get("request").is_some(),
        );

        controller.reconcile(&extensions, &registrations, &epochs, &replacement);
        assert!(controller.resolved().get("request").is_none());
    }

    #[test]
    fn coordinator_retains_all_six_hundred_active_file_results() {
        let runtime = FakeLineHighlightRuntime::new(|_, _, _| Ok(one_mark("match")));
        let extensions = runtime_list(&runtime);
        let registrations = [registration("test-highlighter")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let files = (0..600)
            .map(|index| test_file(&format!("file-{index}"), &format!("content:{index}")))
            .collect::<Vec<_>>();
        let mut controller = LineHighlightPreparationController::default();
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &epochs,
            &files,
            |controller| controller.resolved().len() == files.len(),
        );
        assert_eq!(runtime.calls().len(), files.len());

        runtime.clear_calls();
        let bumped = workdeck_extension_host::bump_scoped_epoch(
            &epochs,
            "test-extension:test-highlighter",
            Some("file-0"),
        );
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &bumped,
            &files,
            |controller| controller.pending_count() == 0 && runtime.calls().len() == 1,
        );
        assert_eq!(
            runtime.calls(),
            vec![("test-highlighter".into(), "file-0".into())]
        );
        assert_eq!(controller.resolved().len(), files.len());
    }

    #[test]
    fn coordinator_drops_the_highlighter_that_exceeds_the_merged_cap() {
        let full = Arc::new(Value::Array(
            (0..workdeck_extension_host::MAX_LINE_HIGHLIGHTS_PER_FILE)
                .map(|index| {
                    json!({
                        "side": "new",
                        "line": index / 50 + 1,
                        "range": [index, index + 1]
                    })
                })
                .collect(),
        ));
        let runtime = FakeLineHighlightRuntime::new(move |id, _, _| {
            if id == "third" {
                Ok(one_mark("match"))
            } else {
                Ok((*full).clone())
            }
        });
        let extensions = runtime_list(&runtime);
        let registrations = [
            registration("first"),
            registration("second"),
            registration("third"),
        ];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let files = [test_file("request", "content:request")];
        let mut controller = LineHighlightPreparationController::default();
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &epochs,
            &files,
            |controller| controller.pending_count() == 0 && runtime.calls().len() == 3,
        );
        assert_eq!(
            controller.resolved().get("request").unwrap().len(),
            MAX_MERGED_LINE_HIGHLIGHTS_PER_FILE
        );
        assert_eq!(runtime.warnings().len(), 1);
        assert!(runtime.warnings()[0].contains("third"));
        assert!(runtime.warnings()[0].contains("merged ranges"));
    }

    #[test]
    fn coordinator_skips_binary_oversized_and_hunkless_files() {
        let runtime = FakeLineHighlightRuntime::new(|_, _, _| Ok(Value::Null));
        let extensions = runtime_list(&runtime);
        let registrations = [registration("test-highlighter")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let mut binary = test_file("binary", "content:binary");
        binary.flags.binary = true;
        let mut oversized = test_file("oversized", "content:oversized");
        oversized.flags.too_large = true;
        let mut hunkless = test_file("hunkless", "content:hunkless");
        hunkless.hunks.clear();
        let files = [
            binary,
            oversized,
            hunkless,
            test_file("request", "content:request"),
        ];
        let mut controller = LineHighlightPreparationController::default();
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &epochs,
            &files,
            |controller| controller.pending_count() == 0 && runtime.calls().len() == 1,
        );
        assert_eq!(
            runtime.calls(),
            vec![("test-highlighter".into(), "request".into())]
        );
    }
}
