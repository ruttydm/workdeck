//! Keep a dead extension's stdin pipe from signalling application shutdown.

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

#[cfg(not(windows))]
pub(crate) type NativeStdin = std::process::ChildStdin;

#[cfg(windows)]
#[path = "child_pipe/windows_pipe.rs"]
mod windows_pipe;
#[cfg(windows)]
pub(crate) use windows_pipe::{NativeStdin, stdin_pair};

#[cfg(any(windows, test))]
fn nonblocking_byte_pipe_result(result: io::Result<usize>, empty: bool) -> io::Result<usize> {
    match result {
        // Windows byte pipes in PIPE_NOWAIT mode can succeed with zero bytes
        // when full. This is backpressure, not a terminal WriteZero failure.
        Ok(0) if !empty => Err(io::ErrorKind::WouldBlock.into()),
        result => result,
    }
}

#[cfg(unix)]
pub(crate) fn configure_child_pipe(pipe: &std::process::ChildStdin) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    #[cfg(target_os = "macos")]
    suppress_pipe_signal(pipe)?;
    // Only the parent's owned write endpoint becomes nonblocking. The child's
    // read endpoint is a different open file description.
    let fd = pipe.as_raw_fd();
    // SAFETY: query and update flags on the live owned descriptor, retaining all
    // existing flags. Neither operation transfers ownership.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags == -1 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn suppress_pipe_signal(pipe: &std::process::ChildStdin) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    // Darwin's <sys/fcntl.h>: F_SETNOSIGPIPE = 73. libc does not expose this
    // Darwin constant. Apply it to this owned pipe only, not process signals.
    const F_SETNOSIGPIPE: libc::c_int = 73;
    // SAFETY: the descriptor belongs to the live ChildStdin; the command takes
    // an integer boolean and does not retain pointers or transfer ownership.
    if unsafe { libc::fcntl(pipe.as_raw_fd(), F_SETNOSIGPIPE, 1) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// A single deadline includes all short writes and backpressure waits. Immediate
/// cleanup may attempt writes but never waits for pipe space. Native endpoints
/// must be configured nonblocking before entering this loop.
#[derive(Clone, Copy)]
pub(crate) enum WriteBudget<'a> {
    Until(Instant, Option<&'a AtomicBool>),
    Immediate,
}

impl WriteBudget<'_> {
    fn check(self) -> io::Result<()> {
        if let Self::Until(deadline, cancelled) = self {
            if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "extension write cancelled",
                ));
            }
            if Instant::now() >= deadline {
                return Err(io::ErrorKind::TimedOut.into());
            }
        }
        Ok(())
    }

    pub(crate) fn wait(self) -> io::Result<()> {
        self.check()?;
        match self {
            Self::Immediate => Err(io::ErrorKind::TimedOut.into()),
            Self::Until(deadline, _) => {
                std::thread::sleep(
                    deadline
                        .saturating_duration_since(Instant::now())
                        .min(Duration::from_millis(2)),
                );
                self.check()
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct FrameWriteFailure {
    pub source: io::Error,
    pub written: usize,
}

impl FrameWriteFailure {
    pub fn kind(&self) -> io::ErrorKind {
        self.source.kind()
    }
}

pub(crate) fn write_child_frame(
    output: &mut impl Write,
    frame: &[u8],
    budget: WriteBudget<'_>,
) -> Result<(), FrameWriteFailure> {
    #[cfg(all(unix, not(target_os = "macos")))]
    let guard = SigpipeGuard::block().map_err(|source| FrameWriteFailure { source, written: 0 })?;
    let mut written = 0;
    let result = write_frame(output, frame, budget, &mut written);
    #[cfg(all(unix, not(target_os = "macos")))]
    if result
        .as_ref()
        .is_err_and(|error| error.kind() == io::ErrorKind::BrokenPipe)
    {
        guard
            .consume_generated_signal()
            .map_err(|source| FrameWriteFailure { source, written })?;
    }
    result.map_err(|source| FrameWriteFailure { source, written })
}

fn write_frame(
    output: &mut impl Write,
    mut frame: &[u8],
    budget: WriteBudget<'_>,
    total_written: &mut usize,
) -> io::Result<()> {
    while !frame.is_empty() {
        budget.check()?;
        match output.write(frame) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(written) => {
                *total_written += written;
                frame = &frame[written..];
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                if matches!(budget, WriteBudget::Immediate) {
                    return Err(io::ErrorKind::TimedOut.into());
                }
                continue;
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => budget.wait()?,
            Err(error) => return Err(error),
        }
    }
    loop {
        budget.check()?;
        match output.flush() {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                if matches!(budget, WriteBudget::Immediate) {
                    return Err(io::ErrorKind::TimedOut.into());
                }
                continue;
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => budget.wait()?,
            result => return result,
        }
    }
}

/// Mask only the calling thread while writing. Never replace the application's
/// signal disposition: an externally sent SIGPIPE must retain its normal owner.
#[cfg(all(unix, any(test, not(target_os = "macos"))))]
struct SigpipeGuard {
    previous: libc::sigset_t,
    pipe: libc::sigset_t,
    previously_pending: bool,
}

#[cfg(all(unix, any(test, not(target_os = "macos"))))]
impl SigpipeGuard {
    fn block() -> io::Result<Self> {
        // SAFETY: sigemptyset initializes the set before use; all pointers below
        // reference live initialized storage. pthread_sigmask affects this thread.
        unsafe {
            let mut pipe = std::mem::zeroed();
            if libc::sigemptyset(&mut pipe) != 0 || libc::sigaddset(&mut pipe, libc::SIGPIPE) != 0 {
                return Err(io::Error::last_os_error());
            }
            let mut previous = std::mem::zeroed();
            let status = libc::pthread_sigmask(libc::SIG_BLOCK, &pipe, &mut previous);
            if status != 0 {
                return Err(io::Error::from_raw_os_error(status));
            }
            let mut guard = Self {
                previous,
                pipe,
                previously_pending: true,
            };
            let mut pending = std::mem::zeroed();
            if libc::sigpending(&mut pending) != 0 {
                return Err(io::Error::last_os_error());
            }
            guard.previously_pending = libc::sigismember(&pending, libc::SIGPIPE) == 1
                || libc::sigismember(&guard.previous, libc::SIGPIPE) == 1;
            Ok(guard)
        }
    }

    fn consume_generated_signal(&self) -> io::Result<()> {
        if self.previously_pending {
            return Ok(());
        }
        // SAFETY: SIGPIPE is still blocked on this thread. An EPIPE write generates
        // a thread-directed pending signal; another thread cannot consume it.
        // Check pending first because an ignored disposition need not queue it.
        unsafe {
            let mut pending = std::mem::zeroed();
            if libc::sigpending(&mut pending) != 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::sigismember(&pending, libc::SIGPIPE) == 1 {
                let mut signal = 0;
                let status = libc::sigwait(&self.pipe, &mut signal);
                if status != 0 {
                    return Err(io::Error::from_raw_os_error(status));
                }
            }
        }
        Ok(())
    }
}

#[cfg(all(unix, any(test, not(target_os = "macos"))))]
impl Drop for SigpipeGuard {
    fn drop(&mut self) {
        // SAFETY: restore the exact valid mask captured for this same thread.
        unsafe {
            libc::pthread_sigmask(libc::SIG_SETMASK, &self.previous, std::ptr::null_mut());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_zero_progress_translation_preserves_empty_writes_counts_and_errors() {
        assert_eq!(
            nonblocking_byte_pipe_result(Ok(0), false)
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(nonblocking_byte_pipe_result(Ok(0), true).unwrap(), 0);
        assert_eq!(nonblocking_byte_pipe_result(Ok(7), false).unwrap(), 7);
        assert_eq!(
            nonblocking_byte_pipe_result(Err(io::ErrorKind::BrokenPipe.into()), false)
                .unwrap_err()
                .kind(),
            io::ErrorKind::BrokenPipe
        );
    }

    #[cfg(unix)]
    #[test]
    fn real_nonreading_child_times_out_after_a_partial_pipe_write() {
        struct ChildGuard(std::process::Child);
        impl Drop for ChildGuard {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        struct CountWrites {
            pipe: std::process::ChildStdin,
            written: usize,
        }
        impl Write for CountWrites {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                let count = self.pipe.write(bytes)?;
                self.written += count;
                Ok(count)
            }
            fn flush(&mut self) -> io::Result<()> {
                self.pipe.flush()
            }
        }
        let mut child = ChildGuard(
            std::process::Command::new("sleep")
                .arg("30")
                .stdin(std::process::Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let pipe = child.0.stdin.take().unwrap();
        configure_child_pipe(&pipe).unwrap();
        let mut output = CountWrites { pipe, written: 0 };
        let bytes = vec![b'x'; 3 * 1024 * 1024];
        let started = Instant::now();
        assert_eq!(
            write_child_frame(
                &mut output,
                &bytes,
                WriteBudget::Until(started + Duration::from_millis(30), None)
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::TimedOut
        );
        assert!(output.written > 0 && output.written < bytes.len());
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn short_writes_interrupts_and_backpressure_preserve_the_exact_frame() {
        struct ShortWriter {
            calls: usize,
            bytes: Vec<u8>,
        }
        impl Write for ShortWriter {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.calls += 1;
                match self.calls {
                    1 => Err(io::ErrorKind::Interrupted.into()),
                    3 => Err(io::ErrorKind::WouldBlock.into()),
                    _ => {
                        let count = bytes.len().min(2);
                        self.bytes.extend_from_slice(&bytes[..count]);
                        Ok(count)
                    }
                }
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut output = ShortWriter {
            calls: 0,
            bytes: Vec::new(),
        };
        write_child_frame(
            &mut output,
            b"complete frame\n",
            WriteBudget::Until(Instant::now() + Duration::from_secs(1), None),
        )
        .unwrap();
        assert_eq!(output.bytes, b"complete frame\n");
        assert!(output.calls > 2);
    }

    #[test]
    fn expired_and_cancelled_budgets_do_not_attempt_a_write() {
        let mut output = Vec::new();
        assert_eq!(
            write_child_frame(
                &mut output,
                b"frame",
                WriteBudget::Until(Instant::now(), None)
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::TimedOut
        );
        let cancelled = AtomicBool::new(true);
        assert_eq!(
            write_child_frame(
                &mut output,
                b"frame",
                WriteBudget::Until(Instant::now() + Duration::from_secs(1), Some(&cancelled))
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::Interrupted
        );
        assert!(output.is_empty());
    }

    #[test]
    fn persistent_backpressure_is_bounded_and_immediate_cleanup_does_not_retry() {
        struct Blocked(usize);
        impl Write for Blocked {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                self.0 += 1;
                Err(io::ErrorKind::WouldBlock.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                unreachable!()
            }
        }
        let mut output = Blocked(0);
        assert_eq!(
            write_child_frame(&mut output, b"frame", WriteBudget::Immediate)
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(output.0, 1);
        let started = Instant::now();
        assert_eq!(
            write_child_frame(
                &mut output,
                b"frame",
                WriteBudget::Until(started + Duration::from_millis(20), None)
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::TimedOut
        );
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn cancellation_after_a_short_write_stops_before_the_next_fragment() {
        let cancelled = AtomicBool::new(false);
        struct CancelAfterPrefix<'a> {
            cancelled: &'a AtomicBool,
            bytes: Vec<u8>,
        }
        impl Write for CancelAfterPrefix<'_> {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.bytes.push(bytes[0]);
                self.cancelled.store(true, Ordering::Release);
                Ok(1)
            }
            fn flush(&mut self) -> io::Result<()> {
                unreachable!()
            }
        }
        let mut output = CancelAfterPrefix {
            cancelled: &cancelled,
            bytes: Vec::new(),
        };
        assert_eq!(
            write_child_frame(
                &mut output,
                b"frame",
                WriteBudget::Until(Instant::now() + Duration::from_secs(1), Some(&cancelled))
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::Interrupted
        );
        assert_eq!(output.bytes, b"f");
    }

    #[cfg(unix)]
    #[test]
    fn scoped_mask_restores_existing_thread_signal_state() {
        // SAFETY: query initialized local storage; no process disposition is changed.
        unsafe {
            let mut before = std::mem::zeroed();
            assert_eq!(
                libc::pthread_sigmask(libc::SIG_BLOCK, std::ptr::null(), &mut before),
                0
            );
            {
                let guard = SigpipeGuard::block().unwrap();
                let mut during = std::mem::zeroed();
                assert_eq!(
                    libc::pthread_sigmask(libc::SIG_BLOCK, std::ptr::null(), &mut during),
                    0
                );
                assert_eq!(libc::sigismember(&during, libc::SIGPIPE), 1);
                guard.consume_generated_signal().unwrap();
            }
            let mut after = std::mem::zeroed();
            assert_eq!(
                libc::pthread_sigmask(libc::SIG_BLOCK, std::ptr::null(), &mut after),
                0
            );
            for signal in [libc::SIGPIPE, libc::SIGINT, libc::SIGTERM, libc::SIGUSR1] {
                assert_eq!(
                    libc::sigismember(&before, signal),
                    libc::sigismember(&after, signal)
                );
            }
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn darwin_owned_child_pipe_suppresses_sigpipe_and_returns_broken_pipe() {
        use std::os::fd::AsRawFd;
        let mut child = std::process::Command::new("/usr/bin/true")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut pipe = child.stdin.take().unwrap();
        configure_child_pipe(&pipe).unwrap();
        // SAFETY: query F_GETNOSIGPIPE from Darwin's fcntl.h on the live owned fd.
        assert_eq!(unsafe { libc::fcntl(pipe.as_raw_fd(), 74) }, 1);
        assert!(child.wait().unwrap().success());
        assert_eq!(
            write_child_frame(&mut pipe, b"frame\n", WriteBudget::Immediate)
                .unwrap_err()
                .kind(),
            io::ErrorKind::BrokenPipe
        );
    }

    #[test]
    fn writes_complete_frames_and_preserves_io_errors() {
        let mut output = Vec::new();
        write_child_frame(&mut output, b"frame\n", WriteBudget::Immediate).unwrap();
        assert_eq!(output, b"frame\n");
        struct Failed;
        impl Write for Failed {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        assert_eq!(
            write_child_frame(&mut Failed, b"frame\n", WriteBudget::Immediate)
                .unwrap_err()
                .kind(),
            io::ErrorKind::BrokenPipe
        );
    }
}
