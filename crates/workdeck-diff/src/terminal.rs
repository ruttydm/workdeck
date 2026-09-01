//! Terminal-safe text boundaries translated from Hunk's terminal text helpers.

use std::borrow::Cow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SanitizeOptions {
    pub preserve_newlines: bool,
    pub preserve_tabs: bool,
    pub preserve_ansi_style: bool,
}

impl Default for SanitizeOptions {
    fn default() -> Self {
        Self {
            preserve_newlines: true,
            preserve_tabs: true,
            preserve_ansi_style: false,
        }
    }
}

/// Remove terminal control strings and visual-spoofing controls from untrusted text.
pub fn sanitize_terminal_text(text: &str, options: SanitizeOptions) -> String {
    let characters = text.chars().collect::<Vec<_>>();
    let mut output = String::with_capacity(text.len());
    let mut index = 0;
    while index < characters.len() {
        let character = characters[index];
        match character {
            '\u{1b}' => {
                let Some(introducer) = characters.get(index + 1).copied() else {
                    index += 1;
                    continue;
                };
                match introducer {
                    '[' => {
                        let (end, final_character) = consume_csi(&characters, index + 2);
                        if options.preserve_ansi_style
                            && final_character == Some('m')
                            && characters[index + 2..end.saturating_sub(1)]
                                .iter()
                                .all(|value| value.is_ascii_digit() || matches!(value, ';' | ':'))
                        {
                            output.extend(characters[index..end].iter());
                        }
                        index = end;
                    }
                    ']' => index = consume_control_string(&characters, index + 2, true),
                    'P' | 'X' | '^' | '_' => {
                        index = consume_control_string(&characters, index + 2, false);
                    }
                    '@'..='_' => index += 2,
                    _ => index += 1,
                }
            }
            '\u{90}' | '\u{98}' | '\u{9e}' | '\u{9f}' => {
                index = consume_control_string(&characters, index + 1, false);
            }
            '\u{9d}' => index = consume_control_string(&characters, index + 1, true),
            '\u{9b}' => index = consume_csi(&characters, index + 1).0,
            '\u{f0000}' | '\u{f0001}' if options.preserve_ansi_style => index += 1,
            '\n' if options.preserve_newlines => {
                output.push(character);
                index += 1;
            }
            '\t' if options.preserve_tabs => {
                output.push(character);
                index += 1;
            }
            value if value.is_control() => index += 1,
            value => {
                output.push(value);
                index += 1;
            }
        }
    }
    output
}

/// Sanitize a single terminal row where physical line breaks are never allowed.
pub fn sanitize_terminal_line(text: &str) -> String {
    sanitize_terminal_text(
        text,
        SanitizeOptions {
            preserve_newlines: false,
            ..SanitizeOptions::default()
        },
    )
}

/// Render path controls as visible escapes, including literal backslashes.
pub fn format_terminal_path(path: &str) -> String {
    let mut output = String::with_capacity(path.len());
    for character in path.chars() {
        match character {
            '\\' => output.push_str("\\\\"),
            '\t' => output.push_str("\\t"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            value if value <= '\u{1f}' || ('\u{7f}'..='\u{9f}').contains(&value) => {
                output.push_str(&format!("\\x{:02x}", value as u32));
            }
            value => output.push(value),
        }
    }
    output
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalSpan<T> {
    pub text: String,
    pub style: T,
}

/// Sanitize styled text without copying an already-safe span slice.
pub fn sanitize_terminal_spans<T: Clone>(spans: &[TerminalSpan<T>]) -> Cow<'_, [TerminalSpan<T>]> {
    let mut sanitized: Option<Vec<TerminalSpan<T>>> = None;
    for (index, span) in spans.iter().enumerate() {
        let text = sanitize_terminal_line(&span.text);
        if text == span.text && !text.is_empty() {
            if let Some(output) = &mut sanitized {
                output.push(span.clone());
            }
            continue;
        }
        let output = sanitized.get_or_insert_with(|| spans[..index].to_vec());
        if !text.is_empty() {
            output.push(TerminalSpan {
                text,
                style: span.style.clone(),
            });
        }
    }
    sanitized.map_or(Cow::Borrowed(spans), Cow::Owned)
}

fn consume_csi(characters: &[char], mut index: usize) -> (usize, Option<char>) {
    while let Some(character) = characters.get(index).copied() {
        index += 1;
        if ('@'..='~').contains(&character) {
            return (index, Some(character));
        }
    }
    (index, None)
}

fn consume_control_string(characters: &[char], mut index: usize, bell_terminates: bool) -> usize {
    while index < characters.len() {
        if characters[index] == '\u{9c}' {
            return index + 1;
        }
        if bell_terminates && characters[index] == '\u{7}' {
            return index + 1;
        }
        if characters[index] == '\u{1b}' && characters.get(index + 1) == Some(&'\\') {
            return index + 2;
        }
        index += 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_terminal_control_strings_without_dropping_surrounding_text() {
        let input = concat!(
            "before",
            "\x1b]52;c;SGVsbG8=\x07",
            "\x1b]8;;https://example.test\x1b\\",
            "\x1b[2J",
            "\x1bPqpayload\x1b\\",
            "\x1b_payload\x1b\\",
            "\x1b^payload\x1b\\",
            "\x1bXpayload\x1b\\",
            "\u{9d}52;c;SGVsbG8=\x07",
            "\u{9b}2J",
            "\u{90}payload\u{9c}",
            "after"
        );
        assert_eq!(
            sanitize_terminal_text(input, SanitizeOptions::default()),
            "beforeafter"
        );
    }

    #[test]
    fn preserves_only_requested_text_and_sgr_controls() {
        assert_eq!(
            sanitize_terminal_text("alpha\n\tbeta", SanitizeOptions::default()),
            "alpha\n\tbeta"
        );
        assert_eq!(
            sanitize_terminal_text(
                "plain\x1b[1;34mblue\x1b[m\x1b]52;c;x\x07\x1b[2J\x1b[2Kdone",
                SanitizeOptions {
                    preserve_ansi_style: true,
                    ..SanitizeOptions::default()
                }
            ),
            "plain\x1b[1;34mblue\x1b[mdone"
        );
        assert_eq!(
            sanitize_terminal_line("safe\rOVER\nWRITE\x08"),
            "safeOVERWRITE"
        );
    }

    #[test]
    fn formats_path_controls_as_visible_escapes() {
        assert_eq!(
            format_terminal_path("dir/literal\\t-tab\tline\nescape\x1b"),
            "dir/literal\\\\t-tab\\tline\\nescape\\x1b"
        );
    }

    #[test]
    fn sanitizes_spans_and_borrows_an_already_safe_slice() {
        let safe = vec![TerminalSpan {
            text: "safe".into(),
            style: "white",
        }];
        assert!(matches!(sanitize_terminal_spans(&safe), Cow::Borrowed(_)));

        let unsafe_spans = vec![
            TerminalSpan {
                text: "before\x1b]52;c;x\x07".into(),
                style: "white",
            },
            TerminalSpan {
                text: "\x1b[2J".into(),
                style: "none",
            },
            TerminalSpan {
                text: "after".into(),
                style: "black",
            },
        ];
        assert_eq!(
            sanitize_terminal_spans(&unsafe_spans).into_owned(),
            vec![
                TerminalSpan {
                    text: "before".into(),
                    style: "white",
                },
                TerminalSpan {
                    text: "after".into(),
                    style: "black",
                },
            ]
        );
    }
}
