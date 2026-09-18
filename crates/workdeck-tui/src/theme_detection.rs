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

#[cfg(all(unix, not(target_os = "macos")))]
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

// Darwin's /dev/tty alias may report POLLNVAL through poll despite being a valid
// controlling-terminal descriptor. select supports that descriptor as well as stdin.
#[cfg(target_os = "macos")]
fn wait_for_unix_input(fd: std::os::fd::RawFd, timeout: Duration) -> io::Result<()> {
    if fd < 0 || fd as usize >= libc::FD_SETSIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "terminal probe descriptor is out of range",
        ));
    }
    // SAFETY: fd_set is initialized before FD_SET, and fd is within its supported range.
    let mut descriptors = unsafe { std::mem::zeroed::<libc::fd_set>() };
    unsafe {
        libc::FD_SET(fd, &mut descriptors);
    }
    let mut interval = libc::timeval {
        tv_sec: timeout.as_secs().min(libc::time_t::MAX as u64) as libc::time_t,
        tv_usec: timeout.subsec_micros() as libc::suseconds_t,
    };
    // SAFETY: all pointers remain valid through this bounded select call.
    let ready = unsafe {
        libc::select(
            fd + 1,
            &mut descriptors,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut interval,
        )
    };
    if ready < 0 {
        return Err(io::Error::last_os_error());
    }
    if ready == 0 {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "terminal probe timed out",
        ));
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
    let mut first_hex = None;
    for (start, prefix) in sequence.match_indices("\x1b]11;") {
        let tail = &sequence[start + prefix.len()..];
        let end = match (tail.find('\x07'), tail.find("\x1b\\")) {
            (Some(a), Some(b)) => a.min(b),
            (Some(end), None) | (None, Some(end)) => end,
            (None, None) => continue,
        };
        let payload = &tail[..end];
        if let Some(color) = parse_background_payload(payload) {
            // Hunk searches the entire buffer for RGB first, then falls back to hex.
            if !payload.starts_with('#') {
                return Some(color);
            }
            first_hex.get_or_insert(color);
        }
    }
    first_hex
}

fn parse_background_payload(payload: &str) -> Option<RgbColor> {
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
///
/// The probe stops reading the moment the accumulated bytes can no longer be
/// part of a reply stream, and returns any consumed bytes that were not reply
/// chatter so the caller can replay them as user input instead of dropping
/// the first keys typed during startup.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TerminalThemeProbe {
    pub mode: Option<TerminalThemeMode>,
    pub replay: Vec<u8>,
}

/// Match a user-key escape encoding (arrows, home/end, editing keys, and
/// modifier variants) at `at`. Terminal replies such as DA or kitty protocol
/// reports carry `?` parameters and never match.
fn key_escape_len(bytes: &[u8], at: usize) -> Option<usize> {
    if bytes.get(at) != Some(&0x1b) {
        return None;
    }
    let rest = &bytes[at + 1..];
    let final_after_params = |params: &[u8], tilde: bool| -> Option<usize> {
        // Parameters must be digits or modifier separators; a key encoding
        // never contains private-mode markers like '?'. Tilde finals need at
        // least one parameter digit, arrows and home/end need none.
        if tilde && params.is_empty() {
            return None;
        }
        if !params
            .iter()
            .all(|b| b.is_ascii_digit() || matches!(b, b';' | b':' | b'<' | b'>' | b'*'))
        {
            return None;
        }
        match rest.get(1 + params.len()) {
            Some(b'A'..=b'D' | b'H' | b'F' | b'~') => Some(2 + params.len() + 1),
            _ => None,
        }
    };
    match rest.first() {
        Some(b'[') => {
            let params = rest[1..]
                .iter()
                .take_while(|b| b.is_ascii_digit() || matches!(b, b';' | b':' | b'<' | b'>' | b'*'))
                .count();
            final_after_params(&rest[1..1 + params], rest.get(1 + params) == Some(&b'~'))
        }
        Some(b'O') => match rest.get(1) {
            Some(b'A'..=b'D') => Some(3),
            _ => None,
        },
        _ => None,
    }
}

/// Split consumed input into complete escape-sequence chatter and a trailing
/// run that cannot belong to one. The trailing run starts either an
/// incomplete escape sequence (still plausible reply chatter) or a foreign
/// byte such as a user keystroke. Key-encoded escapes count as foreign input,
/// not chatter, so typed arrows survive the probe.
fn split_escape_chatter(bytes: &[u8]) -> (&[u8], &[u8]) {
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != 0x1b {
            break;
        }
        if key_escape_len(bytes, index).is_some() {
            return (&bytes[..index], &bytes[index..]);
        }
        match bytes.get(index + 1) {
            Some(b']') => match find_osc_end(bytes, index + 2) {
                Some(end) => index = end,
                None => return (&bytes[..index], &bytes[index..]),
            },
            Some(b'[') => match find_csi_end(bytes, index + 2) {
                Some(end) => index = end,
                None => return (&bytes[..index], &bytes[index..]),
            },
            Some(_) => match bytes.get(index + 2) {
                Some(_) => index += 3,
                None => return (&bytes[..index], &bytes[index..]),
            },
            None => return (&bytes[..index], &bytes[index..]),
        }
    }
    (&bytes[..index], &bytes[index..])
}

fn find_osc_end(bytes: &[u8], from: usize) -> Option<usize> {
    let mut index = from;
    while index < bytes.len() {
        match bytes[index] {
            b'\x07' => return Some(index + 1),
            0x1b if bytes.get(index + 1) == Some(&b'\\') => return Some(index + 2),
            _ => index += 1,
        }
    }
    None
}

fn find_csi_end(bytes: &[u8], from: usize) -> Option<usize> {
    let mut index = from;
    while index < bytes.len() {
        let byte = bytes[index];
        if (0x40..=0x7e).contains(&byte) {
            return Some(index + 1);
        }
        index += 1;
    }
    None
}

/// Find the first complete OSC 11 color reply, preferring RGB over hex across
/// the whole buffer exactly like the source parser, and report the buffer
/// offset where the chosen reply ends.
fn scan_complete_osc_11_replies(bytes: &[u8]) -> Option<(RgbColor, usize)> {
    let mut first_hex: Option<(RgbColor, usize)> = None;
    let mut index = 0;
    while let Some(start) = find_subslice(bytes, b"\x1b]11;", index) {
        let payload_from = start + b"\x1b]11;".len();
        let Some(end) = find_osc_end(bytes, payload_from) else {
            // An unterminated reply may still surround a later complete one,
            // exactly like the source's match_indices scan.
            index = start + 1;
            continue;
        };
        let terminator = if bytes.get(end - 1) == Some(&b'\x07') {
            1
        } else {
            2
        };
        let payload = &bytes[payload_from..end - terminator];
        if let Some(color) = parse_background_payload(&String::from_utf8_lossy(payload)) {
            if payload.first() != Some(&b'#') {
                return Some((color, end));
            }
            first_hex.get_or_insert((color, end));
        }
        // Failed payloads can swallow later replies; keep scanning from the
        // next byte so inner occurrences still get their own match.
        index = start + 1;
    }
    first_hex
}

fn find_subslice(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    (from..=haystack.len() - needle.len())
        .find(|&index| &haystack[index..index + needle.len()] == needle)
}

fn foreign_tail(bytes: &[u8]) -> Option<&[u8]> {
    let (_, tail) = split_escape_chatter(bytes);
    match tail.first() {
        None => None,
        Some(&0x1b) => key_escape_len(tail, 0).map(|_| tail),
        Some(_) => Some(tail),
    }
}

/// Decode terminal input bytes captured during the startup theme probe into
/// key events, so keys typed while the probe held the input are replayed into
/// the review instead of being dropped. Unrecognized sequences are skipped;
/// modifier components of replayed escapes are simplified to none.
#[must_use]
pub fn decode_replayed_terminal_input(bytes: &[u8]) -> Vec<crossterm::event::KeyEvent> {
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    let mut events = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        match byte {
            0x1b => {
                if let Some(length) = key_escape_len(bytes, index) {
                    if let Some(event) = decode_key_escape(&bytes[index..index + length]) {
                        events.push(event);
                    }
                    index += length;
                } else {
                    events.push(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
                    index += 1;
                }
            }
            b'\r' | b'\n' => {
                events.push(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                index += 1;
            }
            b'\t' => {
                events.push(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
                index += 1;
            }
            0x7f | 0x08 => {
                events.push(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
                index += 1;
            }
            0x00..=0x1f => index += 1,
            _ => {
                let text = String::from_utf8_lossy(&bytes[index..]);
                let Some(character) = text.chars().next() else {
                    index += 1;
                    continue;
                };
                events.push(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
                index += character.len_utf8().max(1);
            }
        }
    }
    events
        .into_iter()
        .map(|mut event| {
            event.kind = KeyEventKind::Press;
            event
        })
        .collect()
}

fn decode_key_escape(sequence: &[u8]) -> Option<crossterm::event::KeyEvent> {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let final_byte = *sequence.last()?;
    let code = match sequence.get(1) {
        Some(b'O') => match final_byte {
            b'A' => KeyCode::Up,
            b'B' => KeyCode::Down,
            b'C' => KeyCode::Right,
            b'D' => KeyCode::Left,
            _ => return None,
        },
        Some(b'[') => match final_byte {
            b'A' => KeyCode::Up,
            b'B' => KeyCode::Down,
            b'C' => KeyCode::Right,
            b'D' => KeyCode::Left,
            b'H' => KeyCode::Home,
            b'F' => KeyCode::End,
            b'~' => {
                let params = &sequence[2..sequence.len() - 1];
                let first = params.split(|byte| *byte == b';').next()?;
                let number = std::str::from_utf8(first).ok()?.parse::<u8>().ok()?;
                match number {
                    1 | 7 => KeyCode::Home,
                    2 => KeyCode::Insert,
                    3 => KeyCode::Delete,
                    4 | 8 => KeyCode::End,
                    5 => KeyCode::PageUp,
                    6 => KeyCode::PageDown,
                    _ => return None,
                }
            }
            _ => return None,
        },
        _ => return None,
    };
    Some(KeyEvent::new(code, KeyModifiers::NONE))
}

pub fn detect_terminal_theme_mode_from_background(
    input: &mut impl ThemeProbeInput,
    output: &mut impl Write,
    timeout: Duration,
) -> io::Result<TerminalThemeProbe> {
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
                let replay = foreign_tail(&response).map_or_else(Vec::new, <[u8]>::to_vec);
                return Ok(TerminalThemeProbe { mode: None, replay });
            };
            let chunk = match input.read_chunk(remaining) {
                Ok(Some(chunk)) => chunk,
                Ok(None) => {
                    let replay = foreign_tail(&response).map_or_else(Vec::new, <[u8]>::to_vec);
                    return Ok(TerminalThemeProbe { mode: None, replay });
                }
                Err(error) if error.kind() == io::ErrorKind::TimedOut => {
                    let replay = foreign_tail(&response).map_or_else(Vec::new, <[u8]>::to_vec);
                    return Ok(TerminalThemeProbe { mode: None, replay });
                }
                Err(error) => return Err(error),
            };
            response.extend_from_slice(&chunk);
            if let Some((color, end)) = scan_complete_osc_11_replies(&response) {
                let replay = foreign_tail(&response[end..]).map_or_else(Vec::new, <[u8]>::to_vec);
                return Ok(TerminalThemeProbe {
                    mode: Some(theme_mode_for_background_color(color)),
                    replay,
                });
            }
            if let Some(replay) = foreign_tail(&response) {
                // Bytes that cannot be reply chatter arrived before any reply,
                // typically the first keys typed during startup. Stop reading
                // now and hand them back instead of consuming more input.
                return Ok(TerminalThemeProbe {
                    mode: None,
                    replay: replay.to_vec(),
                });
            }
        }
    })();
    let restore = was_raw.map_or(Ok(()), |was_raw| input.set_raw_mode(was_raw));
    match (result, restore) {
        (Ok(probe), Ok(())) => Ok(probe),
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
    fn osc_scanning_matches_both_frozen_source_captures() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/osc-background-scan.json"
        ))
        .unwrap();
        let captures = fixture["captures"].as_array().unwrap();
        assert_eq!(captures.len(), 2);
        for capture in captures {
            assert_eq!(capture["exitCode"], 0);
            let cases = capture["cases"].as_array().unwrap();
            assert_eq!(cases.len(), 5);
            for case in cases {
                let actual =
                    parse_osc_11_background_color(case["input"].as_str().unwrap()).map(|color| {
                        serde_json::json!({
                            "red": color.red, "green": color.green, "blue": color.blue
                        })
                    });
                assert_eq!(
                    serde_json::to_value(actual).unwrap(),
                    case["expected"],
                    "{}: {}",
                    capture["kind"],
                    case["input"]
                );
            }
        }
    }

    #[test]
    fn osc_scan_skips_invalid_prefixes_and_preserves_source_rgb_precedence() {
        let white = Some(RgbColor {
            red: 255,
            green: 255,
            blue: 255,
        });
        let black = Some(RgbColor {
            red: 0,
            green: 0,
            blue: 0,
        });
        assert_eq!(
            parse_osc_11_background_color("\x1b]11;?\x1b\\\x1b]11;rgb:ff/ff/ff\x07"),
            white
        );
        assert_eq!(
            parse_osc_11_background_color("\x1b]11;#ffffff\x07\x1b]11;rgb:00/00/00\x1b\\"),
            black
        );
        assert_eq!(
            parse_osc_11_background_color("\x1b]11;rgb:ff/ff/ff\x1b\\noise\x07"),
            white
        );
        assert_eq!(
            parse_osc_11_background_color("\x1b]11;rgb:xx/00/00\x07\x1b]11;#ffffff\x07"),
            white
        );
        assert_eq!(
            parse_osc_11_background_color("\x1b]11;unfinished\x1b]11;#ffffff\x07"),
            white
        );
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
    fn fragmented_probe_settles_on_first_complete_response_not_future_chunks() {
        let hex = b"\x1b]11;#ffffff\x07";
        let rgb = b"\x1b]11;rgb:00/00/00\x1b\\";
        let response = [hex.as_slice(), rgb.as_slice()].concat();
        for split in 1..response.len() {
            let mut input = FakeThemeInput {
                raw: false,
                chunks: VecDeque::from([response[..split].to_vec(), response[split..].to_vec()]),
                raw_transitions: Vec::new(),
            };
            let mode = detect_terminal_theme_mode_from_background(
                &mut input,
                &mut Vec::new(),
                Duration::from_secs(1),
            )
            .unwrap();
            let settled_on_hex = split >= hex.len();
            assert_eq!(
                mode.mode,
                Some(if settled_on_hex {
                    TerminalThemeMode::Light
                } else {
                    TerminalThemeMode::Dark
                }),
                "split at {split}"
            );
            assert!(mode.replay.is_empty(), "split at {split}");
            assert_eq!(input.chunks.len(), usize::from(settled_on_hex));
            assert_eq!(input.raw_transitions, [true, false]);
        }
        let valid = b"\x1b]11;?\x1b\\\x1b]11;rgb:ff/ff/ff\x07";
        for split in 1..valid.len() {
            let mut input = FakeThemeInput {
                raw: true,
                chunks: VecDeque::from([valid[..split].to_vec(), valid[split..].to_vec()]),
                raw_transitions: Vec::new(),
            };
            assert_eq!(
                detect_terminal_theme_mode_from_background(
                    &mut input,
                    &mut Vec::new(),
                    Duration::from_secs(1),
                )
                .unwrap()
                .mode,
                Some(TerminalThemeMode::Light),
                "split at {split}"
            );
            assert!(input.chunks.is_empty());
            assert_eq!(input.raw_transitions, [true, true]);
        }
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
            TerminalThemeProbe {
                mode: Some(TerminalThemeMode::Dark),
                replay: Vec::new(),
            }
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
            TerminalThemeProbe::default()
        );
        assert_eq!(input.raw_transitions, vec![true, false]);
    }

    #[test]
    fn a_foreign_first_chunk_aborts_the_probe_and_is_replayed_not_eaten() {
        let mut input = FakeThemeInput {
            raw: false,
            chunks: VecDeque::from([b"q".to_vec(), b"\x1b]11;rgb:00/00/00\x07".to_vec()]),
            raw_transitions: Vec::new(),
        };
        assert_eq!(
            detect_terminal_theme_mode_from_background(
                &mut input,
                &mut Vec::new(),
                Duration::from_secs(1),
            )
            .unwrap(),
            TerminalThemeProbe {
                mode: None,
                replay: b"q".to_vec(),
            }
        );
        // The later reply chunk must remain unconsumed for the caller.
        assert_eq!(input.chunks.len(), 1);
        assert_eq!(input.raw_transitions, vec![true, false]);
    }

    #[test]
    fn a_reply_chunk_with_trailing_user_keys_replays_the_tail() {
        let mut input = FakeThemeInput {
            raw: false,
            chunks: VecDeque::from([b"\x1b]11;#ffffff\x07j".to_vec()]),
            raw_transitions: Vec::new(),
        };
        assert_eq!(
            detect_terminal_theme_mode_from_background(
                &mut input,
                &mut Vec::new(),
                Duration::from_secs(1),
            )
            .unwrap(),
            TerminalThemeProbe {
                mode: Some(TerminalThemeMode::Light),
                replay: b"j".to_vec(),
            }
        );
    }

    #[test]
    fn a_timeout_with_only_partial_chatter_replays_nothing() {
        let mut input = FakeThemeInput {
            raw: false,
            chunks: VecDeque::from([b"\x1b]11;rgb:0".to_vec()]),
            raw_transitions: Vec::new(),
        };
        assert_eq!(
            detect_terminal_theme_mode_from_background(
                &mut input,
                &mut Vec::new(),
                Duration::from_millis(1),
            )
            .unwrap(),
            TerminalThemeProbe {
                mode: None,
                replay: Vec::new(),
            }
        );
    }

    #[test]
    fn an_arrow_key_during_the_probe_window_is_replayed() {
        let mut input = FakeThemeInput {
            raw: false,
            chunks: VecDeque::from([b"\x1b[Bmore-keys".to_vec()]),
            raw_transitions: Vec::new(),
        };
        assert_eq!(
            detect_terminal_theme_mode_from_background(
                &mut input,
                &mut Vec::new(),
                Duration::from_secs(1),
            )
            .unwrap()
            .replay,
            b"\x1b[Bmore-keys".to_vec()
        );
    }

    #[test]
    fn escape_chatter_splitting_separates_replies_from_foreign_tails() {
        let reply = b"\x1b]11;?\x1b\\\x1b]11;#ffffff\x07";
        assert_eq!(split_escape_chatter(reply).1, b"");
        assert_eq!(foreign_tail(reply), None);
        let with_keys = b"\x1b]11;?\x1b\\jk\x1b";
        assert_eq!(split_escape_chatter(with_keys).1, b"jk\x1b");
        assert_eq!(foreign_tail(with_keys), Some(&b"jk\x1b"[..]));
        let incomplete = b"\x1b]11;rgb:0";
        assert_eq!(foreign_tail(incomplete), None);
        let leading_key = b"]\x1b]11;#ffffff\x07";
        assert_eq!(
            foreign_tail(leading_key),
            Some(&b"]\x1b]11;#ffffff\x07"[..])
        );
    }
}
