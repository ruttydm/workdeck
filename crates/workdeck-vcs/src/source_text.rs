//! Bounded source readers used by VCS snapshots and source expansion.

use std::fs;
use std::io::{self, Read};
use std::path::Path;
use std::process::{Child, Command, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

pub const DEFAULT_SOURCE_TEXT_MAX_BYTES: usize = 1_000_000;
const SOURCE_SUBPROCESS_GRACE: Duration = Duration::from_millis(100);
const SOURCE_SUBPROCESS_FORCE_WAIT: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LimitedSourceTextResult {
    Text(String),
    Missing,
    TooLarge { max_bytes: usize },
}

#[derive(Debug, thiserror::Error)]
pub enum SourceTextError {
    #[error("source text exceeds {max_bytes} bytes")]
    TooLarge { max_bytes: usize },
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Bound source bytes even if the file grows after its metadata is inspected.
/// UTF-8 replacement decoding and the fixed read buffer have separate overhead.
pub fn read_file_text_with_limit(path: &Path, max_bytes: usize) -> LimitedSourceTextResult {
    let read = || -> Result<String, SourceTextError> {
        let file = fs::File::open(path)?;
        let length = file.metadata()?.len();
        read_sized_source_with_limit(file, length, max_bytes)
    };
    match read() {
        Ok(text) => LimitedSourceTextResult::Text(text),
        Err(SourceTextError::TooLarge { max_bytes }) => {
            LimitedSourceTextResult::TooLarge { max_bytes }
        }
        Err(SourceTextError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
            LimitedSourceTextResult::Missing
        }
        Err(SourceTextError::Io(error)) => {
            log_source_diagnostic(
                &format!("failed to read source file {}", path.display()),
                &error,
            );
            LimitedSourceTextResult::Missing
        }
    }
}

fn read_sized_source_with_limit(
    source: impl Read,
    observed_length: u64,
    max_bytes: usize,
) -> Result<String, SourceTextError> {
    if observed_length > max_bytes as u64 {
        return Err(SourceTextError::TooLarge { max_bytes });
    }
    // Metadata permits an early rejection, never an unbounded subsequent read.
    read_stream_text_with_limit(Some(source), max_bytes, None)
}

/// Read an optional byte stream while enforcing a caller-defined resource limit.
pub fn read_stream_text_with_limit<R: Read>(
    stream: Option<R>,
    max_bytes: usize,
    mut on_too_large: Option<&mut dyn FnMut()>,
) -> Result<String, SourceTextError> {
    let Some(mut stream) = stream else {
        return Ok(String::new());
    };
    let mut bytes = Vec::with_capacity(max_bytes.min(64 * 1024));
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let bytes_read = match stream.read(&mut buffer) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if bytes_read == 0 {
            break;
        }
        if bytes.len().saturating_add(bytes_read) > max_bytes {
            if let Some(callback) = &mut on_too_large {
                callback();
            }
            return Err(SourceTextError::TooLarge { max_bytes });
        }
        bytes.extend_from_slice(&buffer[..bytes_read]);
    }
    // Bun.file().text() and TextDecoder both consume one initial UTF-8 BOM.
    // Check the byte ceiling first: a BOM still counts toward source size.
    let decoded_bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
    Ok(String::from_utf8_lossy(decoded_bytes).into_owned())
}

pub fn log_source_diagnostic(message: &str, detail: &dyn std::fmt::Display) {
    let first_line = detail
        .to_string()
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned);
    if let Some(detail) = first_line {
        eprintln!("workdeck: {message}: {detail}");
    } else {
        eprintln!("workdeck: {message}");
    }
}

/// Minimal process surface that makes bounded TERM/KILL cleanup independently testable.
pub trait SourceSubprocess {
    fn request_terminate(&mut self) -> io::Result<()>;
    fn force_kill(&mut self) -> io::Result<()>;
    fn try_wait_for_exit(&mut self) -> io::Result<Option<ExitStatus>>;
    fn finish_cleanup(&mut self) {}
}

/// A source command and its explicitly owned Unix process group. Killing only
/// the shell can leave descendants holding the reader pipes open indefinitely.
pub(crate) struct OwnedSourceSubprocess {
    child: Child,
    #[cfg(unix)]
    group: libc::pid_t,
}

pub(crate) fn spawn_source_subprocess(command: &mut Command) -> io::Result<OwnedSourceSubprocess> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let child = command.spawn()?;
    Ok(OwnedSourceSubprocess {
        #[cfg(unix)]
        group: child.id() as libc::pid_t,
        child,
    })
}

impl std::ops::Deref for OwnedSourceSubprocess {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.child
    }
}

impl std::ops::DerefMut for OwnedSourceSubprocess {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.child
    }
}

impl SourceSubprocess for OwnedSourceSubprocess {
    fn request_terminate(&mut self) -> io::Result<()> {
        #[cfg(unix)]
        {
            // SAFETY: spawn assigned this child a new group whose ID is its PID.
            if unsafe { libc::kill(-self.group, libc::SIGTERM) } == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        }
        #[cfg(not(unix))]
        self.child.kill()
    }

    fn force_kill(&mut self) -> io::Result<()> {
        self.finish_cleanup();
        self.child.kill()
    }

    fn try_wait_for_exit(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    fn finish_cleanup(&mut self) {
        #[cfg(unix)]
        // SAFETY: this is only the group explicitly created for this source command.
        // A descendant can retain both pipe descriptors after the parent exits.
        unsafe {
            libc::kill(-self.group, libc::SIGKILL);
        }
    }
}

impl SourceSubprocess for Child {
    fn request_terminate(&mut self) -> io::Result<()> {
        request_child_terminate(self)
    }

    fn force_kill(&mut self) -> io::Result<()> {
        self.kill()
    }

    fn try_wait_for_exit(&mut self) -> io::Result<Option<ExitStatus>> {
        self.try_wait()
    }
}

/// Terminate a source-reader subprocess without allowing cleanup to block indefinitely.
pub fn terminate_source_subprocess(process: &mut impl SourceSubprocess) {
    let _ = process.request_terminate();
    if wait_for_source_subprocess_exit(process, SOURCE_SUBPROCESS_GRACE) {
        process.finish_cleanup();
        return;
    }
    let _ = process.force_kill();
    let _ = wait_for_source_subprocess_exit(process, SOURCE_SUBPROCESS_FORCE_WAIT);
}

fn wait_for_source_subprocess_exit(process: &mut impl SourceSubprocess, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        match process.try_wait_for_exit() {
            Ok(Some(_)) | Err(_) => return true,
            Ok(None) if Instant::now() >= deadline => return false,
            Ok(None) => thread::sleep(Duration::from_millis(5)),
        }
    }
}

#[cfg(unix)]
fn request_child_terminate(child: &mut Child) -> io::Result<()> {
    let result = unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(unix))]
fn request_child_terminate(child: &mut Child) -> io::Result<()> {
    child.kill()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tempfile::TempDir;

    #[test]
    fn returns_text_missing_and_structural_too_large_results() {
        let directory = TempDir::new().unwrap();
        let source = directory.path().join("source.txt");
        fs::write(&source, "source text\n").unwrap();
        assert_eq!(
            read_file_text_with_limit(&source, 100),
            LimitedSourceTextResult::Text("source text\n".into())
        );
        assert_eq!(
            read_file_text_with_limit(&directory.path().join("missing.txt"), 100),
            LimitedSourceTextResult::Missing
        );
        assert_eq!(
            read_file_text_with_limit(&source, 5),
            LimitedSourceTextResult::TooLarge { max_bytes: 5 }
        );
        fs::write(&source, []).unwrap();
        assert_eq!(
            read_file_text_with_limit(&source, 0),
            LimitedSourceTextResult::Text(String::new())
        );
        fs::write(&source, [0xff]).unwrap();
        assert_eq!(
            read_file_text_with_limit(&source, 1),
            LimitedSourceTextResult::Text("\u{fffd}".into())
        );
    }

    #[test]
    fn bounds_streams_and_notifies_before_returning_the_limit_error() {
        let mut notified = false;
        let mut notify = || notified = true;
        let error =
            read_stream_text_with_limit(Some(Cursor::new(b"too much")), 3, Some(&mut notify))
                .unwrap_err();
        assert!(matches!(error, SourceTextError::TooLarge { max_bytes: 3 }));
        assert!(notified);
        assert_eq!(
            read_stream_text_with_limit(Some(Cursor::new(b"okay")), 4, None).unwrap(),
            "okay"
        );
        assert_eq!(
            read_stream_text_with_limit::<Cursor<&[u8]>>(None, 4, None).unwrap(),
            ""
        );
    }

    #[test]
    fn stale_source_length_cannot_authorize_an_unbounded_read() {
        struct GrowingSource {
            consumed: usize,
            ceiling: usize,
        }
        impl Read for GrowingSource {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                self.consumed += buffer.len();
                assert!(self.consumed <= self.ceiling, "reader exceeded its bound");
                buffer.fill(b'x');
                Ok(buffer.len())
            }
        }
        for limit in [0, 17, 65_536] {
            let mut source = GrowingSource {
                consumed: 0,
                ceiling: limit + 65_536,
            };
            assert!(matches!(
                read_sized_source_with_limit(&mut source, 0, limit),
                Err(SourceTextError::TooLarge { max_bytes }) if max_bytes == limit
            ));
            assert!(source.consumed > limit);
        }
        let mut unread = GrowingSource {
            consumed: 0,
            ceiling: 0,
        };
        assert!(matches!(
            read_sized_source_with_limit(&mut unread, 18, 17),
            Err(SourceTextError::TooLarge { max_bytes: 17 })
        ));
        assert_eq!(unread.consumed, 0);
        assert_eq!(
            read_sized_source_with_limit(Cursor::new(b"okay"), 0, 4).unwrap(),
            "okay"
        );
    }

    #[test]
    fn bounded_reads_retry_interrupted_io_without_losing_bytes() {
        struct InterruptedOnce(bool);
        impl Read for InterruptedOnce {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                if !self.0 {
                    self.0 = true;
                    return Err(io::ErrorKind::Interrupted.into());
                }
                buffer[0] = b'x';
                Ok(1)
            }
        }
        assert_eq!(
            read_sized_source_with_limit(InterruptedOnce(false).take(1), 1, 1).unwrap(),
            "x"
        );
    }

    #[test]
    fn source_decoding_matches_both_pinned_file_and_stream_readers() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/source-text-decoding.json"
        ))
        .unwrap();
        let directory = TempDir::new().unwrap();
        for run in oracle["runs"].as_array().unwrap() {
            assert_eq!(run["exitCode"], 0);
            assert_eq!(run["cases"].as_array().unwrap().len(), 8);
            for case in run["cases"].as_array().unwrap() {
                let chunks = case["chunks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|chunk| {
                        chunk
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|byte| u8::try_from(byte.as_u64().unwrap()).unwrap())
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>();
                let bytes = chunks.concat();
                let mut stream: Box<dyn Read> = Box::new(io::empty());
                for chunk in chunks {
                    stream = Box::new(stream.chain(Cursor::new(chunk)));
                }
                let limit = usize::try_from(case["limit"].as_u64().unwrap()).unwrap();
                assert_eq!(
                    read_stream_text_with_limit(Some(stream), limit, None).unwrap(),
                    case["streamText"].as_str().unwrap(),
                    "stream case {} at {}",
                    case["name"],
                    run["pin"]
                );
                let path = directory.path().join("source.bin");
                fs::write(&path, &bytes).unwrap();
                assert_eq!(
                    read_file_text_with_limit(&path, limit),
                    LimitedSourceTextResult::Text(case["fileText"].as_str().unwrap().into()),
                    "file case {} at {}",
                    case["name"],
                    run["pin"]
                );
            }
        }
        assert!(matches!(
            read_stream_text_with_limit(Some(Cursor::new([0xef, 0xbb, 0xbf])), 2, None),
            Err(SourceTextError::TooLarge { max_bytes: 2 })
        ));
    }

    #[derive(Default)]
    struct NeverExits {
        signals: Vec<&'static str>,
    }

    #[cfg(unix)]
    #[test]
    fn owned_cleanup_closes_descendant_pipes_even_when_the_parent_exits_on_term() {
        let mut command = Command::new("sh");
        command
            .args([
                "-c",
                "trap 'exit 0' TERM; (trap '' TERM; sleep 10) & printf R; wait",
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        let mut process = spawn_source_subprocess(&mut command).unwrap();
        let mut output = process.stdout.take().unwrap();
        let mut ready = [0];
        output.read_exact(&mut ready).unwrap();
        assert_eq!(ready, *b"R");
        let mut unrelated = Command::new("sleep").arg("10").spawn().unwrap();
        let started = Instant::now();
        terminate_source_subprocess(&mut process);
        let mut tail = Vec::new();
        output.read_to_end(&mut tail).unwrap();
        let unrelated_survived = unrelated.try_wait().unwrap().is_none();
        let _ = unrelated.kill();
        let _ = unrelated.wait();
        assert!(
            unrelated_survived,
            "cleanup must not signal an unrelated process group"
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(tail.is_empty());
    }

    impl SourceSubprocess for NeverExits {
        fn request_terminate(&mut self) -> io::Result<()> {
            self.signals.push("SIGTERM");
            Ok(())
        }

        fn force_kill(&mut self) -> io::Result<()> {
            self.signals.push("SIGKILL");
            Ok(())
        }

        fn try_wait_for_exit(&mut self) -> io::Result<Option<ExitStatus>> {
            Ok(None)
        }
    }

    #[test]
    fn forces_termination_when_a_subprocess_never_reports_exit() {
        let mut process = NeverExits::default();
        let started = Instant::now();
        terminate_source_subprocess(&mut process);
        assert_eq!(process.signals, ["SIGTERM", "SIGKILL"]);
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
