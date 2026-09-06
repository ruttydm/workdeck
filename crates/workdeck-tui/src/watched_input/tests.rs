use super::*;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tempfile::TempDir;
use workdeck_core::{CliInput, CommonOptions, FileCommandInput, VcsDiffCommandInput};
use workdeck_vcs::{
    GitVcsAdapterOptions, VcsLoadContext, VcsReviewInput, bundled_vcs_catalog,
    load_file_comparison, load_git_changeset,
};

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

    fn callback_at(&self, index: usize) -> WatchObserverCallbacks {
        self.callbacks.lock().unwrap()[index].clone()
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

fn native_watch_config() -> WatchControllerConfig {
    WatchControllerConfig {
        quiet_delay: Duration::from_millis(50),
        maximum_delay: Duration::from_millis(250),
        healthy_check: Duration::from_secs(10),
        degraded_check: Duration::from_millis(250),
        duplicate_error_interval: Duration::from_secs(10),
        startup_timeout: Duration::from_secs(2),
    }
}

fn wait_for_native_refresh<E>(
    driver: &mut WatchedInputDriver,
    mut refresh: impl FnMut() -> Result<(), E>,
) -> WatchedInputOutcome
where
    E: std::fmt::Display,
{
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut total = WatchedInputOutcome::default();
    loop {
        total.merge(driver.poll(Instant::now(), &mut refresh));
        if total.refreshes > 0 {
            return total;
        }
        assert!(Instant::now() < deadline, "native watch refresh timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn git(cwd: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed: {}",
        arguments.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn native_direct_file_driver_refreshes_the_loaded_diff_after_an_atomic_save() {
    let directory = TempDir::new().unwrap();
    let before = directory.path().join("before.ts");
    let after = directory.path().join("after.ts");
    fs::write(&before, "export const watchedValue = 'before';\n").unwrap();
    fs::write(&after, "export const watchedValue = 'initial change';\n").unwrap();
    let input = CliInput::Files(FileCommandInput {
        left: "before.ts".into(),
        right: "after.ts".into(),
        options: CommonOptions {
            watch: Some(true),
            ..CommonOptions::default()
        },
    });
    let runtime = Arc::new(NativeWatchedInputRuntime::new(directory.path(), None));
    let initial_signature = runtime.signature(&input).unwrap();
    let runtime_trait: Arc<dyn WatchedInputRuntime> = runtime;
    let mut driver = WatchedInputDriver::start(
        true,
        input,
        runtime_trait,
        Some(initial_signature),
        Instant::now(),
        native_watch_config(),
    )
    .unwrap()
    .unwrap();

    std::thread::sleep(Duration::from_millis(50));
    let initial = driver.poll(Instant::now(), &mut || {
        Err::<(), _>("unchanged startup must not refresh")
    });
    assert_eq!(initial.refresh_attempts, 0);
    assert!(initial.errors.is_empty());

    let replacement = directory.path().join("after-replacement.ts");
    fs::write(
        &replacement,
        "export const watchedValue = 'atomic replacement';\n",
    )
    .unwrap();
    #[cfg(windows)]
    fs::remove_file(&after).unwrap();
    fs::rename(replacement, &after).unwrap();

    let outcome = wait_for_native_refresh(&mut driver, || {
        let review = load_file_comparison(
            directory.path(),
            Path::new("before.ts"),
            Path::new("after.ts"),
        )
        .map_err(|error| error.to_string())?;
        let patch = &review.files[0].patch;
        assert!(patch.contains("watchedValue = 'atomic replacement'"));
        assert!(!patch.contains("watchedValue = 'initial change'"));
        Ok::<(), String>(())
    });
    assert_eq!(outcome.refresh_attempts, 1);
    assert_eq!(outcome.refreshes, 1);
    assert!(outcome.reload_pending);
    assert!(outcome.errors.is_empty());
}

#[test]
fn native_git_driver_refreshes_a_tracked_file_inside_a_linked_worktree() {
    let repository = TempDir::new().unwrap();
    git(
        repository.path(),
        &["init", "-q", "--initial-branch", "master"],
    );
    git(repository.path(), &["config", "user.name", "Watch Test"]);
    git(
        repository.path(),
        &["config", "user.email", "watch@example.com"],
    );
    git(repository.path(), &["config", "commit.gpgsign", "false"]);
    fs::write(
        repository.path().join("linked.ts"),
        "export const linkedValue = 'committed';\n",
    )
    .unwrap();
    git(repository.path(), &["add", "linked.ts"]);
    git(repository.path(), &["commit", "-q", "-m", "initial"]);

    let linked = TempDir::new().unwrap();
    git(
        repository.path(),
        &[
            "worktree",
            "add",
            "-q",
            linked.path().to_str().unwrap(),
            "-b",
            "linked-watch",
        ],
    );
    let tracked_file = linked.path().join("linked.ts");
    fs::write(
        &tracked_file,
        "export const linkedValue = 'initial change';\n",
    )
    .unwrap();
    let diff_input = VcsDiffCommandInput {
        range: None,
        range_endpoints: None,
        staged: false,
        pathspecs: Vec::new(),
        options: CommonOptions {
            watch: Some(true),
            ..CommonOptions::default()
        },
    };
    let input = CliInput::Vcs(diff_input.clone());
    let runtime = Arc::new(NativeWatchedInputRuntime::new(
        linked.path(),
        Some(bundled_vcs_catalog().clone()),
    ));
    let initial_signature = runtime.signature(&input).unwrap();
    let runtime_trait: Arc<dyn WatchedInputRuntime> = runtime;
    let mut driver = WatchedInputDriver::start(
        true,
        input,
        runtime_trait,
        Some(initial_signature),
        Instant::now(),
        native_watch_config(),
    )
    .unwrap()
    .unwrap();

    std::thread::sleep(Duration::from_millis(50));
    let initial = driver.poll(Instant::now(), &mut || {
        Err::<(), _>("unchanged startup must not refresh")
    });
    assert_eq!(initial.refresh_attempts, 0);
    assert!(initial.errors.is_empty());

    fs::write(
        &tracked_file,
        "export const linkedValue = 'passive worktree refresh';\n",
    )
    .unwrap();
    let outcome = wait_for_native_refresh(&mut driver, || {
        let review = load_git_changeset(
            &VcsReviewInput::Diff(diff_input.clone()),
            &VcsLoadContext {
                cwd: linked.path().to_owned(),
            },
            &GitVcsAdapterOptions::default(),
        )
        .map_err(|error| error.to_string())?;
        let patch = &review.files[0].patch;
        assert!(patch.contains("linkedValue = 'passive worktree refresh'"));
        assert!(!patch.contains("linkedValue = 'initial change'"));
        Ok::<(), String>(())
    });
    assert_eq!(outcome.refresh_attempts, 1);
    assert_eq!(outcome.refreshes, 1);
    assert!(outcome.reload_pending);
    assert!(outcome.errors.is_empty());
}

#[test]
fn frozen_hunk_pty_watch_oracle_maps_both_pins_and_each_source_test() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let oracle: serde_json::Value =
        serde_json::from_str(include_str!("../../../../port/hunk/oracles/pty-watch.json")).unwrap();
    assert_eq!(oracle["runtime"], "Bun 1.3.14");
    assert_eq!(
        oracle["source"]["baseline"],
        serde_json::json!({
            "commit": "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2",
            "blob": "5b671504d11a731e4c31d9b37dfbaa1adb71d0b0",
            "sha256": "f99474213fbb1afaf0680c3b460630a7ff865b3bee8e231ba8a4a4d88fbb797f",
            "bytes": 2_366,
            "lines": 72
        })
    );
    assert_eq!(
        oracle["source"]["stable"],
        serde_json::json!({
            "commit": "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd",
            "blob": "e46c10c7a76e641cdfb64fa0f9c8607ea4e92090",
            "sha256": "3a114b345b65b34a4b0673e328ac9bcb0b79ef00d4c35e1424185d3c5f8c2a90",
            "bytes": 2_355,
            "lines": 72
        })
    );
    for pin in ["baseline", "stable"] {
        assert_eq!(oracle["oracle_runs"][pin]["passed"], 2);
        assert_eq!(oracle["oracle_runs"][pin]["failed"], 0);
        assert_eq!(oracle["oracle_runs"][pin]["expect_calls"], 4);
    }
    assert_eq!(oracle["baseline_delta"].as_array().unwrap().len(), 1);

    let mappings = oracle["test_mapping"].as_array().unwrap();
    assert_eq!(mappings.len(), 2);
    let mut source_tests = BTreeSet::new();
    for mapping in mappings {
        assert!(source_tests.insert(mapping["source_test"].as_str().unwrap()));
        let evidence = mapping["evidence"].as_array().unwrap();
        assert!(!evidence.is_empty());
        for item in evidence {
            let relative = item["file"].as_str().unwrap();
            let test = item["test"].as_str().unwrap();
            let source = fs::read_to_string(workspace.join(relative)).unwrap();
            let function = test.rsplit("::").next().unwrap();
            assert!(
                source.contains(&format!("fn {function}(")),
                "{relative} does not define {test}"
            );
        }
    }
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

    let early = driver.poll(now + Duration::from_millis(199), &mut || {
        refreshes += 1;
        Ok::<(), &'static str>(())
    });
    assert_eq!(early.refresh_attempts, 0);

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
fn successful_refresh_replaces_source_once_and_makes_late_callbacks_inert() {
    let now = Instant::now();
    let runtime = FakeRuntime::hybrid();
    let runtime_trait: Arc<dyn WatchedInputRuntime> = runtime.clone();
    let mut current = WatchedInputDriver::start(
        true,
        input(),
        Arc::clone(&runtime_trait),
        None,
        now,
        WatchControllerConfig::default(),
    )
    .unwrap();
    let old_callback = runtime.callback_at(0);
    runtime.set_signature("signature:replacement");
    (old_callback.on_event)();

    let pending = current
        .as_mut()
        .unwrap()
        .poll(now, &mut || Err::<(), _>("debounce refreshed too early"));
    assert!(pending.reload_pending);
    assert_eq!(pending.refresh_attempts, 0);

    let outcome = current
        .as_mut()
        .unwrap()
        .poll(now + Duration::from_millis(200), &mut || {
            Ok::<(), &'static str>(())
        });
    assert_eq!(outcome.refreshes, 1);
    replace_watched_input_driver(
        &mut current,
        true,
        input(),
        runtime_trait,
        None,
        now + Duration::from_millis(200),
        WatchControllerConfig::default(),
    )
    .unwrap();
    assert_eq!(runtime.source_count.load(Ordering::Relaxed), 2);
    assert_eq!(runtime.close_count.load(Ordering::Relaxed), 1);

    runtime.set_signature("signature:late");
    (old_callback.on_event)();
    let late = current
        .as_mut()
        .unwrap()
        .poll(now + Duration::from_millis(200), &mut || {
            Err::<(), _>("late callback refreshed the replacement")
        });
    assert!(!late.reload_pending);
    assert_eq!(late.refresh_attempts, 0);

    drop(current);
    assert_eq!(runtime.close_count.load(Ordering::Relaxed), 2);
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
