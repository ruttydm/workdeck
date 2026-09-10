use super::*;

use std::sync::Mutex;
use std::time::Instant;

use serde_json::json;

fn env(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|(key, value)| ((*key).into(), (*value).into()))
        .collect()
}

fn facts<'a>(
    environment: &'a BTreeMap<String, String>,
    path: &'a str,
    version: &'a str,
) -> InstallSourceFacts<'a> {
    InstallSourceFacts {
        env: environment,
        executable_path: path,
        version,
        home_dir: Some("/home/reviewer"),
        platform: UpdatePlatform::Linux,
        realpath: None,
    }
}

#[test]
fn declared_install_source_wins_and_accepts_dev() {
    let nix = env(&[(INSTALL_SOURCE_ENV, "nix")]);
    assert_eq!(
        detect_install_source(&facts(&nix, "/usr/local/bin/workdeck", "1.2.3")),
        WorkdeckInstallSource::Nix
    );
    let dev = env(&[(INSTALL_SOURCE_ENV, "dev")]);
    assert_eq!(
        detect_install_source(&facts(&dev, "/opt/workdeck/bin/workdeck", "1.2.3")),
        WorkdeckInstallSource::Dev
    );
}

#[test]
fn unknown_declared_source_is_ignored() {
    let environment = env(&[(INSTALL_SOURCE_ENV, "chocolatey")]);
    assert_eq!(
        detect_install_source(&facts(&environment, "/opt/workdeck/bin/workdeck", "1.2.3")),
        WorkdeckInstallSource::Direct
    );
}

#[test]
fn nix_store_paths_are_detected() {
    assert_eq!(
        detect_install_source(&facts(
            &BTreeMap::new(),
            "/nix/store/hash-workdeck/bin/workdeck",
            "1.2.3"
        )),
        WorkdeckInstallSource::Nix
    );
}

#[test]
fn cargo_installs_require_the_adjacent_cargo_bin_layout() {
    assert_eq!(
        detect_install_source(&facts(
            &BTreeMap::new(),
            "/home/reviewer/.cargo/bin/workdeck",
            "1.2.3"
        )),
        WorkdeckInstallSource::Cargo
    );
    assert_eq!(
        detect_install_source(&facts(
            &BTreeMap::new(),
            "/home/cargo/projects/workdeck/bin/workdeck",
            "1.2.3"
        )),
        WorkdeckInstallSource::Direct
    );
}

#[test]
fn homebrew_uses_resolved_cellar_path() {
    let realpath = |_: &str| Ok("/usr/local/Cellar/workdeck/1.2.3/bin/workdeck".into());
    let environment = BTreeMap::new();
    let mut input = facts(&environment, "/usr/local/bin/workdeck", "1.2.3");
    input.realpath = Some(&realpath);
    assert_eq!(
        detect_install_source(&input),
        WorkdeckInstallSource::Homebrew
    );
}

#[test]
fn homebrew_apple_silicon_and_linux_prefixes_are_detected() {
    for path in [
        "/opt/homebrew/bin/workdeck",
        "/home/linuxbrew/.linuxbrew/bin/workdeck",
    ] {
        assert_eq!(
            detect_install_source(&facts(&BTreeMap::new(), path, "1.2.3")),
            WorkdeckInstallSource::Homebrew
        );
    }
}

#[test]
fn homebrew_detection_requires_the_workdeck_artifact() {
    assert_eq!(
        detect_install_source(&facts(
            &BTreeMap::new(),
            "/opt/homebrew/Cellar/cargo/1.85/bin/cargo",
            "1.2.3"
        )),
        WorkdeckInstallSource::Direct
    );
}

#[test]
fn unrelated_package_tree_under_homebrew_is_not_a_formula_install() {
    assert_eq!(
        detect_install_source(&facts(
            &BTreeMap::new(),
            "/opt/homebrew/lib/node_modules/example/bin/workdeck",
            "1.2.3"
        )),
        WorkdeckInstallSource::Direct
    );
}

#[test]
fn installer_tree_selects_the_platform_script() {
    let environment = BTreeMap::new();
    let mut input = facts(
        &environment,
        "/home/reviewer/.workdeck/bin/workdeck",
        "1.2.3",
    );
    assert_eq!(detect_install_source(&input), WorkdeckInstallSource::Curl);
    input.platform = UpdatePlatform::Windows;
    input.executable_path = r"C:\Users\reviewer\.workdeck\bin\workdeck.exe";
    assert_eq!(
        detect_install_source(&input),
        WorkdeckInstallSource::PowerShell
    );
}

#[test]
fn curl_and_direct_can_be_declared() {
    for (value, source) in [
        ("curl", WorkdeckInstallSource::Curl),
        ("direct", WorkdeckInstallSource::Direct),
    ] {
        let environment = env(&[(INSTALL_SOURCE_ENV, value)]);
        assert_eq!(
            detect_install_source(&facts(&environment, "/opt/workdeck/workdeck", "1.2.3")),
            source
        );
    }
}

#[test]
fn a_workdeck_segment_not_followed_by_bin_is_not_an_installer() {
    assert_eq!(
        detect_install_source(&facts(
            &BTreeMap::new(),
            "/home/reviewer/project/.workdeck/review/workdeck",
            "1.2.3"
        )),
        WorkdeckInstallSource::Direct
    );
}

#[test]
fn explicit_and_default_dev_install_directories_are_detected() {
    let configured = env(&[(INSTALL_DIR_ENV, "/home/reviewer/tools/bin")]);
    assert_eq!(
        detect_install_source(&facts(
            &configured,
            "/home/reviewer/tools/bin/workdeck",
            "1.2.3"
        )),
        WorkdeckInstallSource::Dev
    );
    assert_eq!(
        detect_install_source(&facts(
            &BTreeMap::new(),
            "/home/reviewer/.local/bin/workdeck",
            "1.2.3"
        )),
        WorkdeckInstallSource::Dev
    );
}

#[test]
fn unknown_version_is_a_dev_build_and_other_paths_are_direct() {
    assert_eq!(
        detect_install_source(&facts(
            &BTreeMap::new(),
            "/opt/workdeck/bin/workdeck",
            UNKNOWN_CLI_VERSION
        )),
        WorkdeckInstallSource::Dev
    );
    assert_eq!(
        detect_install_source(&facts(
            &BTreeMap::new(),
            "/opt/workdeck/bin/workdeck",
            "1.2.3"
        )),
        WorkdeckInstallSource::Direct
    );
}

#[test]
fn rust_target_executables_are_dev_builds_even_with_a_package_version() {
    for profile in ["debug", "release"] {
        let path = format!("/home/reviewer/workdeck/target/{profile}/workdeck");
        assert_eq!(
            detect_install_source(&facts(&BTreeMap::new(), &path, "1.2.3")),
            WorkdeckInstallSource::Dev
        );
    }
}

#[test]
fn dev_directory_resolution_uses_environment_and_platform_defaults() {
    assert_eq!(
        resolve_dev_install_dir(
            &env(&[(INSTALL_DIR_ENV, "/srv/bin")]),
            Some("/home/reviewer"),
            UpdatePlatform::Linux
        )
        .as_deref(),
        Some("/srv/bin")
    );
    assert_eq!(
        resolve_dev_install_dir(&BTreeMap::new(), None, UpdatePlatform::Linux),
        None
    );
    assert_eq!(
        resolve_dev_install_dir(
            &BTreeMap::new(),
            Some(r"C:\Users\reviewer"),
            UpdatePlatform::Windows
        )
        .as_deref(),
        Some(r"C:\Users\reviewer\AppData\Local\Programs\workdeck")
    );
}

fn json_lookup(payload: Value) -> (ReleaseLookup, Arc<Mutex<Vec<ReleaseRequest>>>) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let capture = Arc::clone(&requests);
    let body = serde_json::to_string(&payload).unwrap();
    let lookup = ReleaseLookup {
        fetcher: Arc::new(move |request: &ReleaseRequest, _cancelled: &AtomicBool| {
            capture.lock().unwrap().push(request.clone());
            Ok(ReleaseResponse {
                status: 200,
                body: body.clone(),
            })
        }),
        timeout: Duration::from_secs(1),
    };
    (lookup, requests)
}

#[test]
fn cargo_release_lookup_reads_crates_io_channels() {
    let (lookup, requests) = json_lookup(json!({
        "crate": {"max_stable_version":"1.2.3", "newest_version":"1.3.0-beta.1"}
    }));
    assert_eq!(
        fetch_channel_versions(WorkdeckInstallSource::Cargo, &lookup),
        ChannelVersions {
            latest: Some("1.2.3".into()),
            beta: Some("1.3.0-beta.1".into()),
        }
    );
    assert_eq!(requests.lock().unwrap()[0].url, CARGO_CRATE_URL);
}

#[test]
fn homebrew_release_lookup_reads_only_the_formula_stable_version() {
    let (lookup, requests) = json_lookup(json!({"versions":{"stable":"1.2.0","head":"HEAD"}}));
    assert_eq!(
        fetch_channel_versions(WorkdeckInstallSource::Homebrew, &lookup),
        ChannelVersions {
            latest: Some("1.2.0".into()),
            beta: None,
        }
    );
    assert_eq!(requests.lock().unwrap()[0].url, HOMEBREW_FORMULA_URL);
}

#[test]
fn script_and_direct_installs_read_the_github_release_tag() {
    for source in [
        WorkdeckInstallSource::Curl,
        WorkdeckInstallSource::PowerShell,
        WorkdeckInstallSource::Direct,
    ] {
        let (lookup, requests) = json_lookup(json!({"tag_name":"v1.4.0"}));
        assert_eq!(
            fetch_channel_versions(source, &lookup).latest.as_deref(),
            Some("1.4.0")
        );
        let request = &requests.lock().unwrap()[0];
        assert_eq!(request.url, GITHUB_LATEST_RELEASE_URL);
        assert_eq!(
            request.headers.get("accept").map(String::as_str),
            Some("application/vnd.github+json")
        );
    }
}

#[test]
fn github_lookup_drops_prerelease_or_missing_tags() {
    for payload in [json!({"tag_name":"v1.4.0-beta.1"}), json!({"name":"1.4.0"})] {
        let (lookup, _) = json_lookup(payload);
        assert_eq!(
            fetch_channel_versions(WorkdeckInstallSource::Direct, &lookup).latest,
            None
        );
    }
}

#[test]
fn unmanaged_install_sources_do_not_fetch() {
    let lookup = ReleaseLookup {
        fetcher: Arc::new(|_: &ReleaseRequest, _: &AtomicBool| {
            panic!("unmanaged source must not fetch")
        }),
        timeout: Duration::from_millis(10),
    };
    for source in [WorkdeckInstallSource::Nix, WorkdeckInstallSource::Dev] {
        assert_eq!(
            fetch_channel_versions(source, &lookup),
            ChannelVersions::default()
        );
    }
}

#[test]
fn release_lookups_drop_non_normalized_versions() {
    let (lookup, _) = json_lookup(json!({
        "crate": {"max_stable_version":"v1.2.3", "newest_version":"1.3.0"}
    }));
    assert_eq!(
        fetch_channel_versions(WorkdeckInstallSource::Cargo, &lookup),
        ChannelVersions::default()
    );
}

#[test]
fn release_lookups_return_nothing_for_errors_and_non_success_statuses() {
    for fetcher in [
        Arc::new(|_: &ReleaseRequest, _: &AtomicBool| {
            Ok(ReleaseResponse {
                status: 503,
                body: "{}".into(),
            })
        }) as Arc<dyn ReleaseFetcher>,
        Arc::new(|_: &ReleaseRequest, _: &AtomicBool| Err("network down".into())),
    ] {
        let lookup = ReleaseLookup {
            fetcher,
            timeout: Duration::from_millis(100),
        };
        assert_eq!(
            fetch_channel_versions(WorkdeckInstallSource::Homebrew, &lookup),
            ChannelVersions::default()
        );
    }
}

#[test]
fn hung_release_lookup_is_cancelled_at_the_deadline() {
    let observed = Arc::new(AtomicBool::new(false));
    let worker_observed = Arc::clone(&observed);
    let lookup = ReleaseLookup {
        fetcher: Arc::new(move |_: &ReleaseRequest, cancelled: &AtomicBool| {
            while !cancelled.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
            worker_observed.store(true, Ordering::Release);
            Err("cancelled".into())
        }),
        timeout: Duration::from_millis(10),
    };
    let started = Instant::now();
    assert_eq!(
        fetch_channel_versions(WorkdeckInstallSource::Cargo, &lookup),
        ChannelVersions::default()
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    for _ in 0..10_000 {
        if observed.load(Ordering::Acquire) {
            break;
        }
        std::thread::yield_now();
    }
    assert!(observed.load(Ordering::Acquire));
}

#[derive(Clone)]
struct UpdateHarness {
    context: SelfUpdateContext,
    invocations: Arc<Mutex<Vec<UpdateInvocation>>>,
}

struct RecordingUpdateReporter {
    events: Arc<Mutex<Vec<String>>>,
}

impl UpdateReporter for RecordingUpdateReporter {
    fn stdout(&self, message: &str) {
        self.events
            .lock()
            .unwrap()
            .push(format!("stdout:{message}"));
    }

    fn stderr(&self, message: &str) {
        self.events
            .lock()
            .unwrap()
            .push(format!("stderr:{message}"));
    }
}

impl UpdateHarness {
    fn new(source: WorkdeckInstallSource, latest: &str) -> Self {
        let invocations = Arc::new(Mutex::new(Vec::new()));
        let capture = Arc::clone(&invocations);
        let latest = latest.to_owned();
        let fetcher = Arc::new(
            move |_: &ReleaseRequest, _: &AtomicBool| -> Result<ReleaseResponse, String> {
                Ok(ReleaseResponse {
                    status: 200,
                    body: json!({
                        "crate": {"max_stable_version":latest, "newest_version":latest},
                        "versions":{"stable":latest},
                        "tag_name":format!("v{latest}")
                    })
                    .to_string(),
                })
            },
        );
        let runner = Arc::new(move |invocation: &UpdateInvocation| {
            capture.lock().unwrap().push(invocation.clone());
            Ok(UpdateProcessResult {
                exit_code: 0,
                stderr: String::new(),
            })
        });
        Self {
            context: SelfUpdateContext {
                env: BTreeMap::new(),
                executable_path: "/usr/bin/workdeck".into(),
                installed_version: "1.0.0".into(),
                install_source: Some(source),
                platform: UpdatePlatform::Linux,
                architecture: "x86_64".into(),
                release_lookup: ReleaseLookup {
                    fetcher,
                    timeout: Duration::from_secs(1),
                },
                runner,
                reporter: Arc::new(SilentUpdateReporter),
            },
            invocations,
        }
    }

    fn run(&self, input: SelfUpdateCommandInput) -> Result<SelfUpdateResult, UpdateError> {
        run_self_update(&input, &self.context)
    }
}

fn update_input() -> SelfUpdateCommandInput {
    SelfUpdateCommandInput {
        version: None,
        method: None,
        check: false,
    }
}

#[test]
fn update_progress_is_reported_before_the_child_process_starts() {
    let mut harness = UpdateHarness::new(WorkdeckInstallSource::Cargo, "1.1.0");
    let events = Arc::new(Mutex::new(Vec::new()));
    harness.context.reporter = Arc::new(RecordingUpdateReporter {
        events: Arc::clone(&events),
    });
    let runner_events = Arc::clone(&events);
    harness.context.runner = Arc::new(move |_: &UpdateInvocation| {
        runner_events.lock().unwrap().push("run".into());
        Ok(UpdateProcessResult {
            exit_code: 0,
            stderr: String::new(),
        })
    });

    let result = harness.run(update_input()).unwrap();

    assert_eq!(result.exit_code, 0);
    assert_eq!(
        *events.lock().unwrap(),
        [
            "stdout:Updating workdeck 1.0.0 -> 1.1.0 with `cargo install workdeck-cli --version 1.1.0 --locked --force`\n",
            "run",
            "stdout:Updated workdeck to 1.1.0.\n",
        ]
    );
}

#[test]
fn update_method_parser_normalizes_every_supported_channel() {
    for (name, source) in [
        ("cargo", WorkdeckInstallSource::Cargo),
        ("brew", WorkdeckInstallSource::Homebrew),
        ("Homebrew", WorkdeckInstallSource::Homebrew),
        ("nix", WorkdeckInstallSource::Nix),
        ("CURL", WorkdeckInstallSource::Curl),
        ("pwsh", WorkdeckInstallSource::PowerShell),
        ("github", WorkdeckInstallSource::Direct),
    ] {
        assert_eq!(parse_update_method(name).unwrap(), source);
    }
}

#[test]
fn unknown_update_method_names_the_supported_values() {
    let error = parse_update_method("apt").unwrap_err();
    assert_eq!(error.message, "Unknown update method: apt");
    assert_eq!(
        error.suggestions,
        ["Supported methods are `cargo`, `brew`, `nix`, `curl`, `powershell`, and `direct`."]
    );
}

#[test]
fn update_version_parser_accepts_versions_and_rejects_package_specs() {
    for (input, expected) in [
        ("0.19.0", "0.19.0"),
        ("v1.2.3", "1.2.3"),
        ("1.2.3-beta.1", "1.2.3-beta.1"),
    ] {
        assert_eq!(parse_update_version(input).unwrap(), expected);
    }
    for value in [
        "latest",
        "^1.2.0",
        "crate:evil@1.0.0",
        "../local-dir",
        "1.2.3 --flag",
    ] {
        assert_eq!(
            parse_update_version(value).unwrap_err().message,
            format!("Invalid version: {value}")
        );
    }
}

#[test]
fn untagged_source_build_is_detected_without_injection() {
    let mut harness = UpdateHarness::new(WorkdeckInstallSource::Cargo, "1.1.0");
    harness.context.install_source = None;
    harness.context.installed_version = UNKNOWN_CLI_VERSION.into();
    harness.context.executable_path = "/opt/workdeck/workdeck".into();
    let result = harness.run(update_input()).unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.stdout.contains("local source build"));
    assert!(harness.invocations.lock().unwrap().is_empty());
}

#[test]
fn cargo_update_installs_the_newest_release() {
    let harness = UpdateHarness::new(WorkdeckInstallSource::Cargo, "1.1.0");
    let result = harness.run(update_input()).unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(
        harness.invocations.lock().unwrap()[0].command,
        [
            "cargo",
            "install",
            "workdeck-cli",
            "--version",
            "1.1.0",
            "--locked",
            "--force"
        ]
    );
    assert!(result.stdout.contains("Updating workdeck 1.0.0 -> 1.1.0"));
    assert!(result.stdout.contains("Updated workdeck to 1.1.0."));
}

#[test]
fn explicit_version_can_downgrade_cargo_install() {
    let mut harness = UpdateHarness::new(WorkdeckInstallSource::Cargo, "1.1.0");
    harness.context.installed_version = "1.1.0".into();
    let result = harness
        .run(SelfUpdateCommandInput {
            version: Some("0.9.0".into()),
            ..update_input()
        })
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(
        harness.invocations.lock().unwrap()[0]
            .command
            .windows(2)
            .any(|window| window == ["--version", "0.9.0"])
    );
}

#[test]
fn homebrew_upgrade_uses_formula_and_refuses_version_pin() {
    let harness = UpdateHarness::new(WorkdeckInstallSource::Homebrew, "1.1.0");
    harness.run(update_input()).unwrap();
    assert_eq!(
        harness.invocations.lock().unwrap()[0].command,
        ["brew", "upgrade", "workdeck"]
    );
    let error = harness
        .run(SelfUpdateCommandInput {
            version: Some("1.0.5".into()),
            ..update_input()
        })
        .unwrap_err();
    assert!(error.message.contains("Homebrew installs cannot select"));
}

#[test]
fn curl_update_downloads_then_executes_installer_with_version_env() {
    let mut harness = UpdateHarness::new(WorkdeckInstallSource::Curl, "1.1.0");
    harness.context.env = env(&[("PATH", "/usr/bin"), ("HOME", "/home/reviewer")]);
    harness.run(update_input()).unwrap();
    let invocation = &harness.invocations.lock().unwrap()[0];
    assert_eq!(&invocation.command[..2], ["sh", "-c"]);
    assert!(invocation.command[2].contains(CURL_INSTALL_SCRIPT_URL));
    assert!(invocation.command[2].contains("mktemp"));
    assert_eq!(
        invocation
            .env
            .as_ref()
            .and_then(|env| env.get(INSTALL_VERSION_ENV))
            .map(String::as_str),
        Some("1.1.0")
    );
}

#[test]
fn powershell_update_uses_downloaded_script_and_version_env() {
    let mut harness = UpdateHarness::new(WorkdeckInstallSource::PowerShell, "1.1.0");
    harness.context.platform = UpdatePlatform::Windows;
    harness.run(update_input()).unwrap();
    let invocation = &harness.invocations.lock().unwrap()[0];
    assert_eq!(invocation.command[0], "powershell.exe");
    assert!(
        invocation
            .command
            .last()
            .unwrap()
            .contains(POWERSHELL_INSTALL_SCRIPT_URL)
    );
    assert_eq!(
        invocation
            .env
            .as_ref()
            .and_then(|env| env.get(INSTALL_VERSION_ENV))
            .map(String::as_str),
        Some("1.1.0")
    );
}

#[test]
fn direct_update_selects_platform_archive_and_checksum() {
    let mut harness = UpdateHarness::new(WorkdeckInstallSource::Direct, "1.1.0");
    harness.context.platform = UpdatePlatform::Macos;
    harness.context.architecture = "aarch64".into();
    harness.context.executable_path = "/opt/workdeck/bin/workdeck".into();
    harness.run(update_input()).unwrap();
    let invocation = &harness.invocations.lock().unwrap()[0];
    let environment = invocation.env.as_ref().unwrap();
    assert!(
        environment["WORKDECK_DIRECT_ARCHIVE_URL"]
            .ends_with("/workdeck-aarch64-apple-darwin.tar.gz")
    );
    assert_eq!(
        environment["WORKDECK_DIRECT_BINARY"],
        "workdeck-aarch64-apple-darwin/workdeck"
    );
    assert!(invocation.command[2].contains("\"$WORKDECK_DIRECT_BINARY\""));
    assert!(environment["WORKDECK_DIRECT_CHECKSUM_URL"].ends_with(".sha256"));
    assert!(invocation.command[2].contains("sha256sum"));
    assert!(invocation.command[2].contains("mv -f"));
    assert!(invocation.command[2].contains("$WORKDECK_EXECUTABLE.new.$$"));
}

#[test]
fn direct_windows_update_defers_replacement_until_workdeck_exits() {
    let mut harness = UpdateHarness::new(WorkdeckInstallSource::Direct, "1.1.0");
    harness.context.platform = UpdatePlatform::Windows;
    harness.context.architecture = "x86_64".into();
    harness.context.executable_path = r"C:\Tools\workdeck.exe".into();
    harness.run(update_input()).unwrap();
    let invocation = &harness.invocations.lock().unwrap()[0];
    let script = invocation.command.last().unwrap();
    assert!(script.contains("Get-FileHash -Algorithm SHA256"));
    assert!(script.contains("Wait-Process -Id"));
    assert!(script.contains("Move-Item -Force"));
    assert!(
        invocation.env.as_ref().unwrap()["WORKDECK_DIRECT_ARCHIVE_URL"]
            .ends_with("/workdeck-x86_64-pc-windows-msvc.zip")
    );
    assert_eq!(
        invocation.env.as_ref().unwrap()["WORKDECK_DIRECT_BINARY"],
        "workdeck-x86_64-pc-windows-msvc/workdeck.exe"
    );
    assert!(script.contains("Join-Path $tmp $env:WORKDECK_DIRECT_BINARY"));
}

#[test]
fn check_reports_real_latest_without_installing() {
    let harness = UpdateHarness::new(WorkdeckInstallSource::Cargo, "1.1.0");
    let result = harness
        .run(SelfUpdateCommandInput {
            version: Some("0.0.1".into()),
            check: true,
            ..update_input()
        })
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(
        result
            .stdout
            .contains("workdeck 1.0.0 (installed with Cargo)")
    );
    assert!(result.stdout.contains("latest 1.1.0"));
    assert!(result.stdout.contains("requested 0.0.1"));
    assert!(result.stdout.contains("An update is available."));
    assert!(harness.invocations.lock().unwrap().is_empty());
}

#[test]
fn current_or_newer_install_does_not_spawn() {
    for installed in ["1.1.0", "1.2.0"] {
        let mut harness = UpdateHarness::new(WorkdeckInstallSource::Cargo, "1.1.0");
        harness.context.installed_version = installed.into();
        let result = harness.run(update_input()).unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(harness.invocations.lock().unwrap().is_empty());
        assert!(result.stdout.contains("already up to date"));
    }
}

#[test]
fn failed_update_preserves_process_stderr_and_exit_code() {
    let mut harness = UpdateHarness::new(WorkdeckInstallSource::Cargo, "1.1.0");
    harness.context.runner = Arc::new(|_: &UpdateInvocation| {
        Ok(UpdateProcessResult {
            exit_code: 7,
            stderr: "cargo: permission denied\n".into(),
        })
    });
    let result = harness.run(update_input()).unwrap();
    assert_eq!(result.exit_code, 7);
    assert!(result.stderr.contains("cargo: permission denied"));
    assert!(result.stderr.contains("failed with exit code 7"));
}

#[test]
fn missing_release_has_channel_specific_error() {
    let mut harness = UpdateHarness::new(WorkdeckInstallSource::Cargo, "1.1.0");
    harness.context.release_lookup.fetcher = Arc::new(
        |_: &ReleaseRequest, _: &AtomicBool| -> Result<ReleaseResponse, String> {
            Err("network down".into())
        },
    );
    let error = harness.run(update_input()).unwrap_err();
    assert_eq!(
        error.message,
        "Could not read the latest Workdeck version from crates.io."
    );
}

#[test]
fn explicit_method_overrides_detected_source() {
    let harness = UpdateHarness::new(WorkdeckInstallSource::Dev, "1.1.0");
    harness
        .run(SelfUpdateCommandInput {
            method: Some(WorkdeckInstallSource::Homebrew),
            ..update_input()
        })
        .unwrap();
    assert_eq!(
        harness.invocations.lock().unwrap()[0].command,
        ["brew", "upgrade", "workdeck"]
    );
}

#[test]
fn nix_and_source_builds_give_owned_update_guidance() {
    for (source, expected) in [
        (WorkdeckInstallSource::Nix, "installed with Nix"),
        (WorkdeckInstallSource::Dev, "local source build"),
    ] {
        let harness = UpdateHarness::new(source, "1.1.0");
        let result = harness.run(update_input()).unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.contains(expected));
        assert!(harness.invocations.lock().unwrap().is_empty());

        let checked = harness
            .run(SelfUpdateCommandInput {
                check: true,
                ..update_input()
            })
            .unwrap();
        assert_eq!(checked.exit_code, 0);
    }
}

#[test]
fn unmanaged_install_refuses_an_explicit_version() {
    let harness = UpdateHarness::new(WorkdeckInstallSource::Nix, "1.1.0");
    let error = harness
        .run(SelfUpdateCommandInput {
            version: Some("1.2.3".into()),
            ..update_input()
        })
        .unwrap_err();
    assert!(
        error
            .message
            .contains("cannot update to a specific version")
    );
}

// Narrow cases keep the upstream test declarations independently executable where Workdeck's
// native channels replace Hunk's npm, mise, and pacman vocabulary.

#[test]
fn declared_dev_install_source_is_accepted_independently() {
    let environment = env(&[(INSTALL_SOURCE_ENV, "dev")]);
    assert_eq!(
        detect_install_source(&facts(&environment, "/opt/workdeck/bin/workdeck", "1.2.3")),
        WorkdeckInstallSource::Dev
    );
}

#[test]
fn cargo_name_outside_cargo_bin_does_not_claim_the_executable() {
    assert_eq!(
        detect_install_source(&facts(
            &BTreeMap::new(),
            "/home/cargo/projects/workdeck/bin/workdeck",
            "1.2.3"
        )),
        WorkdeckInstallSource::Direct
    );
}

#[test]
fn unix_installer_layout_is_detected_independently() {
    assert_eq!(
        detect_install_source(&facts(
            &BTreeMap::new(),
            "/home/reviewer/.workdeck/bin/workdeck",
            "1.2.3"
        )),
        WorkdeckInstallSource::Curl
    );
}

#[test]
fn declared_curl_install_source_is_accepted_independently() {
    let environment = env(&[(INSTALL_SOURCE_ENV, "curl")]);
    assert_eq!(
        detect_install_source(&facts(&environment, "/opt/workdeck", "1.2.3")),
        WorkdeckInstallSource::Curl
    );
}

#[test]
fn declared_direct_install_source_replaces_unmanaged_package_channels() {
    let environment = env(&[(INSTALL_SOURCE_ENV, "direct")]);
    assert_eq!(
        detect_install_source(&facts(&environment, "/opt/workdeck", "1.2.3")),
        WorkdeckInstallSource::Direct
    );
}

#[test]
fn redirected_install_directory_is_classified_as_development() {
    let environment = env(&[(INSTALL_DIR_ENV, "/home/reviewer/tools/bin")]);
    assert_eq!(
        detect_install_source(&facts(
            &environment,
            "/home/reviewer/tools/bin/workdeck",
            "1.2.3"
        )),
        WorkdeckInstallSource::Dev
    );
}

#[test]
fn default_local_bin_is_classified_as_development() {
    assert_eq!(
        detect_install_source(&facts(
            &BTreeMap::new(),
            "/home/reviewer/.local/bin/workdeck",
            "1.2.3"
        )),
        WorkdeckInstallSource::Dev
    );
}

#[test]
fn unmatched_release_binary_falls_back_to_direct_github() {
    assert_eq!(
        detect_install_source(&facts(
            &BTreeMap::new(),
            "/usr/local/sbin/workdeck",
            "1.2.3"
        )),
        WorkdeckInstallSource::Direct
    );
}

#[test]
fn brew_method_alias_maps_only_to_homebrew() {
    assert_eq!(
        parse_update_method("brew").unwrap(),
        WorkdeckInstallSource::Homebrew
    );
    assert_eq!(
        parse_update_method("Homebrew").unwrap(),
        WorkdeckInstallSource::Homebrew
    );
}

#[test]
fn curl_method_alias_maps_only_to_curl_installer() {
    assert_eq!(
        parse_update_method("CURL").unwrap(),
        WorkdeckInstallSource::Curl
    );
}

#[test]
fn exact_release_versions_with_optional_v_are_accepted_independently() {
    assert_eq!(parse_update_version("0.19.0").unwrap(), "0.19.0");
    assert_eq!(parse_update_version("v1.2.3").unwrap(), "1.2.3");
    assert_eq!(
        parse_update_version("1.2.3-beta.1").unwrap(),
        "1.2.3-beta.1"
    );
}

#[test]
fn package_specs_are_rejected_independently() {
    for value in ["latest", "^1.2.0", "../local", "1.2.3 --flag"] {
        assert!(parse_update_version(value).is_err());
    }
}

#[test]
fn homebrew_upgrade_command_is_independently_verified() {
    let harness = UpdateHarness::new(WorkdeckInstallSource::Homebrew, "1.1.0");
    harness.run(update_input()).unwrap();
    assert_eq!(
        harness.invocations.lock().unwrap()[0].command,
        ["brew", "upgrade", "workdeck"]
    );
}

#[test]
fn homebrew_version_pin_refusal_is_independently_verified() {
    let harness = UpdateHarness::new(WorkdeckInstallSource::Homebrew, "1.1.0");
    assert!(
        harness
            .run(SelfUpdateCommandInput {
                version: Some("1.0.5".into()),
                ..update_input()
            })
            .is_err()
    );
}

#[test]
fn curl_check_reports_github_release_without_spawning() {
    let harness = UpdateHarness::new(WorkdeckInstallSource::Curl, "1.1.0");
    let result = harness
        .run(SelfUpdateCommandInput {
            check: true,
            ..update_input()
        })
        .unwrap();
    assert!(result.stdout.contains("latest 1.1.0"));
    assert!(result.stdout.contains("curl install script"));
    assert!(harness.invocations.lock().unwrap().is_empty());
}

#[test]
fn exactly_current_install_is_independently_a_noop() {
    let mut harness = UpdateHarness::new(WorkdeckInstallSource::Cargo, "1.1.0");
    harness.context.installed_version = "1.1.0".into();
    assert!(
        harness
            .run(update_input())
            .unwrap()
            .stdout
            .contains("already")
    );
    assert!(harness.invocations.lock().unwrap().is_empty());
}

#[test]
fn newer_than_channel_install_is_independently_a_noop() {
    let mut harness = UpdateHarness::new(WorkdeckInstallSource::Cargo, "1.1.0");
    harness.context.installed_version = "1.2.0".into();
    assert_eq!(harness.run(update_input()).unwrap().exit_code, 0);
    assert!(harness.invocations.lock().unwrap().is_empty());
}

#[test]
fn plain_check_reports_versions_without_spawning() {
    let harness = UpdateHarness::new(WorkdeckInstallSource::Cargo, "1.1.0");
    let result = harness
        .run(SelfUpdateCommandInput {
            check: true,
            ..update_input()
        })
        .unwrap();
    assert!(result.stdout.contains("latest 1.1.0"));
    assert!(result.stdout.contains("An update is available."));
    assert!(harness.invocations.lock().unwrap().is_empty());
}

#[test]
fn nix_guidance_is_independently_verified() {
    let harness = UpdateHarness::new(WorkdeckInstallSource::Nix, "1.1.0");
    let result = harness.run(update_input()).unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.stdout.contains("Nix configuration"));
}

#[test]
fn source_build_guidance_is_independently_verified() {
    let harness = UpdateHarness::new(WorkdeckInstallSource::Dev, "1.1.0");
    let result = harness.run(update_input()).unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.stdout.contains("cargo install --path"));
}

#[test]
fn unmanaged_check_succeeds_independently() {
    let harness = UpdateHarness::new(WorkdeckInstallSource::Nix, "1.1.0");
    assert_eq!(
        harness
            .run(SelfUpdateCommandInput {
                check: true,
                ..update_input()
            })
            .unwrap()
            .exit_code,
        0
    );
}
