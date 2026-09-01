//! Bounded source readers used by VCS snapshots and source expansion.

use std::fs;
use std::io::{self, Read};
use std::path::Path;
use std::process::{Child, ExitStatus};
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

/// Read one filesystem source without allocating beyond the supplied byte ceiling.
pub fn read_file_text_with_limit(path: &Path, max_bytes: usize) -> LimitedSourceTextResult {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return LimitedSourceTextResult::Missing;
        }
        Err(error) => {
            log_source_diagnostic(
                &format!("failed to read source file {}", path.display()),
                &error,
            );
            return LimitedSourceTextResult::Missing;
        }
    };
    if metadata.len() > max_bytes as u64 {
        return LimitedSourceTextResult::TooLarge { max_bytes };
    }
    match fs::read(path) {
        Ok(bytes) if bytes.len() <= max_bytes => {
            LimitedSourceTextResult::Text(String::from_utf8_lossy(&bytes).into_owned())
        }
        Ok(_) => LimitedSourceTextResult::TooLarge { max_bytes },
        Err(error) => {
            log_source_diagnostic(
                &format!("failed to read source file {}", path.display()),
                &error,
            );
            LimitedSourceTextResult::Missing
        }
    }
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
        let bytes_read = stream.read(&mut buffer)?;
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
    Ok(String::from_utf8_lossy(&bytes).into_owned())
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

    #[derive(Default)]
    struct NeverExits {
        signals: Vec<&'static str>,
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
