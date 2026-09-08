//! Immutable line-highlight maps consumed by the review painter.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Instant;

use serde_json::Value;
use workdeck_core::{Changeset, DiffFile};
use workdeck_extension_api::ValidatedLineHighlight;
use workdeck_extension_host::{
    ExtensionRequestCancellation, HostError, LineHighlightValidation, LoadedExtension,
    MAX_MERGED_LINE_HIGHLIGHTS_PER_FILE, RegisteredLineHighlighter, scoped_epoch,
    validate_line_highlights,
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
        cancelled: &ExtensionRequestCancellation,
    ) -> Result<Value, LineHighlightRuntimeError>;
    fn highlight_file_with_reader(
        &self,
        highlighter_id: &str,
        file: &DiffFile,
        cancelled: &ExtensionRequestCancellation,
        reader: workdeck_extension_host::ExtensionDocumentReader,
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
        cancelled: &ExtensionRequestCancellation,
    ) -> Result<Value, LineHighlightRuntimeError> {
        if cancelled.is_cancelled() {
            return Err(LineHighlightRuntimeError::Retry);
        }
        let capability = self.sources.as_ref().and_then(|sources| sources.get(file));
        let snapshots = file.sources.clone();
        let reader = workdeck_extension_host::ExtensionDocumentReader::new(move |side| {
            let side = match side {
                workdeck_extension_api::ExtensionFileSide::Old => workdeck_core::ReviewSide::Old,
                workdeck_extension_api::ExtensionFileSide::New => workdeck_core::ReviewSide::New,
            };
            if let Some(capability) = &capability {
                return capability
                    .read(side)
                    .map(|result| match result {
                        workdeck_vcs::VcsFileSourceResult::Source(snapshot) => {
                            Some(snapshot.content)
                        }
                        workdeck_vcs::VcsFileSourceResult::Missing
                        | workdeck_vcs::VcsFileSourceResult::TooLarge { .. } => None,
                    })
                    .map_err(|error| error.to_string());
            }
            Ok(match side {
                workdeck_core::ReviewSide::Old => snapshots.old.as_ref(),
                workdeck_core::ReviewSide::New => snapshots.new.as_ref(),
            }
            .map(|snapshot| snapshot.content.clone()))
        });
        self.runtime
            .highlight_file_with_reader(highlighter_id, file, cancelled, reader)
    }

    fn highlight_file_with_reader(
        &self,
        highlighter_id: &str,
        file: &DiffFile,
        cancelled: &ExtensionRequestCancellation,
        reader: workdeck_extension_host::ExtensionDocumentReader,
    ) -> Result<Value, LineHighlightRuntimeError> {
        self.runtime
            .highlight_file_with_reader(highlighter_id, file, cancelled, reader)
    }

    fn notify_warning(&self, message: String) {
        self.runtime.notify_warning(message);
    }
}

impl LineHighlightRuntime for LoadedExtension {
    fn highlight_file_with_reader(
        &self,
        highlighter_id: &str,
        file: &DiffFile,
        cancelled: &ExtensionRequestCancellation,
        reader: workdeck_extension_host::ExtensionDocumentReader,
    ) -> Result<Value, LineHighlightRuntimeError> {
        self.clone()
            .highlight_file_with_cancellation(highlighter_id, file, cancelled, reader)
            .map_err(|error| match error {
                HostError::Busy(_) | HostError::Cancelled(_) => LineHighlightRuntimeError::Retry,
                error => LineHighlightRuntimeError::Failed(error.to_string()),
            })
    }
    fn request_pending(&self) -> bool {
        LoadedExtension::line_highlight_request_pending(self)
    }

    fn highlight_file(
        &self,
        highlighter_id: &str,
        file: &DiffFile,
        cancelled: &ExtensionRequestCancellation,
    ) -> Result<Value, LineHighlightRuntimeError> {
        let mut runtime = self.clone();
        let snapshots = file.sources.clone();
        let reader = workdeck_extension_host::ExtensionDocumentReader::new(move |side| {
            Ok(match side {
                workdeck_extension_api::ExtensionFileSide::Old => snapshots.old.as_ref(),
                workdeck_extension_api::ExtensionFileSide::New => snapshots.new.as_ref(),
            }
            .map(|snapshot| snapshot.content.clone()))
        });
        runtime
            .highlight_file_with_cancellation(highlighter_id, file, cancelled, reader)
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
    agent_identity: Option<String>,
    source_identity: Option<String>,
    source_generation: Option<u64>,
    highlighter_key: String,
    registration_identity: u64,
    epoch: u64,
}

impl LineHighlightTaskKey {
    /// Refresh epochs do not replace the file or registration being painted.
    fn same_paint_inputs(&self, other: &Self) -> bool {
        self.file_id == other.file_id
            && self.content_identity == other.content_identity
            && self.agent_identity == other.agent_identity
            && self.source_identity == other.source_identity
            && self.source_generation == other.source_generation
            && self.highlighter_key == other.highlighter_key
            && self.registration_identity == other.registration_identity
    }
}

#[derive(Debug, Clone)]
struct LineHighlightTask<F = Arc<DiffFile>> {
    key: LineHighlightTaskKey,
    extension_index: usize,
    extension_id: String,
    highlighter_id: String,
    file: F,
}

impl LineHighlightTask<&DiffFile> {
    /// Planning borrows the live review. Only a request that actually starts
    /// needs an immutable owned snapshot; deadline and completion state share it.
    fn freeze_for_worker(&self) -> LineHighlightTask {
        LineHighlightTask {
            key: self.key.clone(),
            extension_index: self.extension_index,
            extension_id: self.extension_id.clone(),
            highlighter_id: self.highlighter_id.clone(),
            file: Arc::new(self.file.clone()),
        }
    }
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
    cancellation: Arc<ExtensionRequestCancellation>,
    completed_at: Instant,
    cancelled_before_completion: bool,
}

#[derive(Debug, Clone)]
struct MergedLineHighlights {
    content_identity: String,
    published_inputs: Vec<LineHighlightTaskKey>,
    parts: Vec<Option<Arc<[ValidatedLineHighlight]>>>,
    merged: Arc<[ValidatedLineHighlight]>,
}

#[derive(Debug)]
struct PreparationFileIdentity {
    id: String,
    content: String,
    source: Option<String>,
    binary: bool,
    too_large: bool,
    has_hunks: bool,
}

impl PreparationFileIdentity {
    fn capture(file: &DiffFile) -> Self {
        Self {
            id: file.runtime_id.clone(),
            content: file.content_identity.clone(),
            source: file.source_identity.clone(),
            binary: file.flags.binary,
            too_large: file.flags.too_large,
            has_hunks: !file.hunks.is_empty(),
        }
    }

    fn matches(&self, file: &DiffFile) -> bool {
        self.id == file.runtime_id
            && self.content == file.content_identity
            && self.source == file.source_identity
            && self.binary == file.flags.binary
            && self.too_large == file.flags.too_large
            && self.has_hunks != file.hunks.is_empty()
    }
}

/// Background preparation, caching, containment, and publication for native line highlighters.
#[derive(Debug)]
pub struct LineHighlightPreparationController {
    retired: bool,
    generation: Option<Vec<LineHighlightTaskKey>>,
    generation_files: Vec<PreparationFileIdentity>,
    deadlines: BTreeMap<LineHighlightTaskKey, (Instant, LineHighlightTask)>,
    // One lifetime per provider attempt, including transport contention/retries.
    attempt_deadlines: BTreeMap<LineHighlightTaskKey, Instant>,
    cache: BTreeMap<LineHighlightTaskKey, Option<Arc<[ValidatedLineHighlight]>>>,
    pending: BTreeMap<LineHighlightTaskKey, Arc<ExtensionRequestCancellation>>,
    sender: mpsc::Sender<LineHighlightCompletion>,
    receiver: mpsc::Receiver<LineHighlightCompletion>,
    merged: BTreeMap<String, MergedLineHighlights>,
    resolved: LineHighlightMap,
    reported_issues: BTreeSet<String>,
    issue_order: VecDeque<String>,
}

impl Drop for LineHighlightPreparationController {
    fn drop(&mut self) {
        self.cancel_pending();
    }
}

impl Default for LineHighlightPreparationController {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            retired: false,
            generation: None,
            generation_files: Vec::new(),
            deadlines: BTreeMap::new(),
            attempt_deadlines: BTreeMap::new(),
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
    fn cancel_pending(&mut self) {
        for cancellation in self.pending.values() {
            cancellation.cancel();
        }
        self.pending.clear();
        self.deadlines.clear();
        self.attempt_deadlines.clear();
    }

    /// Terminal transition, invoked before retiring native extension processes.
    /// Subsequent reconciliation cannot start or publish work for this owner.
    pub fn retire(&mut self) {
        if self.retired {
            return;
        }
        self.retired = true;
        self.replace_document();
        self.reported_issues.clear();
        self.issue_order.clear();
    }

    /// A committed reload replaces the file objects even when their text matches.
    /// Keep registration-scoped warning history, but not old file derivations.
    pub fn replace_document(&mut self) {
        self.cancel_pending();
        self.generation = None;
        self.generation_files.clear();
        self.cache.clear();
        self.merged.clear();
        self.resolved = LineHighlightMap::default();
    }

    #[must_use]
    pub fn resolved(&self) -> &LineHighlightMap {
        &self.resolved
    }

    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Prepare at most four files concurrently, with registration-ordered work per file.
    ///
    /// The method never waits for extension code. Calling it from successive
    /// Ratatui frames publishes each file as soon as all of that file's
    /// registration-ordered parts have settled.
    pub fn reconcile<'a>(
        &mut self,
        extensions: &[Arc<dyn LineHighlightRuntime>],
        registrations: &[RegisteredLineHighlighter],
        epochs: &workdeck_extension_host::LineHighlightEpochState,
        files: impl IntoIterator<Item = &'a DiffFile>,
    ) {
        if self.retired {
            return;
        }
        let files = files.into_iter().collect::<Vec<_>>();
        let tasks =
            desired_line_highlight_tasks(extensions, registrations, epochs, files.iter().copied());
        // A changed preparation pass aborts all unfinished work, even for a
        // file whose own key survived. Completed derivations remain reusable.
        if self
            .generation
            .as_ref()
            .is_none_or(|generation| generation.iter().ne(tasks.iter().map(|task| &task.key)))
            || self.generation_files.len() != files.len()
            || self
                .generation_files
                .iter()
                .zip(&files)
                .any(|(previous, file)| !previous.matches(file))
        {
            self.cancel_pending();
            self.generation = Some(tasks.iter().map(|task| task.key.clone()).collect());
            self.generation_files = files
                .iter()
                .map(|file| PreparationFileIdentity::capture(file))
                .collect();
        }
        let desired = tasks
            .iter()
            .map(|task| task.key.clone())
            .collect::<BTreeSet<_>>();
        self.pending.retain(|key, cancellation| {
            if !desired.contains(key) {
                cancellation.cancel();
                false
            } else {
                true
            }
        });
        self.poll_completions(&desired, extensions);
        self.expire_requests(Instant::now(), extensions);
        self.deadlines
            .retain(|key, _| self.pending.contains_key(key));
        self.cache.retain(|key, _| desired.contains(key));
        self.attempt_deadlines
            .retain(|key, _| desired.contains(key) && !self.cache.contains_key(key));
        self.merged.retain(|file_id, entry| {
            files.iter().any(|file| {
                file.runtime_id == *file_id && file.content_identity == entry.content_identity
            })
        });

        let mut blocked_files = BTreeSet::new();
        for task in &tasks {
            if self.cache.contains_key(&task.key) {
                continue;
            }
            // The first unresolved registration owns this file's turn, including
            // while its native connection is busy. Later registrations cannot pass it.
            if !blocked_files.insert(task.key.file_id.as_str()) {
                continue;
            }
            // Hunk's worker owns the file until every provider settles, even
            // when the next provider cannot accept a native request yet.
            if blocked_files.len() > LINE_HIGHLIGHT_CONCURRENCY {
                break;
            }
            if self
                .pending
                .keys()
                .any(|key| key.file_id == task.key.file_id)
            {
                continue;
            }
            if self.pending.len() >= LINE_HIGHLIGHT_CONCURRENCY {
                break;
            }
            let Some(extension) = extensions.get(task.extension_index) else {
                continue;
            };
            let now = Instant::now();
            let deadline = *self
                .attempt_deadlines
                .entry(task.key.clone())
                .or_insert(now + workdeck_extension_host::LINE_HIGHLIGHT_TIMEOUT);
            if now >= deadline {
                self.cache.insert(task.key.clone(), None);
                self.attempt_deadlines.remove(&task.key);
                self.report_once(extensions, task, "highlight", format!(
                    "Extension {} line highlighter \"{}\" failed highlighting {} • marks dropped",
                    task.extension_id, task.highlighter_id, task.file.path,
                ));
                continue;
            }
            if extension.request_pending() {
                continue;
            }
            let task = task.freeze_for_worker();
            let cancelled = Arc::new(ExtensionRequestCancellation::default());
            self.pending
                .insert(task.key.clone(), Arc::clone(&cancelled));
            self.deadlines
                .insert(task.key.clone(), (deadline, task.clone()));
            let extension = Arc::clone(extension);
            let sender = self.sender.clone();
            thread::spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    extension.highlight_file(&task.highlighter_id, &task.file, &cancelled)
                }));
                let outcome = match result {
                    Ok(Ok(value)) => LineHighlightTaskOutcome::Value(value),
                    Ok(Err(LineHighlightRuntimeError::Retry)) => LineHighlightTaskOutcome::Retry,
                    Ok(Err(LineHighlightRuntimeError::Failed(error))) => {
                        LineHighlightTaskOutcome::Failed(error)
                    }
                    Err(_) => LineHighlightTaskOutcome::Failed("highlight worker panicked".into()),
                };
                let completed_at = Instant::now();
                let cancelled_before_completion = cancelled.cancel_with_reason(None);
                let _ = sender.send(LineHighlightCompletion {
                    task,
                    outcome,
                    cancellation: cancelled,
                    completed_at,
                    cancelled_before_completion,
                });
            });
        }
        self.publish_complete_files(extensions, registrations, epochs, &files);
    }

    fn poll_completions(
        &mut self,
        desired: &BTreeSet<LineHighlightTaskKey>,
        extensions: &[Arc<dyn LineHighlightRuntime>],
    ) {
        while let Ok(mut completion) = self.receiver.try_recv() {
            let current = self
                .pending
                .get(&completion.task.key)
                .is_some_and(|cancellation| Arc::ptr_eq(cancellation, &completion.cancellation));
            if !current {
                continue;
            }
            self.pending.remove(&completion.task.key);
            if let Some((deadline, _)) = self.deadlines.remove(&completion.task.key)
                && completion.completed_at >= deadline
            {
                completion.outcome = LineHighlightTaskOutcome::Failed("highlight timed out".into());
            }
            if completion.cancelled_before_completion || !desired.contains(&completion.task.key) {
                continue;
            }
            completion.cancellation.cancel();
            match completion.outcome {
                LineHighlightTaskOutcome::Retry => {}
                LineHighlightTaskOutcome::Failed(_error) => {
                    self.cache.insert(completion.task.key.clone(), None);
                    self.report_once(
                        extensions,
                        &completion.task,
                        "highlight",
                        format!(
                            "Extension {} line highlighter \"{}\" failed highlighting {} • marks dropped",
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
                                    "Extension {} line highlighter \"{}\" {issue} for {} • marks dropped",
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
                                        "Extension {} line highlighter \"{}\" returned {dropped_invalid} invalid range{} for {} • dropped",
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

    fn expire_requests(&mut self, now: Instant, extensions: &[Arc<dyn LineHighlightRuntime>]) {
        let expired = self
            .deadlines
            .iter()
            .filter(|(_, (deadline, _))| now >= *deadline)
            .map(|(key, (_, task))| (key.clone(), task.clone()))
            .collect::<Vec<_>>();
        for (key, task) in expired {
            self.deadlines.remove(&key);
            let Some(cancellation) = self.pending.remove(&key) else {
                continue;
            };
            cancellation.cancel_with_reason(Some(
                serde_json::json!({"name":"Error","message":"highlight timed out"}),
            ));
            self.cache.insert(key, None);
            self.report_once(
                extensions,
                &task,
                "highlight",
                format!(
                    "Extension {} line highlighter \"{}\" failed highlighting {} • marks dropped",
                    task.extension_id, task.highlighter_id, task.file.path,
                ),
            );
        }
    }

    fn report_once<F>(
        &mut self,
        extensions: &[Arc<dyn LineHighlightRuntime>],
        task: &LineHighlightTask<F>,
        issue: &str,
        message: String,
    ) {
        let key = format!(
            "{}:{}:{}:{issue}",
            task.key.registration_identity, task.highlighter_id, task.key.file_id
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
        files: &[&DiffFile],
    ) {
        let mut resolved = BTreeMap::new();
        for &file in files {
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
                        agent_identity: agent_context_identity(file),
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
                if let Some(previous) = self.merged.get(&file.runtime_id)
                    && previous.published_inputs.len() == keyed.len()
                    && previous
                        .published_inputs
                        .iter()
                        .zip(&keyed)
                        .all(|(input, (_, _, _, key))| input.same_paint_inputs(key))
                {
                    resolved.insert(file.runtime_id.clone(), Arc::clone(&previous.merged));
                }
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
                        file,
                    };
                    self.report_once(
                        extensions,
                        &task,
                        "merged-cap",
                        format!(
                            "Extension {} line highlighter \"{}\" pushed {} past {} merged ranges • marks dropped",
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
            let reuse = self.merged.get(&file.runtime_id).filter(|previous| {
                previous.content_identity == file.content_identity
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
                    published_inputs: keyed.iter().map(|(_, _, _, key)| key.clone()).collect(),
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

fn desired_line_highlight_tasks<'a>(
    extensions: &[Arc<dyn LineHighlightRuntime>],
    registrations: &[RegisteredLineHighlighter],
    epochs: &workdeck_extension_host::LineHighlightEpochState,
    files: impl IntoIterator<Item = &'a DiffFile>,
) -> Vec<LineHighlightTask<&'a DiffFile>> {
    files
        .into_iter()
        .filter(|file| !file.flags.binary && !file.flags.too_large && !file.hunks.is_empty())
        .flat_map(|file| {
            registrations.iter().map(move |registration| {
                let highlighter_key =
                    workdeck_extension_host::registered_line_highlighter_key(registration);
                LineHighlightTask {
                    key: LineHighlightTaskKey {
                        file_id: file.runtime_id.clone(),
                        content_identity: file.content_identity.clone(),
                        agent_identity: agent_context_identity(file),
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
                    file,
                }
            })
        })
        .collect()
}

fn agent_context_identity(file: &DiffFile) -> Option<String> {
    file.agent.as_ref().map(|agent| {
        workdeck_core::review_digest(
            &serde_json::to_vec(agent).expect("agent context is serializable"),
        )
    })
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

    type HighlightHandler = dyn Fn(
            &str,
            &DiffFile,
            &ExtensionRequestCancellation,
        ) -> Result<Value, LineHighlightRuntimeError>
        + Send
        + Sync;

    #[test]
    fn source_binding_preserves_shared_cancellation_reason_identity() {
        let runtime = FakeLineHighlightRuntime::new(|_, _, cancellation| {
            assert!(!cancellation.cancel_with_reason(Some(json!({"generation":7}))));
            Ok(Value::Null)
        });
        let bound = SourceBoundLineHighlightRuntime {
            runtime,
            sources: None,
        };
        let cancellation = ExtensionRequestCancellation::default();
        bound
            .highlight_file("test", &test_file("file", "content"), &cancellation)
            .unwrap();
        assert!(cancellation.is_cancelled());
        assert!(cancellation.cancel_with_reason(None));
        assert_eq!(cancellation.reason(), Some(json!({"generation":7})));
    }

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
        let annotation: workdeck_core::AgentAnnotation = serde_json::from_value(json!({
            "summary": "live note", "tags": []
        }))
        .unwrap();
        let annotations =
            BTreeMap::from([(changeset.files[0].runtime_id.clone(), vec![annotation])]);
        let merged =
            crate::public_review::merge_file_annotations_borrowed(&changeset.files, &annotations);
        assert_eq!(reads.load(Ordering::SeqCst), 0);
        assert_eq!(
            merged[0].source_identity,
            changeset.files[0].source_identity
        );
        assert!(sources.get(&merged[0]).is_some());
        let runtime = FakeLineHighlightRuntime::new(|_, file, _| {
            assert_eq!(file.sources.old.as_ref().unwrap().content, "old\n");
            assert_eq!(file.sources.new.as_ref().unwrap().content, "new\n");
            assert_eq!(
                file.agent.as_ref().unwrap().annotations[0].summary,
                "live note"
            );
            Ok(json!([]))
        });
        let bound = SourceBoundLineHighlightRuntime {
            runtime: runtime.clone(),
            sources: Some(sources),
        };
        assert_eq!(reads.load(Ordering::SeqCst), 0);
        assert_eq!(
            bound.highlight_file("test", &merged[0], &{
                let cancellation = ExtensionRequestCancellation::default();
                cancellation.cancel();
                cancellation
            }),
            Err(LineHighlightRuntimeError::Retry)
        );
        assert_eq!(reads.load(Ordering::SeqCst), 0);
        assert!(runtime.calls().is_empty());
        for _ in 0..2 {
            bound
                .highlight_file("test", &merged[0], &ExtensionRequestCancellation::default())
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
            handler: impl Fn(
                &str,
                &DiffFile,
                &ExtensionRequestCancellation,
            ) -> Result<Value, LineHighlightRuntimeError>
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
        fn highlight_file_with_reader(
            &self,
            highlighter_id: &str,
            file: &DiffFile,
            cancelled: &ExtensionRequestCancellation,
            reader: workdeck_extension_host::ExtensionDocumentReader,
        ) -> Result<Value, LineHighlightRuntimeError> {
            let read = |side| {
                reader
                    .read_document(side)
                    .wait_until(
                        &workdeck_extension_host::ExtensionRequestCancellation::default(),
                        Instant::now() + Duration::from_secs(5),
                    )
                    .unwrap()
                    .map(|text| {
                        workdeck_core::SourceSnapshot::new(
                            text,
                            workdeck_core::SourceOrigin::WorkingTree,
                            true,
                        )
                    })
            };
            let mut loaded = file.clone();
            loaded.set_sources(workdeck_core::FileSourceSnapshots {
                old: read(workdeck_extension_api::ExtensionFileSide::Old),
                new: read(workdeck_extension_api::ExtensionFileSide::New),
            });
            self.highlight_file(highlighter_id, &loaded, cancelled)
        }
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
            cancelled: &ExtensionRequestCancellation,
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
    fn planning_borrows_files_and_started_work_shares_one_immutable_snapshot() {
        let runtime = FakeLineHighlightRuntime::new(|_, _, _| Ok(one_mark("match")));
        let extensions = runtime_list(&runtime);
        let registrations = [registration("first"), registration("second")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let mut files = [test_file("file", "content")];
        let tasks = desired_line_highlight_tasks(&extensions, &registrations, &epochs, &files);
        assert_eq!(tasks.len(), 2);
        for task in &tasks {
            assert!(std::ptr::eq(task.file, &files[0]));
        }
        let frozen = tasks[0].freeze_for_worker();
        let deadline_copy = frozen.clone();
        assert!(Arc::ptr_eq(&frozen.file, &deadline_copy.file));
        assert!(!std::ptr::eq(frozen.file.as_ref(), &files[0]));
        drop(tasks);
        files[0].path = "replacement.rs".into();
        files[0].hunks.clear();
        assert_eq!(frozen.file.path, "file.rs");
        assert!(!frozen.file.hunks.is_empty());
        assert_eq!(deadline_copy.file.path, "file.rs");
    }

    #[test]
    fn warning_deduplication_is_bounded_fifo_and_duplicates_do_not_refresh_age() {
        let runtime = FakeLineHighlightRuntime::new(|_, _, _| Ok(one_mark("match")));
        let extensions = runtime_list(&runtime);
        let registrations = [registration("warning")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let files = (0..=LINE_HIGHLIGHT_ISSUE_MAX_ENTRIES)
            .map(|index| test_file(&format!("file-{index}"), "content"))
            .collect::<Vec<_>>();
        let tasks = desired_line_highlight_tasks(&extensions, &registrations, &epochs, &files);
        let mut controller = LineHighlightPreparationController::default();
        for (index, task) in tasks
            .iter()
            .take(LINE_HIGHLIGHT_ISSUE_MAX_ENTRIES)
            .enumerate()
        {
            controller.report_once(&extensions, task, "highlight", format!("warning {index}"));
        }
        controller.report_once(&extensions, &tasks[0], "highlight", "duplicate".into());
        assert_eq!(runtime.warnings().len(), LINE_HIGHLIGHT_ISSUE_MAX_ENTRIES);
        controller.report_once(
            &extensions,
            &tasks[LINE_HIGHLIGHT_ISSUE_MAX_ENTRIES],
            "highlight",
            "new warning".into(),
        );
        controller.report_once(&extensions, &tasks[1], "highlight", "still retained".into());
        assert_eq!(
            runtime.warnings().len(),
            LINE_HIGHLIGHT_ISSUE_MAX_ENTRIES + 1
        );
        controller.report_once(
            &extensions,
            &tasks[0],
            "highlight",
            "oldest reports again".into(),
        );
        assert_eq!(runtime.warnings().last().unwrap(), "oldest reports again");
        assert_eq!(
            runtime.warnings().len(),
            LINE_HIGHLIGHT_ISSUE_MAX_ENTRIES + 2
        );
        assert_eq!(
            controller.reported_issues.len(),
            LINE_HIGHLIGHT_ISSUE_MAX_ENTRIES
        );
        assert_eq!(
            controller.issue_order.len(),
            LINE_HIGHLIGHT_ISSUE_MAX_ENTRIES
        );
    }

    #[test]
    fn removing_all_registrations_clears_derivations_and_readding_reexecutes() {
        let runtime = FakeLineHighlightRuntime::new(|_, _, _| Ok(one_mark("match")));
        let extensions = runtime_list(&runtime);
        let registrations = [registration("reset")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let files = [test_file("file", "content")];
        let mut controller = LineHighlightPreparationController::default();
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &epochs,
            &files,
            |controller| !controller.resolved().is_empty(),
        );
        let previous = controller.resolved().get_shared("file").unwrap().clone();
        assert_eq!(runtime.calls().len(), 1);
        controller.reconcile(&extensions, &[], &epochs, &files);
        assert!(controller.resolved().is_empty());
        assert!(controller.cache.is_empty());
        assert_eq!(controller.pending_count(), 0);
        let empty = controller.resolved().clone();
        controller.reconcile(&extensions, &[], &epochs, &files);
        assert!(empty.ptr_eq(controller.resolved()));
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &epochs,
            &files,
            |controller| !controller.resolved().is_empty(),
        );
        assert_eq!(runtime.calls().len(), 2);
        assert!(!Arc::ptr_eq(
            &previous,
            controller.resolved().get_shared("file").unwrap()
        ));
    }

    #[test]
    fn removing_registrations_preserves_warning_history_for_the_same_registration() {
        let runtime = FakeLineHighlightRuntime::new(|_, _, _| {
            Err(LineHighlightRuntimeError::Failed("failed".into()))
        });
        let extensions = runtime_list(&runtime);
        let registrations = [registration("warning")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let files = [test_file("file", "content")];
        let mut controller = LineHighlightPreparationController::default();
        for _ in 0..2 {
            reconcile_until(
                &mut controller,
                &extensions,
                &registrations,
                &epochs,
                &files,
                |controller| controller.cache.len() == 1,
            );
            controller.reconcile(&extensions, &[], &epochs, &files);
            assert!(controller.cache.is_empty());
        }
        assert_eq!(runtime.calls().len(), 2);
        assert_eq!(runtime.warnings().len(), 1);
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
    fn changed_agent_context_rederives_highlights_without_changing_diff_identity() {
        let runtime = FakeLineHighlightRuntime::new(|_, _, _| Ok(one_mark("match")));
        let extensions = runtime_list(&runtime);
        let registrations = [registration("agent-aware")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let mut file = test_file("file", "same-patch");
        file.refresh_identity();
        let content_identity = file.content_identity.clone();
        let mut controller = LineHighlightPreparationController::default();
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &epochs,
            std::slice::from_ref(&file),
            |controller| controller.pending_count() == 0,
        );
        file.agent = Some(workdeck_core::AgentFileContext {
            path: file.path.clone(),
            summary: Some("new rationale".into()),
            annotations: vec![],
        });
        file.refresh_identity();
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &epochs,
            std::slice::from_ref(&file),
            |controller| controller.pending_count() == 0,
        );
        assert_eq!(file.content_identity, content_identity);
        assert_eq!(runtime.calls().len(), 2);
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &epochs,
            std::slice::from_ref(&file.clone()),
            |controller| controller.pending_count() == 0,
        );
        assert_eq!(runtime.calls().len(), 2);
        file.agent = None;
        file.refresh_identity();
        assert_eq!(file.content_identity, content_identity);
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &epochs,
            std::slice::from_ref(&file),
            |controller| controller.pending_count() == 0,
        );
        assert_eq!(runtime.calls().len(), 3);
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
    fn dropping_preparation_owner_cancels_its_running_worker() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let runtime = FakeLineHighlightRuntime::new(move |_, _, cancelled| {
            started_tx.send(()).unwrap();
            release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            finished_tx.send(cancelled.is_cancelled()).unwrap();
            Ok(one_mark("match"))
        });
        let mut controller = LineHighlightPreparationController::default();
        controller.reconcile(
            &runtime_list(&runtime),
            &[registration("running")],
            &workdeck_extension_host::LineHighlightEpochState::default(),
            &[test_file("file", "content")],
        );
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(controller);
        release_tx.send(()).unwrap();
        assert!(finished_rx.recv_timeout(Duration::from_secs(5)).unwrap());
    }

    #[test]
    fn whole_request_deadline_releases_a_blocked_worker_and_rejects_its_late_marks() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let runtime = FakeLineHighlightRuntime::new(move |_, _, cancelled| {
            started_tx.send(()).unwrap();
            release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            finished_tx.send(cancelled.is_cancelled()).unwrap();
            Ok(one_mark("match"))
        });
        let extensions = runtime_list(&runtime);
        let registrations = [registration("blocked")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let files = [test_file("file", "content")];
        let mut controller = LineHighlightPreparationController::default();
        controller.reconcile(&extensions, &registrations, &epochs, &files);
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let deadline = controller.deadlines.values().next().unwrap().0;
        let cancellation = controller.pending.values().next().unwrap().clone();
        controller.expire_requests(deadline, &extensions);
        assert_eq!(
            cancellation.reason(),
            Some(json!({"name":"Error","message":"highlight timed out"}))
        );
        assert_eq!(controller.pending_count(), 0);
        assert!(controller.cache.values().all(Option::is_none));
        assert_eq!(runtime.warnings().len(), 1);
        assert_eq!(
            runtime.warnings()[0],
            "Extension test-extension line highlighter \"blocked\" failed highlighting file.rs • marks dropped"
        );
        release_tx.send(()).unwrap();
        assert!(finished_rx.recv_timeout(Duration::from_secs(5)).unwrap());
        let completion = controller
            .receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        controller.sender.send(completion).unwrap();
        controller.reconcile(&extensions, &registrations, &epochs, &files);
        assert!(controller.resolved().is_empty());
        assert_eq!(runtime.calls().len(), 1);
        assert_eq!(runtime.warnings().len(), 1);
        assert_eq!(
            cancellation.reason(),
            Some(json!({"name":"Error","message":"highlight timed out"}))
        );
    }

    #[test]
    fn queued_result_completed_after_deadline_is_not_published() {
        let runtime = FakeLineHighlightRuntime::new(|_, _, _| Ok(one_mark("match")));
        let extensions = runtime_list(&runtime);
        let files = [test_file("file", "content")];
        let mut tasks = desired_line_highlight_tasks(
            &extensions,
            &[registration("late")],
            &workdeck_extension_host::LineHighlightEpochState::default(),
            &files,
        );
        let task = tasks.remove(0).freeze_for_worker();
        let desired = BTreeSet::from([task.key.clone()]);
        let cancellation = Arc::new(ExtensionRequestCancellation::default());
        let deadline = Instant::now();
        let mut controller = LineHighlightPreparationController::default();
        controller
            .pending
            .insert(task.key.clone(), cancellation.clone());
        controller
            .deadlines
            .insert(task.key.clone(), (deadline, task.clone()));
        controller
            .sender
            .send(LineHighlightCompletion {
                task: task.clone(),
                outcome: LineHighlightTaskOutcome::Value(one_mark("match")),
                cancellation: cancellation.clone(),
                completed_at: deadline,
                cancelled_before_completion: false,
            })
            .unwrap();
        controller.poll_completions(&desired, &extensions);
        assert!(controller.pending.is_empty());
        assert!(controller.deadlines.is_empty());
        assert!(controller.cache.get(&task.key).unwrap().is_none());
        assert!(cancellation.is_cancelled());
        assert_eq!(runtime.warnings().len(), 1);
        assert_eq!(
            runtime.warnings()[0],
            "Extension test-extension line highlighter \"late\" failed highlighting file.rs • marks dropped"
        );
    }

    #[test]
    fn pending_replacement_never_reuses_marks_from_different_paint_inputs() {
        for changed in ["content", "source", "reader", "registration"] {
            let runtime = FakeLineHighlightRuntime::new(|_, _, _| Ok(one_mark("match")));
            let extensions = runtime_list(&runtime);
            let mut registrations = [registration("refresh")];
            let epochs = workdeck_extension_host::LineHighlightEpochState::default();
            let mut files = [test_file("file", "content")];
            let mut controller = LineHighlightPreparationController::default();
            reconcile_until(
                &mut controller,
                &extensions,
                &registrations,
                &epochs,
                &files,
                |controller| controller.resolved().len() == 1,
            );
            runtime.pending.store(true, Ordering::Release);
            match changed {
                "content" => files[0].content_identity = "replacement".into(),
                "source" => files[0].source_identity = Some("replacement".into()),
                "reader" => runtime.source_generation.store(1, Ordering::Release),
                "registration" => registrations[0] = registration("refresh"),
                _ => unreachable!(),
            }
            controller.reconcile(&extensions, &registrations, &epochs, &files);
            assert!(
                controller.resolved().is_empty(),
                "stale marks after {changed} replacement"
            );
            assert_eq!(runtime.calls().len(), 1);
        }
    }

    #[test]
    fn pending_epoch_refresh_keeps_previous_marks_until_replacement_settles() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let calls = AtomicUsize::new(0);
        let runtime = FakeLineHighlightRuntime::new(move |_, _, _| {
            if calls.fetch_add(1, Ordering::AcqRel) == 0 {
                return Ok(one_mark("match"));
            }
            started_tx.send(()).unwrap();
            release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            Ok(json!([]))
        });
        let extensions = runtime_list(&runtime);
        let registrations = [registration("refresh")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let files = [test_file("file", "content")];
        let mut controller = LineHighlightPreparationController::default();
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &epochs,
            &files,
            |controller| controller.resolved().len() == 1,
        );
        let original = controller.resolved().get_shared("file").unwrap().clone();
        let bumped = workdeck_extension_host::bump_scoped_epoch(
            &epochs,
            "test-extension:refresh",
            Some("file"),
        );
        controller.reconcile(&extensions, &registrations, &bumped, &files);
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(
            controller
                .resolved()
                .get_shared("file")
                .is_some_and(|marks| Arc::ptr_eq(marks, &original))
        );
        release_tx.send(()).unwrap();
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &bumped,
            &files,
            |controller| controller.pending_count() == 0,
        );
        assert!(controller.resolved().is_empty());
        assert_eq!(runtime.calls().len(), 2);
    }

    #[test]
    fn refreshing_empty_contributor_preserves_merged_marks_identity() {
        let runtime = FakeLineHighlightRuntime::new(|id, _, _| {
            Ok(if id == "empty" {
                json!([])
            } else {
                one_mark("match")
            })
        });
        let extensions = runtime_list(&runtime);
        let registrations = [registration("marks"), registration("empty")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let files = [test_file("file", "content")];
        let mut controller = LineHighlightPreparationController::default();
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &epochs,
            &files,
            |controller| controller.resolved().len() == 1,
        );
        let original = controller.resolved().get_shared("file").unwrap().clone();
        runtime.clear_calls();
        let bumped = workdeck_extension_host::bump_scoped_epoch(
            &epochs,
            "test-extension:empty",
            Some("file"),
        );
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &bumped,
            &files,
            |controller| controller.pending_count() == 0 && controller.resolved().len() == 1,
        );
        assert_eq!(runtime.calls(), [("empty".into(), "file".into())]);
        assert!(Arc::ptr_eq(
            &original,
            controller.resolved().get_shared("file").unwrap()
        ));
        let replaced = workdeck_extension_host::bump_scoped_epoch(
            &bumped,
            "test-extension:marks",
            Some("file"),
        );
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &replaced,
            &files,
            |controller| controller.pending_count() == 0 && controller.resolved().len() == 1,
        );
        assert!(!Arc::ptr_eq(
            &original,
            controller.resolved().get_shared("file").unwrap()
        ));
    }

    #[test]
    fn panicking_worker_settles_and_cleans_up_without_waiting_for_deadline() {
        let runtime = FakeLineHighlightRuntime::new(|_, _, _| panic!("fixture worker panic"));
        let extensions = runtime_list(&runtime);
        let registrations = [registration("panic")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let files = [test_file("file", "content")];
        let mut controller = LineHighlightPreparationController::default();
        controller.reconcile(&extensions, &registrations, &epochs, &files);
        // Do not poll expiry: the worker must send its own terminal outcome.
        let completion = controller
            .receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(completion.cancellation.is_cancelled());
        assert!(matches!(
            completion.outcome,
            LineHighlightTaskOutcome::Failed(_)
        ));
        controller.sender.send(completion).unwrap();
        controller.reconcile(&extensions, &registrations, &epochs, &files);
        assert_eq!(controller.pending_count(), 0);
        assert!(controller.resolved().is_empty());
        assert_eq!(runtime.warnings().len(), 1);
        assert_eq!(runtime.calls().len(), 1);
    }

    #[test]
    fn worker_completion_cleans_up_signal_before_coordinator_polling() {
        let runtime = FakeLineHighlightRuntime::new(|_, _, _| Ok(one_mark("match")));
        let extensions = runtime_list(&runtime);
        let registrations = [registration("cleanup")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let files = [test_file("file", "content")];
        let mut controller = LineHighlightPreparationController::default();
        controller.reconcile(&extensions, &registrations, &epochs, &files);
        let completion = controller
            .receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(completion.cancellation.is_cancelled());
        assert!(controller.cache.is_empty());
        controller.sender.send(completion).unwrap();
        controller.reconcile(&extensions, &registrations, &epochs, &files);
        assert_eq!(controller.resolved().len(), 1);
        assert_eq!(controller.pending_count(), 0);
        assert!(runtime.warnings().is_empty());
    }

    #[test]
    fn identical_document_reload_rederives_extension_marks() {
        let runtime = FakeLineHighlightRuntime::new(|_, _, _| Ok(one_mark("match")));
        let extensions = runtime_list(&runtime);
        let registrations = [registration("reload")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let document = reloaded_document(&[("file", Some("content"))], "test");
        let mut app = crate::ReviewApp::new(document.clone(), crate::ReviewOptions::default());
        {
            let mut owner = app.extension_pane_runtime.lock().unwrap();
            reconcile_until(
                &mut owner.line_highlight_preparation,
                &extensions,
                &registrations,
                &epochs,
                &document.files,
                |controller| controller.resolved().len() == 1,
            );
        }
        assert_eq!(runtime.calls().len(), 1);
        app.reload(document.clone());
        {
            let mut owner = app.extension_pane_runtime.lock().unwrap();
            assert!(owner.line_highlight_preparation.resolved().is_empty());
            reconcile_until(
                &mut owner.line_highlight_preparation,
                &extensions,
                &registrations,
                &epochs,
                &document.files,
                |controller| controller.resolved().len() == 1,
            );
        }
        assert_eq!(runtime.calls().len(), 2);
    }

    #[test]
    fn review_filter_limits_preparation_and_retires_hidden_marks() {
        let runtime = FakeLineHighlightRuntime::new(|_, _, _| Ok(one_mark("match")));
        let extensions = runtime_list(&runtime);
        let registrations = [registration("filter")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let files = [test_file("alpha", "a"), test_file("beta", "b")];
        let mut controller = LineHighlightPreparationController::default();
        for (filter, expected) in [("alpha", "alpha"), ("beta", "beta"), ("alpha", "alpha")] {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                controller.reconcile(
                    &extensions,
                    &registrations,
                    &epochs,
                    files
                        .iter()
                        .filter(|file| crate::diff_file_matches_filter(file, filter)),
                );
                assert!(
                    controller
                        .resolved()
                        .get(if expected == "alpha" { "beta" } else { "alpha" })
                        .is_none()
                );
                if controller.pending_count() == 0 && controller.resolved().get(expected).is_some()
                {
                    break;
                }
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(1));
            }
        }
        assert_eq!(
            runtime
                .calls()
                .iter()
                .map(|(_, file)| file.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "beta", "alpha"]
        );
        controller.reconcile(
            &extensions,
            &registrations,
            &epochs,
            files
                .iter()
                .filter(|file| crate::diff_file_matches_filter(file, "no-match")),
        );
        assert!(controller.resolved().is_empty());
        assert!(controller.cache.is_empty());
        assert_eq!(controller.pending_count(), 0);
    }

    #[test]
    fn changing_another_file_restarts_unfinished_generation_work() {
        assert_file_change_restarts_generation(test_file("added", "new-content"), 2, false);
    }

    #[test]
    fn ineligible_files_still_supersede_preparation_without_invoking_extensions() {
        for kind in ["binary", "too-large", "empty"] {
            let mut file = test_file("added", "new-content");
            match kind {
                "binary" => file.flags.binary = true,
                "too-large" => file.flags.too_large = true,
                "empty" => file.hunks.clear(),
                _ => unreachable!(),
            }
            for replace in [false, true] {
                assert_file_change_restarts_generation(file.clone(), 1, replace);
            }
        }
    }

    fn assert_file_change_restarts_generation(
        added: DiffFile,
        expected_files: usize,
        replace: bool,
    ) {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let requests = AtomicUsize::new(0);
        let runtime = FakeLineHighlightRuntime::new(move |_, file, cancelled| {
            if file.runtime_id == "unchanged" && requests.fetch_add(1, Ordering::AcqRel) == 0 {
                started_tx.send(()).unwrap();
                release_rx
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
                finished_tx.send(cancelled.is_cancelled()).unwrap();
            }
            Ok(one_mark("match"))
        });
        let extensions = runtime_list(&runtime);
        let registrations = [registration("highlight")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let mut files = vec![test_file("unchanged", "content")];
        if replace {
            files.push(added.clone());
        }
        let mut controller = LineHighlightPreparationController::default();
        controller.reconcile(&extensions, &registrations, &epochs, &files);
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let original = controller.pending.values().next().unwrap().clone();
        controller.reconcile(&extensions, &registrations, &epochs, &files);
        assert!(!original.is_cancelled());
        assert!(Arc::ptr_eq(
            controller.pending.values().next().unwrap(),
            &original
        ));
        if replace {
            files[1].content_identity = "replacement".into();
        } else {
            files.push(added);
        }
        controller.reconcile(&extensions, &registrations, &epochs, &files);
        release_tx.send(()).unwrap();
        assert!(finished_rx.recv_timeout(Duration::from_secs(5)).unwrap());
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &epochs,
            &files,
            |controller| {
                controller.pending_count() == 0 && controller.resolved().len() == expected_files
            },
        );
        assert_eq!(
            runtime
                .calls()
                .iter()
                .filter(|(_, file)| file == "unchanged")
                .count(),
            2
        );
        assert!(runtime.warnings().is_empty());
        assert_eq!(runtime.calls().len(), expected_files + 1);
    }

    #[test]
    fn registry_retirement_cancels_preparation_before_owner_drop() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let runtime = FakeLineHighlightRuntime::new(move |_, _, cancelled| {
            started_tx.send(()).unwrap();
            release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            finished_tx.send(cancelled.is_cancelled()).unwrap();
            Ok(one_mark("match"))
        });
        let extensions = runtime_list(&runtime);
        let registrations = [registration("running")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let files = [test_file("file", "content")];
        let mut owner = crate::ExtensionPaneRuntime::default();
        owner
            .line_highlight_preparation
            .reconcile(&extensions, &registrations, &epochs, &files);
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        owner.retire_extensions();
        release_tx.send(()).unwrap();
        assert!(finished_rx.recv_timeout(Duration::from_secs(5)).unwrap());
        owner
            .line_highlight_preparation
            .reconcile(&extensions, &registrations, &epochs, &files);
        assert_eq!(owner.line_highlight_preparation.pending_count(), 0);
        assert!(owner.line_highlight_preparation.resolved().is_empty());
        assert_eq!(runtime.calls().len(), 1);
    }

    #[test]
    fn preparation_runs_four_files_but_orders_highlighters_within_each_file() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let runtime = FakeLineHighlightRuntime::new(move |id, file, _| {
            started_tx
                .send((id.to_owned(), file.runtime_id.clone()))
                .unwrap();
            if id == "first" {
                release_rx
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
            }
            Ok(one_mark("match"))
        });
        let extensions = runtime_list(&runtime);
        let registrations = [registration("first"), registration("second")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let files = (0..5)
            .map(|index| test_file(&format!("file-{index}"), "content"))
            .collect::<Vec<_>>();
        let mut controller = LineHighlightPreparationController::default();
        controller.reconcile(&extensions, &registrations, &epochs, &files);
        let initial = (0..4)
            .map(|_| started_rx.recv_timeout(Duration::from_secs(5)).unwrap())
            .collect::<Vec<_>>();
        for _ in &files {
            release_tx.send(()).unwrap();
        }
        assert!(initial.iter().all(|(id, _)| id == "first"), "{initial:?}");
        assert_eq!(
            initial
                .iter()
                .map(|(_, file)| file)
                .collect::<BTreeSet<_>>()
                .len(),
            4
        );
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &epochs,
            &files,
            |controller| controller.resolved().len() == 5 && controller.pending_count() == 0,
        );
        let calls = runtime.calls();
        for file in &files {
            assert_eq!(
                calls
                    .iter()
                    .filter(|(_, id)| id == &file.runtime_id)
                    .map(|(id, _)| id.as_str())
                    .collect::<Vec<_>>(),
                vec!["first", "second"]
            );
        }
    }

    #[test]
    fn busy_provider_wait_has_a_deadline_without_starting_a_worker() {
        let runtime = FakeLineHighlightRuntime::new(|_, _, _| Ok(one_mark("match")));
        runtime.pending.store(true, Ordering::Release);
        let extensions = runtime_list(&runtime);
        let registrations = [registration("blocked")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let files = [test_file("file", "content")];
        let mut controller = LineHighlightPreparationController::default();
        controller.reconcile(&extensions, &registrations, &epochs, &files);
        assert_eq!(controller.attempt_deadlines.len(), 1);
        assert_eq!(controller.pending_count(), 0);
        assert!(
            controller.deadlines.is_empty(),
            "queued work must not freeze a file snapshot"
        );
        *controller.attempt_deadlines.values_mut().next().unwrap() = Instant::now();
        controller.reconcile(&extensions, &registrations, &epochs, &files);
        assert!(controller.cache.values().next().unwrap().is_none());
        assert!(controller.attempt_deadlines.is_empty());
        assert_eq!(runtime.warnings().len(), 1);
        runtime.pending.store(false, Ordering::Release);
        controller.reconcile(&extensions, &registrations, &epochs, &files);
        assert!(
            runtime.calls().is_empty(),
            "a timed-out attempt must not start later"
        );
        controller.replace_document();
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &epochs,
            &files,
            |controller| !controller.resolved().is_empty(),
        );
        assert_eq!(runtime.calls().len(), 1);
    }

    #[test]
    fn retry_reuses_original_attempt_deadline_and_cannot_extend_it() {
        let runtime =
            FakeLineHighlightRuntime::new(|_, _, _| Err(LineHighlightRuntimeError::Retry));
        let extensions = runtime_list(&runtime);
        let registrations = [registration("retry")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let files = [test_file("file", "content")];
        let mut controller = LineHighlightPreparationController::default();
        controller.reconcile(&extensions, &registrations, &epochs, &files);
        let original = *controller.attempt_deadlines.values().next().unwrap();
        let completion = controller
            .receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        controller.sender.send(completion).unwrap();
        controller.reconcile(&extensions, &registrations, &epochs, &files);
        assert_eq!(
            *controller.attempt_deadlines.values().next().unwrap(),
            original
        );
        assert_eq!(controller.deadlines.values().next().unwrap().0, original);
        let completion = controller
            .receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        controller.sender.send(completion).unwrap();
        *controller.attempt_deadlines.values_mut().next().unwrap() = Instant::now();
        controller.reconcile(&extensions, &registrations, &epochs, &files);
        assert_eq!(controller.pending_count(), 0);
        assert!(controller.cache.values().next().unwrap().is_none());
        assert_eq!(runtime.calls().len(), 2);
        assert_eq!(runtime.warnings().len(), 1);
    }

    #[test]
    fn busy_later_provider_retains_four_file_preparation_slots() {
        let first = FakeLineHighlightRuntime::new(|_, _, _| Ok(one_mark("match")));
        let second = FakeLineHighlightRuntime::new(|_, _, _| Ok(one_mark("match")));
        second.pending.store(true, Ordering::Release);
        let extensions: Vec<Arc<dyn LineHighlightRuntime>> = vec![first.clone(), second.clone()];
        let mut later = registration("second");
        later.extension_index = 1;
        let registrations = [registration("first"), later];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let files = (0..5)
            .map(|index| test_file(&format!("file-{index}"), "content"))
            .collect::<Vec<_>>();
        let mut controller = LineHighlightPreparationController::default();
        // Four file workers have finished their first provider, but not their second.
        for task in desired_line_highlight_tasks(&extensions, &registrations, &epochs, &files) {
            if task.highlighter_id == "first" && task.key.file_id != "file-4" {
                controller.cache.insert(task.key, None);
            }
        }
        controller.reconcile(&extensions, &registrations, &epochs, &files);
        assert_eq!(
            controller.pending_count(),
            0,
            "fifth file started while four file workers remained occupied"
        );
        assert!(first.calls().is_empty());
        second.pending.store(false, Ordering::Release);
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &epochs,
            &files,
            |controller| controller.resolved().len() == 5 && controller.pending_count() == 0,
        );
        assert_eq!(first.calls(), [("first".into(), "file-4".into())]);
        assert_eq!(second.calls().len(), 5);
    }

    #[test]
    fn busy_first_registration_cannot_be_overtaken_by_a_later_extension() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let first_calls = Arc::clone(&calls);
        let first = FakeLineHighlightRuntime::new(move |_, _, _| {
            first_calls.lock().unwrap().push("first");
            Ok(one_mark("match"))
        });
        let second_calls = Arc::clone(&calls);
        let second = FakeLineHighlightRuntime::new(move |_, _, _| {
            second_calls.lock().unwrap().push("second");
            Ok(one_mark("match"))
        });
        let extensions: Vec<Arc<dyn LineHighlightRuntime>> = vec![first.clone(), second];
        let mut second_registration = registration("second");
        second_registration.extension_index = 1;
        let registrations = [registration("first"), second_registration];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let files = [test_file("file", "content")];
        let mut controller = LineHighlightPreparationController::default();
        first.pending.store(true, Ordering::Release);
        controller.reconcile(&extensions, &registrations, &epochs, &files);
        assert_eq!(controller.pending_count(), 0);
        assert!(calls.lock().unwrap().is_empty());
        first.pending.store(false, Ordering::Release);
        reconcile_until(
            &mut controller,
            &extensions,
            &registrations,
            &epochs,
            &files,
            |controller| controller.resolved().len() == 1 && controller.pending_count() == 0,
        );
        assert_eq!(*calls.lock().unwrap(), vec!["first", "second"]);
    }

    #[test]
    fn retired_highlight_work_releases_current_generation_slots() {
        let runtime = FakeLineHighlightRuntime::new(|_, _, _| Ok(one_mark("match")));
        let extensions = runtime_list(&runtime);
        let registrations = [registration("running")];
        let epochs = workdeck_extension_host::LineHighlightEpochState::default();
        let files = (0..4)
            .map(|index| test_file(&format!("file-{index}"), "content"))
            .collect::<Vec<_>>();
        let tasks = desired_line_highlight_tasks(&extensions, &registrations, &epochs, &files);
        let mut controller = LineHighlightPreparationController::default();
        let cancellations = tasks
            .into_iter()
            .map(|task| {
                let cancellation = Arc::new(ExtensionRequestCancellation::default());
                controller
                    .pending
                    .insert(task.key, Arc::clone(&cancellation));
                cancellation
            })
            .collect::<Vec<_>>();
        controller.reconcile(&extensions, &registrations, &epochs, &[]);
        assert!(
            cancellations
                .iter()
                .all(|cancelled| cancelled.is_cancelled())
        );
        assert_eq!(controller.pending_count(), 0);
    }

    #[test]
    fn late_completion_cannot_settle_a_reused_key_owned_by_a_new_request() {
        let runtime = FakeLineHighlightRuntime::new(|_, _, _| Ok(one_mark("match")));
        let extensions = runtime_list(&runtime);
        let files = [test_file("file", "content")];
        let mut tasks = desired_line_highlight_tasks(
            &extensions,
            &[registration("running")],
            &workdeck_extension_host::LineHighlightEpochState::default(),
            &files,
        );
        let task = tasks.remove(0).freeze_for_worker();
        let desired = BTreeSet::from([task.key.clone()]);
        let old = Arc::new(ExtensionRequestCancellation::default());
        old.cancel();
        let current = Arc::new(ExtensionRequestCancellation::default());
        let mut controller = LineHighlightPreparationController::default();
        controller
            .pending
            .insert(task.key.clone(), Arc::clone(&current));
        for outcome in [
            LineHighlightTaskOutcome::Value(one_mark("match")),
            LineHighlightTaskOutcome::Failed("retired failure".into()),
            LineHighlightTaskOutcome::Retry,
        ] {
            controller
                .sender
                .send(LineHighlightCompletion {
                    task: task.clone(),
                    outcome,
                    cancellation: Arc::clone(&old),
                    completed_at: Instant::now(),
                    cancelled_before_completion: true,
                })
                .unwrap();
        }
        controller.poll_completions(&desired, &extensions);
        assert!(Arc::ptr_eq(
            controller.pending.get(&task.key).unwrap(),
            &current
        ));
        assert!(controller.cache.is_empty());
        assert!(runtime.warnings().is_empty());
        controller
            .sender
            .send(LineHighlightCompletion {
                task: task.clone(),
                outcome: LineHighlightTaskOutcome::Value(one_mark("match")),
                cancellation: current,
                completed_at: Instant::now(),
                cancelled_before_completion: false,
            })
            .unwrap();
        controller.poll_completions(&desired, &extensions);
        assert!(controller.pending.is_empty());
        assert!(controller.cache.get(&task.key).unwrap().is_some());
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
        assert_eq!(
            runtime.warnings()[0],
            "Extension test-extension line highlighter \"failing\" failed highlighting request.rs • marks dropped"
        );
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
                    while !cancelled.is_cancelled() {
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
