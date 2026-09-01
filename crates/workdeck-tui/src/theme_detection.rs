//! Terminal background probing for automatic light/dark theme selection.

use std::io::{self, Write};
use std::time::{Duration, Instant};

pub const OSC_11_BACKGROUND_QUERY: &str = "\x1b]11;?\x1b\\";
pub const DEFAULT_THEME_PROBE_TIMEOUT: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalThemeMode {
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RgbColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

/// Pull-oriented terminal input seam used by native Unix and Windows adapters.
pub trait ThemeProbeInput {
    fn is_raw(&self) -> Option<bool>;
    fn set_raw_mode(&mut self, raw: bool) -> io::Result<()>;
    fn resume(&mut self) -> io::Result<()> {
        Ok(())
    }
    /// Return one available byte chunk, or `None` after the supplied wait expires.
    fn read_chunk(&mut self, timeout: Duration) -> io::Result<Option<Vec<u8>>>;
}

/// Parse common xterm OSC 11 background-color responses.
#[must_use]
pub fn parse_osc_11_background_color(sequence: &str) -> Option<RgbColor> {
    let payload_start = sequence.find("\x1b]11;")? + "\x1b]11;".len();
    let tail = &sequence[payload_start..];
    let payload_end = tail.find('\x07').or_else(|| tail.find("\x1b\\"))?;
    let payload = &tail[..payload_end];
    if payload
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("rgb:"))
    {
        let channels = &payload[4..];
        let mut channels = channels.split('/');
        let red = parse_hex_channel(channels.next()?)?;
        let green = parse_hex_channel(channels.next()?)?;
        let blue = parse_hex_channel(channels.next()?)?;
        if channels.next().is_some() {
            return None;
        }
        return Some(RgbColor { red, green, blue });
    }
    let hex = payload.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(RgbColor {
        red: u8::from_str_radix(&hex[0..2], 16).ok()?,
        green: u8::from_str_radix(&hex[2..4], 16).ok()?,
        blue: u8::from_str_radix(&hex[4..6], 16).ok()?,
    })
}

fn parse_hex_channel(channel: &str) -> Option<u8> {
    if !(2..=4).contains(&channel.len()) || !channel.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(channel, 16).ok()?;
    let max = 16_u32
        .pow(u32::try_from(channel.len()).ok()?)
        .saturating_sub(1);
    u8::try_from((value.saturating_mul(255) + max / 2) / max).ok()
}

/// Classify an sRGB background using WCAG relative luminance.
#[must_use]
pub fn theme_mode_for_background_color(color: RgbColor) -> TerminalThemeMode {
    let linear = [color.red, color.green, color.blue].map(|component| {
        let normalized = f64::from(component) / 255.0;
        if normalized <= 0.039_28 {
            normalized / 12.92
        } else {
            ((normalized + 0.055) / 1.055).powf(2.4)
        }
    });
    let luminance = 0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2];
    if luminance > 0.5 {
        TerminalThemeMode::Light
    } else {
        TerminalThemeMode::Dark
    }
}

/// Query OSC 11 on the same controlling-terminal input used for mouse events.
///
/// The input's prior raw-mode state is restored on success, timeout, or I/O
/// failure. Piped diff stdin is therefore never consumed by theme detection.
pub fn detect_terminal_theme_mode_from_background(
    input: &mut impl ThemeProbeInput,
    output: &mut impl Write,
    timeout: Duration,
) -> io::Result<Option<TerminalThemeMode>> {
    let was_raw = input.is_raw();
    let result = (|| {
        input.set_raw_mode(true)?;
        input.resume()?;
        output.write_all(OSC_11_BACKGROUND_QUERY.as_bytes())?;
        output.flush()?;

        let started = Instant::now();
        let mut response = Vec::new();
        loop {
            let Some(remaining) = timeout.checked_sub(started.elapsed()) else {
                return Ok(None);
            };
            let Some(chunk) = input.read_chunk(remaining)? else {
                return Ok(None);
            };
            response.extend_from_slice(&chunk);
            if let Some(color) = parse_osc_11_background_color(&String::from_utf8_lossy(&response))
            {
                return Ok(Some(theme_mode_for_background_color(color)));
            }
        }
    })();
    let restore = was_raw.map_or(Ok(()), |was_raw| input.set_raw_mode(was_raw));
    match (result, restore) {
        (Ok(mode), Ok(())) => Ok(mode),
        (Err(error), _) | (Ok(_), Err(error)) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[derive(Debug)]
    struct FakeThemeInput {
        raw: bool,
        chunks: VecDeque<Vec<u8>>,
        raw_transitions: Vec<bool>,
    }

    impl ThemeProbeInput for FakeThemeInput {
        fn is_raw(&self) -> Option<bool> {
            Some(self.raw)
        }

        fn set_raw_mode(&mut self, raw: bool) -> io::Result<()> {
            self.raw = raw;
            self.raw_transitions.push(raw);
            Ok(())
        }

        fn read_chunk(&mut self, _timeout: Duration) -> io::Result<Option<Vec<u8>>> {
            Ok(self.chunks.pop_front())
        }
    }

    #[test]
    fn parses_osc_11_rgb_and_hex_responses() {
        assert_eq!(
            parse_osc_11_background_color("\x1b]11;rgb:0000/1111/2222\x1b\\"),
            Some(RgbColor {
                red: 0,
                green: 17,
                blue: 34,
            })
        );
        assert_eq!(
            parse_osc_11_background_color("\x1b]11;#ffffff\x07"),
            Some(RgbColor {
                red: 255,
                green: 255,
                blue: 255,
            })
        );
        assert_eq!(
            parse_osc_11_background_color("prefix\x1b]11;RGB:FF/00/7f\x07"),
            Some(RgbColor {
                red: 255,
                green: 0,
                blue: 127,
            })
        );
    }

    #[test]
    fn classifies_dark_and_light_backgrounds() {
        assert_eq!(
            theme_mode_for_background_color(RgbColor {
                red: 12,
                green: 12,
                blue: 12,
            }),
            TerminalThemeMode::Dark
        );
        assert_eq!(
            theme_mode_for_background_color(RgbColor {
                red: 245,
                green: 245,
                blue: 245,
            }),
            TerminalThemeMode::Light
        );
    }

    #[test]
    fn detects_from_the_queried_input_and_restores_raw_mode() {
        let mut input = FakeThemeInput {
            raw: false,
            chunks: VecDeque::from([b"\x1b]11;rgb:0000/0000/0000\x1b\\".to_vec()]),
            raw_transitions: Vec::new(),
        };
        let mut output = Vec::new();
        assert_eq!(
            detect_terminal_theme_mode_from_background(
                &mut input,
                &mut output,
                Duration::from_millis(50),
            )
            .unwrap(),
            Some(TerminalThemeMode::Dark)
        );
        assert_eq!(output, OSC_11_BACKGROUND_QUERY.as_bytes());
        assert_eq!(input.raw_transitions, vec![true, false]);
        assert!(!input.raw);
    }

    #[test]
    fn a_timed_out_probe_returns_none_and_restores_raw_mode() {
        let mut input = FakeThemeInput {
            raw: false,
            chunks: VecDeque::new(),
            raw_transitions: Vec::new(),
        };
        let mut output = Vec::new();
        assert_eq!(
            detect_terminal_theme_mode_from_background(
                &mut input,
                &mut output,
                Duration::from_millis(1),
            )
            .unwrap(),
            None
        );
        assert_eq!(input.raw_transitions, vec![true, false]);
    }
}
