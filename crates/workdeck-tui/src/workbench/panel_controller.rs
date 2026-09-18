use super::panels::*;
use std::{
    collections::BTreeMap,
    sync::{Arc, mpsc},
    thread,
};

#[derive(Debug, Clone, Default)]
pub(super) struct PanelLocation {
    pub selected: Option<String>,
    pub list_offset: usize,
    pub preview_scroll: u16,
}

#[derive(Debug)]
pub(super) struct PanelPageState {
    pub snapshot: Option<PanelSnapshot>,
    pub location: PanelLocation,
    pub directory: String,
    pub query: String,
    pub query_editing: bool,
    pub query_cursor: usize,
    pub preview_visible: bool,
    pub grouped: bool,
    pub dirstat: bool,
    pub loading: bool,
    pub error: Option<PanelError>,
    pub preview: Option<(PanelTarget, PanelPreview)>,
    pub preview_loading: bool,
    pub preview_error: Option<PanelError>,
    load_generation: u64,
    preview_generation: u64,
    directories: BTreeMap<String, PanelLocation>,
}

impl Default for PanelPageState {
    fn default() -> Self {
        Self {
            snapshot: None,
            location: PanelLocation::default(),
            directory: String::new(),
            query: String::new(),
            query_editing: false,
            query_cursor: 0,
            preview_visible: false,
            grouped: true,
            dirstat: true,
            loading: false,
            error: None,
            preview: None,
            preview_loading: false,
            preview_error: None,
            load_generation: 0,
            preview_generation: 0,
            directories: BTreeMap::new(),
        }
    }
}

impl PanelPageState {
    pub fn selected(&self) -> Option<&PanelEntry> {
        self.snapshot
            .as_ref()?
            .entries
            .iter()
            .find(|entry| Some(&entry.id) == self.location.selected.as_ref())
    }
    pub fn selected_index(&self) -> Option<usize> {
        self.snapshot
            .as_ref()?
            .entries
            .iter()
            .position(|entry| Some(&entry.id) == self.location.selected.as_ref())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum JobKey {
    Load(PanelPage),
    Preview(PanelPage),
}

#[derive(Debug, Clone)]
enum JobInput {
    Load(PanelRequest),
    Preview(PanelTarget),
}

#[derive(Debug, Clone)]
struct PanelJob {
    key: JobKey,
    generation: u64,
    source: RepositoryPanelSource,
    input: JobInput,
}

#[derive(Debug)]
enum PanelResponse {
    Snapshot(PanelSnapshot),
    Preview(PanelPreview),
}

#[derive(Debug)]
struct Completion {
    job: PanelJob,
    result: Result<PanelResponse, PanelError>,
}

/// One repository-bound read worker. It owns no UI or terminal state. Pending
/// requests coalesce, and publication checks both source and request generation.
#[derive(Debug)]
pub(super) struct RepositoryPanels {
    pub source: RepositoryPanelSource,
    /// Retained composition-root provider; reads run on the worker thread,
    /// session-local actions such as base cycling run on the interface thread.
    pub(super) provider: Arc<dyn RepositoryPanelProvider>,
    pub pages: BTreeMap<PanelPage, PanelPageState>,
    sender: mpsc::Sender<PanelJob>,
    receiver: mpsc::Receiver<Completion>,
    pending: BTreeMap<JobKey, PanelJob>,
    in_flight: bool,
    sequence: u64,
}

impl RepositoryPanels {
    pub fn new(provider: Arc<dyn RepositoryPanelProvider>) -> Self {
        let source = provider.source();
        let (sender, jobs) = mpsc::channel::<PanelJob>();
        let (completed, receiver) = mpsc::channel();
        // Dropping this controller closes the input channel. A read already in
        // progress may finish, but cannot publish into a retired application.
        let worker = Arc::clone(&provider);
        thread::spawn(move || {
            while let Ok(job) = jobs.recv() {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if worker.source() != job.source {
                        return Err(PanelError::new(
                            "Repository panel source changed before reading",
                        ));
                    }
                    let result = match &job.input {
                        JobInput::Load(request) => {
                            worker.load(request).map(PanelResponse::Snapshot)
                        }
                        JobInput::Preview(target) => {
                            worker.preview(target).map(PanelResponse::Preview)
                        }
                    };
                    if worker.source() != job.source {
                        return Err(PanelError::new(
                            "Repository panel source changed while reading",
                        ));
                    }
                    result
                }))
                .unwrap_or_else(|_| Err(PanelError::new("Repository panel provider failed")));
                if completed.send(Completion { job, result }).is_err() {
                    break;
                }
            }
        });
        Self {
            source,
            provider,
            pages: BTreeMap::new(),
            sender,
            receiver,
            pending: BTreeMap::new(),
            in_flight: false,
            sequence: 0,
        }
    }

    pub fn state(&mut self, page: PanelPage) -> &mut PanelPageState {
        self.pages.entry(page).or_default()
    }

    pub fn open(&mut self, page: PanelPage) {
        if self.state(page).snapshot.is_none()
            && !self.state(page).loading
            && self.state(page).error.is_none()
        {
            self.refresh(page);
        }
    }

    pub fn refresh(&mut self, page: PanelPage) {
        self.sequence = self.sequence.saturating_add(1);
        let generation = self.sequence;
        self.pending.remove(&JobKey::Preview(page));
        let state = self.state(page);
        state.loading = true;
        state.error = None;
        state.load_generation = generation;
        state.preview_generation = generation;
        state.preview_loading = false;
        let input = JobInput::Load(PanelRequest {
            page,
            directory: state.directory.clone(),
            query: state.query.clone(),
            limit: 20_000,
        });
        self.pending.insert(
            JobKey::Load(page),
            PanelJob {
                key: JobKey::Load(page),
                generation,
                source: self.source.clone(),
                input,
            },
        );
        self.dispatch();
    }

    pub fn set_directory(&mut self, page: PanelPage, directory: String) {
        let state = self.state(page);
        state
            .directories
            .insert(state.directory.clone(), state.location.clone());
        state.directory = directory;
        state.location = state
            .directories
            .get(&state.directory)
            .cloned()
            .unwrap_or_default();
        state.preview_visible = false;
        self.refresh(page);
    }

    pub fn move_selection(&mut self, page: PanelPage, delta: isize) {
        let state = self.state(page);
        let count = state
            .snapshot
            .as_ref()
            .map_or(0, |snapshot| snapshot.entries.len());
        if count == 0 {
            return;
        }
        let index = state
            .selected_index()
            .unwrap_or(0)
            .saturating_add_signed(delta)
            .min(count - 1);
        state.location.selected = state
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.entries.get(index))
            .map(|entry| entry.id.clone());
        state.location.preview_scroll = 0;
        self.request_preview(page);
    }

    pub fn select(&mut self, page: PanelPage, id: &str) {
        let state = self.state(page);
        if state
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.entries.iter().any(|entry| entry.id == id))
        {
            state.location.selected = Some(id.into());
            state.location.preview_scroll = 0;
            self.request_preview(page);
        }
    }

    pub fn request_preview(&mut self, page: PanelPage) {
        let Some(target) = self
            .state(page)
            .selected()
            .map(|entry| entry.target.clone())
        else {
            return;
        };
        if matches!(target, PanelTarget::Directory { .. }) {
            self.state(page).preview = None;
            self.retire_preview_request(page);
            return;
        }
        if self
            .state(page)
            .preview
            .as_ref()
            .is_some_and(|(current, _)| current == &target)
        {
            self.retire_preview_request(page);
            return;
        }
        self.sequence = self.sequence.saturating_add(1);
        let generation = self.sequence;
        let state = self.state(page);
        state.preview_generation = generation;
        state.preview_loading = true;
        state.preview_error = None;
        let input = JobInput::Preview(target);
        self.pending.insert(
            JobKey::Preview(page),
            PanelJob {
                key: JobKey::Preview(page),
                generation,
                source: self.source.clone(),
                input,
            },
        );
        self.dispatch();
    }

    fn retire_preview_request(&mut self, page: PanelPage) {
        self.sequence = self.sequence.saturating_add(1);
        let generation = self.sequence;
        self.pending.remove(&JobKey::Preview(page));
        let state = self.state(page);
        state.preview_generation = generation;
        state.preview_loading = false;
        state.preview_error = None;
    }

    pub fn poll(&mut self) -> usize {
        let mut count = 0;
        while let Ok(completion) = self.receiver.try_recv() {
            self.in_flight = false;
            count += 1;
            let Completion { job, result } = completion;
            if job.source != self.source {
                continue;
            }
            match job.key {
                JobKey::Load(page) => {
                    let state = self.state(page);
                    if state.load_generation != job.generation {
                        continue;
                    }
                    state.loading = false;
                    match result {
                        Ok(PanelResponse::Snapshot(mut snapshot)) if snapshot.page == page => {
                            let mut identifiers = std::collections::BTreeSet::new();
                            if snapshot.entries.iter().any(|entry| {
                                entry.id.is_empty()
                                    || entry.id == "workdeck:parent-directory"
                                    || !identifiers.insert(entry.id.clone())
                            }) {
                                state.error = Some(PanelError::new(
                                    "Provider returned empty, reserved, or duplicate panel identifiers",
                                ));
                                continue;
                            }
                            if let JobInput::Load(request) = &job.input {
                                if snapshot.entries.len() > request.limit {
                                    snapshot.entries.truncate(request.limit);
                                    snapshot.truncated = true;
                                }
                                if page == PanelPage::Files && !request.directory.is_empty() {
                                    let parent = std::path::Path::new(&request.directory)
                                        .parent()
                                        .unwrap_or(std::path::Path::new(""))
                                        .to_string_lossy()
                                        .into_owned();
                                    snapshot.entries.insert(
                                        0,
                                        PanelEntry {
                                            id: "workdeck:parent-directory".into(),
                                            label: "..".into(),
                                            detail: "Parent directory".into(),
                                            section: String::new(),
                                            target: PanelTarget::Directory { path: parent },
                                            changes: None,
                                        },
                                    );
                                }
                            }
                            if !snapshot
                                .entries
                                .iter()
                                .any(|entry| Some(&entry.id) == state.location.selected.as_ref())
                            {
                                state.location.selected =
                                    snapshot.entries.first().map(|entry| entry.id.clone());
                                state.location.list_offset = 0;
                                state.location.preview_scroll = 0;
                            }
                            state.preview = None;
                            state.snapshot = Some(snapshot);
                            state.error = None;
                            self.request_preview(page);
                        }
                        Err(error) => state.error = Some(error),
                        _ => {
                            state.error = Some(PanelError::new(
                                "Provider returned the wrong panel response",
                            ))
                        }
                    }
                }
                JobKey::Preview(page) => {
                    let state = self.state(page);
                    if state.preview_generation != job.generation {
                        continue;
                    }
                    let JobInput::Preview(target) = job.input else {
                        continue;
                    };
                    if state.selected().is_none_or(|entry| entry.target != target) {
                        continue;
                    }
                    state.preview_loading = false;
                    match result {
                        Ok(PanelResponse::Preview(preview)) => {
                            state.preview = Some((target, preview));
                            state.preview_error = None;
                        }
                        Err(error) => state.preview_error = Some(error),
                        _ => {
                            state.preview_error = Some(PanelError::new(
                                "Provider returned the wrong preview response",
                            ))
                        }
                    }
                }
            }
        }
        self.dispatch();
        count
    }

    fn dispatch(&mut self) {
        if self.in_flight {
            return;
        }
        if let Some((_, job)) = self.pending.pop_first()
            && self.sender.send(job).is_ok()
        {
            self.in_flight = true;
        }
    }
}
