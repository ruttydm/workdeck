use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use workdeck_examples::github_pr_extension::{
    GITHUB_PR_HELP, GitHubHttpRequest, GitHubHttpResponse, GitHubPrExtension, GitHubPrRuntime,
    GitHubPullRequestLocator, ResolvedGitHubPullRequest, fetch_github_pull_request_diff,
    parse_github_pr_invocation, parse_github_pull_request_locator, parse_github_remote_repository,
    read_git_origin, required_capabilities, resolve_github_pull_request,
};
use workdeck_extension_api::{
    Capability, CliCommandExecution, CliCommandInvocation, CliCommandResult, CliOutputStream,
    ExtensionManifest, Registration,
};
use workdeck_extension_host::LoadedExtension;

fn invocation(args: &[&str]) -> CliCommandInvocation {
    CliCommandInvocation {
        command_name: "gh".into(),
        args: args.iter().map(|value| (*value).into()).collect(),
        cwd: PathBuf::from("/repo"),
    }
}

fn response(status: u16, body: impl Into<Vec<u8>>) -> GitHubHttpResponse {
    GitHubHttpResponse {
        status,
        headers: BTreeMap::new(),
        body: Some(Box::new(Cursor::new(body.into()))),
    }
}

fn runtime(
    temporary_root: &Path,
    fetch: impl Fn(&GitHubHttpRequest) -> Result<GitHubHttpResponse, String> + Send + Sync + 'static,
    resolve_origin: impl Fn(
        &Path,
        &AtomicBool,
    )
        -> Result<String, workdeck_examples::github_pr_extension::GitHubPrUserError>
    + Send
    + Sync
    + 'static,
) -> GitHubPrRuntime {
    GitHubPrRuntime {
        fetch: Arc::new(fetch),
        environment: BTreeMap::new(),
        resolve_origin: Arc::new(resolve_origin),
        temporary_root: temporary_root.to_owned(),
    }
}

#[test]
fn accepts_numbers_repository_shorthands_urls_and_both_repo_option_forms() {
    let parsed = parse_github_pr_invocation(&["123".into()]).unwrap();
    assert_eq!(parsed.locator.number, "123");
    assert!(!parsed.help);

    let separated =
        parse_github_pr_invocation(&["--repo".into(), "modem-dev/hunk".into(), "123".into()])
            .unwrap();
    assert_eq!(
        separated.explicit_repository.as_deref(),
        Some("modem-dev/hunk")
    );
    let joined =
        parse_github_pr_invocation(&["123".into(), "--repo=modem-dev/hunk".into()]).unwrap();
    assert_eq!(
        joined.explicit_repository.as_deref(),
        Some("modem-dev/hunk")
    );
    let expected = GitHubPullRequestLocator {
        owner: Some("modem-dev".into()),
        repo: Some("hunk".into()),
        number: "123".into(),
    };
    assert_eq!(
        parse_github_pull_request_locator("modem-dev/hunk#123").unwrap(),
        expected
    );
    assert_eq!(
        parse_github_pull_request_locator("https://github.com/modem-dev/hunk/pull/123").unwrap(),
        expected
    );
}

#[test]
fn keeps_tokens_after_separator_for_delegated_patch_options_and_owns_help() {
    let parsed = parse_github_pr_invocation(&[
        "123".into(),
        "--repo".into(),
        "modem-dev/hunk".into(),
        "--".into(),
        "--pager".into(),
    ])
    .unwrap();
    assert_eq!(parsed.patch_args, ["--pager"]);
    assert!(parse_github_pr_invocation(&["--help".into()]).unwrap().help);
    assert!(
        parse_github_pr_invocation(&["123".into(), "-h".into(), "--".into(), "--pager".into()])
            .unwrap()
            .help
    );
}

#[test]
fn rejects_malformed_ambiguous_and_unsafe_invocations() {
    for args in [
        vec![],
        vec!["0"],
        vec!["01"],
        vec!["-1"],
        vec!["1.5"],
        vec!["1", "2"],
        vec!["1", "--unknown"],
        vec!["1", "--repo"],
        vec!["1", "--repo", "a/b", "--repo", "c/d"],
        vec!["a/b#1", "--repo", "a/b"],
    ] {
        let args = args.into_iter().map(String::from).collect::<Vec<_>>();
        assert!(parse_github_pr_invocation(&args).is_err(), "{args:?}");
    }
    assert!(parse_github_pull_request_locator("9007199254740992").is_err());

    for locator in [
        "owner/repo/extra#1",
        "owner%2Frepo/name#1",
        "https://gitlab.com/owner/repo/pull/1",
        "http://github.com/owner/repo/pull/1",
        "https://user@github.com/owner/repo/pull/1",
        "https://github.com/owner/repo/pull/1/files",
        "https://github.com/owner/repo/pull/1?diff=1",
        "https://github.com/owner/repo/pull/1#discussion",
    ] {
        assert!(
            parse_github_pull_request_locator(locator).is_err(),
            "{locator}"
        );
    }
}

#[test]
fn parses_common_github_remotes_and_rejects_malformed_ones() {
    for remote in [
        "https://github.com/modem-dev/hunk.git",
        "ssh://git@github.com/modem-dev/hunk.git",
        "git://github.com/modem-dev/hunk.git",
        "git@github.com:modem-dev/hunk.git",
        "git@GITHUB.COM:modem-dev/hunk.GiT",
    ] {
        assert_eq!(
            parse_github_remote_repository(remote),
            Some(("modem-dev".into(), "hunk".into())),
            "{remote}"
        );
    }
    for remote in [
        "https://gitlab.com/modem-dev/hunk.git",
        "https://github.com/modem-dev/hunk/extra.git",
        "git@github.com:modem-dev.git",
        "a@b@github.com:modem-dev/hunk.git",
        "not a remote",
    ] {
        assert_eq!(parse_github_remote_repository(remote), None, "{remote}");
    }
}

#[test]
fn rejects_an_already_cancelled_origin_lookup_before_spawning_git() {
    let cancelled = AtomicBool::new(true);
    assert!(
        read_git_origin(Path::new("/path/that/must/not/be-read"), &cancelled)
            .unwrap_err()
            .message
            .contains("cancelled")
    );
}

#[test]
fn uses_the_injected_origin_only_for_a_bare_number() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let tracked = Arc::clone(&calls);
    let resolver = move |cwd: &Path, cancelled: &AtomicBool| {
        assert!(!cancelled.load(Ordering::Acquire));
        tracked.lock().unwrap().push(cwd.to_owned());
        Ok("git@github.com:modem-dev/hunk.git".into())
    };
    let cancelled = AtomicBool::new(false);
    let bare = resolve_github_pull_request(
        &parse_github_pr_invocation(&["123".into()]).unwrap(),
        Path::new("/repo"),
        &cancelled,
        &resolver,
    )
    .unwrap();
    assert_eq!(
        bare,
        ResolvedGitHubPullRequest {
            owner: "modem-dev".into(),
            repo: "hunk".into(),
            number: "123".into(),
        }
    );
    let explicit = resolve_github_pull_request(
        &parse_github_pr_invocation(&["modem-dev/hunk#124".into()]).unwrap(),
        Path::new("/elsewhere"),
        &cancelled,
        &resolver,
    )
    .unwrap();
    assert_eq!(explicit.number, "124");
    assert_eq!(*calls.lock().unwrap(), [PathBuf::from("/repo")]);
}

#[test]
fn sends_the_exact_diff_request_with_gh_token_precedence() {
    let captured = Arc::new(Mutex::new(None));
    let output = Arc::clone(&captured);
    let fetch = move |request: &GitHubHttpRequest| {
        *output.lock().unwrap() = Some(request.clone());
        Ok(response(200, b"diff --git a/a.ts b/a.ts\n".to_vec()))
    };
    let environment = BTreeMap::from([
        ("GH_TOKEN".into(), "preferred".into()),
        ("GITHUB_TOKEN".into(), "fallback".into()),
    ]);
    let target = ResolvedGitHubPullRequest {
        owner: "modem-dev".into(),
        repo: "hunk".into(),
        number: "123".into(),
    };
    let bytes = fetch_github_pull_request_diff(
        &target,
        &Arc::new(AtomicBool::new(false)),
        &environment,
        Arc::new(fetch),
    )
    .unwrap();
    let request = captured.lock().unwrap().clone().unwrap();
    assert_eq!(
        request.url,
        "https://api.github.com/repos/modem-dev/hunk/pulls/123"
    );
    assert!(request.redirect_manual);
    assert_eq!(
        request.headers.get("Accept").map(String::as_str),
        Some("application/vnd.github.v3.diff")
    );
    assert_eq!(
        request.headers.get("Authorization").map(String::as_str),
        Some("Bearer preferred")
    );
    assert!(String::from_utf8(bytes).unwrap().contains("diff --git"));
}

#[test]
fn expected_header_http_and_network_errors_remain_credential_safe() {
    let target = ResolvedGitHubPullRequest {
        owner: "private".into(),
        repo: "repo".into(),
        number: "7".into(),
    };
    let fetches = Arc::new(AtomicUsize::new(0));
    let malformed_fetches = Arc::clone(&fetches);
    let malformed = fetch_github_pull_request_diff(
        &target,
        &Arc::new(AtomicBool::new(false)),
        &BTreeMap::from([("GH_TOKEN".into(), "top-secret\nvalue".into())]),
        Arc::new(move |_| {
            malformed_fetches.fetch_add(1, Ordering::Relaxed);
            Ok(response(200, b"unexpected".to_vec()))
        }),
    )
    .unwrap_err();
    assert!(
        malformed
            .message
            .contains("cannot be sent in an HTTP header")
    );
    assert!(!malformed.message.contains("top-secret"));
    assert!(!malformed.message.contains("value"));
    assert_eq!(fetches.load(Ordering::Relaxed), 0);

    for (status, rate_limited, expected) in [
        (401, false, "rejected the configured token"),
        (403, true, "rate limiting blocked"),
        (403, false, "denied access"),
        (404, false, "could not find"),
        (500, false, "HTTP 500"),
        (302, false, "redirected"),
    ] {
        let error = fetch_github_pull_request_diff(
            &target,
            &Arc::new(AtomicBool::new(false)),
            &BTreeMap::from([("GH_TOKEN".into(), "top-secret-token".into())]),
            Arc::new(move |_| {
                let mut response = response(status, b"secret response body".to_vec());
                if rate_limited {
                    response
                        .headers
                        .insert("X-RateLimit-Remaining".into(), "0".into());
                }
                Ok(response)
            }),
        )
        .unwrap_err();
        assert!(!error.message.contains("top-secret-token"));
        assert!(!error.message.contains("secret response body"));
        assert!(error.message.contains(expected), "{}", error.message);
    }

    let network = fetch_github_pull_request_diff(
        &target,
        &Arc::new(AtomicBool::new(false)),
        &BTreeMap::new(),
        Arc::new(|_| Err("network internals and credentials".into())),
    )
    .unwrap_err();
    assert!(network.message.contains("could not be reached"));
    assert!(!network.message.contains("network internals"));
}

#[test]
fn cooperative_cancellation_returns_while_a_native_fetch_is_still_blocked() {
    let target = ResolvedGitHubPullRequest {
        owner: "modem-dev".into(),
        repo: "hunk".into(),
        number: "1".into(),
    };
    let cancelled = Arc::new(AtomicBool::new(false));
    let barrier = Arc::new(Barrier::new(2));
    let worker_barrier = Arc::clone(&barrier);
    let fetch = Arc::new(move |_: &GitHubHttpRequest| {
        worker_barrier.wait();
        thread::sleep(Duration::from_millis(500));
        Ok(response(200, b"diff --git a/a b/a\n".to_vec()))
    });
    let trigger_cancelled = Arc::clone(&cancelled);
    thread::spawn(move || {
        barrier.wait();
        trigger_cancelled.store(true, Ordering::Release);
    });

    let started = Instant::now();
    let error =
        fetch_github_pull_request_diff(&target, &cancelled, &BTreeMap::new(), fetch).unwrap_err();
    assert!(started.elapsed() < Duration::from_millis(200));
    assert!(error.message.contains("cancelled"));
}

#[test]
fn fetches_delegates_and_removes_private_temporary_patch_on_shutdown() {
    let temporary_root = TempDir::new().unwrap();
    let patch = b"diff --git a/src/pr.ts b/src/pr.ts\n--- a/src/pr.ts\n+++ b/src/pr.ts\n";
    let extension = GitHubPrExtension::new(runtime(
        temporary_root.path(),
        move |_| Ok(response(200, patch.to_vec())),
        |_, _| Ok("git@github.com:modem-dev/hunk.git".into()),
    ));
    let registration = extension.register();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let result = extension
        .execute(
            &invocation(&["123", "--", "--pager"]),
            &Arc::new(AtomicBool::new(false)),
            |stream, bytes| {
                match stream {
                    CliOutputStream::Stdout => stdout.extend_from_slice(bytes),
                    CliOutputStream::Stderr => stderr.extend_from_slice(bytes),
                }
                Ok(())
            },
        )
        .unwrap();
    let CliCommandResult::Delegate { argv } = result.result else {
        panic!("expected patch delegation")
    };
    assert_eq!(argv[0], "patch");
    assert_eq!(&argv[2..], ["--pager"]);
    let patch_path = PathBuf::from(&argv[1]);
    assert_eq!(fs::read(&patch_path).unwrap(), patch);
    assert!(stdout.is_empty());
    assert!(!result.stdin_read_started);
    assert!(!result.stdin_consumed);
    assert!(
        String::from_utf8(stderr)
            .unwrap()
            .contains("Fetching GitHub pull request modem-dev/hunk#123")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(patch_path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&patch_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    drop(registration);
    assert!(!patch_path.exists());
}

#[test]
fn cancels_after_temporary_input_creation_without_returning_a_delegate() {
    let temporary_root = TempDir::new().unwrap();
    let extension = GitHubPrExtension::new(runtime(
        temporary_root.path(),
        |_| Ok(response(200, b"diff --git a/a b/a\n".to_vec())),
        |_, _| Ok("unused".into()),
    ));
    let _registration = extension.register();
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut stderr_writes = 0;
    let error = extension
        .execute(
            &invocation(&["1", "--repo", "owner/repo"]),
            &cancelled,
            |stream, _| {
                if stream == CliOutputStream::Stderr {
                    stderr_writes += 1;
                    if stderr_writes == 2 {
                        cancelled.store(true, Ordering::Release);
                    }
                }
                Ok(())
            },
        )
        .unwrap_err();
    assert!(error.message.contains("cancelled"));
    assert_eq!(fs::read_dir(temporary_root.path()).unwrap().count(), 0);
}

#[test]
fn replacement_registry_keeps_delegated_patch_until_the_last_registry_retires() {
    let temporary_root = TempDir::new().unwrap();
    let extension = GitHubPrExtension::new(runtime(
        temporary_root.path(),
        |_| Ok(response(200, b"diff --git a/a b/a\n".to_vec())),
        |_, _| Ok("unused".into()),
    ));
    let first = extension.register();
    let CliCommandExecution {
        result: CliCommandResult::Delegate { argv },
        ..
    } = extension
        .execute(
            &invocation(&["1", "--repo", "owner/repo"]),
            &Arc::new(AtomicBool::new(false)),
            |_, _| Ok(()),
        )
        .unwrap()
    else {
        panic!("expected patch delegation")
    };
    let path = PathBuf::from(&argv[1]);
    let replacement = extension.register();
    drop(first);
    assert!(path.exists());
    drop(replacement);
    assert!(!path.exists());
}

#[test]
fn owns_extension_help_without_reading_git_network_or_stdin() {
    let temporary_root = TempDir::new().unwrap();
    let origin_reads = Arc::new(AtomicUsize::new(0));
    let fetches = Arc::new(AtomicUsize::new(0));
    let tracked_origin = Arc::clone(&origin_reads);
    let tracked_fetches = Arc::clone(&fetches);
    let extension = GitHubPrExtension::new(runtime(
        temporary_root.path(),
        move |_| {
            tracked_fetches.fetch_add(1, Ordering::Relaxed);
            Err("unexpected".into())
        },
        move |_, _| {
            tracked_origin.fetch_add(1, Ordering::Relaxed);
            Ok(String::new())
        },
    ));
    let mut output = Vec::new();
    let result = extension
        .execute(
            &invocation(&["--help"]),
            &Arc::new(AtomicBool::new(false)),
            |stream, bytes| {
                assert_eq!(stream, CliOutputStream::Stdout);
                output.extend_from_slice(bytes);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(result.result, CliCommandResult::Exit { code: 0 });
    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("Usage: workdeck gh")
    );
    assert_eq!(GITHUB_PR_HELP.matches("Usage: workdeck gh").count(), 1);
    assert_eq!(origin_reads.load(Ordering::Relaxed), 0);
    assert_eq!(fetches.load(Ordering::Relaxed), 0);
    assert!(!result.stdin_read_started);
    assert!(!result.stdin_consumed);
}

fn staged_extension() -> (TempDir, PathBuf) {
    let directory = TempDir::new().unwrap();
    let binary_directory = directory.path().join("bin");
    fs::create_dir_all(&binary_directory).unwrap();
    let binary_name = format!(
        "workdeck-example-github-pr-extension{}",
        std::env::consts::EXE_SUFFIX
    );
    fs::copy(
        env!("CARGO_BIN_EXE_workdeck-example-github-pr-extension"),
        binary_directory.join(binary_name),
    )
    .unwrap();
    let manifest = directory.path().join("workdeck-extension.toml");
    fs::write(
        &manifest,
        include_str!("../extensions/github-pr/workdeck-extension.toml"),
    )
    .unwrap();
    (directory, manifest)
}

#[test]
fn compiled_api_v1_extension_manifest_handshake_help_and_shutdown_are_native() {
    assert_eq!(
        required_capabilities(),
        [Capability::CliCommands, Capability::Events]
    );
    let (_directory, manifest_path) = staged_extension();
    let manifest = ExtensionManifest::load(&manifest_path).unwrap();
    assert_eq!(manifest.api_version, 1);
    assert_eq!(manifest.capabilities, required_capabilities());
    assert!(manifest.executable.to_string_lossy().contains("github-pr"));

    let mut loaded = LoadedExtension::spawn(&manifest_path, "test-host").unwrap();
    assert!(loaded.handshake.registrations.iter().any(|registration| {
        matches!(
            registration,
            Registration::CliCommand(command)
                if command.name == "gh"
                    && command.summary == "Review a GitHub pull request"
                    && command.usage.as_deref()
                        == Some("<number|owner/repo#number|pull-request-url> [--repo <owner/repo>]")
        )
    }));
    assert!(loaded.handshake.registrations.iter().any(|registration| {
        matches!(
            registration,
            Registration::EventSubscription { names } if names == &["shutdown"]
        )
    }));
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let result = loaded
        .invoke_cli_command(
            "gh",
            vec!["--help".into()],
            Path::new("/repo"),
            Duration::from_secs(1),
            &mut stdout,
            &mut stderr,
        )
        .unwrap();
    assert_eq!(result.result, CliCommandResult::Exit { code: 0 });
    assert!(
        String::from_utf8(stdout)
            .unwrap()
            .contains("Usage: workdeck gh")
    );
    assert!(stderr.is_empty());
    // Drop exercises the subscribed graceful-shutdown transport; forced termination remains the
    // bounded fallback for non-cooperating native extensions.
    drop(loaded);
}

#[test]
fn rust_projection_matches_the_executed_baseline_oracle_and_stable_absence() {
    let oracle: serde_json::Value =
        serde_json::from_str(include_str!("../../port/hunk/oracles/github-pr.json")).unwrap();
    assert_eq!(
        oracle["capture"]["baseline"],
        "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
    );
    assert_eq!(
        oracle["capture"]["stable"],
        "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"
    );
    assert!(
        oracle["capture"]["stableDisposition"]
            .as_str()
            .unwrap()
            .contains("absent")
    );
    assert_eq!(
        oracle["request"]["url"],
        "https://api.github.com/repos/modem-dev/hunk/pulls/123"
    );
    assert_eq!(oracle["request"]["redirect"], "manual");
    assert_eq!(
        oracle["lifecycle"]["result"]["argv"],
        serde_json::json!(["patch", "<TEMP_PATCH>", "--pager"])
    );
    assert_eq!(oracle["lifecycle"]["stdinReads"], 0);
    assert_eq!(oracle["lifecycle"]["existsAfterShutdown"], false);
    assert_eq!(
        oracle["help"]
            .as_str()
            .unwrap()
            .replace("hunk gh", "workdeck gh"),
        GITHUB_PR_HELP
    );
}
