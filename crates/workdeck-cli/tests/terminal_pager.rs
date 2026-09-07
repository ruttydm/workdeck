//! Native PTY translations of Hunk's MIT-licensed test/pty/pager.test.ts.
#![cfg(unix)]

use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use qwertty_term_vt::stream::{Stream, TerminalHandler};
use qwertty_term_vt::terminal::{Options, Terminal};

struct Session {
    child: Child,
    master: Option<File>,
    parser: Stream<TerminalHandler>,
    directory: tempfile::TempDir,
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        // Closing the last master releases macOS tty teardown before waitpid.
        self.master.take();
        let _ = self.child.wait();
    }
}

impl Session {
    fn launch(patch: &str, args: &[&str], file_stdin: bool, cols: u16, rows: u16) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let patch_path = directory.path().join("input.patch");
        fs::write(&patch_path, patch).unwrap();
        let (mut master, mut slave) = (-1, -1);
        let mut size = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: openpty initializes two descriptors with the supplied window size.
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
            0
        );
        for fd in [master, slave] {
            // SAFETY: these are the newly allocated, live PTY descriptors.
            assert_eq!(
                unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) },
                0
            );
        }
        // SAFETY: each descriptor is transferred to exactly one File owner.
        let (master, slave) = unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) };
        let mut command = Command::new(env!("CARGO_BIN_EXE_workdeck"));
        let pipe_stdin = file_stdin && args.first() == Some(&"diff");
        command
            .args(args)
            .current_dir(directory.path())
            .env("TERM", "xterm-256color")
            .env("XDG_CONFIG_HOME", directory.path().join("config"))
            .env("XDG_RUNTIME_DIR", directory.path().join("runtime"))
            .env("WORKDECK_MCP_DISABLE", "1")
            .stdin(if pipe_stdin {
                Stdio::piped()
            } else if file_stdin {
                Stdio::from(File::open(patch_path).unwrap())
            } else {
                Stdio::from(slave.try_clone().unwrap())
            })
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave));
        // SAFETY: only async-signal-safe operations run before exec. Stdout is the PTY
        // even when stdin contains patch bytes; establish that PTY as controlling terminal.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(1, libc::TIOCSCTTY as libc::c_ulong, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = command.spawn().unwrap();
        if pipe_stdin {
            child
                .stdin
                .take()
                .unwrap()
                .write_all(patch.as_bytes())
                .unwrap();
        }
        drop(command);
        Self {
            child,
            master: Some(master),
            directory,
            parser: Stream::new(TerminalHandler::new(Terminal::new(Options {
                cols,
                rows,
                ..Options::default()
            }))),
        }
    }

    fn write(&mut self, bytes: &[u8]) {
        self.master.as_mut().unwrap().write_all(bytes).unwrap();
        self.master.as_mut().unwrap().flush().unwrap();
    }

    fn wait(&mut self, predicate: impl Fn(&str) -> bool) -> String {
        self.wait_for(Duration::from_secs(20), predicate)
            .unwrap_or_else(|| {
                panic!(
                    "terminal predicate timed out:\n{}",
                    self.parser.terminal().plain_string()
                )
            })
    }

    fn wait_for(&mut self, timeout: Duration, predicate: impl Fn(&str) -> bool) -> Option<String> {
        let deadline = Instant::now() + timeout;
        loop {
            let text = self.parser.terminal().plain_string();
            if predicate(&text) {
                return Some(text);
            }
            if Instant::now() >= deadline {
                return None;
            }
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "review exited early:\n{text}"
            );
            let mut fd = libc::pollfd {
                fd: self.master.as_ref().unwrap().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: one initialized pollfd references our owned master descriptor.
            let ready = unsafe { libc::poll(&mut fd, 1, 50) };
            assert!(ready >= 0);
            if ready == 0 {
                continue;
            }
            let mut bytes = [0; 32768];
            let count = self.master.as_mut().unwrap().read(&mut bytes).unwrap();
            assert!(count > 0, "PTY closed:\n{text}");
            self.parser.feed(&bytes[..count]);
            let replies = self.parser.handler.take_output();
            if !replies.is_empty() {
                self.write(&replies);
            }
        }
    }

    fn wheel_until(&mut self, down: bool, predicate: impl Fn(&str) -> bool) -> String {
        for _ in 0..12 {
            self.write(if down {
                b"\x1b[<65;60;6M"
            } else {
                b"\x1b[<64;60;6M"
            });
            if let Some(text) = self.wait_for(Duration::from_millis(700), &predicate) {
                return text;
            }
        }
        panic!(
            "wheel did not reach target:\n{}",
            self.parser.terminal().plain_string()
        );
    }

    fn quit(&mut self) {
        self.write(b"q");
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert_eq!(status.code(), Some(0));
                break;
            }
            assert!(Instant::now() < deadline, "pager did not quit");
            let mut descriptor = libc::pollfd {
                fd: self.master.as_ref().unwrap().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: drain restoration output while the owned child releases its tty.
            if unsafe { libc::poll(&mut descriptor, 1, 10) } > 0 {
                let mut bytes = [0; 32768];
                if let Ok(count) = self.master.as_mut().unwrap().read(&mut bytes) {
                    self.parser.feed(&bytes[..count]);
                }
            }
        }
        assert!(!self.directory.path().join(".agents").exists());
    }
}

fn patch(lines: usize) -> String {
    let mut patch = format!(
        "diff --git a/scroll.ts b/scroll.ts\n--- a/scroll.ts\n+++ b/scroll.ts\n@@ -1,{lines} +1,{lines} @@\n"
    );
    for line in 1..=lines {
        patch.push_str(&format!("-export const before_{line:02} = {line};\n"));
    }
    for line in 1..=lines {
        patch.push_str(&format!(
            "+export const after_{line:02} = {};\n",
            line + 100
        ));
    }
    patch
}

#[test]
fn explicit_pager_hides_chrome_and_pages_forward_on_space() {
    let mut session = Session::launch(
        &patch(40),
        &["patch", "input.patch", "--pager"],
        false,
        120,
        20,
    );
    let initial = session.wait(|text| text.contains("before_01"));
    assert!(!initial.contains("View  Navigate  Agent  Help"));
    assert!(!initial.contains("before_23"));
    session.write(b" ");
    let paged = session.wait(|text| text.contains("before_23"));
    assert!(!paged.contains("View  Navigate  Agent  Help"));
    session.quit();
}

#[test]
fn pager_half_page_page_up_and_content_jumps() {
    let mut session = Session::launch(
        &patch(60),
        &["patch", "input.patch", "--pager"],
        false,
        120,
        12,
    );
    let initial = session.wait(|text| text.contains("before_01"));
    assert!(!initial.contains("before_12"));
    session.write(b"\x04");
    session.wait(|text| !text.contains("before_01") && text.contains("before_"));
    session.write(b"\x15");
    session.wait(|text| text.contains("before_01"));
    session.write(b" ");
    session.wait(|text| text.contains("before_18"));
    session.write(b"b");
    session.wait(|text| text.contains("before_01") && !text.contains("before_18"));
    session.write(b"\x1b[F");
    session.wait(|text| text.contains("after_60"));
    session.write(b"\x1b[H");
    session.wait(|text| text.contains("before_01") && !text.contains("after_60"));
    session.quit();
}

fn wheel_case(args: &[&str], file_stdin: bool, restore: bool) {
    let mut session = Session::launch(&patch(60), args, file_stdin, 120, 12);
    let initial = session.wait(|text| text.contains("before_01"));
    assert!(!initial.contains("before_12"));
    assert!(!initial.contains("View  Navigate  Agent  Help"));
    session.wait_for(Duration::from_millis(200), |_| false);
    let scrolled = session.wheel_until(true, |text| {
        !text.contains("before_01") && text.contains("before_12")
    });
    assert!(!scrolled.contains("View  Navigate  Agent  Help"));
    if restore {
        session.wheel_until(false, |text| {
            text.contains("before_01") && !text.contains("before_12")
        });
    }
    session.quit();
}

#[test]
fn stdin_patch_accepts_terminal_mouse_wheel() {
    wheel_case(&["patch", "-"], true, true);
}

#[test]
fn stdin_patch_auto_theme_accepts_terminal_mouse_wheel() {
    wheel_case(&["patch", "-", "--theme", "auto"], true, false);
}

#[test]
fn general_pager_accepts_terminal_mouse_wheel() {
    wheel_case(&["pager"], true, true);
}

#[test]
fn explicit_pager_accepts_terminal_mouse_wheel() {
    wheel_case(&["patch", "input.patch", "--pager"], false, true);
}

fn sidebar_case(explicit: bool) {
    let args = if explicit {
        vec!["pager", "--sidebar"]
    } else {
        vec!["pager"]
    };
    let mut session = Session::launch(&patch(40), &args, true, 120, 14);
    let initial = session.wait(|text| text.contains("before_01"));
    assert_eq!(initial.matches("scroll.ts").count(), 1, "{initial}");
    assert!(!initial.contains("View  Navigate  Agent  Help"));
    session.write(b"s");
    let shown = session.wait(|text| text.contains("M scroll.ts"));
    assert!(
        shown
            .lines()
            .any(|line| line.contains("M scroll.ts") && line.contains("+40 -40"))
    );
    assert!(!shown.contains("View  Navigate  Agent  Help"));
    session.quit();
}

#[test]
fn general_pager_can_reveal_sidebar_tree() {
    sidebar_case(false);
}

#[test]
fn explicit_sidebar_flag_still_starts_pager_without_sidebar() {
    sidebar_case(true);
}

fn multi_file_patch() -> String {
    let mut patch = String::from(
        "diff --git a/first.ts b/first.ts\n--- a/first.ts\n+++ b/first.ts\n@@ -1,40 +1,40 @@\n",
    );
    for line in 1..=40 {
        patch.push_str(&format!("-export const line{line:02} = {line};\n"));
    }
    for line in 1..=40 {
        patch.push_str(&format!("+export const line{line:02} = {};\n", line + 100));
    }
    patch.push_str("diff --git a/second.ts b/second.ts\n--- a/second.ts\n+++ b/second.ts\n@@ -1 +1 @@\n-export const secondValue = 1;\n+export const secondValue = 2;\n");
    patch
}

#[test]
fn general_pager_navigates_to_bottom_clamped_final_file_and_back() {
    let mut session = Session::launch(&multi_file_patch(), &["pager"], true, 120, 16);
    let initial = session.wait(|text| text.contains("line01 = 1;"));
    assert!(!initial.contains("secondValue = 2;"));
    session.write(b".");
    session.wait(|text| text.contains("secondValue = 2;") && !text.contains("line01 = 1;"));
    session.write(b",");
    session.wait(|text| text.contains("line01 = 1;") && !text.contains("secondValue = 2;"));
    session.quit();
}

#[test]
fn general_pager_switches_layout_and_reveals_menu_on_demand() {
    let mut session = Session::launch(&multi_file_patch(), &["pager"], true, 220, 20);
    let split = |text: &str| text.lines().any(|line| line.matches('▌').count() >= 2);
    let initial = session.wait(|text| text.contains("line01 = 1;") && split(text));
    assert!(!initial.contains("View  Navigate  Agent  Help"));
    session.write(b"2");
    session.wait(|text| text.contains("line01 = 1;") && !split(text));
    session.write(b"1");
    session.wait(split);
    session.write(b"M");
    let menu = session.wait(|text| text.contains("View  Navigate  Agent  Help"));
    assert!(menu.contains("first.ts"));
    // Pager view changes remain transient and do not prompt to persist preferences.
    session.quit();
}

#[test]
fn piped_stdin_still_allows_concrete_theme_app_terminal_input() {
    // A real pipe carries the source's ignored bytes; keyboard input must come from
    // the controlling terminal, independently of that pipe reaching EOF.
    let directory = tempfile::tempdir().unwrap();
    let before = directory.path().join("before.ts");
    let after = directory.path().join("after.ts");
    fs::write(&before, "export const alpha = 1;\n").unwrap();
    fs::write(&after, "export const alpha = 2;\n").unwrap();
    let mut session = Session::launch(
        "ignored",
        &[
            "diff",
            "--files",
            before.to_str().unwrap(),
            after.to_str().unwrap(),
            "--theme",
            "github-dark-default",
        ],
        true,
        120,
        14,
    );
    session.wait(|text| {
        text.contains("View  Navigate  Agent  Help") && text.contains("export const alpha")
    });
    session.quit();
}
