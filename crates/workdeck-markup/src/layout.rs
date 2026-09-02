//! Deterministic STML-to-terminal-cell layout translated from Hunk's MIT-licensed STML engine.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, LazyLock, Mutex};

use serde::{Deserialize, Serialize};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::{
    STML_REFERENCE_WIDTH, StmlNode, StmlTagRole, decode_stml_entities, is_inline_stml_role,
    parse_stml_default, stml_tag_role,
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StmlStyle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dim: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strike: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StmlSpan {
    pub text: String,
    #[serde(flatten)]
    pub style: StmlStyle,
}

impl StmlSpan {
    fn new(text: impl Into<String>, style: StmlStyle) -> Self {
        Self {
            text: text.into(),
            style,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StmlLine {
    pub spans: Vec<StmlSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StmlLayoutResult {
    pub lines: Vec<StmlLine>,
    pub errors: Vec<String>,
}

pub const MIN_STML_LAYOUT_WIDTH: usize = 8;
const MAX_LAYOUT_ERRORS: usize = 20;

#[derive(Debug, Clone, Copy)]
struct BorderChars {
    top_left: char,
    top_right: char,
    bottom_left: char,
    bottom_right: char,
    horizontal: char,
    vertical: char,
}

const SINGLE: BorderChars = BorderChars {
    top_left: '┌',
    top_right: '┐',
    bottom_left: '└',
    bottom_right: '┘',
    horizontal: '─',
    vertical: '│',
};
const ROUNDED: BorderChars = BorderChars {
    top_left: '╭',
    top_right: '╮',
    bottom_left: '╰',
    bottom_right: '╯',
    horizontal: '─',
    vertical: '│',
};
const DOUBLE: BorderChars = BorderChars {
    top_left: '╔',
    top_right: '╗',
    bottom_left: '╚',
    bottom_right: '╝',
    horizontal: '═',
    vertical: '║',
};
const HEAVY: BorderChars = BorderChars {
    top_left: '┏',
    top_right: '┓',
    bottom_left: '┗',
    bottom_right: '┛',
    horizontal: '━',
    vertical: '┃',
};

#[derive(Debug, Default)]
struct LayoutErrors {
    messages: Vec<String>,
}

impl LayoutErrors {
    fn add(&mut self, message: impl Into<String>) {
        if self.messages.len() < MAX_LAYOUT_ERRORS {
            self.messages.push(message.into());
        } else if self.messages.len() == MAX_LAYOUT_ERRORS {
            self.messages.push("further layout notes omitted".into());
        }
    }
}

#[derive(Debug)]
struct InlineToken {
    span: StmlSpan,
    kind: InlineTokenKind,
    width: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InlineTokenKind {
    Word,
    Space,
    Break,
}

/// Return parse/layout notes at the same reference width used by review sessions.
#[must_use]
pub fn validate_stml_markup(markup: &str, width: usize) -> Vec<String> {
    layout_stml_cached(markup, width).errors.clone()
}

#[must_use]
pub fn validate_stml_markup_default(markup: &str) -> Vec<String> {
    validate_stml_markup(markup, STML_REFERENCE_WIDTH)
}

fn is_inline_tag(tag: &str) -> bool {
    is_inline_stml_role(stml_tag_role(tag))
}

fn truthy_attr(value: Option<&String>) -> bool {
    value.is_none_or(|value| value.is_empty() || matches!(value.as_str(), "true" | "yes" | "on"))
}

fn collapse_whitespace(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut in_whitespace = false;
    for character in text.chars() {
        if character.is_whitespace() {
            if !in_whitespace {
                output.push(' ');
                in_whitespace = true;
            }
        } else {
            output.push(character);
            in_whitespace = false;
        }
    }
    output
}

fn merge_style(base: &StmlStyle, over: &StmlStyle) -> StmlStyle {
    StmlStyle {
        fg: over.fg.clone().or_else(|| base.fg.clone()),
        bg: over.bg.clone().or_else(|| base.bg.clone()),
        bold: over.bold.or(base.bold),
        italic: over.italic.or(base.italic),
        underline: over.underline.or(base.underline),
        dim: over.dim.or(base.dim),
        strike: over.strike.or(base.strike),
    }
}

fn num_attr(value: Option<&String>) -> Option<f64> {
    let value = value?.trim();
    if value.is_empty() {
        return Some(0.0);
    }
    let prefixed = [
        ("0x", 16),
        ("0X", 16),
        ("0b", 2),
        ("0B", 2),
        ("0o", 8),
        ("0O", 8),
    ]
    .into_iter()
    .find_map(|(prefix, radix)| {
        value.strip_prefix(prefix).map(|digits| {
            u64::from_str_radix(digits, radix)
                .ok()
                .map(|value| value as f64)
        })
    });
    let parsed = match prefixed {
        Some(parsed) => parsed?,
        None => value.parse::<f64>().ok()?,
    };
    parsed.is_finite().then_some(parsed)
}

fn count_attr(value: Option<&String>, fallback: usize) -> usize {
    num_attr(value)
        .map(|value| value.max(0.0).trunc() as usize)
        .unwrap_or(fallback)
}

fn width_attr(value: Option<&String>, available: usize) -> Option<usize> {
    let value = value?;
    if let Some(percent) = value.strip_suffix('%')
        && valid_percent_number(percent)
    {
        let percent = percent.parse::<f64>().ok()?;
        return Some(((available as f64 * percent / 100.0).floor() as usize).max(1));
    }
    num_attr(Some(value)).map(|value| value.floor().max(1.0) as usize)
}

fn valid_percent_number(value: &str) -> bool {
    let Some((whole, fraction)) = value.split_once('.') else {
        return !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit());
    };
    !whole.is_empty()
        && !fraction.is_empty()
        && !fraction.contains('.')
        && whole.bytes().all(|byte| byte.is_ascii_digit())
        && fraction.bytes().all(|byte| byte.is_ascii_digit())
}

fn attr_style(attrs: &BTreeMap<String, String>) -> StmlStyle {
    let fg = attrs.get("fg").or_else(|| attrs.get("color"));
    StmlStyle {
        fg: fg.filter(|value| !value.is_empty()).cloned(),
        bg: attrs.get("bg").filter(|value| !value.is_empty()).cloned(),
        bold: attrs
            .contains_key("bold")
            .then(|| truthy_attr(attrs.get("bold"))),
        italic: attrs
            .contains_key("italic")
            .then(|| truthy_attr(attrs.get("italic"))),
        underline: attrs
            .contains_key("underline")
            .then(|| truthy_attr(attrs.get("underline"))),
        dim: attrs
            .contains_key("dim")
            .then(|| truthy_attr(attrs.get("dim"))),
        strike: attrs
            .contains_key("strike")
            .then(|| truthy_attr(attrs.get("strike"))),
    }
}

fn inline_style(tag: &str, attrs: &BTreeMap<String, String>) -> StmlStyle {
    match stml_tag_role(tag) {
        Some(StmlTagRole::Strong) => StmlStyle {
            bold: Some(true),
            ..StmlStyle::default()
        },
        Some(StmlTagRole::Emphasis) => StmlStyle {
            italic: Some(true),
            ..StmlStyle::default()
        },
        Some(StmlTagRole::Underline) => StmlStyle {
            underline: Some(true),
            ..StmlStyle::default()
        },
        Some(StmlTagRole::Strike) => StmlStyle {
            strike: Some(true),
            ..StmlStyle::default()
        },
        Some(StmlTagRole::Muted) => StmlStyle {
            dim: Some(true),
            ..StmlStyle::default()
        },
        Some(StmlTagRole::Key) => StmlStyle {
            bg: Some("subtle".into()),
            fg: Some("heading".into()),
            ..StmlStyle::default()
        },
        Some(StmlTagRole::Badge) => StmlStyle {
            bg: Some(
                attrs
                    .get("color")
                    .or_else(|| attrs.get("bg"))
                    .cloned()
                    .unwrap_or_else(|| "accent".into()),
            ),
            fg: Some(
                attrs
                    .get("fg")
                    .cloned()
                    .unwrap_or_else(|| "badge-text".into()),
            ),
            bold: Some(true),
            ..StmlStyle::default()
        },
        Some(StmlTagRole::Link) => StmlStyle {
            fg: Some("accent".into()),
            underline: Some(true),
            ..StmlStyle::default()
        },
        _ => attr_style(attrs),
    }
}

fn inline_spans(node: &StmlNode, style: &StmlStyle) -> Vec<StmlSpan> {
    match node {
        StmlNode::Text { value } => {
            let text = collapse_whitespace(&decode_stml_entities(value));
            if text.is_empty() {
                Vec::new()
            } else {
                vec![StmlSpan::new(text, style.clone())]
            }
        }
        StmlNode::Element {
            tag,
            attrs,
            children,
        } => {
            let role = stml_tag_role(tag);
            if role == Some(StmlTagRole::LineBreak) {
                return vec![StmlSpan::new("\n", style.clone())];
            }
            let next = merge_style(style, &inline_style(tag, attrs));
            let padded = matches!(role, Some(StmlTagRole::Badge | StmlTagRole::Key));
            let mut output = Vec::new();
            if padded {
                output.push(StmlSpan::new(" ", next.clone()));
            }
            for child in children {
                output.extend(inline_spans(child, &next));
            }
            if padded {
                output.push(StmlSpan::new(" ", next));
            }
            output
        }
    }
}

fn tokenize_spans(spans: &[StmlSpan]) -> Vec<InlineToken> {
    let mut tokens = Vec::new();
    for span in spans {
        let mut cursor = 0;
        let bytes = span.text.as_bytes();
        while cursor < bytes.len() {
            if bytes[cursor] == b'\n' {
                tokens.push(InlineToken {
                    span: StmlSpan::new("\n", span.style.clone()),
                    kind: InlineTokenKind::Break,
                    width: 0,
                });
                cursor += 1;
                continue;
            }
            if bytes[cursor] == b' ' {
                let start = cursor;
                while bytes.get(cursor) == Some(&b' ') {
                    cursor += 1;
                }
                let text = &span.text[start..cursor];
                let kind = if span.style.bg.is_none() {
                    InlineTokenKind::Space
                } else {
                    InlineTokenKind::Word
                };
                tokens.push(InlineToken {
                    span: StmlSpan::new(text, span.style.clone()),
                    kind,
                    width: text.len(),
                });
                continue;
            }
            let start = cursor;
            while cursor < bytes.len() && !matches!(bytes[cursor], b'\n' | b' ') {
                cursor += 1;
            }
            let text = &span.text[start..cursor];
            tokens.push(InlineToken {
                span: StmlSpan::new(text, span.style.clone()),
                kind: InlineTokenKind::Word,
                width: measure_text_width(text),
            });
        }
    }
    tokens
}

fn push_span(line: &mut StmlLine, span: StmlSpan) {
    if let Some(last) = line.spans.last_mut()
        && last.style == span.style
    {
        last.text.push_str(&span.text);
    } else {
        line.spans.push(span);
    }
}

fn trim_plain_trailing_spaces(line: &mut StmlLine) {
    while let Some(last) = line.spans.last_mut() {
        if last.style.bg.is_some() || !last.text.bytes().all(|byte| byte == b' ') {
            if last.style.bg.is_none() {
                let trimmed = last.text.trim_end_matches(' ').len();
                last.text.truncate(trimmed);
            }
            break;
        }
        line.spans.pop();
    }
}

fn flush_line(
    lines: &mut Vec<StmlLine>,
    current: &mut StmlLine,
    current_width: &mut usize,
    started: &mut bool,
) {
    trim_plain_trailing_spaces(current);
    lines.push(std::mem::take(current));
    *current_width = 0;
    *started = false;
}

fn wrap_spans(spans: &[StmlSpan], width: usize) -> Vec<StmlLine> {
    let usable = width.max(1);
    let tokens = tokenize_spans(spans);
    let mut lines = Vec::new();
    let mut current = StmlLine::default();
    let mut current_width = 0;
    let mut started = false;

    for token in tokens {
        match token.kind {
            InlineTokenKind::Break => {
                flush_line(&mut lines, &mut current, &mut current_width, &mut started);
            }
            InlineTokenKind::Space => {
                if !started {
                    continue;
                }
                if current_width + token.width > usable {
                    flush_line(&mut lines, &mut current, &mut current_width, &mut started);
                    continue;
                }
                push_span(&mut current, token.span);
                current_width += token.width;
            }
            InlineTokenKind::Word => {
                if current_width + token.width <= usable {
                    push_span(&mut current, token.span);
                    current_width += token.width;
                    started = true;
                    continue;
                }
                if started {
                    flush_line(&mut lines, &mut current, &mut current_width, &mut started);
                }
                let mut rest = token.span.text.as_str();
                while measure_text_width(rest) > usable {
                    let (slice, consumed) = slice_text_prefix(rest, usable);
                    if consumed == 0 {
                        break;
                    }
                    push_span(&mut current, StmlSpan::new(slice, token.span.style.clone()));
                    flush_line(&mut lines, &mut current, &mut current_width, &mut started);
                    rest = &rest[consumed..];
                }
                if !rest.is_empty() {
                    push_span(&mut current, StmlSpan::new(rest, token.span.style.clone()));
                    current_width = measure_text_width(rest);
                    started = true;
                }
            }
        }
    }
    if !current.spans.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

fn measure_text_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

fn slice_text_prefix(text: &str, width: usize) -> (&str, usize) {
    let mut used = 0;
    let mut end = 0;
    for (index, cluster) in text.grapheme_indices(true) {
        let cluster_width = UnicodeWidthStr::width(cluster);
        if used + cluster_width > width {
            break;
        }
        used += cluster_width;
        end = index + cluster.len();
    }
    (&text[..end], end)
}

fn line_width(line: &StmlLine) -> usize {
    line.spans
        .iter()
        .map(|span| measure_text_width(&span.text))
        .sum()
}

fn pad_lines(lines: &[StmlLine], width: usize, bg: Option<&str>) -> Vec<StmlLine> {
    lines
        .iter()
        .map(|line| {
            let mut spans = line
                .spans
                .iter()
                .map(|span| {
                    let mut span = span.clone();
                    if span.style.bg.is_none() {
                        span.style.bg = bg.map(str::to_owned);
                    }
                    span
                })
                .collect::<Vec<_>>();
            let used = line_width(&StmlLine {
                spans: spans.clone(),
            });
            if used < width {
                let style = StmlStyle {
                    bg: bg.map(str::to_owned),
                    ..StmlStyle::default()
                };
                spans.push(StmlSpan::new(" ".repeat(width - used), style));
            }
            StmlLine { spans }
        })
        .collect()
}

fn raw_text(children: &[StmlNode]) -> String {
    children
        .iter()
        .filter_map(StmlNode::text)
        .collect::<String>()
}

fn dedent(text: &str) -> String {
    let text = text.strip_prefix('\n').unwrap_or(text);
    let text = text.trim_end();
    let lines = text.split('\n').collect::<Vec<_>>();
    let minimum = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);
    if minimum == 0 {
        return lines.join("\n");
    }
    lines
        .iter()
        .map(|line| line.get(minimum..).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n")
}

fn border_chars(style: Option<&String>, fallback: BorderChars) -> (BorderChars, bool) {
    match style.map(String::as_str) {
        None | Some("single") => (style.map_or(fallback, |_| SINGLE), false),
        Some("rounded") => (ROUNDED, false),
        Some("double") => (DOUBLE, false),
        Some("heavy") => (HEAVY, false),
        Some(_) => (fallback, true),
    }
}

#[allow(clippy::too_many_arguments)]
fn frame_lines(
    content: &[StmlLine],
    width: usize,
    border: bool,
    chars: BorderChars,
    border_color: &str,
    title: Option<&String>,
    title_color: &str,
    bg: Option<&String>,
    padding_x: usize,
    padding_y: usize,
) -> Vec<StmlLine> {
    let inner_width = width
        .saturating_sub(usize::from(border) * 2)
        .saturating_sub(padding_x * 2)
        .max(1);
    let padded = pad_lines(content, inner_width, bg.map(String::as_str));
    let side_pad = (padding_x > 0).then(|| {
        let style = StmlStyle {
            bg: bg.cloned(),
            ..StmlStyle::default()
        };
        StmlSpan::new(" ".repeat(padding_x), style)
    });
    let blank_row = || {
        let style = StmlStyle {
            bg: bg.cloned(),
            ..StmlStyle::default()
        };
        StmlLine {
            spans: vec![StmlSpan::new(
                " ".repeat(inner_width + padding_x * 2),
                style,
            )],
        }
    };
    let mut body = Vec::new();
    body.extend((0..padding_y).map(|_| blank_row()));
    for line in padded {
        let mut spans = Vec::new();
        if let Some(side_pad) = &side_pad {
            spans.push(side_pad.clone());
        }
        spans.extend(line.spans);
        if let Some(side_pad) = &side_pad {
            spans.push(side_pad.clone());
        }
        body.push(StmlLine { spans });
    }
    body.extend((0..padding_y).map(|_| blank_row()));
    if !border {
        return body;
    }

    let horizontal_width = width.saturating_sub(2);
    let border_style = StmlStyle {
        fg: Some(border_color.into()),
        ..StmlStyle::default()
    };
    let top = if let Some(title) = title.filter(|title| !title.trim().is_empty()) {
        let label = format!(" {} ", title.trim());
        let (fitted, _) = slice_text_prefix(&label, horizontal_width.saturating_sub(2));
        let remainder = horizontal_width
            .saturating_sub(1)
            .saturating_sub(measure_text_width(fitted));
        StmlLine {
            spans: vec![
                StmlSpan::new(
                    format!("{}{}", chars.top_left, chars.horizontal),
                    border_style.clone(),
                ),
                StmlSpan::new(
                    fitted,
                    StmlStyle {
                        fg: Some(title_color.into()),
                        bold: Some(true),
                        ..StmlStyle::default()
                    },
                ),
                StmlSpan::new(
                    format!(
                        "{}{}",
                        chars.horizontal.to_string().repeat(remainder),
                        chars.top_right
                    ),
                    border_style.clone(),
                ),
            ],
        }
    } else {
        StmlLine {
            spans: vec![StmlSpan::new(
                format!(
                    "{}{}{}",
                    chars.top_left,
                    chars.horizontal.to_string().repeat(horizontal_width),
                    chars.top_right
                ),
                border_style.clone(),
            )],
        }
    };
    let bottom = StmlLine {
        spans: vec![StmlSpan::new(
            format!(
                "{}{}{}",
                chars.bottom_left,
                chars.horizontal.to_string().repeat(horizontal_width),
                chars.bottom_right
            ),
            border_style.clone(),
        )],
    };
    let mut framed = vec![top];
    for line in body {
        let mut side_style = border_style.clone();
        side_style.bg = bg.cloned();
        let mut spans = vec![StmlSpan::new(
            chars.vertical.to_string(),
            side_style.clone(),
        )];
        spans.extend(line.spans);
        spans.push(StmlSpan::new(chars.vertical.to_string(), side_style));
        framed.push(StmlLine { spans });
    }
    framed.push(bottom);
    framed
}

fn bullet_lines(
    prefix: &str,
    children: &[StmlNode],
    width: usize,
    style: &StmlStyle,
    errors: &mut LayoutErrors,
) -> Vec<StmlLine> {
    let prefix_width = measure_text_width(prefix);
    let body = layout_block_nodes(
        children,
        width.saturating_sub(prefix_width).max(1),
        style,
        errors,
    );
    body.into_iter()
        .enumerate()
        .map(|(index, line)| {
            let mut spans = vec![StmlSpan::new(
                if index == 0 {
                    prefix.to_owned()
                } else {
                    " ".repeat(prefix_width)
                },
                if index == 0 {
                    StmlStyle {
                        fg: Some("muted".into()),
                        ..StmlStyle::default()
                    }
                } else {
                    StmlStyle::default()
                },
            )];
            spans.extend(line.spans);
            StmlLine { spans }
        })
        .collect()
}

fn merge_columns(columns: &[Vec<StmlLine>], widths: &[usize], gap: usize) -> Vec<StmlLine> {
    let height = columns.iter().map(Vec::len).max().unwrap_or(0);
    (0..height)
        .map(|row| {
            let mut spans = Vec::new();
            for (column_index, column) in columns.iter().enumerate() {
                if column_index > 0 && gap > 0 {
                    spans.push(StmlSpan::new(" ".repeat(gap), StmlStyle::default()));
                }
                let width = widths[column_index];
                if let Some(line) = column.get(row) {
                    spans.extend(line.spans.clone());
                    let used = line_width(line);
                    if used < width {
                        spans.push(StmlSpan::new(
                            " ".repeat(width - used),
                            StmlStyle::default(),
                        ));
                    }
                } else {
                    spans.push(StmlSpan::new(" ".repeat(width), StmlStyle::default()));
                }
            }
            StmlLine { spans }
        })
        .collect()
}

fn layout_row(
    attrs: &BTreeMap<String, String>,
    children: &[StmlNode],
    width: usize,
    style: &StmlStyle,
    errors: &mut LayoutErrors,
) -> Vec<StmlLine> {
    let block_children = children
        .iter()
        .filter(|node| {
            node.element()
                .is_some_and(|element| !is_inline_tag(element.tag))
        })
        .collect::<Vec<_>>();
    let loose_inline = children
        .iter()
        .filter(|node| {
            node.text().is_some()
                || node
                    .element()
                    .is_some_and(|element| is_inline_tag(element.tag))
        })
        .cloned()
        .collect::<Vec<_>>();
    if block_children.is_empty() {
        return layout_block_nodes(children, width, style, errors);
    }
    if loose_inline
        .iter()
        .any(|node| node.text().is_none_or(|text| !text.trim().is_empty()))
    {
        errors.add("<row> mixes bare text with block children; text laid out above the row");
    }
    let gap = count_attr(attrs.get("gap"), 1);
    let total_gap = gap.saturating_mul(block_children.len().saturating_sub(1));
    let available = width.saturating_sub(total_gap);
    if available < block_children.len() {
        errors.add("<row> too narrow for its columns; stacking vertically");
        return block_children
            .into_iter()
            .flat_map(|child| layout_block(child, width, style, errors))
            .collect();
    }
    let fixed = block_children
        .iter()
        .map(|child| {
            child
                .element()
                .and_then(|element| width_attr(element.attrs.get("width"), available))
        })
        .collect::<Vec<_>>();
    let fixed_total = fixed.iter().flatten().sum::<usize>();
    let flex_count = fixed.iter().filter(|width| width.is_none()).count();
    let flex_space = flex_count.max(available.saturating_sub(fixed_total));
    let flex_width = flex_space.checked_div(flex_count).unwrap_or(0);
    let mut flex_remainder = if flex_count > 0 {
        flex_space - flex_width * flex_count
    } else {
        0
    };
    let widths = fixed
        .into_iter()
        .map(|fixed| {
            fixed.map_or_else(
                || {
                    let extra = usize::from(flex_remainder > 0);
                    flex_remainder = flex_remainder.saturating_sub(extra);
                    (flex_width + extra).max(1)
                },
                |fixed| fixed.clamp(1, available),
            )
        })
        .collect::<Vec<_>>();
    let inline_prefix = if loose_inline.is_empty() {
        Vec::new()
    } else {
        layout_block_nodes(&loose_inline, width, style, errors)
    };
    let columns = block_children
        .iter()
        .enumerate()
        .map(|(index, child)| layout_block(child, widths[index], style, errors))
        .collect::<Vec<_>>();
    let mut output = inline_prefix;
    output.extend(merge_columns(&columns, &widths, gap));
    output
}

fn layout_block(
    node: &StmlNode,
    width: usize,
    style: &StmlStyle,
    errors: &mut LayoutErrors,
) -> Vec<StmlLine> {
    let Some(element) = node.element() else {
        return Vec::new();
    };
    match stml_tag_role(element.tag) {
        Some(StmlTagRole::Container | StmlTagRole::Card) => {
            let card = stml_tag_role(element.tag) == Some(StmlTagRole::Card);
            let border = if element.attrs.contains_key("border") {
                truthy_attr(element.attrs.get("border"))
            } else {
                card || element.attrs.contains_key("border-style")
            };
            let fallback = if card { ROUNDED } else { SINGLE };
            let (chars, unknown) = border_chars(element.attrs.get("border-style"), fallback);
            if unknown {
                errors.add(format!(
                    "unknown border-style \"{}\"",
                    element.attrs["border-style"]
                ));
            }
            let padding = count_attr(element.attrs.get("padding"), usize::from(card));
            let padding_x = count_attr(element.attrs.get("padding-x"), padding);
            let padding_y = count_attr(element.attrs.get("padding-y"), padding);
            let requested = width_attr(element.attrs.get("width"), width);
            let box_width = requested.unwrap_or(width).clamp(4, width.max(4));
            let inner_width = box_width
                .saturating_sub(usize::from(border) * 2)
                .saturating_sub(padding_x * 2)
                .max(1);
            let child_style = merge_style(style, &attr_style(element.attrs));
            let content = layout_block_nodes(element.children, inner_width, &child_style, errors);
            frame_lines(
                &content,
                box_width,
                border,
                chars,
                element
                    .attrs
                    .get("border-color")
                    .map_or("note-border", String::as_str),
                element.attrs.get("title"),
                element
                    .attrs
                    .get("title-color")
                    .map_or("heading", String::as_str),
                element.attrs.get("bg").filter(|value| !value.is_empty()),
                padding_x,
                padding_y,
            )
        }
        Some(StmlTagRole::Row) => layout_row(element.attrs, element.children, width, style, errors),
        Some(StmlTagRole::Paragraph) => {
            let style = merge_style(style, &attr_style(element.attrs));
            let spans = element
                .children
                .iter()
                .flat_map(|child| inline_spans(child, &style))
                .collect::<Vec<_>>();
            wrap_spans(&spans, width)
        }
        Some(StmlTagRole::Heading | StmlTagRole::Title) => {
            let title = stml_tag_role(element.tag) == Some(StmlTagRole::Title);
            let base = merge_style(
                style,
                &StmlStyle {
                    bold: Some(true),
                    underline: title.then_some(true),
                    fg: Some(
                        element
                            .attrs
                            .get("fg")
                            .or_else(|| element.attrs.get("color"))
                            .cloned()
                            .unwrap_or_else(|| "heading".into()),
                    ),
                    ..StmlStyle::default()
                },
            );
            let spans = element
                .children
                .iter()
                .flat_map(|child| inline_spans(child, &base))
                .collect::<Vec<_>>();
            wrap_spans(&spans, width)
        }
        Some(StmlTagRole::Divider) => vec![StmlLine {
            spans: vec![StmlSpan::new(
                "─".repeat(width.max(1)),
                StmlStyle {
                    fg: Some(
                        element
                            .attrs
                            .get("color")
                            .cloned()
                            .unwrap_or_else(|| "muted".into()),
                    ),
                    ..StmlStyle::default()
                },
            )],
        }],
        Some(StmlTagRole::Spacer) => (0..count_attr(element.attrs.get("size"), 1).clamp(1, 20))
            .map(|_| StmlLine {
                spans: vec![StmlSpan::new("", StmlStyle::default())],
            })
            .collect(),
        Some(StmlTagRole::List | StmlTagRole::OrderedList) => {
            let ordered = stml_tag_role(element.tag) == Some(StmlTagRole::OrderedList);
            let marker = element.attrs.get("marker").map_or("•", String::as_str);
            let mut index = 1;
            let mut lines = Vec::new();
            for child in element.children {
                let Some(item) = child.element() else {
                    continue;
                };
                if stml_tag_role(item.tag) != Some(StmlTagRole::ListItem) {
                    continue;
                }
                let prefix = if ordered {
                    let prefix = format!("{index}. ");
                    index += 1;
                    prefix
                } else {
                    format!("{marker} ")
                };
                lines.extend(bullet_lines(&prefix, item.children, width, style, errors));
            }
            lines
        }
        Some(StmlTagRole::ListItem) => bullet_lines("• ", element.children, width, style, errors),
        Some(StmlTagRole::Code) => {
            let (chars, _) = border_chars(element.attrs.get("border-style"), SINGLE);
            let mut code_style = style.clone();
            if let Some(fg) = element.attrs.get("fg") {
                code_style.fg = Some(fg.clone());
            }
            let code_width = width.saturating_sub(4).max(1);
            let content = dedent(&raw_text(element.children))
                .split('\n')
                .map(|line| {
                    let expanded = line.replace('\t', "  ");
                    let (fitted, _) = slice_text_prefix(&expanded, code_width);
                    StmlLine {
                        spans: vec![StmlSpan::new(fitted, code_style.clone())],
                    }
                })
                .collect::<Vec<_>>();
            frame_lines(
                &content,
                width,
                true,
                chars,
                element
                    .attrs
                    .get("border-color")
                    .map_or("subtle", String::as_str),
                element.attrs.get("title"),
                "heading",
                element.attrs.get("bg").filter(|value| !value.is_empty()),
                1,
                0,
            )
        }
        _ => {
            errors.add(format!("unknown tag <{}>", element.tag));
            layout_block_nodes(element.children, width, style, errors)
        }
    }
}

fn layout_block_nodes(
    nodes: &[StmlNode],
    width: usize,
    style: &StmlStyle,
    errors: &mut LayoutErrors,
) -> Vec<StmlLine> {
    let mut output = Vec::new();
    let mut run = Vec::<&StmlNode>::new();
    let flush = |run: &mut Vec<&StmlNode>, output: &mut Vec<StmlLine>| {
        if run.is_empty() {
            return;
        }
        let spans = run
            .iter()
            .flat_map(|node| inline_spans(node, style))
            .collect::<Vec<_>>();
        if spans
            .iter()
            .any(|span| !span.text.trim().is_empty() || span.text == "\n")
        {
            output.extend(wrap_spans(&spans, width));
        }
        run.clear();
    };
    for node in nodes {
        if node.text().is_some()
            || node
                .element()
                .is_some_and(|element| is_inline_tag(element.tag))
        {
            run.push(node);
        } else {
            flush(&mut run, &mut output);
            output.extend(layout_block(node, width, style, errors));
        }
    }
    flush(&mut run, &mut output);
    output
}

/// Parse and lay out STML into exact styled terminal rows.
#[must_use]
pub fn layout_stml(markup: &str, width: usize) -> StmlLayoutResult {
    if width < MIN_STML_LAYOUT_WIDTH {
        return StmlLayoutResult {
            lines: Vec::new(),
            errors: vec![format!(
                "width {width} below minimum {MIN_STML_LAYOUT_WIDTH}"
            )],
        };
    }
    let parsed = parse_stml_default(markup);
    let mut errors = LayoutErrors::default();
    for error in parsed.errors {
        errors.add(error);
    }
    let mut lines = layout_block_nodes(&parsed.nodes, width, &StmlStyle::default(), &mut errors);
    while lines.first().is_some_and(blank_line) {
        lines.remove(0);
    }
    while lines.last().is_some_and(blank_line) {
        lines.pop();
    }
    StmlLayoutResult {
        lines,
        errors: errors.messages,
    }
}

fn blank_line(line: &StmlLine) -> bool {
    line_width(line) == 0 && line.spans.iter().all(|span| span.text.trim().is_empty())
}

type LayoutCache = HashMap<(usize, String), Arc<StmlLayoutResult>>;

static LAYOUT_CACHE: LazyLock<Mutex<LayoutCache>> = LazyLock::new(|| Mutex::new(HashMap::new()));
const LAYOUT_CACHE_LIMIT: usize = 256;

/// Memoize the paired measure/render layout by exact `(markup, width)` identity.
#[must_use]
pub fn layout_stml_cached(markup: &str, width: usize) -> Arc<StmlLayoutResult> {
    let key = (width, markup.to_owned());
    let mut cache = LAYOUT_CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(cached) = cache.get(&key) {
        return Arc::clone(cached);
    }
    let result = Arc::new(layout_stml(markup, width));
    if cache.len() >= LAYOUT_CACHE_LIMIT {
        cache.clear();
    }
    cache.insert(key, Arc::clone(&result));
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Map, Value, json};

    fn frame_text(lines: &[StmlLine]) -> Vec<String> {
        lines
            .iter()
            .map(|line| line.spans.iter().map(|span| span.text.as_str()).collect())
            .collect()
    }

    fn oracle_span(span: &StmlSpan) -> Value {
        let mut value = Map::new();
        value.insert("t".into(), json!(span.text));
        for (key, field) in [("f", span.style.fg.as_ref()), ("g", span.style.bg.as_ref())] {
            if let Some(field) = field {
                value.insert(key.into(), json!(field));
            }
        }
        for (key, field) in [
            ("b", span.style.bold),
            ("i", span.style.italic),
            ("u", span.style.underline),
            ("d", span.style.dim),
            ("s", span.style.strike),
        ] {
            if let Some(field) = field {
                value.insert(key.into(), json!(field));
            }
        }
        Value::Object(value)
    }

    #[test]
    fn matches_frozen_pinned_hunk_layout_oracle() {
        let fixture: Value =
            serde_json::from_str(include_str!("../../../port/hunk/oracles/stml-layout.json"))
                .unwrap();
        assert_eq!(
            fixture["baseline"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        for case in fixture["cases"].as_array().unwrap() {
            let result = layout_stml(
                case["markup"].as_str().unwrap(),
                case["width"].as_u64().unwrap() as usize,
            );
            let lines = result
                .lines
                .iter()
                .map(|line| line.spans.iter().map(oracle_span).collect::<Vec<_>>())
                .collect::<Vec<_>>();
            assert_eq!(json!(lines), case["lines"], "case {}", case["name"]);
            assert_eq!(
                json!(result.errors),
                case["errors"],
                "case {}",
                case["name"]
            );
        }
    }

    #[test]
    fn wraps_plain_text_to_the_given_width() {
        let result = layout_stml("one two three four five", 10);
        assert!(result.errors.is_empty());
        assert_eq!(frame_text(&result.lines), ["one two", "three four", "five"]);
    }

    #[test]
    fn is_deterministic_for_the_same_input() {
        let markup = "<card title=\"t\"><list><item>alpha beta</item></list></card>";
        assert_eq!(layout_stml(markup, 30), layout_stml(markup, 30));
    }

    #[test]
    fn carries_inline_styles_through_wrapping() {
        let result = layout_stml("<b>bold and long words here</b>", 12);
        assert!(result.lines.len() > 1);
        assert!(
            result
                .lines
                .iter()
                .flat_map(|line| &line.spans)
                .all(|span| span.style.bold == Some(true))
        );
    }

    #[test]
    fn color_tags_flow_inline_and_retain_attributes() {
        for tag in ["c", "color", "span"] {
            let markup = format!("one <{tag} fg=\"success\">two</{tag}> three");
            let result = layout_stml(&markup, 20);
            assert!(result.errors.is_empty());
            assert_eq!(frame_text(&result.lines), ["one two three"]);
            assert_eq!(
                result.lines[0]
                    .spans
                    .iter()
                    .find(|span| span.text == "two")
                    .and_then(|span| span.style.fg.as_deref()),
                Some("success")
            );
            assert_eq!(layout_stml(&markup, 20), result);
        }
    }

    #[test]
    fn inline_color_inside_row_is_loose_text_not_a_column() {
        let result = layout_stml(
            "<row><c fg=\"accent\">label</c><box border>a</box></row>",
            20,
        );
        let rows = frame_text(&result.lines);
        assert_eq!(rows[0], "label");
        assert_eq!(rows[1], format!("┌{}┐", "─".repeat(18)));
    }

    #[test]
    fn decodes_entities_in_flowing_text() {
        let result = layout_stml("<text>a &rarr; b &amp; c</text>", 30);
        assert_eq!(frame_text(&result.lines)[0], "a → b & c");
    }

    #[test]
    fn honors_explicit_line_breaks() {
        assert_eq!(
            frame_text(&layout_stml("first<br>second", 40).lines),
            ["first", "second"]
        );
    }

    #[test]
    fn bordered_card_with_title_fills_exact_width() {
        let result = layout_stml("<card title=\"Plan\">hi</card>", 20);
        let rows = frame_text(&result.lines);
        assert!(rows[0].contains("╭─ Plan "));
        assert_eq!(rows.last().unwrap(), &format!("╰{}╯", "─".repeat(18)));
        assert!(rows.iter().all(|row| measure_text_width(row) == 20));
        assert_eq!(rows.len(), 5);
    }

    #[test]
    fn box_without_border_stays_frameless() {
        assert_eq!(
            frame_text(&layout_stml("<box>hi</box>", 20).lines),
            [format!("hi{}", " ".repeat(18))]
        );
    }

    #[test]
    fn double_border_uses_double_glyphs() {
        let rows =
            frame_text(&layout_stml("<box border border-style=\"double\">x</box>", 10).lines);
        assert_eq!(rows[0], format!("╔{}╗", "═".repeat(8)));
    }

    #[test]
    fn row_columns_are_side_by_side_with_a_gap() {
        let rows = frame_text(
            &layout_stml("<row><box border>aa</box><box border>bb</box></row>", 21).lines,
        );
        assert_eq!(rows[0], format!("┌{}┐ ┌{}┐", "─".repeat(8), "─".repeat(8)));
        assert!(rows[1].contains("│aa"));
        assert!(rows[1].contains("│bb"));
    }

    #[test]
    fn row_honors_fixed_column_widths() {
        let rows = frame_text(
            &layout_stml(
                "<row gap=\"1\"><box border width=\"6\">a</box><box border>b</box></row>",
                20,
            )
            .lines,
        );
        assert!(rows[0].starts_with(&format!("┌{}┐ ┌", "─".repeat(4))));
        assert_eq!(measure_text_width(&rows[0]), 20);
    }

    #[test]
    fn too_narrow_row_stacks_with_a_note() {
        let result = layout_stml(
            &format!("<row>{}</row>", "<box border>x</box>".repeat(6)),
            9,
        );
        assert!(
            result
                .errors
                .iter()
                .any(|error| error.contains("too narrow"))
        );
        assert!(result.lines.len() > 6);
    }

    #[test]
    fn ordered_and_unordered_lists_use_hanging_indents() {
        let rows = frame_text(
            &layout_stml(
                "<ol><item>first item that wraps around</item><item>second</item></ol>",
                16,
            )
            .lines,
        );
        assert!(rows[0].starts_with("1. first"));
        assert!(rows[1].starts_with("   "));
        assert!(rows.last().unwrap().starts_with("2. second"));
    }

    #[test]
    fn code_is_verbatim_and_clipped_instead_of_wrapped() {
        let markup = "<code>\n      const value = 1;\n      const aVeryLongLineThatShouldClipInsteadOfWrappingAnywhereAtAll = true;\n    </code>";
        let rows = frame_text(&layout_stml(markup, 24).lines);
        assert!(rows[1].contains("const value = 1;"));
        assert_eq!(rows.len(), 4);
        assert!(rows.iter().all(|row| measure_text_width(row) <= 24));
    }

    #[test]
    fn divider_fills_width() {
        assert_eq!(
            frame_text(&layout_stml("<hr>", 12).lines)[0],
            "─".repeat(12)
        );
    }

    #[test]
    fn headings_are_bold_and_title_is_underlined() {
        let result = layout_stml("<h1>Title</h1><h2>Sub</h2>", 20);
        assert_eq!(result.lines[0].spans[0].style.bold, Some(true));
        assert_eq!(result.lines[0].spans[0].style.underline, Some(true));
        assert_eq!(
            result.lines[0].spans[0].style.fg.as_deref(),
            Some("heading")
        );
        assert_eq!(result.lines[1].spans[0].style.bold, Some(true));
        assert_eq!(
            result.lines[1].spans[0].style.fg.as_deref(),
            Some("heading")
        );
    }

    #[test]
    fn badges_pad_labels_and_default_to_accent_background() {
        let result = layout_stml("<badge>OK</badge>", 20);
        assert_eq!(frame_text(&result.lines)[0], " OK ");
        assert!(
            result.lines[0]
                .spans
                .iter()
                .all(|span| span.style.bg.as_deref() == Some("accent"))
        );
    }

    #[test]
    fn unknown_tags_degrade_to_children_with_a_note() {
        let result = layout_stml("<wat>content</wat>", 20);
        assert_eq!(frame_text(&result.lines), ["content"]);
        assert!(
            result
                .errors
                .iter()
                .any(|error| error.contains("unknown tag"))
        );
    }

    #[test]
    fn below_minimum_width_returns_no_lines() {
        let result = layout_stml("hello", 3);
        assert!(result.lines.is_empty());
        assert!(!result.errors.is_empty());
    }

    #[test]
    fn spacer_emits_blank_rows() {
        assert_eq!(
            frame_text(&layout_stml("a<spacer size=\"2\"/>b", 10).lines),
            ["a", "", "", "b"]
        );
    }

    #[test]
    fn hard_slices_a_word_wider_than_the_line() {
        assert_eq!(
            frame_text(&layout_stml("abcdefghijklmnop", 8).lines),
            ["abcdefgh", "ijklmnop"]
        );
    }

    #[test]
    fn background_fills_padded_box_rows() {
        let result = layout_stml("<box bg=\"subtle\" padding=\"1\">x</box>", 12);
        for line in &result.lines {
            assert_eq!(
                measure_text_width(&frame_text(std::slice::from_ref(line))[0]),
                12
            );
            assert!(
                line.spans
                    .iter()
                    .all(|span| span.style.bg.as_deref() == Some("subtle"))
            );
        }
    }

    #[test]
    fn cache_returns_stable_object_identity() {
        let markup = "<text>cache me</text>";
        assert!(Arc::ptr_eq(
            &layout_stml_cached(markup, 20),
            &layout_stml_cached(markup, 20)
        ));
        assert!(!Arc::ptr_eq(
            &layout_stml_cached(markup, 20),
            &layout_stml_cached(markup, 24)
        ));
    }

    #[test]
    fn empty_and_javascript_numeric_attributes_keep_upstream_semantics() {
        let styled = layout_stml("<c fg color=\"success\">x</c>", 20);
        assert_eq!(styled.lines[0].spans[0].style.fg, None);

        let card = layout_stml("<card padding>x</card>", 12);
        assert_eq!(card.lines.len(), 3);

        let background = layout_stml("<box bg>x</box>", 12);
        assert!(
            background.lines[0]
                .spans
                .iter()
                .all(|span| span.style.bg.is_none())
        );

        let hexadecimal_width = layout_stml("<box width=\"0xA\">x</box>", 20);
        assert_eq!(line_width(&hexadecimal_width.lines[0]), 10);

        let binary_width = layout_stml("<box width=\"0b1010\">x</box>", 20);
        assert_eq!(line_width(&binary_width.lines[0]), 10);

        let octal_width = layout_stml("<box width=\"0o12\">x</box>", 20);
        assert_eq!(line_width(&octal_width.lines[0]), 10);
    }
}
