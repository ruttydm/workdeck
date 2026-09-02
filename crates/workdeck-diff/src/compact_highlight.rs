//! Text-free syntax highlight payloads translated from Hunk's highlight worker protocol.
//!
//! The compact representation deliberately stores UTF-16 source ranges rather than token text.
//! That keeps the authoritative source in the review model and makes native cache entries match
//! the browser worker boundary that Hunk validates before rendering.

use std::collections::HashMap;
use std::mem::size_of;

use serde_json::Value;
use thiserror::Error;

use crate::{HighlightLineArrays, HighlightedLine, SyntaxColor, SyntaxToken};

pub const COMPACT_HIGHLIGHT_PROTOCOL_VERSION: u8 = 1;
pub const COMPACT_HIGHLIGHT_FLAG_WORD_DIFF: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HastAppearance {
    Dark,
    Light,
}

/// Minimal HAST shape emitted by Pierre for a syntax-highlighted line.
#[derive(Debug, Clone, PartialEq)]
pub enum HastNode {
    Text {
        value: String,
    },
    Element {
        tag_name: String,
        properties: HashMap<String, Value>,
        children: Vec<HastNode>,
    },
}

impl HastNode {
    #[must_use]
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text {
            value: value.into(),
        }
    }

    #[must_use]
    pub fn element(
        tag_name: impl Into<String>,
        properties: HashMap<String, Value>,
        children: Vec<Self>,
    ) -> Self {
        Self::Element {
            tag_name: tag_name.into(),
            properties,
            children,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HastHighlightRun {
    pub text: String,
    pub foreground: Option<String>,
    pub word_diff: bool,
}

fn clean_last_newline(value: &str) -> &str {
    value.strip_suffix('\n').unwrap_or(value)
}

fn parse_style_value(style: Option<&Value>) -> HashMap<&str, &str> {
    let Some(style) = style.and_then(Value::as_str) else {
        return HashMap::new();
    };
    style
        .split(';')
        .filter_map(|segment| {
            let (key, value) = segment.split_once(':')?;
            let key = key.trim();
            let value = value.trim();
            (!key.is_empty() && !value.is_empty()).then_some((key, value))
        })
        .collect()
}

fn append_hast_run(target: &mut Vec<HastHighlightRun>, next: HastHighlightRun) {
    if next.text.is_empty() {
        return;
    }
    if let Some(previous) = target.last_mut()
        && previous.foreground == next.foreground
        && previous.word_diff == next.word_diff
    {
        previous.text.push_str(&next.text);
        return;
    }
    target.push(next);
}

/// Flatten nested Pierre HAST while retaining terminal-relevant foreground inheritance and
/// semantic word-diff markers.
#[must_use]
pub fn collect_hast_highlight_runs(
    node: Option<&HastNode>,
    appearance: HastAppearance,
) -> Vec<HastHighlightRun> {
    fn visit(
        node: &HastNode,
        color_variable: &str,
        inherited_foreground: Option<&str>,
        inherited_word_diff: bool,
        runs: &mut Vec<HastHighlightRun>,
    ) {
        match node {
            HastNode::Text { value } => append_hast_run(
                runs,
                HastHighlightRun {
                    text: clean_last_newline(value).to_owned(),
                    foreground: inherited_foreground.map(str::to_owned),
                    word_diff: inherited_word_diff,
                },
            ),
            HastNode::Element {
                properties,
                children,
                ..
            } => {
                let styles = parse_style_value(properties.get("style"));
                let foreground = styles
                    .get(color_variable)
                    .or_else(|| styles.get("color"))
                    .copied()
                    .or(inherited_foreground);
                let word_diff = properties.contains_key("data-diff-span") || inherited_word_diff;
                for child in children {
                    visit(child, color_variable, foreground, word_diff, runs);
                }
            }
        }
    }

    let Some(node) = node else {
        return Vec::new();
    };
    let color_variable = match appearance {
        HastAppearance::Dark => "--diffs-token-dark",
        HastAppearance::Light => "--diffs-token-light",
    };
    let mut runs = Vec::new();
    visit(node, color_variable, None, false, &mut runs);
    runs
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactHighlightSide {
    pub line_offsets: Vec<u32>,
    pub starts: Vec<u32>,
    pub ends: Vec<u32>,
    pub style_ids: Vec<u16>,
    pub flags: Vec<u8>,
}

impl Default for CompactHighlightSide {
    fn default() -> Self {
        Self {
            line_offsets: vec![0],
            starts: Vec::new(),
            ends: Vec::new(),
            style_ids: Vec::new(),
            flags: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactHighlightedDiff {
    pub version: u8,
    pub foreground_palette: Vec<String>,
    pub deletion: CompactHighlightSide,
    pub addition: CompactHighlightSide,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactHighlightRun {
    pub start: u32,
    pub end: u32,
    pub foreground: Option<String>,
    pub word_diff: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactHighlightLineLengths {
    pub deletion: Vec<u32>,
    pub addition: Vec<u32>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CompactHighlightError {
    #[error("Compact syntax palette exceeded Uint16 style IDs.")]
    PaletteOverflow,
    #[error("Unsupported compact highlight protocol version: {0}")]
    UnsupportedVersion(u8),
    #[error("Compact syntax palette contains an invalid color.")]
    InvalidPalette,
    #[error("Compact {side} highlight run arrays disagree.")]
    RunArraysDisagree { side: &'static str },
    #[error("Compact {side} highlight line count does not match its source.")]
    LineCountMismatch { side: &'static str },
    #[error("Compact {side} highlight offsets are invalid.")]
    InvalidOffsets { side: &'static str },
    #[error("Compact {side} highlight final offset does not reach its runs.")]
    InvalidFinalOffset { side: &'static str },
    #[error("Compact {side} highlight ranges are invalid at line {line}.")]
    InvalidRanges { side: &'static str, line: usize },
    #[error("Compact {side} highlight ranges do not cover line {line}.")]
    IncompleteRanges { side: &'static str, line: usize },
    #[error("Compact {side} highlight style ID is outside its palette.")]
    InvalidStyleId { side: &'static str },
    #[error("Compact {side} highlight contains unsupported flags.")]
    UnsupportedFlags { side: &'static str },
    #[error("Compact {side} highlight line index is outside its payload.")]
    LineIndexOutOfRange { side: &'static str },
    #[error("Compact {side} highlight range is not a UTF-16 source boundary at line {line}.")]
    InvalidUtf16Boundary { side: &'static str, line: usize },
}

fn palette_id(
    foreground: Option<&str>,
    palette: &mut Vec<String>,
    palette_ids: &mut HashMap<String, u16>,
) -> Result<u16, CompactHighlightError> {
    let Some(foreground) = foreground else {
        return Ok(0);
    };
    if let Some(existing) = palette_ids.get(foreground) {
        return Ok(*existing);
    }
    if palette.len() == usize::from(u16::MAX) {
        return Err(CompactHighlightError::PaletteOverflow);
    }
    palette.push(foreground.to_owned());
    let id = u16::try_from(palette.len()).map_err(|_| CompactHighlightError::PaletteOverflow)?;
    palette_ids.insert(foreground.to_owned(), id);
    Ok(id)
}

fn encode_hast_side(
    lines: &[Option<HastNode>],
    appearance: HastAppearance,
    palette: &mut Vec<String>,
    palette_ids: &mut HashMap<String, u16>,
) -> Result<CompactHighlightSide, CompactHighlightError> {
    let mut side = CompactHighlightSide::default();
    for line in lines {
        let mut source_column = 0_u32;
        for run in collect_hast_highlight_runs(line.as_ref(), appearance) {
            let start = source_column;
            source_column = source_column
                .saturating_add(u32::try_from(run.text.encode_utf16().count()).unwrap_or(u32::MAX));
            if source_column == start {
                continue;
            }
            side.starts.push(start);
            side.ends.push(source_column);
            side.style_ids
                .push(palette_id(run.foreground.as_deref(), palette, palette_ids)?);
            side.flags.push(if run.word_diff {
                COMPACT_HIGHLIGHT_FLAG_WORD_DIFF
            } else {
                0
            });
        }
        side.line_offsets
            .push(u32::try_from(side.starts.len()).unwrap_or(u32::MAX));
    }
    Ok(side)
}

/// Encode HAST using Hunk's version-one compact worker protocol.
pub fn encode_compact_highlighted_diff(
    deletion_lines: &[Option<HastNode>],
    addition_lines: &[Option<HastNode>],
    appearance: HastAppearance,
) -> Result<CompactHighlightedDiff, CompactHighlightError> {
    let mut foreground_palette = Vec::new();
    let mut palette_ids = HashMap::new();
    let deletion = encode_hast_side(
        deletion_lines,
        appearance,
        &mut foreground_palette,
        &mut palette_ids,
    )?;
    let addition = encode_hast_side(
        addition_lines,
        appearance,
        &mut foreground_palette,
        &mut palette_ids,
    )?;
    Ok(CompactHighlightedDiff {
        version: COMPACT_HIGHLIGHT_PROTOCOL_VERSION,
        foreground_palette,
        deletion,
        addition,
    })
}

fn syntax_color_css(color: SyntaxColor) -> String {
    format!("#{:02x}{:02x}{:02x}", color.red, color.green, color.blue)
}

fn encode_syntax_side(
    lines: &[Option<HighlightedLine>],
    palette: &mut Vec<String>,
    palette_ids: &mut HashMap<String, u16>,
) -> Result<CompactHighlightSide, CompactHighlightError> {
    let mut side = CompactHighlightSide::default();
    for line in lines {
        let mut source_column = 0_u32;
        let mut previous_color: Option<String> = None;
        for token in line.iter().flatten() {
            let start = source_column;
            source_column = source_column.saturating_add(
                u32::try_from(token.text.encode_utf16().count()).unwrap_or(u32::MAX),
            );
            if source_column == start {
                continue;
            }
            let foreground = syntax_color_css(token.foreground);
            if previous_color.as_deref() == Some(&foreground) {
                if let Some(end) = side.ends.last_mut() {
                    *end = source_column;
                }
                continue;
            }
            side.starts.push(start);
            side.ends.push(source_column);
            side.style_ids
                .push(palette_id(Some(&foreground), palette, palette_ids)?);
            side.flags.push(0);
            previous_color = Some(foreground);
        }
        side.line_offsets
            .push(u32::try_from(side.starts.len()).unwrap_or(u32::MAX));
    }
    Ok(side)
}

/// Encode native highlighter output through the same text-free cache format.
pub(crate) fn encode_compact_syntax_lines(
    lines: &HighlightLineArrays<HighlightedLine>,
) -> Result<CompactHighlightedDiff, CompactHighlightError> {
    let mut foreground_palette = Vec::new();
    let mut palette_ids = HashMap::new();
    let deletion = encode_syntax_side(
        &lines.deletion_lines,
        &mut foreground_palette,
        &mut palette_ids,
    )?;
    let addition = encode_syntax_side(
        &lines.addition_lines,
        &mut foreground_palette,
        &mut palette_ids,
    )?;
    Ok(CompactHighlightedDiff {
        version: COMPACT_HIGHLIGHT_PROTOCOL_VERSION,
        foreground_palette,
        deletion,
        addition,
    })
}

fn validate_side(
    side: &CompactHighlightSide,
    palette_length: usize,
    line_lengths: Option<&[u32]>,
    name: &'static str,
) -> Result<(), CompactHighlightError> {
    let run_count = side.starts.len();
    if side.ends.len() != run_count
        || side.style_ids.len() != run_count
        || side.flags.len() != run_count
        || side.line_offsets.is_empty()
    {
        return Err(CompactHighlightError::RunArraysDisagree { side: name });
    }
    if line_lengths.is_some_and(|lengths| lengths.len() != side.line_offsets.len() - 1) {
        return Err(CompactHighlightError::LineCountMismatch { side: name });
    }

    let mut previous_offset = 0_u32;
    for &offset in &side.line_offsets {
        if offset < previous_offset || usize::try_from(offset).unwrap_or(usize::MAX) > run_count {
            return Err(CompactHighlightError::InvalidOffsets { side: name });
        }
        previous_offset = offset;
    }
    if usize::try_from(previous_offset).unwrap_or(usize::MAX) != run_count {
        return Err(CompactHighlightError::InvalidFinalOffset { side: name });
    }

    for line_index in 0..side.line_offsets.len() - 1 {
        let start_offset = side.line_offsets[line_index] as usize;
        let end_offset = side.line_offsets[line_index + 1] as usize;
        let mut previous_end = 0_u32;
        for run_index in start_offset..end_offset {
            let start = side.starts[run_index];
            let end = side.ends[run_index];
            if start != previous_end
                || end <= start
                || line_lengths.is_some_and(|lengths| end > lengths[line_index])
            {
                return Err(CompactHighlightError::InvalidRanges {
                    side: name,
                    line: line_index,
                });
            }
            if usize::from(side.style_ids[run_index]) > palette_length {
                return Err(CompactHighlightError::InvalidStyleId { side: name });
            }
            if side.flags[run_index] & !COMPACT_HIGHLIGHT_FLAG_WORD_DIFF != 0 {
                return Err(CompactHighlightError::UnsupportedFlags { side: name });
            }
            previous_end = end;
        }
        if start_offset < end_offset
            && line_lengths.is_some_and(|lengths| previous_end != lengths[line_index])
        {
            return Err(CompactHighlightError::IncompleteRanges {
                side: name,
                line: line_index,
            });
        }
    }
    Ok(())
}

/// Validate a received compact payload before it enters a native cache or renderer.
pub fn validate_compact_highlighted_diff(
    payload: &CompactHighlightedDiff,
    line_lengths: Option<&CompactHighlightLineLengths>,
) -> Result<(), CompactHighlightError> {
    if payload.version != COMPACT_HIGHLIGHT_PROTOCOL_VERSION {
        return Err(CompactHighlightError::UnsupportedVersion(payload.version));
    }
    if payload.foreground_palette.iter().any(String::is_empty) {
        return Err(CompactHighlightError::InvalidPalette);
    }
    validate_side(
        &payload.deletion,
        payload.foreground_palette.len(),
        line_lengths.map(|lengths| lengths.deletion.as_slice()),
        "deletion",
    )?;
    validate_side(
        &payload.addition,
        payload.foreground_palette.len(),
        line_lengths.map(|lengths| lengths.addition.as_slice()),
        "addition",
    )
}

/// Read one compact line without reconstructing HAST nodes or token text.
pub fn compact_highlight_runs_for_line(
    payload: &CompactHighlightedDiff,
    side_name: &'static str,
    line_index: usize,
) -> Result<Vec<CompactHighlightRun>, CompactHighlightError> {
    let side = match side_name {
        "deletion" => &payload.deletion,
        "addition" => &payload.addition,
        _ => return Err(CompactHighlightError::LineIndexOutOfRange { side: side_name }),
    };
    if line_index >= side.line_offsets.len().saturating_sub(1) {
        return Err(CompactHighlightError::LineIndexOutOfRange { side: side_name });
    }
    let start_offset = side.line_offsets[line_index] as usize;
    let end_offset = side.line_offsets[line_index + 1] as usize;
    Ok((start_offset..end_offset)
        .map(|run_index| {
            let style_id = side.style_ids[run_index];
            CompactHighlightRun {
                start: side.starts[run_index],
                end: side.ends[run_index],
                foreground: (style_id != 0)
                    .then(|| payload.foreground_palette[usize::from(style_id - 1)].clone()),
                word_diff: side.flags[run_index] & COMPACT_HIGHLIGHT_FLAG_WORD_DIFF != 0,
            }
        })
        .collect())
}

#[must_use]
pub fn compact_highlighted_diff_byte_length(payload: &CompactHighlightedDiff) -> usize {
    fn side_bytes(side: &CompactHighlightSide) -> usize {
        side.line_offsets.len().saturating_mul(size_of::<u32>())
            + side.starts.len().saturating_mul(size_of::<u32>())
            + side.ends.len().saturating_mul(size_of::<u32>())
            + side.style_ids.len().saturating_mul(size_of::<u16>())
            + side.flags.len().saturating_mul(size_of::<u8>())
    }
    side_bytes(&payload.deletion)
        .saturating_add(side_bytes(&payload.addition))
        .saturating_add(
            serde_json::to_vec(&payload.foreground_palette).map_or(0, |palette| palette.len()),
        )
}

fn utf16_column_to_byte(text: &str, column: u32) -> Option<usize> {
    if column == 0 {
        return Some(0);
    }
    let mut units = 0_u32;
    for (byte_index, character) in text.char_indices() {
        units = units.checked_add(character.len_utf16() as u32)?;
        if units == column {
            return Some(byte_index + character.len_utf8());
        }
        if units > column {
            return None;
        }
    }
    (units == column).then_some(text.len())
}

fn parse_css_color(value: &str) -> Option<SyntaxColor> {
    let value = value.strip_prefix('#')?;
    let expand = |value: u8| value.saturating_mul(17);
    let byte = |pair: &str| u8::from_str_radix(pair, 16).ok();
    match value.len() {
        3 | 4 => Some(SyntaxColor {
            red: expand(u8::from_str_radix(&value[0..1], 16).ok()?),
            green: expand(u8::from_str_radix(&value[1..2], 16).ok()?),
            blue: expand(u8::from_str_radix(&value[2..3], 16).ok()?),
        }),
        6 | 8 => Some(SyntaxColor {
            red: byte(&value[0..2])?,
            green: byte(&value[2..4])?,
            blue: byte(&value[4..6])?,
        }),
        _ => None,
    }
}

fn decode_side(
    payload: &CompactHighlightedDiff,
    source_lines: &[String],
    name: &'static str,
) -> Result<Vec<Option<HighlightedLine>>, CompactHighlightError> {
    source_lines
        .iter()
        .enumerate()
        .map(|(line_index, source)| {
            let source = clean_last_newline(source);
            let runs = compact_highlight_runs_for_line(payload, name, line_index)?;
            let mut tokens = Vec::with_capacity(runs.len());
            for run in runs {
                let start = utf16_column_to_byte(source, run.start).ok_or(
                    CompactHighlightError::InvalidUtf16Boundary {
                        side: name,
                        line: line_index,
                    },
                )?;
                let end = utf16_column_to_byte(source, run.end).ok_or(
                    CompactHighlightError::InvalidUtf16Boundary {
                        side: name,
                        line: line_index,
                    },
                )?;
                tokens.push(SyntaxToken {
                    text: source[start..end].to_owned(),
                    foreground: run
                        .foreground
                        .as_deref()
                        .and_then(parse_css_color)
                        .unwrap_or(SyntaxColor {
                            red: 201,
                            green: 209,
                            blue: 217,
                        }),
                    // Hunk's compact protocol intentionally projects HAST to foreground and
                    // semantic word-diff only; terminal font modifiers are not on the wire.
                    bold: false,
                    italic: false,
                    underline: false,
                });
            }
            Ok(Some(tokens))
        })
        .collect()
}

pub(crate) fn compact_line_lengths(
    deletion_lines: &[String],
    addition_lines: &[String],
) -> CompactHighlightLineLengths {
    let lengths = |lines: &[String]| {
        lines
            .iter()
            .map(|line| {
                u32::try_from(clean_last_newline(line).encode_utf16().count()).unwrap_or(u32::MAX)
            })
            .collect()
    };
    CompactHighlightLineLengths {
        deletion: lengths(deletion_lines),
        addition: lengths(addition_lines),
    }
}

pub(crate) fn decode_compact_syntax_lines(
    payload: &CompactHighlightedDiff,
    deletion_lines: &[String],
    addition_lines: &[String],
) -> Result<HighlightLineArrays<HighlightedLine>, CompactHighlightError> {
    let line_lengths = compact_line_lengths(deletion_lines, addition_lines);
    validate_compact_highlighted_diff(payload, Some(&line_lengths))?;
    Ok(HighlightLineArrays {
        deletion_lines: decode_side(payload, deletion_lines, "deletion")?,
        addition_lines: decode_side(payload, addition_lines, "addition")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn nested_hast_inheritance_and_word_diff_round_trip_without_text_payload() {
        let mut root_properties = HashMap::new();
        root_properties.insert("style".into(), json!("color:#base"));
        let mut changed_properties = HashMap::new();
        changed_properties.insert(
            "style".into(),
            json!("--diffs-token-dark:#keyword;--diffs-token-light:#light-keyword"),
        );
        changed_properties.insert("data-diff-span".into(), json!("changed"));
        let line = HastNode::element(
            "span",
            root_properties,
            vec![
                HastNode::text("const "),
                HastNode::element("span", changed_properties, vec![HastNode::text("answer")]),
                HastNode::text("\n"),
            ],
        );
        let payload = encode_compact_highlighted_diff(&[Some(line)], &[], HastAppearance::Dark)
            .expect("nested HAST encodes");
        validate_compact_highlighted_diff(
            &payload,
            Some(&CompactHighlightLineLengths {
                deletion: vec![12],
                addition: vec![],
            }),
        )
        .unwrap();

        assert_eq!(payload.foreground_palette, ["#base", "#keyword"]);
        assert_eq!(
            compact_highlight_runs_for_line(&payload, "deletion", 0).unwrap(),
            vec![
                CompactHighlightRun {
                    start: 0,
                    end: 6,
                    foreground: Some("#base".into()),
                    word_diff: false,
                },
                CompactHighlightRun {
                    start: 6,
                    end: 12,
                    foreground: Some("#keyword".into()),
                    word_diff: true,
                },
            ]
        );
        assert_eq!(
            payload.deletion.flags,
            [0, COMPACT_HIGHLIGHT_FLAG_WORD_DIFF]
        );
        let debug = format!("{payload:?}");
        assert!(!debug.contains("const "));
        assert!(compact_highlighted_diff_byte_length(&payload) > 0);
    }

    #[test]
    fn appearance_selects_the_resolved_pierre_foreground() {
        let mut properties = HashMap::new();
        properties.insert(
            "style".into(),
            json!("--diffs-token-dark:#111111; --diffs-token-light: #eeeeee; color:#333333"),
        );
        let node = HastNode::element("span", properties, vec![HastNode::text("x")]);
        let dark = collect_hast_highlight_runs(Some(&node), HastAppearance::Dark);
        let light = collect_hast_highlight_runs(Some(&node), HastAppearance::Light);
        assert_eq!(dark[0].foreground.as_deref(), Some("#111111"));
        assert_eq!(light[0].foreground.as_deref(), Some("#eeeeee"));
    }

    #[test]
    fn utf16_ranges_decode_as_independent_old_and_new_source_sides() {
        let old = vec![Some(vec![SyntaxToken {
            text: "a🦀b".into(),
            foreground: SyntaxColor {
                red: 1,
                green: 2,
                blue: 3,
            },
            bold: true,
            italic: false,
            underline: false,
        }])];
        let new = vec![Some(vec![SyntaxToken {
            text: "a🦀c".into(),
            foreground: SyntaxColor {
                red: 4,
                green: 5,
                blue: 6,
            },
            bold: false,
            italic: true,
            underline: false,
        }])];
        let payload = encode_compact_syntax_lines(&HighlightLineArrays {
            deletion_lines: old,
            addition_lines: new,
        })
        .unwrap();
        let decoded =
            decode_compact_syntax_lines(&payload, &["a🦀b\n".into()], &["a🦀c\n".into()]).unwrap();
        assert_eq!(decoded.deletion_lines[0].as_ref().unwrap()[0].text, "a🦀b");
        assert_eq!(decoded.addition_lines[0].as_ref().unwrap()[0].text, "a🦀c");
        assert_eq!(payload.deletion.ends, [4]);
        assert_eq!(payload.addition.ends, [4]);
    }

    #[test]
    fn malformed_ranges_and_payload_fields_are_rejected() {
        let payload = encode_compact_highlighted_diff(
            &[Some(HastNode::text("answer\n"))],
            &[],
            HastAppearance::Dark,
        )
        .unwrap();
        let lengths = CompactHighlightLineLengths {
            deletion: vec![6],
            addition: vec![],
        };
        validate_compact_highlighted_diff(&payload, Some(&lengths)).unwrap();

        let mut malformed = payload.clone();
        malformed.deletion.starts[0] = 1;
        assert!(matches!(
            validate_compact_highlighted_diff(&malformed, Some(&lengths)),
            Err(CompactHighlightError::InvalidRanges { .. })
        ));
        malformed.deletion.starts[0] = 0;
        malformed.deletion.ends[0] = 7;
        assert!(matches!(
            validate_compact_highlighted_diff(&malformed, Some(&lengths)),
            Err(CompactHighlightError::InvalidRanges { .. })
        ));

        let mut unsupported = payload;
        unsupported.deletion.flags[0] = 2;
        assert!(matches!(
            validate_compact_highlighted_diff(&unsupported, Some(&lengths)),
            Err(CompactHighlightError::UnsupportedFlags { .. })
        ));
    }

    #[test]
    fn clone_is_deep_and_line_bounds_are_checked() {
        let payload = encode_compact_highlighted_diff(
            &[Some(HastNode::text("answer\n"))],
            &[],
            HastAppearance::Dark,
        )
        .unwrap();
        let mut cloned = payload.clone();
        cloned.deletion.starts[0] = 1;
        assert_eq!(payload.deletion.starts, [0]);
        assert!(matches!(
            compact_highlight_runs_for_line(&payload, "deletion", 1),
            Err(CompactHighlightError::LineIndexOutOfRange { .. })
        ));
    }
}
