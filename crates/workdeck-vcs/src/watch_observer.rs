//! Native filesystem observation for hybrid watch plans.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use thiserror::Error;

use crate::{VcsWatchCoverage, VcsWatchPlan, VcsWatchTarget, WatchPlatform};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct WatchSourceError {
    pub code: Option<String>,
    pub message: String,
}

impl WatchSourceError {
    #[must_use]
    pub fn new(code: Option<impl Into<String>>, message: impl Into<String>) -> Self {
        Self {
            code: code.map(Into::into),
            message: message.into(),
        }
    }

    fn from_notify(error: notify::Error) -> Self {
        let message = error.to_string();
        let code = if message.contains("ENOSPC") || message.contains("No space left on device") {
            Some("ENOSPC".into())
        } else if message.contains("EMFILE") || message.contains("Too many open files") {
            Some("EMFILE".into())
        } else {
            None
        };
        Self { code, message }
    }
}

#[derive(Clone)]
pub struct WatchObserverCallbacks {
    pub on_event: Arc<dyn Fn() + Send + Sync>,
    pub on_error: Arc<dyn Fn(WatchSourceError) + Send + Sync>,
    pub on_ready: Option<Arc<dyn Fn() + Send + Sync>>,
}

#[derive(Clone)]
pub struct WatchRegistrationCallbacks {
    pub on_event: Arc<dyn Fn() + Send + Sync>,
    pub on_error: Arc<dyn Fn(WatchSourceError) + Send + Sync>,
    pub on_ready: Arc<dyn Fn() + Send + Sync>,
}

pub trait WatchRegistration: Send {
    fn close(&mut self) -> Result<(), WatchSourceError>;
}

pub trait WatchBackend: Send + Sync {
    fn register(
        &self,
        target: &VcsWatchTarget,
        callbacks: WatchRegistrationCallbacks,
    ) -> Result<Box<dyn WatchRegistration>, WatchSourceError>;
}

#[derive(Clone)]
pub struct WatchObserverOptions {
    pub platform: WatchPlatform,
    pub native_backend: Arc<dyn WatchBackend>,
    pub portable_backend: Arc<dyn WatchBackend>,
}

impl Default for WatchObserverOptions {
    fn default() -> Self {
        let backend: Arc<dyn WatchBackend> = Arc::new(NotifyWatchBackend);
        Self {
            platform: WatchPlatform::current(),
            native_backend: Arc::clone(&backend),
            portable_backend: backend,
        }
    }
}

struct NotifyWatchBackend;

struct NotifyRegistration {
    watcher: Option<RecommendedWatcher>,
}

impl WatchRegistration for NotifyRegistration {
    fn close(&mut self) -> Result<(), WatchSourceError> {
        self.watcher.take();
        Ok(())
    }
}

impl WatchBackend for NotifyWatchBackend {
    fn register(
        &self,
        target: &VcsWatchTarget,
        callbacks: WatchRegistrationCallbacks,
    ) -> Result<Box<dyn WatchRegistration>, WatchSourceError> {
        let filter = EventFilter::new(target);
        let event_filter = filter.clone();
        let on_event = Arc::clone(&callbacks.on_event);
        let on_error = Arc::clone(&callbacks.on_error);
        let mut watcher = notify::recommended_watcher(
            move |result: notify::Result<notify::Event>| match result {
                Ok(event) if event_filter.matches(&event.paths) => on_event(),
                Ok(_) => {}
                Err(error) => on_error(WatchSourceError::from_notify(error)),
            },
        )
        .map_err(WatchSourceError::from_notify)?;
        watcher
            .watch(filter.directory(), filter.recursive_mode())
            .map_err(WatchSourceError::from_notify)?;
        (callbacks.on_ready)();
        Ok(Box::new(NotifyRegistration {
            watcher: Some(watcher),
        }))
    }
}

#[derive(Debug, Clone)]
enum EventFilter {
    Entries {
        directory: PathBuf,
        entries: Vec<PathBuf>,
    },
    Tree {
        directory: PathBuf,
        ignored_roots: Vec<PathBuf>,
    },
}

impl EventFilter {
    fn new(target: &VcsWatchTarget) -> Self {
        match target {
            VcsWatchTarget::DirectoryEntries {
                directory, entries, ..
            } => Self::Entries {
                directory: absolute_path(directory),
                entries: entries.iter().map(Path::new).map(absolute_path).collect(),
            },
            VcsWatchTarget::DirectoryTree {
                directory,
                ignored_roots,
                ..
            } => Self::Tree {
                directory: absolute_path(directory),
                ignored_roots: ignored_roots
                    .iter()
                    .map(|path| absolute_path(path))
                    .collect(),
            },
        }
    }

    fn directory(&self) -> &Path {
        match self {
            Self::Entries { directory, .. } | Self::Tree { directory, .. } => directory,
        }
    }

    const fn recursive_mode(&self) -> RecursiveMode {
        match self {
            Self::Entries { .. } => RecursiveMode::NonRecursive,
            Self::Tree { .. } => RecursiveMode::Recursive,
        }
    }

    fn matches(&self, paths: &[PathBuf]) -> bool {
        if paths.is_empty() {
            return true;
        }
        match self {
            Self::Entries { entries, .. } => {
                paths.iter().map(|path| absolute_path(path)).any(|path| {
                    entries
                        .iter()
                        .any(|entry| paths_equal(entry, &path, WatchPlatform::current()))
                })
            }
            Self::Tree { ignored_roots, .. } => {
                paths.iter().map(|path| absolute_path(path)).any(|path| {
                    !ignored_roots
                        .iter()
                        .any(|root| path_is_within(&path, root, WatchPlatform::current()))
                })
            }
        }
    }
}

fn absolute_path(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_owned())
    };
    if let Ok(canonical) = absolute.canonicalize() {
        return canonical;
    }
    absolute
        .parent()
        .and_then(|parent| parent.canonicalize().ok())
        .zip(absolute.file_name())
        .map_or(absolute.clone(), |(parent, name)| parent.join(name))
}

fn comparable_path(path: &Path, platform: WatchPlatform) -> String {
    let value = path.to_string_lossy().replace('\\', "/");
    if platform == WatchPlatform::Windows {
        value.to_ascii_lowercase()
    } else {
        value
    }
}

fn paths_equal(left: &Path, right: &Path, platform: WatchPlatform) -> bool {
    comparable_path(left, platform) == comparable_path(right, platform)
}

fn path_is_within(path: &Path, root: &Path, platform: WatchPlatform) -> bool {
    let path = comparable_path(path, platform);
    let root = comparable_path(root, platform);
    path == root
        || path
            .strip_prefix(&root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

#[derive(Default)]
struct LifecycleState {
    ready: bool,
    closed: bool,
    constructing: bool,
    pending_ready_callback: bool,
}

struct WatchObserverInner {
    closed: AtomicBool,
    remaining_ready: AtomicUsize,
    lifecycle: (Mutex<LifecycleState>, Condvar),
    registrations: Mutex<Option<Vec<Box<dyn WatchRegistration>>>>,
    callbacks: WatchObserverCallbacks,
}

impl WatchObserverInner {
    fn mark_ready(&self) {
        if self.closed.load(Ordering::Acquire)
            || self.remaining_ready.fetch_sub(1, Ordering::AcqRel) != 1
        {
            return;
        }
        let (state, condition) = &self.lifecycle;
        let mut state = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.ready = true;
        state.pending_ready_callback = state.constructing;
        condition.notify_all();
        if !state.constructing {
            drop(state);
            if let Some(callback) = &self.callbacks.on_ready {
                callback();
            }
        }
    }

    fn finish_construction(&self) {
        let (state, condition) = &self.lifecycle;
        let mut state = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.constructing = false;
        if self.remaining_ready.load(Ordering::Acquire) == 0 {
            state.ready = true;
            condition.notify_all();
        }
        let notify = std::mem::take(&mut state.pending_ready_callback);
        drop(state);
        if notify && let Some(callback) = &self.callbacks.on_ready {
            callback();
        }
    }

    fn close(&self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Some(mut registrations) = self
            .registrations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            for registration in &mut registrations {
                let _ = registration.close();
            }
        }
        let (state, condition) = &self.lifecycle;
        let mut state = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.ready = true;
        state.closed = true;
        condition.notify_all();
    }
}

pub struct WatchObserver {
    inner: Arc<WatchObserverInner>,
}

impl WatchObserver {
    pub fn close(&self) {
        self.inner.close();
    }

    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.inner
            .lifecycle
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .ready
    }

    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::Acquire)
    }

    pub fn wait_ready(&self, timeout: std::time::Duration) -> bool {
        wait_lifecycle(&self.inner.lifecycle, timeout, |state| state.ready)
    }

    pub fn wait_closed(&self, timeout: std::time::Duration) -> bool {
        wait_lifecycle(&self.inner.lifecycle, timeout, |state| state.closed)
    }
}

impl Drop for WatchObserver {
    fn drop(&mut self) {
        self.inner.close();
    }
}

fn wait_lifecycle(
    lifecycle: &(Mutex<LifecycleState>, Condvar),
    timeout: std::time::Duration,
    predicate: impl Fn(&LifecycleState) -> bool,
) -> bool {
    let (state, condition) = lifecycle;
    let state = state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let (state, _) = condition
        .wait_timeout_while(state, timeout, |state| !predicate(state))
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    predicate(&state)
}

pub fn create_watch_observer(
    plan: &VcsWatchPlan,
    callbacks: WatchObserverCallbacks,
    options: WatchObserverOptions,
) -> Result<WatchObserver, WatchSourceError> {
    let inner = Arc::new(WatchObserverInner {
        closed: AtomicBool::new(false),
        remaining_ready: AtomicUsize::new(plan.targets.len()),
        lifecycle: (
            Mutex::new(LifecycleState {
                constructing: true,
                ..LifecycleState::default()
            }),
            Condvar::new(),
        ),
        registrations: Mutex::new(Some(Vec::new())),
        callbacks,
    });

    for target in &plan.targets {
        let backend = match target {
            VcsWatchTarget::DirectoryEntries { .. } => &options.portable_backend,
            VcsWatchTarget::DirectoryTree { .. }
                if matches!(
                    options.platform,
                    WatchPlatform::MacOs | WatchPlatform::Windows
                ) =>
            {
                &options.native_backend
            }
            VcsWatchTarget::DirectoryTree { .. } => &options.portable_backend,
        };
        let event_inner = Arc::clone(&inner);
        let error_inner = Arc::clone(&inner);
        let ready_inner = Arc::clone(&inner);
        let registration = backend.register(
            target,
            WatchRegistrationCallbacks {
                on_event: Arc::new(move || {
                    if !event_inner.closed.load(Ordering::Acquire) {
                        (event_inner.callbacks.on_event)();
                    }
                }),
                on_error: Arc::new(move |error| {
                    if !error_inner.closed.load(Ordering::Acquire) {
                        (error_inner.callbacks.on_error)(error);
                    }
                }),
                on_ready: Arc::new(move || ready_inner.mark_ready()),
            },
        );
        match registration {
            Ok(registration) => inner
                .registrations
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_mut()
                .expect("observer registrations are open during construction")
                .push(registration),
            Err(error) => {
                inner.close();
                return Err(error);
            }
        }
    }
    inner.finish_construction();
    Ok(WatchObserver { inner })
}

#[derive(Clone)]
pub struct WatchEventSourceFactory {
    plan: VcsWatchPlan,
    options: WatchObserverOptions,
}

impl WatchEventSourceFactory {
    pub fn create(
        &self,
        callbacks: WatchObserverCallbacks,
    ) -> Result<WatchObserver, WatchSourceError> {
        create_watch_observer(&self.plan, callbacks, self.options.clone())
    }
}

#[must_use]
pub fn create_watch_event_source(plan: &VcsWatchPlan) -> Option<WatchEventSourceFactory> {
    (plan.coverage != VcsWatchCoverage::PollOnly).then(|| WatchEventSourceFactory {
        plan: plan.clone(),
        options: WatchObserverOptions::default(),
    })
}

#[cfg(test)]
mod tests;
