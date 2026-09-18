//! Headless rendering translated from Hunk's MIT-licensed STML engine.

use serde::{Deserialize, Serialize};

use crate::{StmlSpan, StmlThemeColors, layout_stml, resolve_stml_color};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StmlTextRenderResult {
    pub lines: Vec<String>,
    pub errors: Vec<String>,
}

fn render_lines(
    markup: &str,
    width: usize,
    mut format_span: impl FnMut(&StmlSpan) -> String,
) -> StmlTextRenderResult {
    let result = layout_stml(markup, width);
    StmlTextRenderResult {
        lines: result
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(&mut format_span)
                    .collect::<String>()
                    .trim_end_matches(char::is_whitespace)
                    .to_owned()
            })
            .collect(),
        errors: result.errors,
    }
}

/// Render STML to plain terminal rows with trailing whitespace removed.
#[must_use]
pub fn render_stml_to_text(markup: &str, width: usize) -> StmlTextRenderResult {
    render_lines(markup, width, |span| span.text.clone())
}

fn hex_to_rgb(color: &str) -> Option<(u8, u8, u8)> {
    let hex = color.strip_prefix('#').unwrap_or(color);
    let full = match hex.len() {
        3 => hex
            .chars()
            .flat_map(|character| [character, character])
            .collect::<String>(),
        6 => hex.to_owned(),
        _ => return None,
    };
    if !full.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some((
        u8::from_str_radix(&full[0..2], 16).ok()?,
        u8::from_str_radix(&full[2..4], 16).ok()?,
        u8::from_str_radix(&full[4..6], 16).ok()?,
    ))
}

fn span_sgr(span: &StmlSpan, theme: &StmlThemeColors) -> String {
    let mut codes = Vec::<String>::new();
    if span.style.bold == Some(true) {
        codes.push("1".into());
    }
    if span.style.dim == Some(true) {
        codes.push("2".into());
    }
    if span.style.italic == Some(true) {
        codes.push("3".into());
    }
    if span.style.underline == Some(true) {
        codes.push("4".into());
    }
    if span.style.strike == Some(true) {
        codes.push("9".into());
    }
    if let Some((red, green, blue)) =
        resolve_stml_color(span.style.fg.as_deref(), theme).and_then(|color| hex_to_rgb(&color))
    {
        codes.push(format!("38;2;{red};{green};{blue}"));
    }
    if let Some((red, green, blue)) =
        resolve_stml_color(span.style.bg.as_deref(), theme).and_then(|color| hex_to_rgb(&color))
    {
        codes.push(format!("48;2;{red};{green};{blue}"));
    }
    if codes.is_empty() {
        String::new()
    } else {
        format!("\x1b[{}m", codes.join(";"))
    }
}

/// Render STML to truecolor ANSI terminal rows using the supplied application palette.
#[must_use]
pub fn render_stml_to_ansi(
    markup: &str,
    width: usize,
    theme: &StmlThemeColors,
) -> StmlTextRenderResult {
    render_lines(markup, width, |span| {
        let sgr = span_sgr(span, theme);
        if sgr.is_empty() {
            span.text.clone()
        } else {
            format!("{sgr}{}\x1b[0m", span.text)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> StmlThemeColors {
        StmlThemeColors {
            accent: "#58a6ff".into(),
            accent_muted: "#388bfd".into(),
            added_sign_color: "#3fb950".into(),
            removed_sign_color: "#f85149".into(),
            file_modified: "#d29922".into(),
            muted: "#8b949e".into(),
            panel_alt: "#161b22".into(),
            text: "#c9d1d9".into(),
            panel: "#0d1117".into(),
            note_border: "#30363d".into(),
            background: "#010409".into(),
        }
    }

    #[test]
    fn plain_render_has_no_trailing_whitespace() {
        let result = render_stml_to_text("<box border>hi</box>", 12);
        assert!(result.errors.is_empty());
        assert_eq!(
            result.lines,
            [
                format!("┌{}┐", "─".repeat(10)),
                "│hi        │".into(),
                format!("└{}┘", "─".repeat(10))
            ]
        );
    }

    #[test]
    fn plain_render_surfaces_degradation_notes() {
        let result = render_stml_to_text("<wat>x</wat>", 40);
        assert!(
            result
                .errors
                .iter()
                .any(|error| error.contains("unknown tag"))
        );
    }

    #[test]
    fn ansi_render_emits_truecolor_sgr_for_styled_spans() {
        let result = render_stml_to_ansi("<c fg=\"success\">ok</c>", 20, &theme());
        assert_eq!(result.lines, ["\x1b[38;2;63;185;80mok\x1b[0m"]);
    }

    #[test]
    fn ansi_render_emits_bold_attribute_code() {
        let result = render_stml_to_ansi("<b>bold</b>", 20, &theme());
        assert!(result.lines[0].contains("\x1b[1m"));
    }

    #[test]
    fn ansi_render_leaves_plain_spans_escape_free() {
        let result = render_stml_to_ansi("plain", 20, &theme());
        assert_eq!(result.lines, ["plain"]);
    }
}
