//! Native MIT port of Hunk scripts/probe-terminal-theme.ts (Modem Labs Inc.).
use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};
use std::time::Duration;
use std::{
    fs,
    io::{self, IsTerminal, Write},
    path::Path,
};
use workdeck_tui::ThemeProbeInput;

const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
const STABLE: &str = "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd";
const SOURCE_PATH: &str = "scripts/probe-terminal-theme.ts";
const SOURCE_BYTES: usize = 1_158;
const SOURCE_LINES: usize = 46;
const SOURCE_SHA256: &str = "5dd31f87c16c24a0fb64b298d8bd5012e0998d75c03a6c527ef38fa5342cac0e";

fn pinned_source(repo: &Path, commit: &str) -> Result<Vec<u8>> {
    let source = crate::git_stdout_bytes(repo, ["show", &format!("{commit}:{SOURCE_PATH}")])?;
    ensure!(
        source.len() == SOURCE_BYTES,
        "pinned {SOURCE_PATH} {commit} changed size: {} != {SOURCE_BYTES}",
        source.len()
    );
    ensure!(
        source.split(|byte| *byte == b'\n').count() == SOURCE_LINES + 1,
        "pinned {SOURCE_PATH} {commit} changed line count"
    );
    ensure!(
        format!("{:x}", Sha256::digest(&source)) == SOURCE_SHA256,
        "pinned {SOURCE_PATH} {commit} changed SHA-256"
    );
    Ok(source)
}

/// Verify that the native theme probe preserves the complete pinned CLI
/// contract while routing terminal I/O through the Rust/Crossterm adapter.
pub(crate) fn verify(repo: &Path, baseline: &str) -> Result<()> {
    ensure!(
        baseline == BASELINE,
        "theme probe verifier received unexpected baseline {baseline}"
    );
    let source = pinned_source(repo, BASELINE)?;
    let stable = pinned_source(repo, STABLE)?;
    ensure!(source == stable, "pinned theme probe diverged between pins");
    let source = std::str::from_utf8(&source)?;
    for marker in [
        "#!/usr/bin/env bun",
        "openSync(\"/dev/tty\", \"r\")",
        "new tty.ReadStream",
        "process.stdout.isTTY",
        "new tty.WriteStream",
        "detectTerminalThemeModeFromBackground",
        "timeoutMs: 500",
        "parseOsc11BackgroundColor",
        "themeModeForBackgroundColor",
        "raw.replaceAll(\"\\x1b\", \"\\\\e\")",
        "stdoutIsTTY",
        "stdinIsTTY",
        "input.destroy()",
        "output.destroy()",
    ] {
        ensure!(
            source.contains(marker),
            "pinned theme probe is missing marker {marker:?}"
        );
    }
    for (path, marker) in [
        ("xtask/src/theme_probe.rs", "pub fn run("),
        ("xtask/src/theme_probe.rs", "fn probe("),
        ("xtask/src/theme_probe.rs", "struct RecordingInput"),
        (
            "crates/workdeck-tui/src/theme_detection.rs",
            "pub fn detect_terminal_theme_mode_from_background(",
        ),
        (
            "crates/workdeck-tui/src/theme_detection.rs",
            "pub fn parse_osc_11_background_color(",
        ),
        (
            "crates/workdeck-tui/src/theme_detection.rs",
            "pub fn theme_mode_for_background_color(",
        ),
        (
            "xtask/src/theme_probe.rs",
            "probe_restores_original_mode_on_resume_read_write_and_flush_errors",
        ),
        (
            "xtask/src/theme_probe.rs",
            "reports_fragmented_background_and_timeout_and_restores_raw_mode",
        ),
    ] {
        let native = fs::read_to_string(repo.join(path))
            .with_context(|| format!("read theme probe native surface {path}"))?;
        ensure!(
            native.contains(marker),
            "theme probe native surface {path} is missing {marker:?}"
        );
    }
    let docs = fs::read_to_string(repo.join("docs/theme-probe-migration.md"))
        .context("read theme probe migration documentation")?;
    for marker in [
        SOURCE_PATH,
        "1,158",
        SOURCE_SHA256,
        "OSC 11",
        "500ms",
        "raw-mode",
        "stdout",
        "stderr",
        "Crossterm",
    ] {
        ensure!(
            docs.contains(marker),
            "theme probe migration documentation is missing {marker:?}"
        );
    }
    Ok(())
}

struct RecordingInput<'a, I> {
    inner: &'a mut I,
    raw: String,
}

impl<I: ThemeProbeInput> ThemeProbeInput for RecordingInput<'_, I> {
    fn is_raw(&self) -> Option<bool> {
        self.inner.is_raw()
    }
    fn set_raw_mode(&mut self, raw: bool) -> io::Result<()> {
        self.inner.set_raw_mode(raw)
    }
    fn resume(&mut self) -> io::Result<()> {
        self.inner.resume()
    }
    fn read_chunk(&mut self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        let chunk = self.inner.read_chunk(timeout)?;
        if let Some(bytes) = &chunk {
            self.raw.push_str(&String::from_utf8_lossy(bytes));
        }
        Ok(chunk)
    }
}

fn probe(
    input: &mut impl ThemeProbeInput,
    output: &mut impl Write,
    stdout_tty: bool,
    stdin_tty: bool,
) -> io::Result<serde_json::Value> {
    let mut input = RecordingInput {
        inner: input,
        raw: String::new(),
    };
    let probe = workdeck_tui::detect_terminal_theme_mode_from_background(
        &mut input,
        output,
        Duration::from_millis(500),
    )?;
    let color = workdeck_tui::parse_osc_11_background_color(&input.raw);
    Ok(serde_json::json!({
        "mode": probe.mode.map(mode_name),
        "color": color.map(|c| serde_json::json!({"red": c.red, "green": c.green, "blue": c.blue})),
        "classified": color.map(workdeck_tui::theme_mode_for_background_color).map(mode_name),
        "raw": input.raw.replace('\u{1b}', "\\e"),
        "stdoutIsTTY": stdout_tty, "stdinIsTTY": stdin_tty,
    }))
}

fn mode_name(mode: workdeck_tui::TerminalThemeMode) -> &'static str {
    match mode {
        workdeck_tui::TerminalThemeMode::Light => "light",
        workdeck_tui::TerminalThemeMode::Dark => "dark",
    }
}

pub fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    if args.next().is_some() {
        bail!("themes probe accepts no options");
    }
    let mut terminal = workdeck_tui::open_controlling_terminal()
        .context("open controlling terminal for theme probe")?;
    let stdout_tty = io::stdout().is_terminal();
    let stdin_tty = io::stdin().is_terminal();
    let report = if stdout_tty {
        probe(
            &mut terminal.input,
            &mut io::stdout(),
            stdout_tty,
            stdin_tty,
        )?
    } else {
        #[cfg(windows)]
        let path = "CONOUT$";
        #[cfg(not(windows))]
        let path = "/dev/tty";
        let mut output = std::fs::OpenOptions::new().write(true).open(path)?;
        probe(&mut terminal.input, &mut output, stdout_tty, stdin_tty)?
    };
    writeln!(io::stderr(), "{}", serde_json::to_string_pretty(&report)?)?;
    Ok(())
}

#[test]
fn probe_restores_original_mode_on_resume_read_write_and_flush_errors() {
    struct Input {
        raw: bool,
        fail: &'static str,
        transitions: Vec<bool>,
    }
    impl ThemeProbeInput for Input {
        fn is_raw(&self) -> Option<bool> {
            Some(self.raw)
        }
        fn set_raw_mode(&mut self, raw: bool) -> io::Result<()> {
            self.transitions.push(raw);
            self.raw = raw;
            Ok(())
        }
        fn resume(&mut self) -> io::Result<()> {
            if self.fail == "resume" {
                return Err(io::Error::other("resume"));
            }
            Ok(())
        }
        fn read_chunk(&mut self, _: Duration) -> io::Result<Option<Vec<u8>>> {
            Err(io::Error::other("read"))
        }
    }
    struct Output(&'static str);
    impl Write for Output {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.0 == "write" {
                return Err(io::Error::other("write"));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            if self.0 == "flush" {
                return Err(io::Error::other("flush"));
            }
            Ok(())
        }
    }
    for original_raw in [false, true] {
        for failure in ["resume", "read", "write", "flush"] {
            let mut input = Input {
                raw: original_raw,
                fail: failure,
                transitions: Vec::new(),
            };
            let error = probe(&mut input, &mut Output(failure), false, false).unwrap_err();
            assert_eq!(error.to_string(), failure);
            assert_eq!(input.raw, original_raw);
            assert_eq!(input.transitions, [true, original_raw]);
        }
    }
}

#[test]
fn native_rust_theme_probe_replaces_both_pinned_scripts() {
    let repo = crate::repo_root().unwrap();
    verify(&repo, BASELINE).unwrap();
}

#[test]
fn reports_fragmented_background_and_timeout_and_restores_raw_mode() {
    struct Input {
        chunks: std::collections::VecDeque<Vec<u8>>,
        raw: bool,
    }
    impl ThemeProbeInput for Input {
        fn is_raw(&self) -> Option<bool> {
            Some(self.raw)
        }
        fn set_raw_mode(&mut self, raw: bool) -> io::Result<()> {
            self.raw = raw;
            Ok(())
        }
        fn read_chunk(&mut self, _: Duration) -> io::Result<Option<Vec<u8>>> {
            Ok(self.chunks.pop_front())
        }
    }
    let mut input = Input {
        chunks: [b"\x1b]11;rgb:ff/".to_vec(), b"ff/ff\x07".to_vec()].into(),
        raw: false,
    };
    let mut output = Vec::new();
    let report = probe(&mut input, &mut output, false, true).unwrap();
    assert_eq!(
        report,
        serde_json::json!({"mode":"light", "classified":"light", "color":{"red":255,"green":255,"blue":255}, "raw":"\\e]11;rgb:ff/ff/ff\u{7}", "stdoutIsTTY":false,"stdinIsTTY":true})
    );
    assert_eq!(output, workdeck_tui::OSC_11_BACKGROUND_QUERY.as_bytes());
    assert!(!input.raw);
    let report = probe(&mut input, &mut Vec::new(), true, false).unwrap();
    assert!(
        report["mode"].is_null() && report["color"].is_null() && report["classified"].is_null()
    );
    assert_eq!(report["raw"], "");
    assert!(!input.raw);
    assert!(run(["unexpected".into()].into_iter()).is_err());
}
