use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use workdeck_extension_api::{CliCommandResult, Registration};
use workdeck_extension_host::{HostError, LoadedExtension};

fn staged_extension() -> (TempDir, std::path::PathBuf) {
    let directory = TempDir::new().unwrap();
    let binary_directory = directory.path().join("bin");
    fs::create_dir_all(&binary_directory).unwrap();
    let binary_name = format!(
        "workdeck-example-cli-tools-extension{}",
        std::env::consts::EXE_SUFFIX
    );
    let source = env!("CARGO_BIN_EXE_workdeck-example-cli-tools-extension");
    fs::copy(source, binary_directory.join(binary_name)).unwrap();
    let manifest = directory.path().join("workdeck-extension.toml");
    fs::write(
        &manifest,
        include_str!("../extensions/cli-tools/workdeck-extension.toml"),
    )
    .unwrap();
    (directory, manifest)
}

#[test]
fn compiled_extension_handshakes_streams_and_preserves_raw_delegation_args() {
    let (_directory, manifest) = staged_extension();
    let mut extension = LoadedExtension::spawn(&manifest, "test-host").unwrap();
    let registration = extension
        .handshake
        .registrations
        .iter()
        .find_map(|registration| match registration {
            Registration::CliCommand(command) => Some(command),
            _ => None,
        })
        .unwrap();
    assert_eq!(registration.name, "cli-tools");
    assert_eq!(
        registration.usage.as_deref(),
        Some("<status|review> [args...]")
    );

    let cwd = std::path::Path::new("/tmp/work deck");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let status = extension
        .invoke_cli_command(
            "cli-tools",
            vec!["status".into()],
            cwd,
            Duration::from_secs(1),
            &mut stdout,
            &mut stderr,
        )
        .unwrap();
    assert_eq!(status.result, CliCommandResult::Exit { code: 0 });
    assert_eq!(stdout, b"cli-tools is ready in /tmp/work deck\n");
    assert!(stderr.is_empty());

    stdout.clear();
    let started = Instant::now();
    let review = extension
        .invoke_cli_command(
            "cli-tools",
            vec![
                "review".into(),
                "--".into(),
                "-leading".into(),
                "two words".into(),
            ],
            cwd,
            Duration::from_secs(1),
            &mut stdout,
            &mut stderr,
        )
        .unwrap();
    assert!(started.elapsed() >= Duration::from_millis(95));
    assert!(stdout.is_empty());
    assert_eq!(stderr, "Preparing review input…\n".as_bytes());
    assert_eq!(
        review.result,
        CliCommandResult::Delegate {
            argv: vec![
                "diff".into(),
                "--".into(),
                "-leading".into(),
                "two words".into()
            ]
        }
    );
}

#[test]
fn compiled_extension_reports_user_errors_and_cooperative_cancellation() {
    let (_directory, manifest) = staged_extension();
    let mut extension = LoadedExtension::spawn(&manifest, "test-host").unwrap();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let invalid = extension
        .invoke_cli_command(
            "cli-tools",
            vec!["wat".into()],
            std::path::Path::new("."),
            Duration::from_secs(1),
            &mut stdout,
            &mut stderr,
        )
        .unwrap_err();
    assert!(matches!(invalid, HostError::Remote { .. }));
    assert!(invalid.to_string().contains("Choose a cli-tools action."));
    assert!(
        invalid
            .to_string()
            .contains("Run `workdeck cli-tools status` or `workdeck cli-tools review`.")
    );

    let cancelled = Arc::new(AtomicBool::new(false));
    let trigger = Arc::clone(&cancelled);
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(10));
        trigger.store(true, Ordering::Release);
    });
    let started = Instant::now();
    let cancelled_result = extension
        .invoke_cli_command_cancellable(
            "cli-tools",
            vec!["review".into()],
            std::path::Path::new("."),
            Duration::from_secs(1),
            &cancelled,
            &mut stdout,
            &mut stderr,
        )
        .unwrap_err();
    assert!(started.elapsed() < Duration::from_millis(90));
    assert!(cancelled_result.to_string().contains("interrupted"));
    assert_eq!(stderr, "Preparing review input…\n".as_bytes());
}

#[test]
fn declared_capability_matches_the_manifest_and_registration() {
    assert_eq!(
        workdeck_examples::cli_tools_extension::required_capabilities(),
        [workdeck_extension_api::Capability::CliCommands]
    );
}
