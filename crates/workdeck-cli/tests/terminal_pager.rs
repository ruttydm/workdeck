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
    broker: Option<Broker>,
}

struct Broker(Child);

impl Drop for Broker {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        // Closing the last master releases macOS tty teardown before waitpid.
        self.master.take();
        let _ = self.child.wait();
        self.broker.take();
    }
}

impl Session {
    fn launch(patch: &str, args: &[&str], file_stdin: bool, cols: u16, rows: u16) -> Self {
        Self::launch_with_broker(patch, args, file_stdin, cols, rows, None)
    }

    fn launch_with_broker(
        patch: &str,
        args: &[&str],
        file_stdin: bool,
        cols: u16,
        rows: u16,
        port: Option<u16>,
    ) -> Self {
        Self::launch_in(patch, args, file_stdin, cols, rows, port, None)
    }

    fn launch_in(
        patch: &str,
        args: &[&str],
        file_stdin: bool,
        cols: u16,
        rows: u16,
        port: Option<u16>,
        cwd: Option<&std::path::Path>,
    ) -> Self {
        Self::launch_in_config(patch, args, file_stdin, cols, rows, port, cwd, None)
    }

    #[allow(clippy::too_many_arguments)]
    fn launch_in_config(
        patch: &str,
        args: &[&str],
        file_stdin: bool,
        cols: u16,
        rows: u16,
        port: Option<u16>,
        cwd: Option<&std::path::Path>,
        config: Option<&std::path::Path>,
    ) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let config = config
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| directory.path().join("config"));
        let patch_path = directory.path().join("input.patch");
        fs::write(&patch_path, patch).unwrap();
        let broker = port.map(|port| {
            let log_path = directory.path().join("broker.log");
            let log = File::create(&log_path).unwrap();
            let mut broker = Broker(
                Command::new(env!("CARGO_BIN_EXE_workdeck"))
                    .args(["daemon", "serve"])
                    .current_dir(directory.path())
                    .env("XDG_CONFIG_HOME", &config)
                    .env("XDG_RUNTIME_DIR", directory.path().join("runtime"))
                    .env("WORKDECK_MCP_PORT", port.to_string())
                    .env("WORKDECK_MCP_DISABLE", "0")
                    .stdin(Stdio::null())
                    .stdout(Stdio::from(log.try_clone().unwrap()))
                    .stderr(Stdio::from(log))
                    .spawn()
                    .unwrap(),
            );
            let deadline = Instant::now() + Duration::from_secs(5);
            while std::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).is_err() {
                assert!(
                    broker.0.try_wait().unwrap().is_none(),
                    "private broker exited: {}",
                    fs::read_to_string(&log_path).unwrap_or_default()
                );
                assert!(
                    Instant::now() < deadline,
                    "private broker did not become ready: {}",
                    fs::read_to_string(&log_path).unwrap_or_default()
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            broker
        });
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
            .current_dir(cwd.unwrap_or(directory.path()))
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            .env_remove("NO_COLOR")
            .env("XDG_CONFIG_HOME", &config)
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
        if let Some(port) = port {
            command
                .env("WORKDECK_MCP_DISABLE", "0")
                .env("WORKDECK_MCP_PORT", port.to_string());
        }
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
            broker,
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

    fn move_mouse(&mut self, column: usize, row: usize) {
        self.write(format!("\x1b[<35;{};{}M", column + 1, row + 1).as_bytes());
    }

    fn click_label(&mut self, label: &str) {
        let frame = self.wait(|text| text.contains(label));
        let (row, line, offset) = frame
            .lines()
            .enumerate()
            .find_map(|(row, line)| line.find(label).map(|offset| (row, line, offset)))
            .unwrap();
        let column = line[..offset].chars().count();
        self.write(
            format!(
                "\x1b[<0;{};{}M\x1b[<0;{};{}m",
                column + 1,
                row + 1,
                column + 1,
                row + 1
            )
            .as_bytes(),
        );
    }

    #[track_caller]
    fn wait(&mut self, predicate: impl Fn(&str) -> bool) -> String {
        let Some(text) = self.wait_for(Duration::from_secs(20), predicate) else {
            panic!(
                "terminal predicate timed out:\n{}",
                self.parser.terminal().plain_string()
            );
        };
        text
    }

    fn wait_for(&mut self, timeout: Duration, predicate: impl Fn(&str) -> bool) -> Option<String> {
        let deadline = Instant::now() + timeout;
        loop {
            let text = self.parser.terminal().plain_string();
            if !self
                .parser
                .terminal()
                .modes
                .get(qwertty_term_vt::modes::Mode::SynchronizedOutput)
                && predicate(&text)
            {
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
            let remaining = deadline.saturating_duration_since(Instant::now());
            let poll_ms = remaining.as_millis().clamp(1, 50) as i32;
            // SAFETY: one initialized pollfd references our owned master descriptor.
            let ready = unsafe { libc::poll(&mut fd, 1, poll_ms) };
            assert!(ready >= 0);
            if ready == 0 {
                continue;
            }
            let mut bytes = [0; 32768];
            let count = self.master.as_mut().unwrap().read(&mut bytes).unwrap();
            assert!(
                count > 0,
                "PTY closed (child status: {:?}):\n{text}",
                self.child.try_wait().unwrap()
            );
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

    fn resize(&mut self, cols: u16, rows: u16) {
        self.parser.terminal_mut().resize(cols, rows);
        let size = libc::winsize {
            ws_col: cols,
            ws_row: rows,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: the descriptor is this fixture's live master and size is initialized.
        assert_eq!(
            unsafe {
                libc::ioctl(
                    self.master.as_ref().unwrap().as_raw_fd(),
                    libc::TIOCSWINSZ,
                    &size,
                )
            },
            0
        );
    }
}

#[path = "terminal_pager/layout.rs"]
mod layout;

#[path = "terminal_pager/harness.rs"]
mod harness;

#[path = "terminal_pager/file_views.rs"]
mod file_views;

#[path = "terminal_pager/extensions.rs"]
mod extensions;

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
    let menu = session
        .wait(|text| text.contains("View  Navigate  Agent  Help") && text.contains("first.ts"));
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

#[test]
fn real_git_review_defers_source_until_expansion_in_both_layouts() {
    for layout in ["stack", "split"] {
        let repo = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .args(args)
                .current_dir(repo.path())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["-c", "init.defaultBranch=main", "init", "-q"]);
        let path = repo.path().join("source.txt");
        let before = "hidden-source-one\nline-two\nline-three\nline-four\nline-five\nline-six\nline-seven\nold-value\nline-nine\nline-ten\nline-eleven\nline-twelve\n";
        fs::write(&path, before).unwrap();
        git(&["add", "source.txt"]);
        git(&[
            "-c",
            "user.name=Workdeck Parity",
            "-c",
            "user.email=parity@example.invalid",
            "commit",
            "-qm",
            "fixture",
        ]);
        let changed = before.replace("old-value", "new-value");
        fs::write(&path, &changed).unwrap();
        let mut session = Session::launch_in(
            "",
            &["diff", "--mode", layout, "--no-watch", "--no-sidebar"],
            false,
            120,
            24,
            None,
            Some(repo.path()),
        );
        let initial =
            session.wait(|text| text.contains("unchanged lines") && text.contains("new-value"));
        assert!(!initial.contains("hidden-source-one"));
        assert!(!repo.path().join(".agents/workdeck").exists());
        // Watch is disabled: this edit must reach expansion through its first
        // source read, not by reloading the original diff or an eager snapshot.
        fs::write(
            &path,
            changed.replace("hidden-source-one", "late-source-one"),
        )
        .unwrap();
        session.click_label("unchanged lines");
        session.wait(|text| text.contains("late-source-one") && text.contains("Hide"));
        session.write(b"c");
        session.wait(|text| text.contains("Draft note"));
        session.write(b"\x1b");
        session.wait(|text| !text.contains("Draft note"));
        session.quit();
        assert!(!repo.path().join(".agents/workdeck").exists());
    }
}

#[test]
fn real_terminal_gap_click_expands_and_collapses_source_in_both_layouts() {
    for layout in ["stack", "split"] {
        let (_fixture, mut session) = harness::launch_file_pair(
            "createExpandableContextFilePair",
            layout,
            120,
            20,
            &["--theme", "github-dark-default"],
        );
        let initial = session.wait(|text| text.contains("1 unchanged line"));
        assert!(!initial.contains("hiddenLine01"));
        session.click_label("1 unchanged line");
        session
            .wait(|text| text.contains("hiddenLine01") && text.contains("Hide 1 unchanged line"));
        session.click_label("Hide 1 unchanged line");
        session.wait(|text| text.contains("1 unchanged line") && !text.contains("hiddenLine01"));
        session.quit();
    }
}

// Hunk MIT: test/pty/notes.test.ts. All nineteen baseline cases; eighteen in stable.
mod notes {
    use super::*;

    pub(super) fn pair(
        before: &str,
        after: &str,
        mode: &str,
        width: u16,
        height: u16,
        extra: &[&str],
    ) -> (tempfile::TempDir, Session) {
        let fixture = tempfile::tempdir().unwrap();
        let left = fixture.path().join("before.ts");
        let right = fixture.path().join("after.ts");
        fs::write(&left, before).unwrap();
        fs::write(&right, after).unwrap();
        let mut args = vec![
            "diff",
            "--files",
            left.to_str().unwrap(),
            right.to_str().unwrap(),
            "--mode",
            mode,
        ];
        args.extend_from_slice(extra);
        let session = Session::launch("", &args, false, width, height);
        (fixture, session)
    }

    fn long_wrap(height: u16) -> (tempfile::TempDir, Session) {
        super::harness::launch_file_pair("createLongWrapFilePair", "split", 120, height, &[])
    }

    fn row(text: &str, needle: &str) -> usize {
        super::harness::line_index_of(text, needle)
            .unwrap_or_else(|| panic!("missing {needle}:\n{text}"))
    }

    fn agent_note_visibility(experimental: bool) {
        let sidecar = tempfile::tempdir().unwrap();
        let path = sidecar.path().join("agent.json");
        fs::write(&path, serde_json::json!({"version":1,"files":[{"path":"after.ts","annotations":[{"newRange":[2,2],"summary":"Adds bonus export.","rationale":"Highlights the follow-up addition for review.","markup":"<badge color=\"success\">STML ACTIVE</badge>"}]}]}).to_string()).unwrap();
        let mut extra = vec!["--agent-context", path.to_str().unwrap()];
        if experimental {
            extra.push("--experimental");
        }
        let (_fixture, mut session) = pair(
            "export const answer = 41;\n",
            "export const answer = 42;\nexport const added = true;\n",
            "split",
            140,
            20,
            &extra,
        );
        let initial = session.wait(|text| text.contains("export const added"));
        assert!(!initial.contains("Adds bonus export."));
        session.write(b"a");
        if experimental {
            let shown = session.wait(|text| text.contains("STML ACTIVE"));
            assert!(!shown.contains("Highlights the follow-up addition for review."));
        } else {
            let shown = session.wait(|text| {
                text.contains("Adds bonus export.")
                    && text.contains("Highlights the follow-up addition for review.")
            });
            assert!(!shown.contains("STML ACTIVE"));
        }
        session.write(b"a");
        session.wait(|text| !text.contains("Adds bonus export.") && !text.contains("STML ACTIVE"));
        session.quit();
    }

    fn reveal_note_actions(session: &mut Session, body: &str) -> String {
        let snapshot = session.wait(|text| text.contains(body));
        let target_row = row(&snapshot, body);
        let line = snapshot.lines().nth(target_row).unwrap();
        let column = line[..line.find(body).unwrap()].chars().count() + 1;
        session.move_mouse(0, 0);
        session.wait(|text| !text.contains("r reply e edit"));
        session.move_mouse(column, target_row);
        session.wait(|text| text.contains("r reply e edit"))
    }

    #[test]
    fn saved_notes_support_clickable_threaded_edit_reply_and_delete() {
        let (_fixture, mut session) = pair(
            "export const message = 'short';\n",
            "export const message = 'this is a very long wrapped line for tuistory integration coverage';\n",
            "stack",
            100,
            30,
            &[],
        );
        session.wait(|text| text.contains("export const message"));
        session.write(b"c");
        session.wait(|text| text.contains("Draft note"));
        session.write(b"Root review note.\x13");
        let root =
            session.wait(|text| text.contains("Your note") && text.contains("Root review note."));
        assert!(!root.contains("r reply"));
        assert!(
            root.contains("before.ts -> after.ts L1") || root.contains("before.ts -> after.ts R1")
        );
        let hovered = reveal_note_actions(&mut session, "Root review note.");
        let footer = hovered
            .lines()
            .find(|line| line.contains("r reply e edit d delete"))
            .unwrap();
        assert!(footer.trim_start().starts_with('╰') && footer.trim_end().ends_with('╯'));
        let original_row = row(&hovered, "Root review note.");
        session.click_label("e edit");
        let editing =
            session.wait(|text| text.contains("Edit note") && text.contains("Root review note."));
        assert_eq!(row(&editing, "Root review note."), original_row);
        session.write(b"Updated. ");
        session.wait(|text| text.contains("Updated. Root review note."));
        session.move_mouse(0, 0);
        session.write(b"\x13");
        let edited = session.wait(|text| {
            !text.contains("Edit note") && text.contains("Updated. Root review note.")
        });
        assert_eq!(edited.matches("Your note").count(), 1);
        assert!(!edited.contains("r reply"));
        let hovered = reveal_note_actions(&mut session, "Updated. Root review note.");
        let parent_row = row(&hovered, "Updated. Root review note.");
        session.click_label("r reply");
        let draft = session.wait(|text| text.contains("╰─╭─ Reply -"));
        assert_eq!(row(&draft, "Updated. Root review note."), parent_row);
        session.write(b"First reply.\x13");
        session.wait(|text| text.contains("First reply.") && text.contains("╰─╭─ Your note"));
        reveal_note_actions(&mut session, "First reply.");
        session.click_label("r reply");
        session.wait(|text| text.contains("╰─╭─ Reply -"));
        session.write(b"Nested reply.\x13");
        session.wait(|text| text.contains("Nested reply.") && !text.contains("╭─ Reply -"));
        reveal_note_actions(&mut session, "Updated. Root review note.");
        session.click_label("r reply");
        let siblings = session.wait(|text| text.contains("╰─╭─ Reply -"));
        assert!(siblings.contains("├─╭─ Your note"), "{siblings}");
        assert!(siblings.contains("│ ╰─╭─ Your note"), "{siblings}");
        session.click_label("Esc cancel");
        session.wait(|text| !text.contains("╭─ Reply -"));
        session.write(b"E");
        session.wait(|text| text.contains("╭─ Edit note -"));
        session.click_label("Esc cancel");
        session.wait(|text| !text.contains("╭─ Edit note -"));
        session.write(b"R");
        let keyboard = session.wait(|text| text.contains("╭─ Reply -"));
        let titles = keyboard
            .lines()
            .filter(|line| line.contains("╭─ Your note"))
            .collect::<Vec<_>>();
        assert!(titles.len() >= 3);
        assert!(titles[1].find('╭').unwrap() > titles[0].find('╭').unwrap());
        session.click_label("Esc cancel");
        session.wait(|text| !text.contains("╭─ Reply -"));
        reveal_note_actions(&mut session, "Nested reply.");
        session.click_label("d delete");
        session.wait(|text| !text.contains("Nested reply."));
        session.quit();
    }

    #[test]
    fn agent_notes_reveal_and_hide_without_experimental_markup() {
        agent_note_visibility(false);
    }

    #[test]
    fn experimental_agent_notes_render_stml_instead_of_rationale() {
        agent_note_visibility(true);
    }

    #[test]
    fn collapsed_gap_note_is_inside_owning_hunk_after_first_row() {
        let sidecar = tempfile::tempdir().unwrap();
        let path = sidecar.path().join("agent.json");
        fs::write(&path, serde_json::json!({"version":1,"files":[{"path":"after.ts","annotations":[{"newRange":[6,7],"summary":"GAP NOTE","rationale":"Anchored to lines the patch collapsed away."}]}]}).to_string()).unwrap();
        let before = (1..=12)
            .map(|line| format!("export const line{line} = {line};\n"))
            .collect::<String>();
        let after = before
            .replace("line2 = 2;", "line2 = 200;")
            .replace("line11 = 11;", "line11 = 1100;");
        let (_fixture, mut session) = pair(
            &before,
            &after,
            "split",
            140,
            30,
            &["--agent-context", path.to_str().unwrap()],
        );
        session.wait(|text| text.contains("line11"));
        session.write(b"a");
        let shown = session.wait(|text| text.contains("GAP NOTE") && text.contains("line9 = 9;"));
        assert!(
            row(&shown, "GAP NOTE") > row(&shown, "line8 = 8;"),
            "{shown}"
        );
        assert!(
            row(&shown, "GAP NOTE") < row(&shown, "line9 = 9;"),
            "{shown}"
        );
        session.write(b"a");
        session.wait(|text| !text.contains("GAP NOTE"));
        session.quit();
    }

    #[test]
    fn draft_focus_blocks_app_hunk_shortcut_until_cancelled() {
        let before = (1..=80)
            .map(|line| format!("export const line{line} = {line};\n"))
            .collect::<String>();
        let mut after = before.replace("line1 = 1;", "line1 = 100;");
        for line in 60..=65 {
            after = after.replace(
                &format!("line{line} = {line};"),
                &format!("line{line} = {line}00;"),
            );
        }
        let (_fixture, mut session) = pair(&before, &after, "split", 104, 12, &[]);
        let initial = session.wait(|text| text.contains("line1 = 100"));
        assert!(!initial.contains("line60 = 6000"));
        session.write(b"c");
        session.wait(|text| text.contains("Draft note"));
        session.write(b"Keep focus here]");
        let focused = session.wait(|text| text.contains("Keep focus here]"));
        assert!(focused.contains("Draft note"));
        assert!(!focused.contains("line60 = 6000"));
        session.write(b"\x1b");
        session.wait(|text| !text.contains("Draft note"));
        session.write(b"]");
        let after = session.wait(|text| text.contains("line60 = 6000"));
        assert!(!after.contains("Keep focus here]"));
        session.quit();
    }

    #[test]
    fn draft_focus_blocks_pager_sidebar_shortcut_until_cancelled() {
        let mut session = Session::launch(
            &patch(40),
            &["patch", "input.patch", "--pager"],
            false,
            120,
            20,
        );
        let initial = session.wait(|text| text.contains("before_01"));
        assert!(!initial.contains("M scroll.ts"));
        open_on_row(&mut session, row(&initial, "before_01"));
        session.write(b"sidebar-trigger text");
        let focused = session
            .wait(|text| text.contains("sidebar-trigger text") && text.contains("Draft note"));
        assert!(!focused.contains("M scroll.ts"));
        session.click_label("Esc cancel");
        session.wait(|text| !text.contains("Draft note"));
        session.write(b"s");
        session.wait(|text| {
            text.lines()
                .any(|line| line.contains("M scroll.ts") && line.contains("+40 -40"))
        });
        session.quit();
    }

    #[test]
    fn user_notes_draft_and_save_inline_with_newline_geometry() {
        let (_fixture, mut session) = long_wrap(20);
        session.wait(|text| text.contains("this is a very long"));
        session.write(b"c");
        let fresh = session.wait(|text| {
            text.contains("Draft note")
                && text.contains("Write a note")
                && text.contains("Esc cancel")
        });
        let border = fresh
            .lines()
            .find(|line| line.contains("^S save") && line.contains("Esc cancel"))
            .unwrap();
        assert!(
            border.trim_start().starts_with('╰') && border.trim_end().ends_with('╯'),
            "{fresh}"
        );
        session.write(b"Please cover this edge case.");
        let first = session.wait(|text| text.contains("Please cover this edge case."));
        let previous = row(&first, "^S save");
        session.write(b"\x0a");
        session.wait(|text| {
            text.contains("Please cover this edge case.")
                && text
                    .lines()
                    .position(|line| line.contains("^S save"))
                    .is_some_and(|row| row > previous)
        });
        session.write(b"Second line.\x13");
        let saved = session.wait(|text| text.contains("Your note") && !text.contains("Draft note"));
        assert!(saved.contains("Please cover this edge case."));
        assert!(saved.contains("Second line."));
        session.quit();
    }

    #[test]
    fn cjk_drafts_wrap_and_retain_both_ends_when_saved() {
        let (_fixture, mut session) = long_wrap(24);
        session.wait(|text| text.contains("this is a very long"));
        session.write(b"c");
        session.wait(|text| text.contains("Draft note"));
        let body = "这个包主要是为了在普通的chatmodel外面包一层,把工具调用的编号统一转换后再返回给调用方使用";
        session.write(body.as_bytes());
        session
            .wait(|text| text.contains("这个包主要是为了在普") && text.contains("回给调用方使用"));
        session.write(b"\x13");
        let saved = session.wait(|text| text.contains("Your note") && !text.contains("Draft note"));
        assert!(saved.contains("这个包主要是为了在普"), "{saved}");
        assert!(saved.contains("回给调用方使用"), "{saved}");
        session.quit();
    }

    #[test]
    fn rapid_control_s_saves_a_draft_exactly_once() {
        let (_fixture, mut session) = long_wrap(24);
        session.wait(|text| text.contains("this is a very long"));
        session.write(b"c");
        session.wait(|text| text.contains("Draft note"));
        session.write(b"Save exactly one note.");
        session.wait(|text| text.contains("Save exactly one note."));
        session.write(b"\x13\x13");
        session.wait(|text| text.contains("Your note") && !text.contains("Draft note"));
        session.wait_for(Duration::from_millis(250), |_| false);
        let settled = session.parser.terminal().plain_string();
        assert!(settled.contains("Save exactly one note."));
        assert!(!settled.contains("Your note 1/"));
        assert_eq!(settled.matches("Your note").count(), 1);
        session.quit();
    }

    #[test]
    fn first_escape_cancels_an_empty_draft() {
        let before = (1..=80)
            .map(|line| format!("export const line{line} = {line};\n"))
            .collect::<String>();
        let mut after = before.replace("line1 = 1;", "line1 = 100;");
        for line in 60..=65 {
            after = after.replace(
                &format!("line{line} = {line};"),
                &format!("line{line} = {line}00;"),
            );
        }
        let (_fixture, mut session) = pair(&before, &after, "split", 120, 20, &[]);
        session.wait(|text| text.contains("line1 = 100"));
        session.write(b"c");
        session.wait(|text| text.contains("Draft note"));
        session.write(b"\x1b");
        session.wait(|text| !text.contains("Draft note") && text.contains("line1 = 100"));
        session.quit();
    }

    #[test]
    fn clicked_add_note_can_cancel_and_save_with_mouse_controls() {
        let (_fixture, mut session) = long_wrap(20);
        let initial = session.wait(|text| text.contains("this is a very long"));
        let target_row = row(&initial, "this is a very long");
        for (body, save) in [
            ("Cancel this draft.", false),
            ("Save this clicked draft.", true),
        ] {
            session.move_mouse(0, 0);
            session.move_mouse(8, target_row);
            session.click_label("[+]");
            session.wait(|text| text.contains("Draft note"));
            session.write(body.as_bytes());
            session.wait(|text| text.contains(body));
            session.click_label(if save { "^S save" } else { "Esc cancel" });
            if save {
                session.wait(|text| {
                    text.contains("Your note")
                        && text.contains(body)
                        && !text.contains("Draft note")
                });
            } else {
                let cancelled =
                    session.wait(|text| !text.contains("Draft note") && !text.contains(body));
                assert!(!cancelled.contains("Your note"));
            }
        }
        session.quit();
    }

    fn open_on_row(session: &mut Session, target_row: usize) {
        session.move_mouse(0, 0);
        session.move_mouse(8, target_row);
        session.click_label("[+]");
        session.wait(|text| text.contains("Draft note"));
    }

    #[test]
    fn clicked_add_note_owns_keyboard_cancel_and_save() {
        let (_fixture, mut session) = long_wrap(20);
        let initial = session.wait(|text| text.contains("this is a very long"));
        let target_row = row(&initial, "this is a very long");
        open_on_row(&mut session, target_row);
        session.write(b"Cancel this shortcut draft.\x1b");
        let cancelled = session.wait(|text| {
            !text.contains("Draft note") && !text.contains("Cancel this shortcut draft.")
        });
        assert!(!cancelled.contains("Your note"));
        open_on_row(&mut session, target_row);
        session.write(b"Save this shortcut draft.\x13");
        session.wait(|text| {
            text.contains("Your note")
                && text.contains("Save this shortcut draft.")
                && !text.contains("Draft note")
        });
        session.quit();
    }

    #[test]
    fn stack_add_note_affordance_saves_clicked_target() {
        let (_fixture, mut session) = pair(
            "export const message = 'short';\n",
            "export const message = 'this is a very long wrapped line for tuistory integration coverage';\n",
            "stack",
            100,
            20,
            &[],
        );
        let initial = session.wait(|text| text.contains("this is a very long"));
        open_on_row(&mut session, row(&initial, "this is a very long"));
        session.write(b"Save this stack draft.\x13");
        session.wait(|text| {
            text.contains("Your note")
                && text.contains("Save this stack draft.")
                && !text.contains("Draft note")
        });
        session.quit();
    }

    fn deletion_pair(height: u16) -> (tempfile::TempDir, Session) {
        pair(
            "export const keep = true;\nexport const removeMe = true;\n",
            "export const keep = true;\n",
            "split",
            120,
            height,
            &[],
        )
    }

    #[test]
    fn deletion_only_add_note_affordance_saves_old_side() {
        let (_fixture, mut session) = deletion_pair(16);
        let initial = session.wait(|text| text.contains("removeMe"));
        open_on_row(&mut session, row(&initial, "removeMe"));
        session.write(b"Save this deletion draft.\x13");
        session.wait(|text| {
            text.contains("Your note")
                && text.contains("Save this deletion draft.")
                && !text.contains("Draft note")
        });
        session.quit();
    }

    #[test]
    fn context_click_overrides_keyboard_cursor_without_moving_target_row() {
        let (_fixture, mut session) = deletion_pair(16);
        session.wait(|text| text.contains("keep = true") && text.contains("removeMe"));
        session.write(b"\x1b[B");
        session.wait_for(Duration::from_millis(100), |_| false);
        let initial = session.parser.terminal().plain_string();
        let target_row = row(&initial, "keep = true");
        open_on_row(&mut session, target_row);
        session.wait_for(Duration::from_millis(100), |_| false);
        let draft = session.parser.terminal().plain_string();
        assert_eq!(row(&draft, "keep = true"), target_row, "{draft}");
        assert!(row(&draft, "Draft note") > target_row);
        session.write(b"Save this context draft.\x13");
        session.wait(|text| {
            text.contains("Your note")
                && text.contains("Save this context draft.")
                && !text.contains("Draft note")
        });
        session.quit();
    }

    #[test]
    fn multiple_clicked_notes_survive_on_one_hunk() {
        let (_fixture, mut session) = deletion_pair(20);
        let initial = session.wait(|text| text.contains("keep = true"));
        open_on_row(&mut session, row(&initial, "keep = true"));
        session.write(b"First note on the context row.\x13");
        let first = session.wait(|text| {
            text.contains("First note on the context row.")
                && text.contains("Your note")
                && !text.contains("Draft note")
                && text.contains("removeMe")
        });
        open_on_row(&mut session, row(&first, "removeMe"));
        session.write(b"Second note on the deletion row.\x13");
        let second = session.wait(|text| {
            text.contains("Second note on the deletion row.") && !text.contains("Draft note")
        });
        assert!(
            second.contains("First note on the context row."),
            "{second}"
        );
        session.quit();
    }

    #[test]
    fn mouse_movement_is_required_to_restore_affordance_after_wheel_or_key() {
        let before = (1..=18)
            .map(|line| format!("export const line{line:02} = {line};\n"))
            .collect::<String>();
        let after = (1..=18)
            .map(|line| format!("export const line{line:02} = {};\n", line + 100))
            .collect::<String>();
        let (_fixture, mut session) = pair(&before, &after, "split", 120, 12, &[]);
        session.wait(|text| text.contains("line01"));
        session.move_mouse(8, 5);
        session.wait(|text| text.contains("[+]"));
        session.write(b"\x1b[<65;60;6M\x1b[<65;60;6M");
        session.wait(|text| !text.contains("[+]"));
        session.wait_for(Duration::from_millis(250), |_| false);
        assert!(!session.parser.terminal().plain_string().contains("[+]"));
        session.move_mouse(9, 5);
        session.wait(|text| text.contains("[+]"));
        session.write(b"\x1b[B");
        session.wait(|text| !text.contains("[+]"));
        session.wait_for(Duration::from_millis(250), |_| false);
        assert!(!session.parser.terminal().plain_string().contains("[+]"));
        session.quit();
    }

    #[test]
    fn cursor_off_draft_reveals_default_target_and_full_composer() {
        let before = (1..=18)
            .map(|line| format!("export const line{line:02} = {line};\n"))
            .collect::<String>();
        let after = (1..=18)
            .map(|line| format!("export const line{line:02} = {};\n", line + 100))
            .collect::<String>();
        let (_fixture, mut session) =
            pair(&before, &after, "stack", 120, 12, &["--cursor-line", "off"]);
        session.wait(|text| text.contains("line01 = 1;"));
        session.write(b" ");
        session.wait(|text| !text.contains("line01 = 1;") && text.contains("line"));
        session.write(b"c");
        session.wait(|text| {
            text.contains("Draft note - before.ts -> after.ts R1") && text.contains("Esc cancel")
        });
        session.write(b"\x1b");
        session.wait(|text| !text.contains("Draft note"));
        session.quit();
    }

    #[test]
    fn opening_drafts_preserves_active_line_and_pushes_following_code() {
        let before = (1..=18)
            .map(|line| format!("export const line{line:02} = {line};\n"))
            .collect::<String>();
        let after = (1..=18)
            .map(|line| format!("export const line{line:02} = {};\n", line + 100))
            .collect::<String>();
        let (_fixture, mut session) = pair(&before, &after, "stack", 120, 26, &[]);
        session.wait(|text| text.contains("line01 = 1;"));
        for (active, following, target) in [
            ("line09 = 9;", "line10 = 10;", "L9"),
            ("line17 = 17;", "line18 = 18;", "L17"),
        ] {
            for _ in 0..8 {
                session.write(b"\x1b[B");
            }
            session.wait_for(Duration::from_millis(100), |_| false);
            let initial = session.parser.terminal().plain_string();
            let active_row = row(&initial, active);
            let following_row = row(&initial, following);
            assert!(active_row > 0);
            session.write(b"c");
            session.wait(|text| {
                text.contains(&format!("Draft note - before.ts -> after.ts {target}"))
            });
            // A PTY read can end inside the title's repaint; inspect the complete
            // subsequent frame just as the source waits 100ms after opening a draft.
            session.wait_for(Duration::from_millis(100), |_| false);
            let draft = session.parser.terminal().plain_string();
            assert_eq!(row(&draft, active), active_row, "{draft}");
            assert_eq!(row(&draft, "Draft note"), active_row + 1, "{draft}");
            if target == "L9" {
                assert!(row(&draft, following) > following_row);
            }
            session.write(b"\x1b");
            session.wait(|text| !text.contains("Draft note"));
        }
        session.quit();
    }
}

// Hunk MIT: test/pty/session-attention-integration.test.ts.
#[test]
fn session_attention_highlight_reveals_and_paints_exact_range_then_clears_and_navigates() {
    let fixture = tempfile::tempdir().unwrap();
    let before = fixture.path().join("before.ts");
    let after = fixture.path().join("after.ts");
    let mut old = String::new();
    let mut new = String::new();
    for line in 1..=130 {
        old.push_str(&format!("export const line{line:03} = {line};\n"));
        if line == 111 {
            new.push_str("export const needle = \"ATTENTIONNEEDLE\";\n");
        } else {
            new.push_str(&format!("export const line{line:03} = {};\n", line + 1000));
        }
    }
    fs::write(&before, old).unwrap();
    fs::write(&after, new).unwrap();
    let reservation = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = reservation.local_addr().unwrap().port();
    drop(reservation);
    let mut session = Session::launch_with_broker(
        "",
        &[
            "diff",
            "--files",
            before.to_str().unwrap(),
            after.to_str().unwrap(),
            "--mode",
            "stack",
        ],
        false,
        140,
        24,
        Some(port),
    );
    let initial = session.wait(|text| text.contains("line001"));
    assert!(!initial.contains("ATTENTIONNEEDLE"));
    let run_cli = |session: &mut Session, args: &[&str]| {
        let config_directory = session.directory.path().to_owned();
        let args = args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        let worker = std::thread::spawn(move || {
            let output = Command::new(env!("CARGO_BIN_EXE_workdeck"))
                .arg("session")
                .args(&args)
                .current_dir(&config_directory)
                .env("XDG_CONFIG_HOME", config_directory.join("config"))
                .env("XDG_RUNTIME_DIR", config_directory.join("runtime"))
                .env("WORKDECK_MCP_PORT", port.to_string())
                .env("WORKDECK_MCP_DISABLE", "0")
                .stdin(Stdio::null())
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(0),
                "session {args:?}: stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                output.stderr.is_empty(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
        });
        // Like the source harness, keep capturing terminal output while the daemon
        // forwards a request and the review repaints before returning its response.
        let deadline = Instant::now() + Duration::from_secs(15);
        while !worker.is_finished() {
            assert!(
                Instant::now() < deadline,
                "session CLI exceeded its deadline"
            );
            session.wait_for(Duration::from_millis(25), |_| false);
        }
        worker.join().unwrap()
    };
    let deadline = Instant::now() + Duration::from_secs(20);
    let session_id = loop {
        let listed = run_cli(&mut session, &["list", "--json"]);
        if let Some(id) = listed["sessions"][0]["sessionId"].as_str() {
            break id.to_owned();
        }
        assert!(
            Instant::now() < deadline,
            "review did not register: {listed}"
        );
        std::thread::sleep(Duration::from_millis(150));
    };
    let highlighted = run_cli(
        &mut session,
        &[
            "highlight",
            "add",
            &session_id,
            "--file",
            "after.ts",
            "--new-line",
            "111",
            "--start",
            "13",
            "--end",
            "19",
            "--tone",
            "warning",
            "--focus",
            "--json",
        ],
    );
    for (key, expected) in serde_json::json!({"filePath":"after.ts", "side":"new", "line":111, "start":13, "end":19, "tone":"warning", "fileMarkCount":1, "revealed":"line"}).as_object().unwrap() {
        assert_eq!(&highlighted["result"][key], expected, "{highlighted}");
    }
    let cleared_command = ["highlight", "clear", &session_id, "--json"];
    let revealed = session.wait(|text| text.contains("ATTENTIONNEEDLE"));
    let row = revealed
        .lines()
        .position(|line| line.contains("ATTENTIONNEEDLE"))
        .unwrap();
    assert!(row > 0 && row < 12, "{revealed}");
    let text = revealed.lines().nth(row).unwrap();
    let marked = text.find("needle").unwrap();
    let unmarked = text.find("ATTENTIONNEEDLE").unwrap();
    // All text preceding these tokens is ASCII except the leading rail. Count cells,
    // not UTF-8 bytes, to address the actual terminal background colors.
    let marked = text[..marked].chars().count();
    let unmarked = text[..unmarked].chars().count();
    let snapshot = session.parser.terminal().snapshot();
    let rows = snapshot.visible_window(0);
    assert_eq!(rows[row].cells[marked].ch, 'n', "{revealed}");
    assert_eq!(rows[row].cells[unmarked].ch, 'A', "{revealed}");
    assert_ne!(
        rows[row].cells[marked].style.bg,
        rows[row].cells[unmarked].style.bg
    );
    let cleared = run_cli(&mut session, &cleared_command);
    assert_eq!(cleared["result"]["removedCount"], 1);
    assert_eq!(cleared["result"]["remainingCount"], 0);
    let navigated = run_cli(
        &mut session,
        &[
            "navigate",
            &session_id,
            "--file",
            "after.ts",
            "--new-line",
            "25",
            "--json",
        ],
    );
    for (key, expected) in
        serde_json::json!({"filePath":"after.ts", "revealed":"line", "side":"new", "line":25})
            .as_object()
            .unwrap()
    {
        assert_eq!(&navigated["result"][key], expected, "{navigated}");
    }
    let frame = session.wait(|text| text.contains("line025") && !text.contains("ATTENTIONNEEDLE"));
    let row = frame
        .lines()
        .position(|line| line.contains("line025"))
        .unwrap();
    assert!(row > 0 && row < 12, "{frame}");
    session.quit();
}
