//! Terminal background probing for automatic light/dark theme selection.

use std::fs::File;
use std::io::{self, Read, Stdin, Write};
use std::time::{Duration, Instant};
pub use workdeck_core::TerminalThemeMode;

pub const OSC_11_BACKGROUND_QUERY: &str = "\x1b]11;?\x1b\\";
pub const DEFAULT_THEME_PROBE_TIMEOUT: Duration = Duration::from_millis(150);

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

impl ThemeProbeInput for File {
    fn is_raw(&self) -> Option<bool> {
        crossterm::terminal::is_raw_mode_enabled().ok()
    }

    fn set_raw_mode(&mut self, raw: bool) -> io::Result<()> {
        if raw {
            crossterm::terminal::enable_raw_mode()
        } else {
            crossterm::terminal::disable_raw_mode()
        }
    }

    fn read_chunk(&mut self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        wait_for_file_input(self, timeout)?;
        let mut chunk = vec![0; 256];
        let read = self.read(&mut chunk)?;
        if read == 0 {
            return Ok(None);
        }
        chunk.truncate(read);
        Ok(Some(chunk))
    }
}

impl ThemeProbeInput for Stdin {
    fn is_raw(&self) -> Option<bool> {
        crossterm::terminal::is_raw_mode_enabled().ok()
    }

    fn set_raw_mode(&mut self, raw: bool) -> io::Result<()> {
        if raw {
            crossterm::terminal::enable_raw_mode()
        } else {
            crossterm::terminal::disable_raw_mode()
        }
    }

    fn read_chunk(&mut self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        wait_for_stdin_input(self, timeout)?;
        let mut chunk = vec![0; 256];
        let read = self.read(&mut chunk)?;
        if read == 0 {
            return Ok(None);
        }
        chunk.truncate(read);
        Ok(Some(chunk))
    }
}

#[cfg(unix)]
fn wait_for_file_input(file: &File, timeout: Duration) -> io::Result<()> {
    use std::os::fd::AsRawFd;

    wait_for_unix_input(file.as_raw_fd(), timeout)
}

#[cfg(unix)]
fn wait_for_stdin_input(stdin: &Stdin, timeout: Duration) -> io::Result<()> {
    use std::os::fd::AsRawFd;

    wait_for_unix_input(stdin.as_raw_fd(), timeout)
}

#[cfg(unix)]
fn wait_for_unix_input(fd: std::os::fd::RawFd, timeout: Duration) -> io::Result<()> {
    let mut descriptor = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let timeout_ms = timeout.as_millis().min(i32::MAX as u128) as i32;
    // SAFETY: descriptor points to one initialized pollfd for the duration of the call.
    let result = unsafe { libc::poll(&mut descriptor, 1, timeout_ms) };
    if result < 0 {
        return Err(io::Error::last_os_error());
    }
    if result == 0 {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "terminal probe timed out",
        ));
    }
    if descriptor.revents & libc::POLLNVAL != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "terminal probe input is invalid",
        ));
    }
    if descriptor.revents & libc::POLLERR != 0 {
        return Err(io::Error::other("terminal probe input reported an error"));
    }
    Ok(())
}

#[cfg(windows)]
fn wait_for_file_input(file: &File, timeout: Duration) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;

    wait_for_windows_input(file.as_raw_handle() as _, timeout)
}

#[cfg(windows)]
fn wait_for_stdin_input(stdin: &Stdin, timeout: Duration) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;

    wait_for_windows_input(stdin.as_raw_handle() as _, timeout)
}

#[cfg(windows)]
fn wait_for_windows_input(
    handle: windows_sys::Win32::Foundation::HANDLE,
    timeout: Duration,
) -> io::Result<()> {
    use windows_sys::Win32::Foundation::{WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::WaitForSingleObject;

    let timeout_ms = timeout.as_millis().min(u32::MAX as u128) as u32;
    // SAFETY: the caller provides a live terminal input handle for the duration of the wait.
    let result = unsafe { WaitForSingleObject(handle, timeout_ms) };
    match result {
        WAIT_OBJECT_0 => Ok(()),
        WAIT_TIMEOUT => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "terminal probe timed out",
        )),
        WAIT_FAILED => Err(io::Error::last_os_error()),
        result => Err(io::Error::other(format!(
            "terminal probe wait returned status {result}"
        ))),
    }
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
            let chunk = match input.read_chunk(remaining) {
                Ok(Some(chunk)) => chunk,
                Ok(None) => return Ok(None),
                Err(error) if error.kind() == io::ErrorKind::TimedOut => return Ok(None),
                Err(error) => return Err(error),
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
