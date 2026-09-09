//! Actual controlling-terminal coverage for the Rust theme diagnostic.
#![cfg(unix)]
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[test]
fn theme_probe_exchanges_osc11_and_reports_timeout_on_a_real_pty() {
    for (response, redirect_stdout, redirect_stdin) in [
        (Some("\x1b]11;rgb:00/00/00\x07"), false, false),
        (None, false, false),
        (Some("\x1b]11;rgb:00/00/00\x07"), true, false),
        (None, true, false),
        (Some("\x1b]11;rgb:00/00/00\x07"), false, true),
        (None, false, true),
        (Some("\x1b]11;rgb:00/00/00\x07"), true, true),
        (None, true, true),
    ] {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let initial_mode = pair.master.get_termios().expect("initial PTY termios");
        let redirected = tempfile::NamedTempFile::new().unwrap();
        let mut stdin_file = tempfile::NamedTempFile::new().unwrap();
        let untouched = b"diff input must remain unread\n";
        stdin_file.write_all(untouched).unwrap();
        let remainder = tempfile::NamedTempFile::new().unwrap();
        let command = if redirect_stdin {
            let mut command = CommandBuilder::new("/bin/sh");
            let script = if redirect_stdout {
                "exec 3< \"$3\"; \"$1\" themes probe <&3 > \"$2\"; probe_status=$?; /bin/cat <&3 > \"$4\"; exit \"$probe_status\""
            } else {
                "exec 3< \"$3\"; \"$1\" themes probe <&3; probe_status=$?; /bin/cat <&3 > \"$4\"; exit \"$probe_status\""
            };
            command.args([
                "-c",
                script,
                "theme-probe-test",
                env!("CARGO_BIN_EXE_xtask"),
            ]);
            command.arg(redirected.path());
            command.arg(stdin_file.path());
            command.arg(remainder.path());
            command
        } else if redirect_stdout {
            let mut command = CommandBuilder::new("/bin/sh");
            command.args([
                "-c",
                "exec \"$1\" themes probe > \"$2\"",
                "theme-probe-test",
                env!("CARGO_BIN_EXE_xtask"),
            ]);
            command.arg(redirected.path());
            command
        } else {
            let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_xtask"));
            command.args(["themes", "probe"]);
            command
        };
        let mut child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let mut writer = pair.master.take_writer().unwrap();
        let (tx, rx) = mpsc::channel();
        let reader_thread = std::thread::spawn(move || {
            let mut bytes = [0; 4096];
            while let Ok(count) = reader.read(&mut bytes) {
                if count == 0 || tx.send(bytes[..count].to_vec()).is_err() {
                    break;
                }
            }
        });
        let deadline = Instant::now() + Duration::from_secs(120);
        let mut output = Vec::new();
        let mut queried = false;
        let mut exited = None;
        while Instant::now() < deadline {
            if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(20)) {
                output.extend(chunk);
            }
            if !queried && output.windows(8).any(|w| w == b"\x1b]11;?\x1b\\") {
                queried = true;
                if let Some(response) = response {
                    writer.write_all(response.as_bytes()).unwrap();
                    writer.flush().unwrap();
                }
            }
            if let Some(status) = child.try_wait().unwrap() {
                exited = Some(status);
                break;
            }
        }
        if exited.is_none() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let restored_mode = pair.master.get_termios();
        drop(writer);
        drop(pair.master);
        reader_thread.join().unwrap();
        for chunk in rx.try_iter() {
            output.extend(chunk);
        }
        let text = String::from_utf8_lossy(&output);
        assert!(exited.is_some_and(|status| status.success()), "{text}");
        assert_eq!(
            restored_mode.as_ref(),
            Some(&initial_mode),
            "PTY mode must be restored after response or timeout"
        );
        assert!(queried, "{text}");
        let json_start = text.find('{').expect("diagnostic JSON");
        let report: serde_json::Value = serde_json::from_str(text[json_start..].trim()).unwrap();
        assert_eq!(report["stdoutIsTTY"], !redirect_stdout);
        assert!(
            std::fs::read(redirected.path()).unwrap().is_empty(),
            "stdout must not contain query or diagnostic output"
        );
        assert_eq!(report["stdinIsTTY"], !redirect_stdin);
        if redirect_stdin {
            assert_eq!(
                std::fs::read(remainder.path()).unwrap(),
                untouched,
                "probe must not consume redirected stdin"
            );
        }
        if response.is_some() {
            assert_eq!(report["mode"], "dark");
            assert_eq!(report["classified"], "dark");
            assert_eq!(
                report["color"],
                serde_json::json!({"red":0,"green":0,"blue":0})
            );
            assert_eq!(report["raw"], "\\e]11;rgb:00/00/00\u{7}");
        } else {
            assert!(report["mode"].is_null());
            assert!(report["color"].is_null());
            assert_eq!(report["raw"], "");
        }
    }
}
