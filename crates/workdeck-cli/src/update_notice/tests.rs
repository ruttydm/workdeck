use super::*;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;
use tempfile::TempDir;

use crate::update::{
    GITHUB_LATEST_RELEASE_URL, HOMEBREW_FORMULA_URL, ReleaseLookup, ReleaseRequest,
    ReleaseResponse, SilentUpdateReporter, UpdateInvocation, UpdatePlatform, UpdateProcessResult,
};

struct NoticeHarness {
    _directory: TempDir,
    context: StartupUpdateNoticeContext,
    requests: Arc<Mutex<Vec<ReleaseRequest>>>,
    calls: Arc<AtomicUsize>,
}

impl NoticeHarness {
    fn new(
        source: Option<WorkdeckInstallSource>,
        installed_version: &str,
        response: Result<ReleaseResponse, String>,
    ) -> Self {
        let directory = TempDir::new().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let calls = Arc::new(AtomicUsize::new(0));
        let captured_requests = Arc::clone(&requests);
        let captured_calls = Arc::clone(&calls);
        let fetcher = Arc::new(move |request: &ReleaseRequest, _: &AtomicBool| {
            captured_calls.fetch_add(1, Ordering::Relaxed);
            captured_requests.lock().unwrap().push(request.clone());
            response.clone()
        });
        let runner = Arc::new(|_: &UpdateInvocation| {
            Ok(UpdateProcessResult {
                exit_code: 0,
                stderr: String::new(),
            })
        });
        Self {
            context: StartupUpdateNoticeContext {
                update: SelfUpdateContext {
                    env: BTreeMap::new(),
                    executable_path: "/usr/local/bin/workdeck".into(),
                    installed_version: installed_version.into(),
                    install_source: source,
                    platform: UpdatePlatform::Linux,
                    architecture: "x86_64".into(),
                    release_lookup: ReleaseLookup {
                        fetcher,
                        timeout: Duration::from_secs(1),
                    },
                    runner,
                    reporter: Arc::new(SilentUpdateReporter),
                },
                state_path: Some(directory.path().join("state.json")),
            },
            _directory: directory,
            requests,
            calls,
        }
    }

    fn resolve(&self) -> Option<StartupNotice> {
        resolve_startup_update_notice(&self.context)
    }
}

fn cargo_response(latest: &str, beta: Option<&str>) -> Result<ReleaseResponse, String> {
    Ok(ReleaseResponse {
        status: 200,
        body: json!({
            "crate": {
                "max_stable_version": latest,
                "newest_version": beta.unwrap_or(latest),
            }
        })
        .to_string(),
    })
}

fn formula_response(stable: &str) -> Result<ReleaseResponse, String> {
    Ok(ReleaseResponse {
        status: 200,
        body: json!({"versions": {"stable": stable}}).to_string(),
    })
}

fn github_response(version: &str) -> Result<ReleaseResponse, String> {
    Ok(ReleaseResponse {
        status: 200,
        body: json!({"tag_name": format!("v{version}")}).to_string(),
    })
}

fn expected(key: &str, message: &str) -> Option<StartupNotice> {
    Some(StartupNotice::new(key, message))
}

#[test]
fn stable_cargo_install_prefers_a_newer_latest_release() {
    let harness = NoticeHarness::new(
        Some(WorkdeckInstallSource::Cargo),
        "0.7.0",
        cargo_response("0.7.1", Some("0.8.0-beta.1")),
    );
    assert_eq!(
        harness.resolve(),
        expected(
            "latest:0.7.1",
            "Update available: 0.7.1 (latest) • run `workdeck update`"
        )
    );
}

#[test]
fn stable_cargo_install_falls_back_to_a_newer_beta() {
    let harness = NoticeHarness::new(
        Some(WorkdeckInstallSource::Cargo),
        "0.7.0",
        cargo_response("0.7.0", Some("0.8.0-beta.1")),
    );
    assert_eq!(
        harness.resolve(),
        expected(
            "beta:0.8.0-beta.1",
            "Update available: 0.8.0-beta.1 (beta) • run `workdeck update 0.8.0-beta.1`"
        )
    );
}

#[test]
fn cargo_prerelease_install_selects_the_higher_newer_channel() {
    let harness = NoticeHarness::new(
        Some(WorkdeckInstallSource::Cargo),
        "0.8.0-beta.1",
        cargo_response("0.8.0", Some("0.8.1-beta.1")),
    );
    assert_eq!(harness.resolve().unwrap().key, "beta:0.8.1-beta.1");
}

#[test]
fn homebrew_install_reads_the_formula_registry() {
    let harness = NoticeHarness::new(
        Some(WorkdeckInstallSource::Homebrew),
        "0.7.0",
        formula_response("0.7.1"),
    );
    assert_eq!(harness.resolve().unwrap().key, "latest:0.7.1");
    assert_eq!(
        harness.requests.lock().unwrap()[0].url,
        HOMEBREW_FORMULA_URL
    );
}

#[test]
fn homebrew_install_stays_quiet_while_the_formula_lags() {
    let harness = NoticeHarness::new(
        Some(WorkdeckInstallSource::Homebrew),
        "0.7.0",
        formula_response("0.7.0"),
    );
    assert_eq!(harness.resolve(), None);
}

#[test]
fn curl_install_reads_the_github_release_registry() {
    let harness = NoticeHarness::new(
        Some(WorkdeckInstallSource::Curl),
        "0.7.0",
        github_response("0.7.1"),
    );
    assert_eq!(harness.resolve().unwrap().key, "latest:0.7.1");
    assert_eq!(
        harness.requests.lock().unwrap()[0].url,
        GITHUB_LATEST_RELEASE_URL
    );
}

#[test]
fn curl_install_stays_quiet_when_current() {
    let harness = NoticeHarness::new(
        Some(WorkdeckInstallSource::Curl),
        "0.7.0",
        github_response("0.7.0"),
    );
    assert_eq!(harness.resolve(), None);
}

#[test]
fn declared_homebrew_environment_is_detected() {
    let mut harness = NoticeHarness::new(None, "0.7.0", formula_response("0.7.1"));
    harness
        .context
        .update
        .env
        .insert("WORKDECK_INSTALL_SOURCE".into(), "homebrew".into());
    assert_eq!(harness.resolve().unwrap().key, "latest:0.7.1");
    assert_eq!(
        harness.requests.lock().unwrap()[0].url,
        HOMEBREW_FORMULA_URL
    );
}

#[test]
fn unmarked_homebrew_cellar_install_is_detected() {
    let mut harness = NoticeHarness::new(None, "0.7.0", formula_response("0.7.1"));
    harness.context.update.executable_path =
        "/opt/homebrew/Cellar/workdeck/0.7.0/bin/workdeck".into();
    assert_eq!(harness.resolve().unwrap().key, "latest:0.7.1");
}

#[test]
fn local_source_build_suppresses_lookup() {
    let harness = NoticeHarness::new(
        Some(WorkdeckInstallSource::Dev),
        "0.7.0",
        Err("must not fetch".into()),
    );
    assert_eq!(harness.resolve(), None);
    assert_eq!(harness.calls.load(Ordering::Relaxed), 0);
}

#[test]
fn nix_notice_uses_externally_managed_instruction() {
    let harness = NoticeHarness::new(
        Some(WorkdeckInstallSource::Nix),
        "0.7.0",
        github_response("0.7.1"),
    );
    assert_eq!(
        harness.resolve(),
        expected(
            "latest:0.7.1",
            "Update available: 0.7.1 (latest) • update Workdeck through your Nix configuration"
        )
    );
}

#[test]
fn unmarked_nix_store_install_is_detected() {
    let mut harness = NoticeHarness::new(None, "0.7.0", github_response("0.7.1"));
    harness.context.update.executable_path = "/nix/store/hash-workdeck/bin/workdeck".into();
    assert!(
        harness
            .resolve()
            .unwrap()
            .message
            .contains("Nix configuration")
    );
}

#[test]
fn nix_install_never_surfaces_a_cargo_beta() {
    let harness = NoticeHarness::new(
        Some(WorkdeckInstallSource::Nix),
        "0.7.0",
        Ok(ReleaseResponse {
            status: 200,
            body: json!({"tag_name": "v0.8.0-beta.1"}).to_string(),
        }),
    );
    assert_eq!(harness.resolve(), None);
}

#[test]
fn powershell_install_uses_the_stable_github_stream() {
    let harness = NoticeHarness::new(
        Some(WorkdeckInstallSource::PowerShell),
        "0.7.0",
        github_response("0.7.1"),
    );
    assert_eq!(harness.resolve().unwrap().key, "latest:0.7.1");
}

#[test]
fn direct_install_uses_the_stable_github_stream() {
    let harness = NoticeHarness::new(
        Some(WorkdeckInstallSource::Direct),
        "0.7.0",
        github_response("0.7.1"),
    );
    assert_eq!(harness.resolve().unwrap().key, "latest:0.7.1");
}

#[test]
fn unmarked_unix_installer_path_selects_curl_notice_lookup() {
    let mut harness = NoticeHarness::new(None, "0.7.0", github_response("0.7.1"));
    harness.context.update.executable_path = "/home/user/.workdeck/bin/workdeck".into();
    assert_eq!(harness.resolve().unwrap().key, "latest:0.7.1");
}

#[test]
fn unmarked_windows_installer_path_selects_powershell_notice_lookup() {
    let mut harness = NoticeHarness::new(None, "0.7.0", github_response("0.7.1"));
    harness.context.update.platform = UpdatePlatform::Windows;
    harness.context.update.executable_path = r"C:\Users\user\.workdeck\bin\workdeck.exe".into();
    assert_eq!(harness.resolve().unwrap().key, "latest:0.7.1");
}

#[test]
fn explicit_dev_source_remains_silent_even_when_a_release_exists() {
    let harness = NoticeHarness::new(
        Some(WorkdeckInstallSource::Dev),
        "0.7.0",
        github_response("0.7.1"),
    );
    assert_eq!(harness.resolve(), None);
    assert_eq!(harness.calls.load(Ordering::Relaxed), 0);
}

#[test]
fn incidental_workdeck_directory_name_does_not_claim_installer_ownership() {
    let mut harness = NoticeHarness::new(None, "0.7.0", github_response("0.7.1"));
    harness.context.update.executable_path = "/home/workdeck/projects/reviewer/bin/workdeck".into();
    assert_eq!(harness.resolve().unwrap().key, "latest:0.7.1");
}

#[test]
fn current_install_returns_no_notice() {
    let harness = NoticeHarness::new(
        Some(WorkdeckInstallSource::Cargo),
        "0.7.0",
        cargo_response("0.7.0", Some("0.7.0-beta.1")),
    );
    assert_eq!(harness.resolve(), None);
}

#[test]
fn first_run_persists_version_without_skill_notice() {
    let harness = NoticeHarness::new(
        Some(WorkdeckInstallSource::Cargo),
        "0.7.0",
        cargo_response("0.7.0", None),
    );
    assert_eq!(harness.resolve(), None);
    assert_eq!(
        read_app_state_record(harness.context.state_path.as_ref().unwrap()),
        json!({"version": 1, "lastSeenCliVersion": "0.7.0"})
            .as_object()
            .unwrap()
            .clone()
    );
}

#[test]
fn version_change_shows_one_skill_refresh_notice_before_fetching() {
    let harness = NoticeHarness::new(
        Some(WorkdeckInstallSource::Cargo),
        "0.8.0",
        cargo_response("0.8.0", None),
    );
    let path = harness.context.state_path.as_ref().unwrap();
    update_app_state_record(
        path,
        json!({"version": 1, "lastSeenCliVersion": "0.7.0", "trust": "kept"})
            .as_object()
            .unwrap()
            .clone(),
    )
    .unwrap();
    assert_eq!(
        harness.resolve(),
        expected(
            "skill:0.8.0",
            "Workdeck 0.8.0 installed • If your agent copied Workdeck's skill, run workdeck skill path"
        )
    );
    assert_eq!(harness.calls.load(Ordering::Relaxed), 0);
    let state = read_app_state_record(path);
    assert_eq!(state.get("trust"), Some(&Value::String("kept".into())));
    assert_eq!(harness.resolve(), None);
}

#[test]
fn unresolved_local_version_returns_no_notice() {
    let harness = NoticeHarness::new(
        None,
        UNKNOWN_CLI_VERSION,
        cargo_response("0.7.0", Some("0.8.0-beta.1")),
    );
    assert_eq!(harness.resolve(), None);
    assert_eq!(harness.calls.load(Ordering::Relaxed), 0);
}

#[test]
fn non_success_registry_response_returns_no_notice() {
    let harness = NoticeHarness::new(
        Some(WorkdeckInstallSource::Cargo),
        "0.7.0",
        Ok(ReleaseResponse {
            status: 503,
            body: json!({"crate": {"max_stable_version": "0.7.1"}}).to_string(),
        }),
    );
    assert_eq!(harness.resolve(), None);
}

#[test]
fn registry_failure_returns_no_notice() {
    let harness = NoticeHarness::new(
        Some(WorkdeckInstallSource::Cargo),
        "0.7.0",
        Err("network down".into()),
    );
    assert_eq!(harness.resolve(), None);
}

#[test]
fn disable_environment_returns_immediately_without_state_or_network() {
    let mut harness = NoticeHarness::new(
        Some(WorkdeckInstallSource::Cargo),
        "0.7.0",
        Err("must not fetch".into()),
    );
    harness
        .context
        .update
        .env
        .insert(DISABLE_STARTUP_UPDATE_NOTICE_ENV.into(), "1".into());
    assert_eq!(harness.resolve(), None);
    assert_eq!(harness.calls.load(Ordering::Relaxed), 0);
    assert!(!harness.context.state_path.as_ref().unwrap().exists());
}

#[test]
fn hung_registry_lookup_is_cancelled_at_the_deadline() {
    let (observed, acknowledgement) = std::sync::mpsc::sync_channel(1);
    let mut harness = NoticeHarness::new(
        Some(WorkdeckInstallSource::Cargo),
        "0.7.0",
        Err("unused".into()),
    );
    harness.context.update.release_lookup = ReleaseLookup {
        fetcher: Arc::new(move |_: &ReleaseRequest, cancelled: &AtomicBool| {
            while !cancelled.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
            let _ = observed.send(());
            Err("cancelled".into())
        }),
        timeout: Duration::from_millis(10),
    };
    assert_eq!(harness.resolve(), None);
    // The lookup deadline is still 10 ms. Observe the worker's cancellation
    // acknowledgement with a bounded wait, not a scheduler-dependent yield count.
    acknowledgement
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
}
