//! Adapt redirected input to Crossterm's terminal-only Unix input source.
//!
//! A private raw PTY preserves Crossterm's full escape-sequence parser. This is
//! not a controlling terminal and owns no process group or external process.

use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;

pub(super) struct PipedInputBridge {
    original: File,
    _master: File,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl PipedInputBridge {
    pub(super) fn start() -> io::Result<Self> {
        // SAFETY: duplicate the live standard input descriptor into a new owned descriptor.
        let original = owned_fd(unsafe { libc::fcntl(0, libc::F_DUPFD_CLOEXEC, 3) })?;
        let (mut master, mut slave) = (-1, -1);
        // SAFETY: openpty initializes the two descriptors; optional output/settings are null.
        if unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        } < 0
        {
            return Err(io::Error::last_os_error());
        }
        let master = owned_fd(master)?;
        let slave = owned_fd(slave)?;
        for file in [&master, &slave] {
            // SAFETY: these descriptors are owned and valid for the calls.
            if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        // SAFETY: configure this private master as nonblocking; no shared caller descriptor changes.
        if unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) } < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut attributes = std::mem::MaybeUninit::<libc::termios>::uninit();
        // SAFETY: tcgetattr initializes the termios allocation on success.
        if unsafe { libc::tcgetattr(slave.as_raw_fd(), attributes.as_mut_ptr()) } < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: tcgetattr succeeded; cfmakeraw only mutates this initialized termios.
        let mut attributes = unsafe { attributes.assume_init() };
        unsafe {
            libc::cfmakeraw(&mut attributes);
        }
        // SAFETY: apply raw mode to the private slave before any bytes are forwarded.
        if unsafe { libc::tcsetattr(slave.as_raw_fd(), libc::TCSANOW, &attributes) } < 0 {
            return Err(io::Error::last_os_error());
        }
        let stop = Arc::new(AtomicBool::new(false));
        let input = original.try_clone()?;
        let output = master.try_clone()?;
        let worker_stop = Arc::clone(&stop);
        let mut bridge = Self {
            original,
            _master: master,
            stop,
            worker: None,
        };
        // SAFETY: replace only this process's stdin, retaining its original descriptor for Drop.
        if unsafe { libc::dup2(slave.as_raw_fd(), 0) } < 0 {
            return Err(io::Error::last_os_error());
        }
        bridge.worker = Some(
            std::thread::Builder::new()
                .name("workdeck-piped-input".into())
                .spawn(move || forward(input, output, &worker_stop))?,
        );
        Ok(bridge)
    }
}

impl Drop for PipedInputBridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        // SAFETY: restore stdin from the descriptor retained exclusively by this guard.
        unsafe {
            libc::dup2(self.original.as_raw_fd(), 0);
        }
    }
}

fn owned_fd(fd: libc::c_int) -> io::Result<File> {
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: callers transfer a newly allocated descriptor, once, into File ownership.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn ready(fd: libc::c_int, events: libc::c_short, stop: &AtomicBool) -> bool {
    while !stop.load(Ordering::Acquire) {
        let mut descriptor = libc::pollfd {
            fd,
            events,
            revents: 0,
        };
        // SAFETY: poll borrows one initialized descriptor for a bounded wait.
        let result = unsafe { libc::poll(&mut descriptor, 1, 50) };
        if result > 0 {
            return true;
        }
        if result < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            return false;
        }
    }
    false
}

fn forward(input: File, output: File, stop: &AtomicBool) {
    let mut buffer = [0u8; 8192];
    while ready(input.as_raw_fd(), libc::POLLIN, stop) {
        // SAFETY: read writes at most buffer.len() bytes to the valid buffer.
        let count =
            unsafe { libc::read(input.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len()) };
        if count == 0 {
            return;
        }
        if count < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return;
        }
        let mut offset = 0;
        while offset < count as usize && ready(output.as_raw_fd(), libc::POLLOUT, stop) {
            let bytes = &buffer[offset..count as usize];
            // SAFETY: write borrows exactly the initialized bytes read above.
            let written =
                unsafe { libc::write(output.as_raw_fd(), bytes.as_ptr().cast(), bytes.len()) };
            if written < 0 {
                if matches!(
                    io::Error::last_os_error().kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                ) {
                    continue;
                }
                return;
            }
            if written == 0 {
                return;
            }
            offset += written as usize;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};

    fn file(stream: UnixStream) -> File {
        File::from(OwnedFd::from(stream))
    }

    #[test]
    fn transport_preserves_unicode_escape_sequences_and_eof() {
        let (mut producer, input) = UnixStream::pair().unwrap();
        let (output, mut consumer) = UnixStream::pair().unwrap();
        output.set_nonblocking(true).unwrap();
        consumer
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let worker =
            std::thread::spawn(move || forward(file(input), file(output), &AtomicBool::new(false)));
        let bytes = "\x1b[Bλ🦀\x1b[200~hello\x1b[201~q".as_bytes();
        producer.write_all(bytes).unwrap();
        drop(producer);
        let mut actual = Vec::new();
        consumer.read_to_end(&mut actual).unwrap();
        worker.join().unwrap();
        assert_eq!(actual, bytes);
    }

    #[test]
    fn stop_cancels_idle_input_without_waiting_for_producer_eof() {
        let (_producer, input) = UnixStream::pair().unwrap();
        let (output, _consumer) = UnixStream::pair().unwrap();
        output.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = std::thread::spawn(move || forward(file(input), file(output), &worker_stop));
        let started = Instant::now();
        stop.store(true, Ordering::Release);
        worker.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
