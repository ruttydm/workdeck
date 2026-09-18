use crate::{ErrorCode, PmError, Result};
use std::{
    io::{Read, Write},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

pub(super) struct Output {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

type Reader = thread::JoinHandle<std::io::Result<Vec<u8>>>;
#[cfg(test)]
thread_local! { static INVOCATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
#[cfg(test)]
pub(super) fn take_invocations() -> usize {
    INVOCATIONS.with(|count| count.replace(0))
}
struct ProcessGroup {
    child: Child,
    readers: Vec<Reader>,
    stdin: Option<thread::JoinHandle<std::io::Result<()>>>,
    stopped: bool,
}
impl ProcessGroup {
    fn stop(&mut self) {
        if self.stopped {
            return;
        }
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        let _ = self.child.wait();
        self.stopped = true;
    }
}
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        self.stop();
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
        if let Some(stdin) = self.stdin.take() {
            let _ = stdin.join();
        }
    }
}

pub(super) fn run(
    command: Command,
    input: Option<Vec<u8>>,
    max_bytes: usize,
    timeout: Duration,
) -> Result<Output> {
    #[cfg(test)]
    INVOCATIONS.with(|count| count.set(count.get() + 1));
    run_inner(command, input, max_bytes, timeout, |_| {})
}
fn run_inner(
    mut command: Command,
    input: Option<Vec<u8>>,
    max_bytes: usize,
    timeout: Duration,
    after_spawn: impl FnOnce(u32),
) -> Result<Output> {
    if !cfg!(unix) {
        return Err(PmError::new(
            ErrorCode::Unsupported,
            "bounded Git process groups require Unix",
        ));
    }
    let deadline = Instant::now() + timeout;
    command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Express group setup directly so the platform can use its native spawn
        // path. A pre_exec closure forces a fork even for this standard setup.
        command.process_group(0);
    }
    let child = command
        .spawn()
        .map_err(|_| PmError::new(ErrorCode::Io, "could not start Git"))?;
    // Ownership exists before any callback or thread setup can unwind.
    let mut group = ProcessGroup {
        child,
        readers: Vec::new(),
        stdin: None,
        stopped: false,
    };
    after_spawn(group.child.id());
    let overflow = Arc::new(AtomicBool::new(false));
    fn reader(
        mut input: impl Read + Send + 'static,
        cap: usize,
        overflow: Arc<AtomicBool>,
        deadline: Instant,
    ) -> thread::JoinHandle<std::io::Result<Vec<u8>>> {
        thread::spawn(move || {
            let mut result = Vec::new();
            let mut buffer = [0; 16 * 1024];
            loop {
                if Instant::now() >= deadline {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "Git pipe deadline",
                    ));
                }
                let count = match input.read(&mut buffer) {
                    Ok(count) => count,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                if count == 0 {
                    break;
                }
                let keep = count.min(cap.saturating_sub(result.len()));
                result.extend_from_slice(&buffer[..keep]);
                if count > keep {
                    overflow.store(true, Ordering::SeqCst);
                }
            }
            Ok(result)
        })
    }
    let stdout = group.child.stdout.take().expect("piped");
    let stderr = group.child.stderr.take().expect("piped");
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        nonblocking(stdout.as_raw_fd())?;
        nonblocking(stderr.as_raw_fd())?;
    }
    group
        .readers
        .push(reader(stdout, max_bytes, overflow.clone(), deadline));
    group
        .readers
        .push(reader(stderr, 64 * 1024, overflow.clone(), deadline));
    if let Some(bytes) = input {
        let mut stdin = group.child.stdin.take().expect("piped");
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            nonblocking(stdin.as_raw_fd())?;
        }
        group.stdin = Some(thread::spawn(move || {
            let mut offset = 0;
            while offset < bytes.len() {
                if Instant::now() >= deadline {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "Git input deadline",
                    ));
                }
                match stdin.write(&bytes[offset..]) {
                    Ok(0) => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::WriteZero,
                            "Git stdin closed",
                        ));
                    }
                    Ok(count) => offset += count,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) => return Err(error),
                }
            }
            Ok(())
        }));
    }
    let mut failure = None;
    let status = loop {
        match group.child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(_) => {
                failure = Some("Git process status failed");
                break None;
            }
        }
        if overflow.load(Ordering::SeqCst) {
            failure = Some("Git output exceeds capture bound");
            break None;
        }
        if Instant::now() >= deadline {
            failure = Some("Git operation exceeded its timeout");
            break None;
        }
        thread::sleep(Duration::from_millis(5));
    };
    // A finished Git leader must not leave descendants holding our pipes open.
    group.stop();
    let status = status.or_else(|| group.child.wait().ok());
    let stdout = group
        .readers
        .remove(0)
        .join()
        .map_err(|_| PmError::new(ErrorCode::Io, "Git stdout reader failed"))?
        .map_err(|_| PmError::new(ErrorCode::Io, "Git stdout read failed"))?;
    let stderr = group
        .readers
        .remove(0)
        .join()
        .map_err(|_| PmError::new(ErrorCode::Io, "Git stderr reader failed"))?
        .map_err(|_| PmError::new(ErrorCode::Io, "Git stderr read failed"))?;
    if let Some(stdin) = group.stdin.take() {
        let _ = stdin.join();
    }
    if let Some(reason) = failure {
        return Err(PmError::new(ErrorCode::Io, reason));
    }
    if overflow.load(Ordering::SeqCst) {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "Git output exceeds capture bound",
        ));
    }
    Ok(Output {
        status: status.ok_or_else(|| PmError::new(ErrorCode::Io, "Git completion is unknown"))?,
        stdout,
        stderr,
    })
}

#[cfg(unix)]
fn nonblocking(fd: std::os::fd::RawFd) -> Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(PmError::new(ErrorCode::Io, "could not bound Git pipe I/O"));
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn unwind_after_spawn_reaps_the_owned_process_group() {
        let mut pid = 0;
        let mut group_id = -1;
        let parent_group = unsafe { libc::getpgrp() };
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut command = Command::new("sh");
            command.args(["-c", "sleep 30"]);
            let _ = run_inner(command, None, 4096, Duration::from_secs(1), |spawned| {
                pid = spawned;
                group_id = unsafe { libc::getpgid(pid as i32) };
                panic!("injected post-spawn unwind");
            });
        }));
        assert!(caught.is_err());
        assert_ne!(pid, 0);
        assert_eq!(
            group_id, pid as i32,
            "child must lead its own process group"
        );
        assert_ne!(
            group_id, parent_group,
            "cleanup must not target the parent group"
        );
        let alive = unsafe { libc::kill(pid as i32, 0) } == 0;
        // The RED baseline must not leave its demonstrated child running.
        if alive {
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
                libc::waitpid(pid as i32, std::ptr::null_mut(), 0);
            }
        }
        assert!(
            !alive,
            "unwind dropped Child without killing/reaping its process group"
        );
    }
}
