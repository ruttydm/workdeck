//! Partial MIT port of Hunk scripts/probe-terminal-theme.ts (Modem Labs Inc.).
use anyhow::{Context, Result, bail};
use std::io::{self, IsTerminal, Write};
use std::time::Duration;
use workdeck_tui::ThemeProbeInput;

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
    let mode = workdeck_tui::detect_terminal_theme_mode_from_background(
        &mut input,
        output,
        Duration::from_millis(500),
    )?;
    let color = workdeck_tui::parse_osc_11_background_color(&input.raw);
    Ok(serde_json::json!({
        "mode": mode.map(mode_name),
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
