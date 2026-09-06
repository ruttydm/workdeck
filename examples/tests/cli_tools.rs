use std::fs;
use std::io::{self, Cursor, Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use workdeck_extension_api::{CliCommandResult, Registration};
use workdeck_extension_host::{ExtensionCliStdin, HostError, LoadedExtension};

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
        Some("<status|review|stdin> [args...]")
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

struct PanicRead;

impl Read for PanicRead {
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        panic!("an extension that does not request stdin must not claim it")
    }
}

#[derive(Default)]
struct FailFirstWriter {
    writes: Vec<Vec<u8>>,
}

impl Write for FailFirstWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.writes.push(bytes.to_vec());
        if self.writes.len() == 1 {
            Err(io::Error::other("first write failed"))
        } else {
            Ok(bytes.len())
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn compiled_extension_leases_stdin_lazily_and_tracks_host_owned_consumption() {
    let (_directory, manifest) = staged_extension();
    let mut extension = LoadedExtension::spawn(&manifest, "test-host").unwrap();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let untouched = extension
        .invoke_cli_command_with_input(
            "cli-tools",
            vec!["status".into()],
            std::path::Path::new("/tmp/work deck"),
            Duration::from_secs(1),
            &mut PanicRead,
            &mut stdout,
            &mut stderr,
        )
        .unwrap();
    assert!(!untouched.stdin_read_started);
    assert!(!untouched.stdin_consumed);

    stdout.clear();
    let mut input = Cursor::new(vec![0, 1, 2, 0xff]);
    let copied = extension
        .invoke_cli_command_with_input(
            "cli-tools",
            vec!["stdin".into()],
            std::path::Path::new("/tmp/work deck"),
            Duration::from_secs(1),
            &mut input,
            &mut stdout,
            &mut stderr,
        )
        .unwrap();
    assert_eq!(copied.result, CliCommandResult::Exit { code: 7 });
    assert!(copied.stdin_read_started);
    assert!(copied.stdin_consumed);
    assert_eq!(stdout, [0, 1, 2, 0xff]);

    let touched = extension
        .invoke_cli_command_with_input(
            "cli-tools",
            vec!["touch-stdin".into()],
            std::path::Path::new("/tmp/work deck"),
            Duration::from_secs(1),
            &mut Cursor::new(Vec::<u8>::new()),
            &mut Vec::new(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(touched.to_string().contains("read stdin before delegating"));
}

#[derive(Default)]
struct NeverReadyStdin {
    polls: usize,
}

impl ExtensionCliStdin for NeverReadyStdin {
    fn try_read(&mut self, _max_bytes: usize) -> io::Result<Option<Vec<u8>>> {
        self.polls += 1;
        Ok(None)
    }
}

#[test]
fn pending_stdin_read_is_revoked_when_the_command_exits_without_consuming_input() {
    let (_directory, manifest) = staged_extension();
    let mut extension = LoadedExtension::spawn(&manifest, "test-host").unwrap();
    let mut stdin = NeverReadyStdin::default();
    let started = Instant::now();
    let execution = extension
        .invoke_cli_command_cancellable_with_stdin(
            "cli-tools",
            vec!["pending-stdin".into()],
            std::path::Path::new("/tmp/work deck"),
            Duration::from_secs(1),
            &AtomicBool::new(false),
            &mut stdin,
            &mut Vec::new(),
            &mut Vec::new(),
        )
        .unwrap();

    assert!(started.elapsed() < Duration::from_millis(500));
    assert_eq!(execution.result, CliCommandResult::Exit { code: 0 });
    assert!(execution.stdin_read_started);
    assert!(!execution.stdin_consumed);
    assert!(stdin.polls > 0);
}

#[test]
fn host_drains_all_accepted_output_before_reporting_the_first_failure() {
    let (_directory, manifest) = staged_extension();
    let mut extension = LoadedExtension::spawn(&manifest, "test-host").unwrap();
    let mut stdout = FailFirstWriter::default();
    let error = extension
        .invoke_cli_command(
            "cli-tools",
            vec!["write-twice".into()],
            std::path::Path::new("."),
            Duration::from_secs(1),
            &mut stdout,
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("first write failed"));
    assert_eq!(stdout.writes, [b"first".to_vec(), b"second".to_vec()]);
}

#[test]
fn revoked_late_output_cannot_reach_the_terminal_or_poison_the_next_request() {
    let (_directory, manifest) = staged_extension();
    let mut extension = LoadedExtension::spawn(&manifest, "test-host").unwrap();
    let late = extension
        .invoke_cli_command(
            "cli-tools",
            vec!["late-output".into()],
            std::path::Path::new("/tmp/work deck"),
            Duration::from_secs(1),
            &mut Vec::new(),
            &mut Vec::new(),
        )
        .unwrap();
    assert_eq!(late.result, CliCommandResult::Exit { code: 0 });
    thread::sleep(Duration::from_millis(20));

    let mut stdout = Vec::new();
    let next = extension
        .invoke_cli_command(
            "cli-tools",
            vec!["status".into()],
            std::path::Path::new("/tmp/work deck"),
            Duration::from_secs(1),
            &mut stdout,
            &mut Vec::new(),
        )
        .unwrap();
    assert_eq!(next.result, CliCommandResult::Exit { code: 0 });
    assert_eq!(stdout, b"cli-tools is ready in /tmp/work deck\n");
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
