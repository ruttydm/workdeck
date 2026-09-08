//! Keep a dead extension's stdin pipe from signalling application shutdown.

use std::io::{self, Write};

#[cfg(target_os = "macos")]
pub(crate) fn configure_child_pipe(pipe: &std::process::ChildStdin) -> io::Result<()> {
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

pub(crate) fn write_child_frame(output: &mut impl Write, frame: &[u8]) -> io::Result<()> {
    #[cfg(all(unix, not(target_os = "macos")))]
    let guard = SigpipeGuard::block()?;
    let result = output.write_all(frame).and_then(|()| output.flush());
    #[cfg(all(unix, not(target_os = "macos")))]
    if result
        .as_ref()
        .is_err_and(|error| error.kind() == io::ErrorKind::BrokenPipe)
    {
        guard.consume_generated_signal()?;
    }
    result
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
            write_child_frame(&mut pipe, b"frame\n").unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
    }

    #[test]
    fn writes_complete_frames_and_preserves_io_errors() {
        let mut output = Vec::new();
        write_child_frame(&mut output, b"frame\n").unwrap();
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
            write_child_frame(&mut Failed, b"frame\n")
                .unwrap_err()
                .kind(),
            io::ErrorKind::BrokenPipe
        );
    }
}
