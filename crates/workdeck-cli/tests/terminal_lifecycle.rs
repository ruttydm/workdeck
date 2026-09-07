//! Native subprocess coverage for Hunk's MIT-licensed test/pty/lifecycle.test.ts.
//! Source: baseline 2c00f435; Hunk MIT attribution is retained in THIRD_PARTY_NOTICES.
#![cfg(unix)]

use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn revoke(path: *const libc::c_char) -> libc::c_int;
}

struct ReviewChild(Child);

impl ReviewChild {
    fn wait_clean(&mut self) {
        let status = self.wait_status();
        assert_eq!(status.code(), Some(0), "review exited abnormally: {status}");
    }

    fn wait_status(&mut self) -> ExitStatus {
        self.wait_status_with_timeout(Duration::from_secs(3))
    }

    fn wait_status_with_timeout(&mut self, timeout: Duration) -> ExitStatus {
        let deadline = Instant::now() + timeout;
        let status: ExitStatus = loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                break status;
            }
            assert!(
                Instant::now() < deadline,
                "review did not exit after disconnect"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        status
    }
}

impl Drop for ReviewChild {
    fn drop(&mut self) {
        // This guard owns this exact child, including on assertion failure.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn pty_pair() -> (File, File) {
    let (mut master, mut slave) = (-1, -1);
    let mut size = libc::winsize {
        ws_row: 24,
        ws_col: 140,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: openpty initializes both descriptors; the optional name and termios are null.
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        },
        0,
        "{}",
        std::io::Error::last_os_error()
    );
    for fd in [master, slave] {
        // SAFETY: both descriptors were returned by openpty. Do not leak the master to child.
        assert_eq!(
            unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) },
            0
        );
    }
    // SAFETY: transfer each newly allocated descriptor to exactly one File owner.
    unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) }
}

fn wait_for_frame(master: &mut (impl Read + AsRawFd)) {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut output = Vec::new();
    loop {
        assert!(
            Instant::now() < deadline,
            "review never painted: {}",
            String::from_utf8_lossy(&output)
        );
        let mut poll = libc::pollfd {
            fd: master.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: poll references one valid descriptor for this bounded call.
        let ready = unsafe { libc::poll(&mut poll, 1, 100) };
        assert!(ready >= 0, "{}", std::io::Error::last_os_error());
        if ready == 0 {
            continue;
        }
        let mut buffer = [0; 8192];
        let count = master.read(&mut buffer).unwrap();
        assert!(count > 0, "review output ended before initial frame");
        output.extend_from_slice(&buffer[..count]);
        if String::from_utf8_lossy(&output).contains("this is a very long wrapped ") {
            return;
        }
    }
}

#[test]
fn exits_cleanly_when_host_closes_pty_master() {
    exercise_pty_shutdown(Shutdown::CloseMaster);
}

#[test]
fn exits_cleanly_on_sighup_in_terminal() {
    exercise_pty_shutdown(Shutdown::Signal(libc::SIGHUP));
}

#[test]
fn exits_cleanly_on_sigquit_in_terminal() {
    exercise_pty_shutdown(Shutdown::Signal(libc::SIGQUIT));
}

#[test]
fn exits_cleanly_on_sigpipe_in_terminal() {
    exercise_pty_shutdown(Shutdown::Signal(libc::SIGPIPE));
}

#[test]
fn exits_cleanly_on_sighup_with_pipes_and_broker() {
    exercise_pipe_shutdown(Some(libc::SIGHUP));
}

#[test]
fn exits_cleanly_on_sigquit_with_pipes_and_broker() {
    exercise_pipe_shutdown(Some(libc::SIGQUIT));
}

#[test]
fn exits_cleanly_on_sigpipe_with_pipes_and_broker() {
    exercise_pipe_shutdown(Some(libc::SIGPIPE));
}

#[test]
fn redirected_input_accepts_arrow_and_quit_keys_through_crossterm() {
    exercise_pipe_shutdown(None);
}

fn exercise_pipe_shutdown(signal: Option<i32>) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("before.txt"), "old line\n").unwrap();
    fs::write(
        dir.path().join("after.txt"),
        "this is a very long wrapped line\n",
    )
    .unwrap();
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reservation.local_addr().unwrap().port();
    let command = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_workdeck"));
        command
            .current_dir(dir.path())
            .env("TERM", "xterm-256color")
            .env("XDG_CONFIG_HOME", dir.path().join("config"))
            .env("XDG_RUNTIME_DIR", dir.path().join("runtime"))
            .env("WORKDECK_MCP_HOST", "127.0.0.1")
            .env("WORKDECK_MCP_PORT", port.to_string())
            .env("WORKDECK_MCP_DISABLE", "0");
        command
    };
    drop(reservation);
    let mut daemon = ReviewChild(
        command()
            .args(["daemon", "serve"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while std::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).is_err() {
        assert!(
            daemon.0.try_wait().unwrap().is_none(),
            "private broker exited before readiness"
        );
        assert!(
            Instant::now() < deadline,
            "private broker never became ready"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut child = ReviewChild(
        command()
            .args(["diff", "--files", "before.txt", "after.txt"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut stderr = child.0.stderr.take().unwrap();
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });
    let mut stdout = child.0.stdout.take().unwrap();
    wait_for_frame(&mut stdout);
    let drain = std::thread::spawn(move || {
        let _ = std::io::copy(&mut stdout, &mut std::io::sink());
    });
    if let Some(signal) = signal {
        // SAFETY: signal the exact owned review process after observing its rendered frame.
        assert_eq!(unsafe { libc::kill(child.0.id() as _, signal) }, 0);
    } else {
        child
            .0
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"\x1b[Bq")
            .unwrap();
    }
    let status = child.wait_status_with_timeout(Duration::from_secs(2));
    drain.join().unwrap();
    let stderr = errors.join().unwrap();
    assert_eq!(status.code(), Some(0), "{status}: {stderr}");
    assert!(stderr.is_empty(), "review emitted shutdown errors");
    // SAFETY: daemon is another exact child owned by this test, not a discovered user process.
    assert_eq!(unsafe { libc::kill(daemon.0.id() as _, libc::SIGTERM) }, 0);
    assert_eq!(
        daemon
            .wait_status_with_timeout(Duration::from_secs(6))
            .code(),
        Some(0)
    );
    assert!(!dir.path().join(".agents").exists());
}

#[cfg(target_os = "macos")]
#[test]
fn exits_cleanly_when_controlling_terminal_is_revoked() {
    exercise_pty_shutdown(Shutdown::Revoke);
}

enum Shutdown {
    CloseMaster,
    Signal(i32),
    #[cfg(target_os = "macos")]
    Revoke,
}

fn exercise_pty_shutdown(action: Shutdown) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("before.txt"), "old line\n").unwrap();
    fs::write(
        dir.path().join("after.txt"),
        "this is a very long wrapped line\n",
    )
    .unwrap();
    let (mut master, slave) = pty_pair();
    #[cfg(target_os = "macos")]
    let tty_path = {
        let mut name = [0 as libc::c_char; 1024];
        // SAFETY: the live slave descriptor and writable name buffer are valid.
        assert_eq!(
            unsafe { libc::ttyname_r(slave.as_raw_fd(), name.as_mut_ptr(), name.len()) },
            0
        );
        // SAFETY: successful ttyname_r has terminated the buffer.
        unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) }.to_owned()
    };
    let mut command = Command::new(env!("CARGO_BIN_EXE_workdeck"));
    #[cfg(target_os = "macos")]
    if matches!(action, Shutdown::Revoke) {
        use std::os::unix::process::CommandExt;
        // SAFETY: only async-signal-safe libc operations run between fork and exec.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as libc::c_ulong, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    let mut child = ReviewChild(
        command
            .args(["diff", "--files", "before.txt", "after.txt"])
            .current_dir(dir.path())
            .env("TERM", "xterm-256color")
            .env("XDG_CONFIG_HOME", dir.path().join("config"))
            .env("XDG_RUNTIME_DIR", dir.path().join("runtime"))
            .env("WORKDECK_MCP_DISABLE", "1")
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave))
            .spawn()
            .unwrap(),
    );
    drop(command);
    wait_for_frame(&mut master);
    let drain = match action {
        Shutdown::CloseMaster => {
            drop(master);
            None
        }
        Shutdown::Signal(signal) => {
            // SAFETY: target the exact child owned by the guard, after its initial frame.
            assert_eq!(unsafe { libc::kill(child.0.id() as _, signal) }, 0);
            Some(std::thread::spawn(move || {
                let mut buffer = [0; 8192];
                while let Ok(count) = master.read(&mut buffer) {
                    if count == 0 {
                        break;
                    }
                }
            }))
        }
        #[cfg(target_os = "macos")]
        Shutdown::Revoke => {
            assert!(tty_path.to_bytes().starts_with(b"/dev/tty"));
            // The source harness continuously drains output. Darwin revoke can block
            // behind an in-flight tty write if capture stops midway through a frame.
            let drain = std::thread::spawn(move || {
                let mut buffer = [0; 8192];
                while let Ok(count) = master.read(&mut buffer) {
                    if count == 0 {
                        break;
                    }
                }
            });
            // SAFETY: tty_path is the NUL-terminated name of this test's private slave.
            assert_eq!(unsafe { revoke(tty_path.as_ptr()) }, 0);
            Some(drain)
        }
    };
    child.wait_clean();
    if let Some(drain) = drain {
        drain.join().unwrap();
    }
    assert!(
        !dir.path().join(".agents").exists(),
        "viewing created repository state"
    );
}
