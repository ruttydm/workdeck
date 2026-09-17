use super::*;

use std::collections::BTreeSet;
use std::fs;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use notify::event::{CreateKind, MetadataKind, ModifyKind, RenameMode};
use notify::{Event, EventKind};
use tempfile::tempdir;

use crate::VcsWatchTargetSource;

#[derive(Debug, Clone, Copy)]
enum BackendBehavior {
    Ready,
    Stall,
    Fail,
}

struct RecordingBackend {
    name: &'static str,
    behavior: BackendBehavior,
    calls: Arc<Mutex<Vec<String>>>,
    callbacks: Arc<Mutex<Option<WatchRegistrationCallbacks>>>,
}

impl RecordingBackend {
    fn new(name: &'static str, behavior: BackendBehavior, calls: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            name,
            behavior,
            calls,
            callbacks: Arc::new(Mutex::new(None)),
        }
    }
}

struct RecordingRegistration {
    name: &'static str,
    calls: Arc<Mutex<Vec<String>>>,
}

impl WatchRegistration for RecordingRegistration {
    fn close(&mut self) -> Result<(), WatchSourceError> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("{}:close", self.name));
        Ok(())
    }
}

impl WatchBackend for RecordingBackend {
    fn register(
        &self,
        _target: &VcsWatchTarget,
        callbacks: WatchRegistrationCallbacks,
    ) -> Result<Box<dyn WatchRegistration>, WatchSourceError> {
        self.calls.lock().unwrap().push(self.name.into());
        *self.callbacks.lock().unwrap() = Some(callbacks.clone());
        match self.behavior {
            BackendBehavior::Ready => (callbacks.on_ready)(),
            BackendBehavior::Stall => {}
            BackendBehavior::Fail => {
                return Err(WatchSourceError::new(None::<String>, "construction failed"));
            }
        }
        Ok(Box::new(RecordingRegistration {
            name: self.name,
            calls: Arc::clone(&self.calls),
        }))
    }
}

fn tree_target(directory: impl Into<PathBuf>) -> VcsWatchTarget {
    let directory = directory.into();
    VcsWatchTarget::DirectoryTree {
        ignored_roots: vec![directory.join(".git"), directory.join("node_modules")],
        directory,
        sources: [VcsWatchTargetSource::Worktree].into_iter().collect(),
    }
}

fn tree_plan(directory: impl Into<PathBuf>) -> VcsWatchPlan {
    VcsWatchPlan {
        coverage: VcsWatchCoverage::Hybrid,
        targets: vec![tree_target(directory)],
    }
}

fn callbacks() -> WatchObserverCallbacks {
    WatchObserverCallbacks {
        on_event: Arc::new(|| {}),
        on_error: Arc::new(|_| {}),
        on_ready: None,
    }
}

fn recording_options(
    platform: WatchPlatform,
    native: Arc<RecordingBackend>,
    portable: Arc<RecordingBackend>,
) -> WatchObserverOptions {
    WatchObserverOptions {
        platform,
        native_backend: native,
        portable_backend: portable,
    }
}

#[test]
fn stalled_backend_keeps_the_observer_unready_until_close() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let stalled = Arc::new(RecordingBackend::new(
        "portable",
        BackendBehavior::Stall,
        Arc::clone(&calls),
    ));
    let observer = create_watch_observer(
        &tree_plan("/repo"),
        callbacks(),
        recording_options(WatchPlatform::Unix, stalled.clone(), stalled),
    )
    .unwrap();
    assert!(!observer.wait_ready(Duration::from_millis(5)));
    observer.close();
    assert!(observer.wait_ready(Duration::from_millis(5)));
    assert_eq!(
        calls.lock().unwrap().as_slice(),
        ["portable", "portable:close"]
    );
}

#[test]
fn macos_and_windows_select_native_recursion() {
    for platform in [WatchPlatform::MacOs, WatchPlatform::Windows] {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let native = Arc::new(RecordingBackend::new(
            "native",
            BackendBehavior::Ready,
            Arc::clone(&calls),
        ));
        let portable = Arc::new(RecordingBackend::new(
            "portable",
            BackendBehavior::Ready,
            Arc::clone(&calls),
        ));
        let observer = create_watch_observer(
            &tree_plan("/repo"),
            callbacks(),
            recording_options(platform, native, portable),
        )
        .unwrap();
        observer.close();
        assert_eq!(calls.lock().unwrap().as_slice(), ["native", "native:close"]);
    }
}

#[test]
fn unix_platforms_select_portable_pruned_recursion() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let native = Arc::new(RecordingBackend::new(
        "native",
        BackendBehavior::Ready,
        Arc::clone(&calls),
    ));
    let portable = Arc::new(RecordingBackend::new(
        "portable",
        BackendBehavior::Ready,
        Arc::clone(&calls),
    ));
    let observer = create_watch_observer(
        &tree_plan("/repo"),
        callbacks(),
        recording_options(WatchPlatform::Unix, native, portable),
    )
    .unwrap();
    observer.close();
    assert_eq!(
        calls.lock().unwrap().as_slice(),
        ["portable", "portable:close"]
    );
}

#[test]
fn native_construction_failure_does_not_fall_back_to_portable() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let native = Arc::new(RecordingBackend::new(
        "native",
        BackendBehavior::Fail,
        Arc::clone(&calls),
    ));
    let portable = Arc::new(RecordingBackend::new(
        "portable",
        BackendBehavior::Ready,
        Arc::clone(&calls),
    ));
    let result = create_watch_observer(
        &tree_plan("/repo"),
        callbacks(),
        recording_options(WatchPlatform::MacOs, native, portable),
    );
    assert!(result.is_err());
    assert_eq!(calls.lock().unwrap().as_slice(), ["native"]);
}

#[test]
fn readiness_is_visible_only_after_successful_registration() {
    let directory = tempdir().unwrap();
    let observer = create_watch_observer(
        &tree_plan(directory.path()),
        callbacks(),
        WatchObserverOptions::default(),
    )
    .unwrap();
    assert!(observer.wait_ready(Duration::from_secs(1)));
}

#[test]
fn recursive_filter_suppresses_paths_inside_ignored_roots() {
    let target = tree_target("/repo");
    let filter = EventFilter::new(&target);
    assert!(!filter.matches(&["/repo/node_modules/pkg/index.js".into()]));
    assert!(!filter.matches(&["/repo/.git/objects/pack/data".into()]));
    assert!(filter.matches(&["/repo/src/index.ts".into()]));
}

#[test]
fn recursive_filter_suppresses_watch_root_markers_but_keeps_root_children() {
    let target = tree_target("/repo");
    let filter = EventFilter::new(&target);
    assert!(!filter.matches_event(
        &Event::new(EventKind::Create(CreateKind::Folder)).add_path("/repo".into())
    ));
    assert!(
        !filter.matches_event(
            &Event::new(EventKind::Modify(ModifyKind::Metadata(
                MetadataKind::Extended
            )))
            .add_path("/repo".into())
        )
    );
    assert!(filter.matches_event(
        &Event::new(EventKind::Create(CreateKind::File)).add_path("/repo/README.md".into())
    ));
    assert!(filter.matches_event(
        &Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Any))).add_path("/repo".into())
    ));
}

#[test]
fn recursive_filter_conservatively_emits_for_missing_or_ambiguous_paths() {
    let target = tree_target("/repo");
    let filter = EventFilter::new(&target);
    assert!(filter.matches(&[]));
    assert!(filter.matches(&["index.js".into()]));
}

#[test]
fn backend_errors_are_forwarded_and_close_releases_the_handle_once() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let backend = Arc::new(RecordingBackend::new(
        "portable",
        BackendBehavior::Ready,
        Arc::clone(&calls),
    ));
    let captured = Arc::new(Mutex::new(Vec::new()));
    let errors = Arc::clone(&captured);
    let observer = create_watch_observer(
        &tree_plan("/repo"),
        WatchObserverCallbacks {
            on_event: Arc::new(|| {}),
            on_error: Arc::new(move |error| errors.lock().unwrap().push(error)),
            on_ready: None,
        },
        recording_options(WatchPlatform::Unix, backend.clone(), backend.clone()),
    )
    .unwrap();
    let callback = backend.callbacks.lock().unwrap().clone().unwrap();
    (callback.on_error)(WatchSourceError::new(Some("EIO"), "watch failed"));
    observer.close();
    observer.close();
    assert_eq!(captured.lock().unwrap()[0].code.as_deref(), Some("EIO"));
    assert_eq!(
        calls.lock().unwrap().as_slice(),
        ["portable", "portable:close"]
    );
}

fn entries_plan(directory: &Path, entries: &[PathBuf]) -> VcsWatchPlan {
    VcsWatchPlan {
        coverage: VcsWatchCoverage::Hybrid,
        targets: vec![VcsWatchTarget::DirectoryEntries {
            directory: directory.to_owned(),
            entries: entries
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
            sources: BTreeSet::from([VcsWatchTargetSource::Content]),
        }],
    }
}

fn start_real_observer(
    plan: &VcsWatchPlan,
) -> (WatchObserver, Receiver<()>, Receiver<WatchSourceError>) {
    let (event_tx, event_rx) = mpsc::channel();
    let (error_tx, error_rx) = mpsc::channel();
    let observer = create_watch_observer(
        plan,
        WatchObserverCallbacks {
            on_event: Arc::new(move || {
                let _ = event_tx.send(());
            }),
            on_error: Arc::new(move |error| {
                let _ = error_tx.send(error);
            }),
            on_ready: None,
        },
        WatchObserverOptions::default(),
    )
    .unwrap();
    assert!(observer.wait_ready(Duration::from_secs(2)));
    (observer, event_rx, error_rx)
}

fn next_event(receiver: &Receiver<()>) {
    receiver
        .recv_timeout(Duration::from_secs(3))
        .expect("filesystem event did not arrive");
}

fn drain(receiver: &Receiver<()>) {
    while receiver.try_recv().is_ok() {}
}

fn assert_no_event(receiver: &Receiver<()>) {
    assert!(receiver.recv_timeout(Duration::from_millis(250)).is_err());
}

#[test]
fn poll_only_plan_has_no_event_source_factory() {
    assert!(create_watch_event_source(&VcsWatchPlan::poll_only()).is_none());
}

#[test]
fn observes_an_ordinary_file_write_after_readiness() {
    let directory = tempdir().unwrap();
    let file = directory.path().join("input.patch");
    fs::write(&file, "before").unwrap();
    let (_observer, events, errors) =
        start_real_observer(&entries_plan(directory.path(), std::slice::from_ref(&file)));
    fs::write(file, "after").unwrap();
    next_event(&events);
    assert!(errors.try_recv().is_err());
}

#[test]
fn observes_temp_file_atomic_replacement() {
    let directory = tempdir().unwrap();
    let file = directory.path().join("input.patch");
    let temporary = directory.path().join(".input.patch.tmp");
    fs::write(&file, "before").unwrap();
    let (_observer, events, _) =
        start_real_observer(&entries_plan(directory.path(), std::slice::from_ref(&file)));
    fs::write(&temporary, "after").unwrap();
    fs::rename(temporary, file).unwrap();
    next_event(&events);
}

#[test]
fn observes_deletion_and_recreation() {
    let directory = tempdir().unwrap();
    let file = directory.path().join("input.patch");
    fs::write(&file, "before").unwrap();
    let (_observer, events, _) =
        start_real_observer(&entries_plan(directory.path(), std::slice::from_ref(&file)));
    fs::remove_file(&file).unwrap();
    next_event(&events);
    std::thread::sleep(Duration::from_millis(20));
    drain(&events);
    fs::write(file, "after").unwrap();
    next_event(&events);
}

#[test]
fn exact_entry_target_ignores_sibling_files() {
    let directory = tempdir().unwrap();
    let target = directory.path().join("target.patch");
    let sibling = directory.path().join("sibling.patch");
    fs::write(&target, "target").unwrap();
    fs::write(&sibling, "before").unwrap();
    let (_observer, events, _) = start_real_observer(&entries_plan(directory.path(), &[target]));
    fs::write(sibling, "after").unwrap();
    assert_no_event(&events);
}

#[test]
fn observes_recursive_worktree_events() {
    let directory = tempdir().unwrap();
    let nested = directory.path().join("src/nested");
    fs::create_dir_all(&nested).unwrap();
    let file = nested.join("file.ts");
    fs::write(&file, "before").unwrap();
    let (_observer, events, _) = start_real_observer(&tree_plan(directory.path()));
    fs::write(file, "after").unwrap();
    next_event(&events);
}

#[test]
fn observes_atomic_replacement_in_a_recursive_tree() {
    let directory = tempdir().unwrap();
    let nested = directory.path().join("src/nested");
    fs::create_dir_all(&nested).unwrap();
    let file = nested.join("file.ts");
    let temporary = nested.join(".file.ts.tmp");
    fs::write(&file, "before").unwrap();
    let (_observer, events, _) = start_real_observer(&tree_plan(directory.path()));
    fs::write(&temporary, "after").unwrap();
    fs::rename(temporary, file).unwrap();
    next_event(&events);
}

#[test]
fn observes_files_written_into_a_new_directory() {
    let directory = tempdir().unwrap();
    let (_observer, events, _) = start_real_observer(&tree_plan(directory.path()));
    let nested = directory.path().join("new/nested");
    fs::create_dir_all(&nested).unwrap();
    next_event(&events);
    std::thread::sleep(Duration::from_millis(20));
    drain(&events);
    fs::write(nested.join("file.ts"), "content").unwrap();
    next_event(&events);
}

#[test]
fn recursive_target_ignores_excluded_metadata_churn() {
    let directory = tempdir().unwrap();
    let metadata_directory = directory.path().join(".git");
    fs::create_dir(&metadata_directory).unwrap();
    let metadata = metadata_directory.join("index");
    fs::write(&metadata, "before").unwrap();
    let (_observer, events, _) = start_real_observer(&tree_plan(directory.path()));
    // FSEvents can deliver creation of the temporary root after registration.
    // Settle fixture setup before the excluded write, never after it.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        assert!(
            std::time::Instant::now() < deadline,
            "setup events did not settle"
        );
        match events.recv_timeout(Duration::from_millis(250)) {
            Ok(()) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => break,
            Err(error) => panic!("observer disconnected during setup: {error}"),
        }
    }
    fs::write(metadata, "after").unwrap();
    assert_no_event(&events);
}

#[test]
fn close_releases_handles_and_suppresses_later_events() {
    let directory = tempdir().unwrap();
    let file = directory.path().join("input.patch");
    fs::write(&file, "before").unwrap();
    let (observer, events, _) =
        start_real_observer(&entries_plan(directory.path(), std::slice::from_ref(&file)));
    observer.close();
    assert!(observer.wait_closed(Duration::from_secs(1)));
    fs::write(file, "after").unwrap();
    assert_no_event(&events);
}
