use super::*;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use workdeck_core::{CliInput, CommonOptions, FileCommandInput};

struct FakeSource {
    close_count: Arc<AtomicUsize>,
}

impl WatchedInputEventSource for FakeSource {
    fn close(&mut self) {
        self.close_count.fetch_add(1, Ordering::Relaxed);
    }
}

struct FakeRuntime {
    plan: Mutex<Option<VcsWatchPlan>>,
    signature: Mutex<String>,
    callbacks: Mutex<Vec<WatchObserverCallbacks>>,
    resolve_count: AtomicUsize,
    signature_count: AtomicUsize,
    source_count: AtomicUsize,
    close_count: Arc<AtomicUsize>,
    fail_source: AtomicBool,
}

impl FakeRuntime {
    fn hybrid() -> Arc<Self> {
        Arc::new(Self {
            plan: Mutex::new(Some(VcsWatchPlan {
                coverage: VcsWatchCoverage::Hybrid,
                targets: Vec::new(),
            })),
            signature: Mutex::new("signature:0".into()),
            callbacks: Mutex::new(Vec::new()),
            resolve_count: AtomicUsize::new(0),
            signature_count: AtomicUsize::new(0),
            source_count: AtomicUsize::new(0),
            close_count: Arc::new(AtomicUsize::new(0)),
            fail_source: AtomicBool::new(false),
        })
    }

    fn set_signature(&self, signature: &str) {
        *self.signature.lock().unwrap() = signature.into();
    }

    fn callback(&self) -> WatchObserverCallbacks {
        self.callbacks.lock().unwrap()[0].clone()
    }
}

impl WatchedInputRuntime for FakeRuntime {
    fn resolve_plan(&self, _input: &CliInput) -> Result<Option<VcsWatchPlan>, WatchSourceError> {
        self.resolve_count.fetch_add(1, Ordering::Relaxed);
        Ok(self.plan.lock().unwrap().clone())
    }

    fn signature(&self, _input: &CliInput) -> Result<String, WatchSourceError> {
        self.signature_count.fetch_add(1, Ordering::Relaxed);
        Ok(self.signature.lock().unwrap().clone())
    }

    fn create_event_source(
        &self,
        _plan: &VcsWatchPlan,
        callbacks: WatchObserverCallbacks,
    ) -> Result<Option<Box<dyn WatchedInputEventSource>>, WatchSourceError> {
        self.source_count.fetch_add(1, Ordering::Relaxed);
        if self.fail_source.load(Ordering::Relaxed) {
            return Err(WatchSourceError::new(Some("EMFILE"), "watch unavailable"));
        }
        self.callbacks.lock().unwrap().push(callbacks);
        Ok(Some(Box::new(FakeSource {
            close_count: Arc::clone(&self.close_count),
        })))
    }
}

fn input() -> CliInput {
    CliInput::Files(FileCommandInput {
        left: "before.ts".into(),
        right: "after.ts".into(),
        options: CommonOptions {
            watch: Some(true),
            ..CommonOptions::default()
        },
    })
}

#[test]
fn disabled_or_unplanned_inputs_create_no_event_source() {
    let now = Instant::now();
    let disabled = FakeRuntime::hybrid();
    let disabled_runtime: Arc<dyn WatchedInputRuntime> = disabled.clone();
    assert!(
        WatchedInputDriver::start(
            false,
            input(),
            disabled_runtime,
            None,
            now,
            WatchControllerConfig::default(),
        )
        .unwrap()
        .is_none()
    );
    assert_eq!(disabled.resolve_count.load(Ordering::Relaxed), 0);

    let unplanned = FakeRuntime::hybrid();
    *unplanned.plan.lock().unwrap() = None;
    let unplanned_runtime: Arc<dyn WatchedInputRuntime> = unplanned.clone();
    assert!(
        WatchedInputDriver::start(
            true,
            input(),
            unplanned_runtime,
            None,
            now,
            WatchControllerConfig::default(),
        )
        .unwrap()
        .is_none()
    );
    assert_eq!(unplanned.source_count.load(Ordering::Relaxed), 0);
    assert_eq!(unplanned.signature_count.load(Ordering::Relaxed), 0);
}

#[test]
fn event_changes_debounce_and_refresh_exactly_once() {
    let now = Instant::now();
    let runtime = FakeRuntime::hybrid();
    let runtime_trait: Arc<dyn WatchedInputRuntime> = runtime.clone();
    let mut driver = WatchedInputDriver::start(
        true,
        input(),
        runtime_trait,
        None,
        now,
        WatchControllerConfig::default(),
    )
    .unwrap()
    .unwrap();
    runtime.set_signature("signature:1");
    (runtime.callback().on_event)();

    let mut refreshes = 0;
    let first = driver.poll(now, &mut || {
        refreshes += 1;
        Ok::<(), &'static str>(())
    });
    assert!(first.reload_pending);
    assert_eq!(first.refresh_attempts, 0);

    let second = driver.poll(now + Duration::from_millis(200), &mut || {
        refreshes += 1;
        Ok::<(), &'static str>(())
    });
    assert!(!second.reload_pending);
    assert_eq!(second.refresh_attempts, 1);
    assert_eq!(second.refreshes, 1);
    assert_eq!(refreshes, 1);
    assert_eq!(driver.state().applied_signature, "signature:1");

    driver.close();
    driver.close();
    assert_eq!(runtime.close_count.load(Ordering::Relaxed), 1);
}

#[test]
fn readiness_bootstrap_uses_the_short_direct_file_safety_interval() {
    let now = Instant::now();
    let runtime = FakeRuntime::hybrid();
    *runtime.plan.lock().unwrap() = Some(VcsWatchPlan {
        coverage: VcsWatchCoverage::Hybrid,
        targets: vec![VcsWatchTarget::DirectoryEntries {
            directory: PathBuf::from("/repo"),
            entries: vec!["before.ts".into()],
            sources: BTreeSet::from([VcsWatchTargetSource::Content]),
        }],
    });
    let runtime_trait: Arc<dyn WatchedInputRuntime> = runtime.clone();
    let mut driver = WatchedInputDriver::start(
        true,
        input(),
        runtime_trait,
        None,
        now,
        WatchControllerConfig::default(),
    )
    .unwrap()
    .unwrap();
    (runtime.callback().on_ready.as_ref().unwrap())();
    let outcome = driver.poll(now, &mut || Ok::<(), &'static str>(()));
    assert_eq!(outcome.refresh_attempts, 0);
    assert_eq!(
        driver.next_deadline(),
        Some(now + DIRECT_FILE_WATCH_SAFETY_CHECK)
    );
}

#[test]
fn poll_only_plans_skip_observers_and_use_degraded_checks() {
    let now = Instant::now();
    let runtime = FakeRuntime::hybrid();
    *runtime.plan.lock().unwrap() = Some(VcsWatchPlan::poll_only());
    let runtime_trait: Arc<dyn WatchedInputRuntime> = runtime.clone();
    let driver = WatchedInputDriver::start(
        true,
        input(),
        runtime_trait,
        Some("bootstrap".into()),
        now,
        WatchControllerConfig::default(),
    )
    .unwrap()
    .unwrap();
    assert!(driver.state().degraded);
    assert_eq!(driver.next_deadline(), Some(now + Duration::from_secs(2)));
    assert_eq!(runtime.source_count.load(Ordering::Relaxed), 0);
    assert_eq!(runtime.signature_count.load(Ordering::Relaxed), 0);
}

#[test]
fn preloaded_signature_is_authoritative_when_the_driver_mounts() {
    let now = Instant::now();
    let runtime = FakeRuntime::hybrid();
    runtime.set_signature("content changed during initial load");
    let runtime_trait: Arc<dyn WatchedInputRuntime> = runtime.clone();
    let driver = WatchedInputDriver::start(
        true,
        input(),
        runtime_trait,
        Some("captured before initial load".into()),
        now,
        WatchControllerConfig::default(),
    )
    .unwrap()
    .unwrap();

    assert_eq!(
        driver.state().applied_signature,
        "captured before initial load"
    );
    assert_eq!(runtime.signature_count.load(Ordering::Relaxed), 0);
}

#[test]
fn source_construction_failure_degrades_without_disabling_refresh() {
    let now = Instant::now();
    let runtime = FakeRuntime::hybrid();
    runtime.fail_source.store(true, Ordering::Relaxed);
    let runtime_trait: Arc<dyn WatchedInputRuntime> = runtime.clone();
    let mut driver = WatchedInputDriver::start(
        true,
        input(),
        runtime_trait,
        None,
        now,
        WatchControllerConfig::default(),
    )
    .unwrap()
    .unwrap();
    assert!(driver.state().degraded);
    let pending = driver.poll(now, &mut || Ok::<(), &'static str>(()));
    assert_eq!(pending.errors.len(), 1);
    assert_eq!(pending.errors[0].code.as_deref(), Some("EMFILE"));

    runtime.set_signature("signature:2");
    let outcome = driver.poll(now + Duration::from_secs(2), &mut || {
        Ok::<(), &'static str>(())
    });
    assert_eq!(outcome.refreshes, 1);
}
