//! Bounded diagnostic capture for trusted native extension processes.

use std::collections::VecDeque;
use std::io::{self, BufRead};
use std::sync::{Arc, Mutex};

pub const MAX_EXTENSION_LOG_LINE_BYTES: usize = 16 * 1024;
pub const MAX_EXTENSION_LOG_ENTRIES: usize = 1024;
/// Total retained UTF-8 bytes, including each entry's extension ID.
pub const MAX_EXTENSION_LOG_BYTES: usize = 1024 * 1024;
const TRUNCATED: &str = "... [truncated]";

/// One line written by an extension to its reserved stderr log stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionLogEntry {
    pub extension_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtensionLogStats {
    pub retained_entries: usize,
    pub retained_bytes: usize,
    pub dropped_entries: u64,
    pub truncated_lines: u64,
}

#[derive(Debug, Default)]
struct LogState {
    entries: VecDeque<ExtensionLogEntry>,
    retained_bytes: usize,
    dropped_entries: u64,
    truncated_lines: u64,
}

/// Shared ordered collection of the newest diagnostics in one load result.
/// Retention limits never stop draining a child's stderr pipe.
#[derive(Debug, Clone, Default)]
pub struct ExtensionLogHub {
    state: Arc<Mutex<LogState>>,
}

impl ExtensionLogHub {
    fn record(&self, extension_id: &str, message: String, truncated: bool) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if truncated {
            state.truncated_lines = state.truncated_lines.saturating_add(1);
        }
        let Some(bytes) = extension_id
            .len()
            .checked_add(message.len())
            .filter(|bytes| *bytes <= MAX_EXTENSION_LOG_BYTES)
        else {
            state.dropped_entries = state.dropped_entries.saturating_add(1);
            return;
        };
        while state.entries.len() >= MAX_EXTENSION_LOG_ENTRIES
            || state.retained_bytes + bytes > MAX_EXTENSION_LOG_BYTES
        {
            let removed = state
                .entries
                .pop_front()
                .expect("retained log entry exists");
            state.retained_bytes -= removed.extension_id.len() + removed.message.len();
            state.dropped_entries = state.dropped_entries.saturating_add(1);
        }
        state.retained_bytes += bytes;
        state.entries.push_back(ExtensionLogEntry {
            extension_id: extension_id.to_owned(),
            message,
        });
    }

    #[must_use]
    pub fn snapshot(&self) -> Vec<ExtensionLogEntry> {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .entries
            .iter()
            .cloned()
            .collect()
    }

    #[must_use]
    pub fn stats(&self) -> ExtensionLogStats {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        ExtensionLogStats {
            retained_entries: state.entries.len(),
            retained_bytes: state.retained_bytes,
            dropped_entries: state.dropped_entries,
            truncated_lines: state.truncated_lines,
        }
    }
}

fn read_log_line(reader: &mut impl BufRead) -> io::Result<Option<(String, bool)>> {
    // One extra byte permits a full-sized payload followed by CRLF.
    let capacity = MAX_EXTENSION_LOG_LINE_BYTES + 1;
    let mut bytes = Vec::new();
    let mut seen = false;
    let mut truncated = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            if !seen {
                return Ok(None);
            }
            break;
        }
        seen = true;
        let newline = available.iter().position(|byte| *byte == b'\n');
        let content = newline.unwrap_or(available.len());
        let retained = content.min(capacity - bytes.len());
        bytes.extend_from_slice(&available[..retained]);
        truncated |= retained < content;
        reader.consume(content + usize::from(newline.is_some()));
        if newline.is_some() {
            break;
        }
    }
    if !truncated && bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    let mut message = String::from_utf8_lossy(&bytes).into_owned();
    truncated |= message.len() > MAX_EXTENSION_LOG_LINE_BYTES;
    if truncated {
        let mut end = (MAX_EXTENSION_LOG_LINE_BYTES - TRUNCATED.len()).min(message.len());
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
        message.push_str(TRUNCATED);
        message.shrink_to_fit();
    }
    Ok(Some((message, truncated)))
}

pub(crate) fn capture_stderr(
    mut reader: impl BufRead,
    hub: &ExtensionLogHub,
    extension_id: &str,
) -> io::Result<()> {
    while let Some((message, truncated)) = read_log_line(&mut reader)? {
        hub.record(extension_id, message, truncated);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor, Read};

    #[test]
    fn preserves_line_order_empty_lines_crlf_lossy_utf8_and_final_fragment() {
        let hub = ExtensionLogHub::default();
        capture_stderr(Cursor::new(b"first\r\n\ninvalid:\xff\nlast"), &hub, "one").unwrap();
        assert_eq!(
            hub.snapshot()
                .iter()
                .map(|entry| entry.message.as_str())
                .collect::<Vec<_>>(),
            ["first", "", "invalid:\u{fffd}", "last"]
        );
        assert!(
            hub.snapshot()
                .iter()
                .all(|entry| entry.extension_id == "one")
        );
        assert_eq!(hub.stats().truncated_lines, 0);
        assert_eq!(hub.stats().dropped_entries, 0);
    }

    #[test]
    fn accepts_exact_line_limit_and_marks_overflow_without_losing_the_next_line() {
        let mut input = vec![b'x'; MAX_EXTENSION_LOG_LINE_BYTES];
        input.extend_from_slice(b"\r\n");
        input.extend(std::iter::repeat_n(b'y', MAX_EXTENSION_LOG_LINE_BYTES + 1));
        input.extend_from_slice(b"\nnext\n");
        let hub = ExtensionLogHub::default();
        capture_stderr(BufReader::with_capacity(7, Cursor::new(input)), &hub, "one").unwrap();
        let entries = hub.snapshot();
        assert_eq!(entries[0].message, "x".repeat(MAX_EXTENSION_LOG_LINE_BYTES));
        assert_eq!(entries[1].message.len(), MAX_EXTENSION_LOG_LINE_BYTES);
        assert!(entries[1].message.ends_with(TRUNCATED));
        assert_eq!(entries[2].message, "next");
        assert_eq!(hub.stats().truncated_lines, 1);
    }

    #[test]
    fn drains_large_unterminated_input_and_bounds_utf8_replacement_growth() {
        let hub = ExtensionLogHub::default();
        let input = io::repeat(0xff).take(4 * 1024 * 1024);
        capture_stderr(BufReader::with_capacity(31, input), &hub, "one").unwrap();
        let entries = hub.snapshot();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].message.len() <= MAX_EXTENSION_LOG_LINE_BYTES);
        assert!(entries[0].message.ends_with(TRUNCATED));
        assert_eq!(hub.stats().truncated_lines, 1);
    }

    #[test]
    fn retention_evicts_oldest_entries_and_counts_empty_line_floods() {
        let hub = ExtensionLogHub::default();
        for _ in 0..MAX_EXTENSION_LOG_ENTRIES + 7 {
            capture_stderr(Cursor::new(b"\n"), &hub, "").unwrap();
        }
        assert_eq!(
            hub.stats(),
            ExtensionLogStats {
                retained_entries: MAX_EXTENSION_LOG_ENTRIES,
                retained_bytes: 0,
                dropped_entries: 7,
                truncated_lines: 0,
            }
        );
        capture_stderr(Cursor::new(b"newest\n"), &hub, "two").unwrap();
        assert_eq!(hub.snapshot().last().unwrap().message, "newest");
        assert_eq!(hub.stats().dropped_entries, 8);
    }

    #[test]
    fn retention_byte_limit_counts_extension_ids_and_does_not_stop_capture() {
        let hub = ExtensionLogHub::default();
        let input = format!("{}\n", "a".repeat(MAX_EXTENSION_LOG_LINE_BYTES));
        for _ in 0..100 {
            capture_stderr(Cursor::new(input.as_bytes()), &hub, "extension").unwrap();
        }
        let entries = hub.snapshot();
        let bytes: usize = entries
            .iter()
            .map(|entry| entry.extension_id.len() + entry.message.len())
            .sum();
        assert_eq!(hub.stats().retained_bytes, bytes);
        assert!(hub.stats().retained_bytes <= MAX_EXTENSION_LOG_BYTES);
        assert_eq!(hub.stats().dropped_entries as usize + entries.len(), 100);
        capture_stderr(Cursor::new(b"still draining\n"), &hub, "extension").unwrap();
        assert_eq!(hub.snapshot().last().unwrap().message, "still draining");
    }

    #[test]
    fn concurrent_writers_share_one_retention_budget() {
        let hub = ExtensionLogHub::default();
        let workers = (0..8)
            .map(|worker| {
                let hub = hub.clone();
                std::thread::spawn(move || {
                    for line in 0..200 {
                        capture_stderr(
                            Cursor::new(format!("{line}\n")),
                            &hub,
                            &format!("worker-{worker}"),
                        )
                        .unwrap();
                    }
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap();
        }
        let stats = hub.stats();
        let entries = hub.snapshot();
        assert_eq!(stats.retained_entries, MAX_EXTENSION_LOG_ENTRIES);
        assert_eq!(
            stats.dropped_entries as usize,
            1600 - MAX_EXTENSION_LOG_ENTRIES
        );
        assert_eq!(
            stats.retained_bytes,
            entries
                .iter()
                .map(|entry| entry.extension_id.len() + entry.message.len())
                .sum::<usize>()
        );
        assert!(
            entries
                .iter()
                .all(|entry| entry.extension_id.starts_with("worker-"))
        );
    }

    #[test]
    fn oversized_identity_is_accounted_for_without_destroying_existing_logs() {
        let hub = ExtensionLogHub::default();
        capture_stderr(Cursor::new(b"kept\n"), &hub, "one").unwrap();
        capture_stderr(
            Cursor::new(b"omitted\n"),
            &hub,
            &"x".repeat(MAX_EXTENSION_LOG_BYTES),
        )
        .unwrap();
        assert_eq!(hub.snapshot().len(), 1);
        assert_eq!(hub.snapshot()[0].message, "kept");
        assert_eq!(hub.stats().dropped_entries, 1);
    }
}
