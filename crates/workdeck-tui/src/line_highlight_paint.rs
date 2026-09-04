//! Paint-time resolution of extension and agent line highlights.
//!
//! Source ranges use UTF-16 code units. This module resolves them to terminal
//! columns after terminal sanitization and tab expansion, then repaints
//! immutable render spans without changing text or cell geometry.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Deref;
use std::sync::{Arc, OnceLock};

use ratatui::style::{Color, Style};
use ratatui::text::Span;
use unicode_segmentation::UnicodeSegmentation;
use workdeck_core::{DiffFile, DiffLineKind, ReviewSide};
use workdeck_diff::{
    RenderForegroundTransform, RenderSpan, expand_diff_tabs, sanitize_terminal_line,
};
use workdeck_extension_api::{HighlightTone, ValidatedLineHighlight};
use workdeck_review::{
    normalized_review_source_lines, review_expansion_side, review_gap_source_for_file,
    review_leading_gap, review_trailing_gap,
};

use crate::{
    AppTheme, blend_hex, contrast_ratio, hex_color_distance, measure_cluster_width,
    measure_sanitized_text_width, measure_text_width, ratatui_theme_color,
};

const MIN_LINE_HIGHLIGHT_BG_DISTANCE: u16 = 72;
const LINE_HIGHLIGHT_BLEND_STEP: f64 = 0.05;
const LINE_HIGHLIGHT_MAX_BLEND: f64 = 0.85;
const MIN_LINE_HIGHLIGHT_TEXT_CONTRAST: f64 = 3.1;
const DEFAULT_DIM_RATIO: f64 = 0.45;
const MIN_DIM_TEXT_CONTRAST: f64 = 1.6;

/// One mark resolved to terminal columns of the rendered, expanded line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineHighlightColRange {
    pub start_col: usize,
    pub end_col: usize,
    pub tone: HighlightTone,
}

/// Column ranges keyed by [`line_highlight_paint_key`].
///
/// Context and expanded-gap lines share one `Arc` under both side keys. This
/// mirrors their paint in split layout and avoids duplicate work in stack.
#[derive(Debug, Clone, Default)]
pub struct LineHighlightPaintIndex(BTreeMap<String, Arc<LineHighlightRangeList>>);

/// Identity-stable ranges with a lazily cached overlap plan.
#[derive(Debug, Default)]
pub struct LineHighlightRangeList {
    ranges: Arc<[LineHighlightColRange]>,
    plan: OnceLock<LineHighlightCutPlan>,
}

impl LineHighlightRangeList {
    #[must_use]
    pub fn new(ranges: impl Into<Arc<[LineHighlightColRange]>>) -> Self {
        Self {
            ranges: ranges.into(),
            plan: OnceLock::new(),
        }
    }

    #[must_use]
    pub fn as_slice(&self) -> &[LineHighlightColRange] {
        &self.ranges
    }
}

impl Deref for LineHighlightRangeList {
    type Target = [LineHighlightColRange];

    fn deref(&self) -> &Self::Target {
        &self.ranges
    }
}

impl AsRef<[LineHighlightColRange]> for LineHighlightRangeList {
    fn as_ref(&self) -> &[LineHighlightColRange] {
        self
    }
}

impl LineHighlightPaintIndex {
    #[must_use]
    pub fn get(&self, side: ReviewSide, line: u64) -> Option<&Arc<LineHighlightRangeList>> {
        self.0.get(&line_highlight_paint_key(side, line))
    }

    #[must_use]
    pub fn get_key(&self, key: &str) -> Option<&Arc<LineHighlightRangeList>> {
        self.0.get(key)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Key one rendered line by its side and one-based line number.
#[must_use]
pub fn line_highlight_paint_key(side: ReviewSide, line: u64) -> String {
    let side = match side {
        ReviewSide::Old => "old",
        ReviewSide::New => "new",
    };
    format!("{side}:{line}")
}

#[derive(Debug, Clone)]
struct AddressedLine {
    raw_text: String,
    counterpart_key: Option<String>,
}

fn strip_trailing_newline(text: &str) -> &str {
    text.strip_suffix('\n').unwrap_or(text)
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

fn utf16_boundary_to_byte(text: &str, target: usize) -> usize {
    if target == 0 {
        return 0;
    }
    let mut units = 0;
    for (byte, character) in text.char_indices() {
        if units == target {
            return byte;
        }
        units += character.len_utf16();
    }
    text.len()
}

#[derive(Debug, Clone, Copy)]
enum SnapDirection {
    Down,
    Up,
}

/// Widen a mid-cluster UTF-16 offset outward to a grapheme boundary.
fn snap_to_cluster_boundary(text: &str, offset: usize, direction: SnapDirection) -> usize {
    if offset == 0 {
        return 0;
    }
    let text_len = utf16_len(text);
    if offset >= text_len {
        return text_len;
    }
    let mut boundary = 0;
    for cluster in UnicodeSegmentation::graphemes(text, true) {
        let next = boundary + utf16_len(cluster);
        if next == offset {
            return offset;
        }
        if next > offset {
            return match direction {
                SnapDirection::Down => boundary,
                SnapDirection::Up => next,
            };
        }
        boundary = next;
    }
    text_len
}

/// Map a raw UTF-16 offset through terminal sanitization.
fn raw_offset_to_sanitized_offset(raw: &str, sanitized: &str, offset: u64) -> usize {
    let raw_len = utf16_len(raw);
    let clamped = usize::try_from(offset).unwrap_or(usize::MAX).min(raw_len);
    if raw == sanitized {
        return clamped;
    }

    // JavaScript can slice between a surrogate pair. Rust strings cannot
    // represent the resulting lone surrogate, so retain its one-code-unit
    // contribution after sanitizing the preceding scalar boundary. The next
    // cluster snap then contains it to the same complete visible glyph.
    let mut units = 0;
    let mut byte_end = 0;
    let mut partial_surrogate = 0;
    for (byte, character) in raw.char_indices() {
        let next = units + character.len_utf16();
        if next > clamped {
            byte_end = byte;
            partial_surrogate = clamped - units;
            break;
        }
        units = next;
        byte_end = byte + character.len_utf8();
        if units == clamped {
            break;
        }
    }
    utf16_len(&sanitize_terminal_line(&raw[..byte_end]))
        .saturating_add(partial_surrogate)
        .min(utf16_len(sanitized))
}

fn mark_to_col_range(
    mark: &ValidatedLineHighlight,
    raw_text: &str,
    tab_width: u16,
) -> Option<LineHighlightColRange> {
    let raw = strip_trailing_newline(raw_text);
    let sanitized = sanitize_terminal_line(raw);
    if sanitized.is_empty() {
        return None;
    }
    let start = snap_to_cluster_boundary(
        &sanitized,
        raw_offset_to_sanitized_offset(raw, &sanitized, mark.start),
        SnapDirection::Down,
    );
    let end = snap_to_cluster_boundary(
        &sanitized,
        raw_offset_to_sanitized_offset(raw, &sanitized, mark.end),
        SnapDirection::Up,
    );
    if start >= end {
        return None;
    }
    let start_byte = utf16_boundary_to_byte(&sanitized, start);
    let end_byte = utf16_boundary_to_byte(&sanitized, end);
    let start_col = measure_text_width(&expand_diff_tabs(&sanitized[..start_byte], tab_width, 0));
    let end_col = measure_text_width(&expand_diff_tabs(&sanitized[..end_byte], tab_width, 0));
    (end_col > start_col).then_some(LineHighlightColRange {
        start_col,
        end_col,
        tone: mark.tone,
    })
}

fn resolve_patch_lines(
    file: &DiffFile,
    addressed_keys: &BTreeSet<String>,
) -> BTreeMap<String, AddressedLine> {
    let mut resolved = BTreeMap::new();
    for line in file.hunks.iter().flat_map(|hunk| &hunk.lines) {
        match line.kind {
            DiffLineKind::Context => {
                let (Some(old_line), Some(new_line)) = (line.old_line, line.new_line) else {
                    continue;
                };
                let old_key = line_highlight_paint_key(ReviewSide::Old, u64::from(old_line));
                let new_key = line_highlight_paint_key(ReviewSide::New, u64::from(new_line));
                if !addressed_keys.contains(&old_key) && !addressed_keys.contains(&new_key) {
                    continue;
                }
                resolved.insert(
                    old_key.clone(),
                    AddressedLine {
                        raw_text: line.content.clone(),
                        counterpart_key: Some(new_key.clone()),
                    },
                );
                resolved.insert(
                    new_key,
                    AddressedLine {
                        raw_text: line.content.clone(),
                        counterpart_key: Some(old_key),
                    },
                );
            }
            DiffLineKind::Deletion => {
                let Some(number) = line.old_line else {
                    continue;
                };
                let key = line_highlight_paint_key(ReviewSide::Old, u64::from(number));
                if addressed_keys.contains(&key) {
                    resolved.insert(
                        key,
                        AddressedLine {
                            raw_text: line.content.clone(),
                            counterpart_key: None,
                        },
                    );
                }
            }
            DiffLineKind::Addition => {
                let Some(number) = line.new_line else {
                    continue;
                };
                let key = line_highlight_paint_key(ReviewSide::New, u64::from(number));
                if addressed_keys.contains(&key) {
                    resolved.insert(
                        key,
                        AddressedLine {
                            raw_text: line.content.clone(),
                            counterpart_key: None,
                        },
                    );
                }
            }
        }
    }
    resolved
}

fn resolve_gap_lines(
    file: &DiffFile,
    addressed_keys: &BTreeSet<String>,
    resolved: &mut BTreeMap<String, AddressedLine>,
    source_text: &str,
) {
    let source = review_gap_source_for_file(file);
    let mut gaps = file
        .hunks
        .iter()
        .enumerate()
        .filter_map(|(index, _)| review_leading_gap(&source, index))
        .collect::<Vec<_>>();
    if let Some(trailing) = review_trailing_gap(&source) {
        gaps.push(trailing);
    }
    let source_lines = normalized_review_source_lines(source_text);
    let expansion_side = review_expansion_side(file.change_kind);
    let pending = addressed_keys
        .iter()
        .filter(|key| !resolved.contains_key(*key))
        .cloned()
        .collect::<Vec<_>>();

    for key in pending {
        let Some((side, line)) = parse_paint_key(&key) else {
            continue;
        };
        for gap in &gaps {
            let (range, other_range) = match side {
                ReviewSide::Old => (gap.old_range, gap.new_range),
                ReviewSide::New => (gap.new_range, gap.old_range),
            };
            let Ok(line) = u32::try_from(line) else {
                break;
            };
            if line < range.start || line > range.end {
                continue;
            }
            let offset = line - range.start;
            let Some(counterpart) = other_range.start.checked_add(offset) else {
                break;
            };
            let expansion_line = if side == expansion_side {
                line
            } else {
                counterpart
            };
            let Some(raw_text) = expansion_line
                .checked_sub(1)
                .and_then(|line| usize::try_from(line).ok())
                .and_then(|index| source_lines.get(index))
                .cloned()
            else {
                break;
            };
            let counterpart_side = match side {
                ReviewSide::Old => ReviewSide::New,
                ReviewSide::New => ReviewSide::Old,
            };
            let counterpart_key =
                line_highlight_paint_key(counterpart_side, u64::from(counterpart));
            resolved.insert(
                key.clone(),
                AddressedLine {
                    raw_text: raw_text.clone(),
                    counterpart_key: Some(counterpart_key.clone()),
                },
            );
            resolved.insert(
                counterpart_key,
                AddressedLine {
                    raw_text,
                    counterpart_key: Some(key.clone()),
                },
            );
            break;
        }
    }
}

fn parse_paint_key(key: &str) -> Option<(ReviewSide, u64)> {
    let (side, line) = key.split_once(':')?;
    Some((
        match side {
            "old" => ReviewSide::Old,
            "new" => ReviewSide::New,
            _ => return None,
        },
        line.parse().ok()?,
    ))
}

/// Resolve validated source-coordinate marks to terminal-column ranges.
#[must_use]
pub fn build_line_highlight_paint_index(
    file: &DiffFile,
    marks: &[ValidatedLineHighlight],
    tab_width: u16,
    source_text: Option<&str>,
) -> Option<LineHighlightPaintIndex> {
    if marks.is_empty() {
        return None;
    }
    let addressed_keys = marks
        .iter()
        .map(|mark| line_highlight_paint_key(mark.side, mark.line))
        .collect::<BTreeSet<_>>();
    let mut lines = resolve_patch_lines(file, &addressed_keys);
    if let Some(source_text) = source_text {
        resolve_gap_lines(file, &addressed_keys, &mut lines, source_text);
    }

    let mut buckets: Vec<Vec<LineHighlightColRange>> = Vec::new();
    let mut key_to_bucket = BTreeMap::<String, usize>::new();
    for mark in marks {
        let key = line_highlight_paint_key(mark.side, mark.line);
        let Some(line) = lines.get(&key) else {
            continue;
        };
        let Some(range) = mark_to_col_range(mark, &line.raw_text, tab_width) else {
            continue;
        };
        let bucket = key_to_bucket
            .get(&key)
            .copied()
            .or_else(|| {
                line.counterpart_key
                    .as_ref()
                    .and_then(|counterpart| key_to_bucket.get(counterpart).copied())
            })
            .unwrap_or_else(|| {
                buckets.push(Vec::new());
                buckets.len() - 1
            });
        key_to_bucket.insert(key, bucket);
        if let Some(counterpart) = &line.counterpart_key {
            key_to_bucket.insert(counterpart.clone(), bucket);
        }
        buckets[bucket].push(range);
    }
    if key_to_bucket.is_empty() {
        return None;
    }

    let buckets = buckets
        .into_iter()
        .map(|ranges| Arc::new(LineHighlightRangeList::new(ranges)))
        .collect::<Vec<_>>();
    Some(LineHighlightPaintIndex(
        key_to_bucket
            .into_iter()
            .map(|(key, bucket)| (key, Arc::clone(&buckets[bucket])))
            .collect(),
    ))
}

/// Resolved paint for one tone.
#[derive(Clone)]
pub struct LineHighlightSpanStyle {
    pub background: Option<String>,
    pub foreground: Option<String>,
    pub transform_foreground: Option<RenderForegroundTransform>,
}

impl std::fmt::Debug for LineHighlightSpanStyle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LineHighlightSpanStyle")
            .field("background", &self.background)
            .field("foreground", &self.foreground)
            .field(
                "transform_foreground",
                &self.transform_foreground.as_ref().map(|_| "<transform>"),
            )
            .finish()
    }
}

#[derive(Debug)]
struct LineHighlightCutPlan {
    cuts: Vec<usize>,
    tones: Vec<Option<HighlightTone>>,
}

fn next_unclaimed(skip: &mut [usize], index: usize) -> usize {
    let mut current = index;
    while skip[current] != current {
        current = skip[current];
    }
    let root = current;
    let mut current = index;
    while skip[current] != current {
        let next = skip[current];
        skip[current] = root;
        current = next;
    }
    root
}

fn line_highlight_cut_plan(ranges: &[LineHighlightColRange]) -> LineHighlightCutPlan {
    let mut cuts = ranges
        .iter()
        .flat_map(|range| [range.start_col, range.end_col])
        .collect::<Vec<_>>();
    cuts.sort_unstable();
    cuts.dedup();
    let column_index = cuts
        .iter()
        .copied()
        .enumerate()
        .map(|(index, column)| (column, index))
        .collect::<HashMap<_, _>>();
    let intervals = cuts.len().saturating_sub(1);
    let mut tones = vec![None; intervals];
    let mut skip = (0..=intervals).collect::<Vec<_>>();
    for range in ranges.iter().rev() {
        let to = column_index[&range.end_col];
        let mut interval = next_unclaimed(&mut skip, column_index[&range.start_col]);
        while interval < to {
            tones[interval] = Some(range.tone);
            skip[interval] = interval + 1;
            interval = next_unclaimed(&mut skip, interval + 1);
        }
    }
    LineHighlightCutPlan { cuts, tones }
}

fn tone_at_column(plan: &LineHighlightCutPlan, column: usize) -> Option<HighlightTone> {
    let insertion = plan.cuts.partition_point(|cut| *cut <= column);
    insertion
        .checked_sub(1)
        .filter(|index| *index < plan.tones.len())
        .and_then(|index| plan.tones[index])
}

fn append_span(target: &mut Vec<RenderSpan>, span: RenderSpan) {
    if let Some(previous) = target.last_mut().filter(|previous| {
        previous.foreground == span.foreground
            && previous.background == span.background
            && previous.transform_foreground == span.transform_foreground
    }) {
        previous.text.push_str(&span.text);
    } else {
        target.push(span);
    }
}

fn painted_span(
    span: &RenderSpan,
    text: String,
    start_col: usize,
    plan: &LineHighlightCutPlan,
    resolve_style: &impl Fn(HighlightTone) -> Option<LineHighlightSpanStyle>,
) -> RenderSpan {
    let Some(style) = tone_at_column(plan, start_col).and_then(resolve_style) else {
        let mut preserved = span.clone();
        preserved.text = text;
        return preserved;
    };
    let mut painted = span.clone();
    painted.text = text;
    if let Some(transform) = style.transform_foreground {
        if style.background.is_some() {
            painted.background = style.background;
        }
        painted.transform_foreground = Some(transform);
        return painted;
    }
    if style.foreground.is_some() {
        painted.background = style.background;
        painted.foreground = style.foreground;
    } else if style.background.is_some() {
        painted.background = style.background;
    }
    painted
}

/// Repaint immutable spans over column ranges without changing their text or width.
#[must_use]
pub fn apply_line_highlights_to_spans(
    spans: &[RenderSpan],
    ranges: &[LineHighlightColRange],
    resolve_style: impl Fn(HighlightTone) -> Option<LineHighlightSpanStyle>,
) -> Vec<RenderSpan> {
    if ranges.is_empty() {
        return spans.to_vec();
    }
    let plan = line_highlight_cut_plan(ranges);
    apply_line_highlights_with_plan(spans, &plan, resolve_style)
}

/// Repaint one identity-stable range list, deriving its overlap plan only once.
#[must_use]
pub fn apply_prepared_line_highlights_to_spans(
    spans: &[RenderSpan],
    ranges: &LineHighlightRangeList,
    resolve_style: impl Fn(HighlightTone) -> Option<LineHighlightSpanStyle>,
) -> Vec<RenderSpan> {
    if ranges.is_empty() {
        return spans.to_vec();
    }
    let plan = ranges
        .plan
        .get_or_init(|| line_highlight_cut_plan(ranges.as_ref()));
    apply_line_highlights_with_plan(spans, plan, resolve_style)
}

fn apply_line_highlights_with_plan(
    spans: &[RenderSpan],
    plan: &LineHighlightCutPlan,
    resolve_style: impl Fn(HighlightTone) -> Option<LineHighlightSpanStyle>,
) -> Vec<RenderSpan> {
    let mut result = Vec::new();
    let mut col = 0;
    let mut cut_cursor = 0;

    for span in spans {
        let safe_text = sanitize_terminal_line(&span.text);
        let span_width = measure_sanitized_text_width(&safe_text);
        if span_width == 0 {
            append_span(&mut result, span.clone());
            continue;
        }
        let span_start = col;
        let span_end = col + span_width;
        col = span_end;
        while cut_cursor < plan.cuts.len() && plan.cuts[cut_cursor] <= span_start {
            cut_cursor += 1;
        }
        if cut_cursor >= plan.cuts.len() || plan.cuts[cut_cursor] >= span_end {
            append_span(
                &mut result,
                painted_span(span, span.text.clone(), span_start, plan, &resolve_style),
            );
            continue;
        }

        let mut cursor = cut_cursor;
        let mut piece_byte = 0;
        let mut piece_col = span_start;
        if safe_text.is_ascii() && safe_text.bytes().all(|byte| (0x20..=0x7e).contains(&byte)) {
            while cursor < plan.cuts.len() && plan.cuts[cursor] < span_end {
                let cut = plan.cuts[cursor];
                let next_byte = cut - span_start;
                append_span(
                    &mut result,
                    painted_span(
                        span,
                        safe_text[piece_byte..next_byte].to_owned(),
                        piece_col,
                        plan,
                        &resolve_style,
                    ),
                );
                piece_byte = next_byte;
                piece_col = cut;
                cursor += 1;
            }
            append_span(
                &mut result,
                painted_span(
                    span,
                    safe_text[piece_byte..].to_owned(),
                    piece_col,
                    plan,
                    &resolve_style,
                ),
            );
            continue;
        }

        let mut cluster_col = span_start;
        for (cluster_byte, cluster) in
            UnicodeSegmentation::grapheme_indices(safe_text.as_str(), true)
        {
            while cursor < plan.cuts.len() && plan.cuts[cursor] < cluster_col {
                cursor += 1;
            }
            if cursor < plan.cuts.len() && plan.cuts[cursor] == cluster_col {
                append_span(
                    &mut result,
                    painted_span(
                        span,
                        safe_text[piece_byte..cluster_byte].to_owned(),
                        piece_col,
                        plan,
                        &resolve_style,
                    ),
                );
                piece_byte = cluster_byte;
                piece_col = cluster_col;
                cursor += 1;
            }
            cluster_col += measure_cluster_width(cluster);
        }
        append_span(
            &mut result,
            painted_span(
                span,
                safe_text[piece_byte..].to_owned(),
                piece_col,
                plan,
                &resolve_style,
            ),
        );
    }
    result
}

fn is_hex_theme_color(color: &str) -> bool {
    color.len() == 7
        && color.starts_with('#')
        && color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn effective_highlight_background(base: &str, theme: &AppTheme) -> String {
    if is_hex_theme_color(base) {
        return base.to_owned();
    }
    if is_hex_theme_color(&theme.background) {
        return theme.background.clone();
    }
    match theme.appearance {
        crate::ThemeAppearance::Dark => "#000000".into(),
        crate::ThemeAppearance::Light => "#ffffff".into(),
    }
}

fn line_highlight_tone_anchor(tone: HighlightTone, theme: &AppTheme) -> &str {
    match tone {
        HighlightTone::Info => &theme.badge_neutral,
        HighlightTone::Warning => &theme.file_modified,
        HighlightTone::Error => &theme.removed_sign_color,
        HighlightTone::Current | HighlightTone::Match | HighlightTone::Dim => &theme.accent,
    }
}

fn strengthen_line_highlight_background(base: &str, anchor: &str, text_color: &str) -> String {
    let mut strongest_readable = base.to_owned();
    let max_steps = (LINE_HIGHLIGHT_MAX_BLEND / LINE_HIGHLIGHT_BLEND_STEP).floor() as usize;
    for step in 1..=max_steps {
        let candidate = blend_hex(anchor, base, step as f64 * LINE_HIGHLIGHT_BLEND_STEP);
        if contrast_ratio(text_color, &candidate) < MIN_LINE_HIGHLIGHT_TEXT_CONTRAST {
            return strongest_readable;
        }
        strongest_readable.clone_from(&candidate);
        if hex_color_distance(&candidate, base) >= MIN_LINE_HIGHLIGHT_BG_DISTANCE {
            return candidate;
        }
    }
    strongest_readable
}

fn color_hex(color: Color) -> Option<String> {
    match color {
        Color::Rgb(red, green, blue) => Some(format!("#{red:02x}{green:02x}{blue:02x}")),
        _ => None,
    }
}

fn dim_span_foreground(
    source_foreground: Option<Color>,
    span_background: Option<Color>,
    base_background: &str,
    theme: &AppTheme,
) -> Color {
    let background = span_background
        .and_then(color_hex)
        .unwrap_or_else(|| base_background.to_owned());
    let background = effective_highlight_background(&background, theme);
    let fallback = if is_hex_theme_color(&theme.syntax_colors.default) {
        theme.syntax_colors.default.as_str()
    } else if is_hex_theme_color(&theme.text) {
        theme.text.as_str()
    } else {
        match theme.appearance {
            crate::ThemeAppearance::Dark => "#adbac7",
            crate::ThemeAppearance::Light => "#24292f",
        }
    };
    let foreground = source_foreground
        .and_then(color_hex)
        .filter(|color| is_hex_theme_color(color))
        .unwrap_or_else(|| fallback.to_owned());
    let mut result = foreground.clone();
    let candidate = blend_hex(&foreground, &background, DEFAULT_DIM_RATIO);
    if contrast_ratio(&candidate, &background) >= MIN_DIM_TEXT_CONTRAST {
        result = candidate;
    } else {
        for step in 1..=9 {
            let ratio = DEFAULT_DIM_RATIO + f64::from(step) * 0.05;
            if ratio > 0.901 {
                break;
            }
            let strengthened = blend_hex(&foreground, &background, ratio);
            if contrast_ratio(&strengthened, &background) >= MIN_DIM_TEXT_CONTRAST {
                result = strengthened;
                break;
            }
        }
    }
    ratatui_theme_color(&result)
}

fn paint_ratatui_style(
    style: Style,
    tone: HighlightTone,
    base_background: &str,
    theme: &AppTheme,
) -> Style {
    if tone == HighlightTone::Dim {
        return style.fg(dim_span_foreground(
            style.fg,
            style.bg,
            base_background,
            theme,
        ));
    }
    if tone == HighlightTone::Current && is_hex_theme_color(&theme.text) {
        return style
            .bg(ratatui_theme_color(&theme.text))
            .fg(ratatui_theme_color(&effective_highlight_background(
                &theme.background,
                theme,
            )));
    }
    let anchor = line_highlight_tone_anchor(tone, theme);
    if !is_hex_theme_color(anchor) || !is_hex_theme_color(&theme.text) {
        return style;
    }
    let background = strengthen_line_highlight_background(
        &effective_highlight_background(base_background, theme),
        anchor,
        &theme.text,
    );
    style.bg(ratatui_theme_color(&background))
}

fn append_ratatui_span(target: &mut Vec<Span<'static>>, span: Span<'static>) {
    if let Some(previous) = target
        .last_mut()
        .filter(|previous| previous.style == span.style)
    {
        previous.content.to_mut().push_str(&span.content);
    } else {
        target.push(span);
    }
}

fn painted_ratatui_span(
    span: &Span<'static>,
    text: String,
    start_col: usize,
    plan: &LineHighlightCutPlan,
    base_background: &str,
    theme: &AppTheme,
) -> Span<'static> {
    let style = tone_at_column(plan, start_col).map_or(span.style, |tone| {
        paint_ratatui_style(span.style, tone, base_background, theme)
    });
    Span::styled(text, style)
}

/// Apply a prepared line's marks directly to Ratatui spans after syntax and word-diff paint.
#[must_use]
pub fn apply_prepared_line_highlights_to_ratatui_spans(
    spans: Vec<Span<'static>>,
    ranges: &LineHighlightRangeList,
    base_background: &str,
    theme: &AppTheme,
) -> Vec<Span<'static>> {
    if ranges.is_empty() {
        return spans;
    }
    let plan = ranges
        .plan
        .get_or_init(|| line_highlight_cut_plan(ranges.as_ref()));
    let mut result = Vec::new();
    let mut col = 0;
    let mut cut_cursor = 0;
    for span in spans {
        let safe_text = sanitize_terminal_line(&span.content);
        let span_width = measure_sanitized_text_width(&safe_text);
        if span_width == 0 {
            append_ratatui_span(&mut result, span);
            continue;
        }
        let span_start = col;
        let span_end = col + span_width;
        col = span_end;
        while cut_cursor < plan.cuts.len() && plan.cuts[cut_cursor] <= span_start {
            cut_cursor += 1;
        }
        if cut_cursor >= plan.cuts.len() || plan.cuts[cut_cursor] >= span_end {
            append_ratatui_span(
                &mut result,
                painted_ratatui_span(
                    &span,
                    span.content.to_string(),
                    span_start,
                    plan,
                    base_background,
                    theme,
                ),
            );
            continue;
        }
        let mut cursor = cut_cursor;
        let mut piece_byte = 0;
        let mut piece_col = span_start;
        if safe_text.is_ascii() && safe_text.bytes().all(|byte| (0x20..=0x7e).contains(&byte)) {
            while cursor < plan.cuts.len() && plan.cuts[cursor] < span_end {
                let cut = plan.cuts[cursor];
                let next_byte = cut - span_start;
                append_ratatui_span(
                    &mut result,
                    painted_ratatui_span(
                        &span,
                        safe_text[piece_byte..next_byte].to_owned(),
                        piece_col,
                        plan,
                        base_background,
                        theme,
                    ),
                );
                piece_byte = next_byte;
                piece_col = cut;
                cursor += 1;
            }
            append_ratatui_span(
                &mut result,
                painted_ratatui_span(
                    &span,
                    safe_text[piece_byte..].to_owned(),
                    piece_col,
                    plan,
                    base_background,
                    theme,
                ),
            );
            continue;
        }
        let mut cluster_col = span_start;
        for (cluster_byte, cluster) in
            UnicodeSegmentation::grapheme_indices(safe_text.as_str(), true)
        {
            while cursor < plan.cuts.len() && plan.cuts[cursor] < cluster_col {
                cursor += 1;
            }
            if cursor < plan.cuts.len() && plan.cuts[cursor] == cluster_col {
                append_ratatui_span(
                    &mut result,
                    painted_ratatui_span(
                        &span,
                        safe_text[piece_byte..cluster_byte].to_owned(),
                        piece_col,
                        plan,
                        base_background,
                        theme,
                    ),
                );
                piece_byte = cluster_byte;
                piece_col = cluster_col;
                cursor += 1;
            }
            cluster_col += measure_cluster_width(cluster);
        }
        append_ratatui_span(
            &mut result,
            painted_ratatui_span(
                &span,
                safe_text[piece_byte..].to_owned(),
                piece_col,
                plan,
                base_background,
                theme,
            ),
        );
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use unicode_width::UnicodeWidthStr;
    use workdeck_core::{
        DiffHunk, DiffLine, FileChangeKind, FileFlags, FileSourceSnapshots, FileStats,
        SourceOrigin, SourceSnapshot,
    };
    use workdeck_diff::DEFAULT_TAB_WIDTH;

    fn mark(
        side: ReviewSide,
        line: u64,
        start: u64,
        end: u64,
        tone: HighlightTone,
    ) -> ValidatedLineHighlight {
        ValidatedLineHighlight {
            side,
            line,
            start,
            end,
            tone,
        }
    }

    fn diff_line(
        kind: DiffLineKind,
        content: &str,
        old_line: Option<u32>,
        new_line: Option<u32>,
    ) -> DiffLine {
        DiffLine {
            kind,
            content: content.into(),
            old_line,
            new_line,
            moved: false,
            no_newline_at_eof: false,
        }
    }

    fn test_file(hunk: DiffHunk) -> DiffFile {
        DiffFile {
            key: "test:file".into(),
            runtime_id: "test-file".into(),
            path: "test.rs".into(),
            previous_path: None,
            change_kind: FileChangeKind::Modified,
            language: Some("rust".into()),
            stats: FileStats::default(),
            flags: FileFlags::default(),
            patch: String::new(),
            split_row_count: hunk.lines.len(),
            stack_row_count: hunk.lines.len(),
            hunks: vec![hunk],
            content_identity: "test-content".into(),
            sources: FileSourceSnapshots::default(),
            source_identity: None,
            source_attested: false,
            agent: None,
        }
    }

    fn one_line_change(before: &str, after: &str) -> DiffFile {
        test_file(DiffHunk {
            index: 0,
            header: "@@ -1 +1 @@".into(),
            context: None,
            old_start: 1,
            old_count: 1,
            new_start: 1,
            new_count: 1,
            split_row_start: 0,
            split_row_count: 1,
            stack_row_start: 0,
            stack_row_count: 2,
            lines: vec![
                diff_line(DiffLineKind::Deletion, before, Some(1), None),
                diff_line(DiffLineKind::Addition, after, None, Some(1)),
            ],
        })
    }

    fn context_change() -> DiffFile {
        test_file(DiffHunk {
            index: 0,
            header: "@@ -1,2 +1,2 @@".into(),
            context: None,
            old_start: 1,
            old_count: 2,
            new_start: 1,
            new_count: 2,
            split_row_start: 0,
            split_row_count: 2,
            stack_row_start: 0,
            stack_row_count: 3,
            lines: vec![
                diff_line(DiffLineKind::Context, "shared line", Some(1), Some(1)),
                diff_line(DiffLineKind::Deletion, "old only", Some(2), None),
                diff_line(DiffLineKind::Addition, "new only", None, Some(2)),
            ],
        })
    }

    fn span(text: &str) -> RenderSpan {
        RenderSpan {
            text: text.into(),
            foreground: None,
            background: None,
            transform_foreground: None,
        }
    }

    fn background_style(tone: HighlightTone) -> Option<LineHighlightSpanStyle> {
        Some(LineHighlightSpanStyle {
            background: Some(format!("bg-{tone:?}").to_lowercase()),
            foreground: None,
            transform_foreground: None,
        })
    }

    #[test]
    fn maps_addition_and_deletion_offsets_to_columns_on_their_own_sides() {
        let file = one_line_change("const alpha = 1;", "const alpha = 10;");
        let index = build_line_highlight_paint_index(
            &file,
            &[
                mark(ReviewSide::New, 1, 6, 11, HighlightTone::Match),
                mark(ReviewSide::Old, 1, 14, 15, HighlightTone::Error),
            ],
            DEFAULT_TAB_WIDTH,
            None,
        )
        .unwrap();
        assert_eq!(
            index.get(ReviewSide::New, 1).unwrap().as_slice(),
            [LineHighlightColRange {
                start_col: 6,
                end_col: 11,
                tone: HighlightTone::Match,
            }]
        );
        assert_eq!(
            index.get(ReviewSide::Old, 1).unwrap().as_slice(),
            [LineHighlightColRange {
                start_col: 14,
                end_col: 15,
                tone: HighlightTone::Error,
            }]
        );
    }

    #[test]
    fn mirrors_context_marks_under_both_keys_with_shared_identity() {
        let file = context_change();
        let index = build_line_highlight_paint_index(
            &file,
            &[mark(ReviewSide::New, 1, 0, 6, HighlightTone::Match)],
            DEFAULT_TAB_WIDTH,
            None,
        )
        .unwrap();
        let new = index.get(ReviewSide::New, 1).unwrap();
        let old = index.get(ReviewSide::Old, 1).unwrap();
        assert_eq!(
            new.as_slice(),
            [LineHighlightColRange {
                start_col: 0,
                end_col: 6,
                tone: HighlightTone::Match,
            }]
        );
        assert!(Arc::ptr_eq(old, new));
    }

    #[test]
    fn expands_tabs_when_converting_offsets_to_columns() {
        let file = one_line_change("none", "\tfoo = 1;");
        let index = build_line_highlight_paint_index(
            &file,
            &[mark(ReviewSide::New, 1, 1, 4, HighlightTone::Match)],
            4,
            None,
        )
        .unwrap();
        assert_eq!(
            index.get(ReviewSide::New, 1).unwrap().as_slice(),
            [LineHighlightColRange {
                start_col: 4,
                end_col: 7,
                tone: HighlightTone::Match,
            }]
        );
    }

    #[test]
    fn widens_a_mid_surrogate_offset_to_the_whole_glyph() {
        let file = one_line_change("none", "x = \"👍ok\";");
        let index = build_line_highlight_paint_index(
            &file,
            &[mark(ReviewSide::New, 1, 6, 9, HighlightTone::Match)],
            DEFAULT_TAB_WIDTH,
            None,
        )
        .unwrap();
        assert_eq!(
            index.get(ReviewSide::New, 1).unwrap().as_slice(),
            [LineHighlightColRange {
                start_col: 5,
                end_col: 9,
                tone: HighlightTone::Match,
            }]
        );
    }

    #[test]
    fn drops_marks_on_lines_the_patch_does_not_carry() {
        let file = one_line_change("const alpha = 1;", "const alpha = 10;");
        assert!(
            build_line_highlight_paint_index(
                &file,
                &[mark(ReviewSide::New, 99, 0, 4, HighlightTone::Match)],
                DEFAULT_TAB_WIDTH,
                None,
            )
            .is_none()
        );
    }

    #[test]
    fn resolves_collapsed_gap_lines_through_loaded_source_on_both_sides() {
        let before = "line one\nline two\nline three\nline four\noriginal\n";
        let after = "line one\nline two\nline three\nline four\nchanged\n";
        let mut file = test_file(DiffHunk {
            index: 0,
            header: "@@ -5 +5 @@".into(),
            context: None,
            old_start: 5,
            old_count: 1,
            new_start: 5,
            new_count: 1,
            split_row_start: 0,
            split_row_count: 1,
            stack_row_start: 0,
            stack_row_count: 2,
            lines: vec![
                diff_line(DiffLineKind::Deletion, "original", Some(5), None),
                diff_line(DiffLineKind::Addition, "changed", None, Some(5)),
            ],
        });
        file.sources = FileSourceSnapshots {
            old: Some(SourceSnapshot::new(
                before.into(),
                SourceOrigin::WorkingTree,
                true,
            )),
            new: Some(SourceSnapshot::new(
                after.into(),
                SourceOrigin::WorkingTree,
                true,
            )),
        };
        let marks = [mark(ReviewSide::New, 2, 5, 8, HighlightTone::Match)];
        assert!(build_line_highlight_paint_index(&file, &marks, DEFAULT_TAB_WIDTH, None).is_none());
        let index = build_line_highlight_paint_index(&file, &marks, DEFAULT_TAB_WIDTH, Some(after))
            .unwrap();
        let new = index.get(ReviewSide::New, 2).unwrap();
        assert_eq!(
            new.as_slice(),
            [LineHighlightColRange {
                start_col: 5,
                end_col: 8,
                tone: HighlightTone::Match,
            }]
        );
        assert!(Arc::ptr_eq(index.get(ReviewSide::Old, 2).unwrap(), new));
    }

    #[test]
    fn preserves_semantic_input_order_when_ranges_start_out_of_order() {
        let file = one_line_change("none", "abcdefghij");
        let index = build_line_highlight_paint_index(
            &file,
            &[
                mark(ReviewSide::New, 1, 6, 8, HighlightTone::Info),
                mark(ReviewSide::New, 1, 1, 3, HighlightTone::Match),
            ],
            DEFAULT_TAB_WIDTH,
            None,
        )
        .unwrap();
        assert_eq!(
            index.get(ReviewSide::New, 1).unwrap().as_slice(),
            [
                LineHighlightColRange {
                    start_col: 6,
                    end_col: 8,
                    tone: HighlightTone::Info,
                },
                LineHighlightColRange {
                    start_col: 1,
                    end_col: 3,
                    tone: HighlightTone::Match,
                },
            ]
        );
    }

    #[test]
    fn preserves_interleaved_old_new_order_on_one_context_line() {
        let file = context_change();
        let index = build_line_highlight_paint_index(
            &file,
            &[
                mark(ReviewSide::Old, 1, 7, 11, HighlightTone::Dim),
                mark(ReviewSide::New, 1, 0, 6, HighlightTone::Info),
                mark(ReviewSide::Old, 1, 3, 9, HighlightTone::Current),
            ],
            DEFAULT_TAB_WIDTH,
            None,
        )
        .unwrap();
        let ranges = index.get(ReviewSide::New, 1).unwrap();
        assert_eq!(
            ranges.iter().map(|range| range.tone).collect::<Vec<_>>(),
            [
                HighlightTone::Dim,
                HighlightTone::Info,
                HighlightTone::Current
            ]
        );
        assert!(Arc::ptr_eq(index.get(ReviewSide::Old, 1).unwrap(), ranges));
    }

    #[test]
    fn returns_none_for_no_marks() {
        assert!(
            build_line_highlight_paint_index(
                &one_line_change("old", "new"),
                &[],
                DEFAULT_TAB_WIDTH,
                None,
            )
            .is_none()
        );
    }

    #[test]
    fn splits_one_span_at_range_boundaries() {
        let mut source = span("const alpha = 10;");
        source.foreground = Some("#ffffff".into());
        let painted = apply_line_highlights_to_spans(
            &[source],
            &[LineHighlightColRange {
                start_col: 6,
                end_col: 11,
                tone: HighlightTone::Match,
            }],
            background_style,
        );
        assert_eq!(
            painted
                .iter()
                .map(|span| span.text.as_str())
                .collect::<Vec<_>>(),
            ["const ", "alpha", " = 10;"]
        );
        assert_eq!(painted[1].background.as_deref(), Some("bg-match"));
        assert_eq!(painted[1].foreground.as_deref(), Some("#ffffff"));
    }

    #[test]
    fn never_mutates_shared_input_spans() {
        let spans = vec![span("const alpha = 10;")];
        let before = spans.clone();
        let _ = apply_line_highlights_to_spans(
            &spans,
            &[LineHighlightColRange {
                start_col: 0,
                end_col: 5,
                tone: HighlightTone::Match,
            }],
            background_style,
        );
        assert_eq!(spans, before);
    }

    #[test]
    fn paints_across_span_boundaries_while_preserving_foregrounds() {
        let spans = [
            RenderSpan {
                foreground: Some("#111111".into()),
                ..span("const ")
            },
            RenderSpan {
                foreground: Some("#222222".into()),
                ..span("alpha")
            },
            RenderSpan {
                foreground: Some("#333333".into()),
                ..span(" = 10;")
            },
        ];
        let painted = apply_line_highlights_to_spans(
            &spans,
            &[LineHighlightColRange {
                start_col: 3,
                end_col: 8,
                tone: HighlightTone::Info,
            }],
            background_style,
        );
        assert_eq!(
            painted
                .iter()
                .map(|span| span.text.as_str())
                .collect::<Vec<_>>(),
            ["con", "st ", "al", "pha", " = 10;"]
        );
        assert_eq!(painted[1].foreground.as_deref(), Some("#111111"));
        assert_eq!(painted[2].foreground.as_deref(), Some("#222222"));
        assert_eq!(painted[1].background.as_deref(), Some("bg-info"));
        assert_eq!(painted[2].background.as_deref(), Some("bg-info"));
    }

    #[test]
    fn overrides_word_diff_background_only_inside_marked_range() {
        let spans = [
            RenderSpan {
                background: Some("#204020".into()),
                ..span("alpha")
            },
            RenderSpan {
                background: Some("#204020".into()),
                ..span("beta")
            },
        ];
        let painted = apply_line_highlights_to_spans(
            &spans,
            &[LineHighlightColRange {
                start_col: 5,
                end_col: 9,
                tone: HighlightTone::Match,
            }],
            background_style,
        );
        assert_eq!(painted[0].background.as_deref(), Some("#204020"));
        assert_eq!(painted[1].background.as_deref(), Some("bg-match"));
    }

    #[test]
    fn reverse_video_style_overrides_foreground_and_background() {
        let source = RenderSpan {
            foreground: Some("#ffffff".into()),
            ..span("const alpha = 10;")
        };
        let painted = apply_line_highlights_to_spans(
            &[source],
            &[LineHighlightColRange {
                start_col: 6,
                end_col: 11,
                tone: HighlightTone::Current,
            }],
            |_| {
                Some(LineHighlightSpanStyle {
                    background: Some("#eeeeee".into()),
                    foreground: Some("#111111".into()),
                    transform_foreground: None,
                })
            },
        );
        assert_eq!(painted[1].foreground.as_deref(), Some("#111111"));
        assert_eq!(painted[1].background.as_deref(), Some("#eeeeee"));
    }

    #[test]
    fn defers_foreground_transforms_until_the_final_background_is_known() {
        let spans = [
            RenderSpan {
                foreground: Some("#c678dd".into()),
                ..span("const ")
            },
            RenderSpan {
                foreground: Some("#e5c07b".into()),
                ..span("alpha")
            },
            RenderSpan {
                foreground: Some("#abb2bf".into()),
                ..span(" = 10;")
            },
        ];
        let transform = RenderForegroundTransform::new(|foreground, background| {
            format!("{}:{background}", foreground.unwrap_or("default"))
        });
        let painted = apply_line_highlights_to_spans(
            &spans,
            &[LineHighlightColRange {
                start_col: 0,
                end_col: 17,
                tone: HighlightTone::Dim,
            }],
            |_| {
                Some(LineHighlightSpanStyle {
                    background: None,
                    foreground: None,
                    transform_foreground: Some(transform.clone()),
                })
            },
        );
        assert_eq!(
            painted
                .iter()
                .map(|span| {
                    span.transform_foreground
                        .as_ref()
                        .unwrap()
                        .apply(span.foreground.as_deref(), "#101010")
                })
                .collect::<Vec<_>>(),
            ["#c678dd:#101010", "#e5c07b:#101010", "#abb2bf:#101010"]
        );
    }

    #[test]
    fn dim_transforms_preserve_word_diff_backgrounds() {
        let transform = RenderForegroundTransform::new(|foreground, background| {
            format!("{}:{background}", foreground.unwrap_or_default())
        });
        let spans = [
            RenderSpan {
                foreground: Some("#c678dd".into()),
                background: Some("#204020".into()),
                ..span("alpha")
            },
            RenderSpan {
                foreground: Some("#e5c07b".into()),
                ..span("beta")
            },
        ];
        let painted = apply_line_highlights_to_spans(
            &spans,
            &[LineHighlightColRange {
                start_col: 0,
                end_col: 9,
                tone: HighlightTone::Dim,
            }],
            |_| {
                Some(LineHighlightSpanStyle {
                    background: None,
                    foreground: None,
                    transform_foreground: Some(transform.clone()),
                })
            },
        );
        assert_eq!(painted[0].background.as_deref(), Some("#204020"));
        assert_eq!(painted[1].background, None);
        assert!(
            painted
                .iter()
                .all(|span| span.transform_foreground.is_some())
        );
    }

    #[test]
    fn later_current_overlay_wins_over_earlier_dim_range() {
        let ranges = [
            LineHighlightColRange {
                start_col: 6,
                end_col: 17,
                tone: HighlightTone::Dim,
            },
            LineHighlightColRange {
                start_col: 0,
                end_col: 11,
                tone: HighlightTone::Current,
            },
        ];
        let transform = RenderForegroundTransform::new(|_, _| "#dimmed".into());
        let painted = apply_line_highlights_to_spans(
            &[RenderSpan {
                foreground: Some("#ffffff".into()),
                ..span("abcdefghijklmnopq")
            }],
            &ranges,
            |tone| {
                Some(if tone == HighlightTone::Dim {
                    LineHighlightSpanStyle {
                        background: None,
                        foreground: None,
                        transform_foreground: Some(transform.clone()),
                    }
                } else {
                    LineHighlightSpanStyle {
                        background: Some("#eeeeee".into()),
                        foreground: Some("#111111".into()),
                        transform_foreground: None,
                    }
                })
            },
        );
        assert_eq!(painted[0].text, "abcdefghijk");
        assert_eq!(painted[0].foreground.as_deref(), Some("#111111"));
        assert_eq!(painted[1].text, "lmnopq");
        assert!(painted[1].transform_foreground.is_some());
    }

    #[test]
    fn later_range_wins_over_an_overlap() {
        let painted = apply_line_highlights_to_spans(
            &[span("abcdefghij")],
            &[
                LineHighlightColRange {
                    start_col: 0,
                    end_col: 6,
                    tone: HighlightTone::Match,
                },
                LineHighlightColRange {
                    start_col: 4,
                    end_col: 8,
                    tone: HighlightTone::Current,
                },
            ],
            background_style,
        );
        assert_eq!(
            painted
                .iter()
                .map(|span| (span.text.as_str(), span.background.as_deref()))
                .collect::<Vec<_>>(),
            [
                ("abcd", Some("bg-match")),
                ("efgh", Some("bg-current")),
                ("ij", None),
            ]
        );
    }

    #[test]
    fn keeps_original_background_when_resolver_declines_tone() {
        let painted = apply_line_highlights_to_spans(
            &[RenderSpan {
                background: Some("#101010".into()),
                ..span("abcdef")
            }],
            &[LineHighlightColRange {
                start_col: 0,
                end_col: 3,
                tone: HighlightTone::Match,
            }],
            |_| None,
        );
        assert_eq!(painted.len(), 1);
        assert_eq!(painted[0].text, "abcdef");
        assert_eq!(painted[0].background.as_deref(), Some("#101010"));
    }

    #[test]
    fn dense_overlapping_ranges_resolve_in_near_linear_time() {
        let text = "日".repeat(5_000);
        let width = UnicodeWidthStr::width(text.as_str());
        let ranges = (0..10_000)
            .map(|index| {
                let start_col = (index * 7_919) % (width - 12);
                LineHighlightColRange {
                    start_col,
                    end_col: start_col + 1 + (index % 11),
                    tone: HighlightTone::Match,
                }
            })
            .collect::<Vec<_>>();
        let started = Instant::now();
        let painted = apply_line_highlights_to_spans(
            &[RenderSpan {
                foreground: Some("#ffffff".into()),
                ..span(&text)
            }],
            &ranges,
            background_style,
        );
        assert_eq!(
            painted
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>(),
            text
        );
        assert!(started.elapsed() < Duration::from_millis(150));
    }

    #[test]
    fn prepared_range_lists_cache_one_overlap_plan_across_repaints() {
        let ranges = LineHighlightRangeList::new(vec![LineHighlightColRange {
            start_col: 1,
            end_col: 4,
            tone: HighlightTone::Match,
        }]);
        assert!(ranges.plan.get().is_none());
        let first =
            apply_prepared_line_highlights_to_spans(&[span("abcdef")], &ranges, background_style);
        let first_plan = ranges.plan.get().unwrap() as *const LineHighlightCutPlan;
        let second =
            apply_prepared_line_highlights_to_spans(&[span("abcdef")], &ranges, background_style);
        assert_eq!(first, second);
        assert_eq!(
            ranges.plan.get().unwrap() as *const LineHighlightCutPlan,
            first_plan
        );
    }

    #[test]
    fn preserves_zero_width_spans_and_wide_glyph_boundaries() {
        let painted = apply_line_highlights_to_spans(
            &[span(""), span("x = 👍ok")],
            &[LineHighlightColRange {
                start_col: 4,
                end_col: 6,
                tone: HighlightTone::Match,
            }],
            background_style,
        );
        assert_eq!(
            painted
                .iter()
                .map(|span| span.text.as_str())
                .collect::<Vec<_>>(),
            ["x = ", "👍", "ok"]
        );
        assert_eq!(painted[1].background.as_deref(), Some("bg-match"));
    }

    const HOSTILE_LINES: [&str; 3] = [
        "const 名前 = '日本語テキスト'; // 🎉 emoji",
        "e\u{301}combining + 👩\u{200d}💻 zwj 👍",
        "\tif (値 === 真) {\treturn 結果;\t}",
    ];

    #[test]
    fn arbitrary_column_cuts_never_change_width_or_text() {
        for line in HOSTILE_LINES {
            let text = expand_diff_tabs(line, DEFAULT_TAB_WIDTH, 0);
            let width = measure_text_width(&text);
            for start_col in 0..width {
                for end_col in start_col + 1..=width {
                    let painted = apply_line_highlights_to_spans(
                        &[RenderSpan {
                            foreground: Some("#ffffff".into()),
                            ..span(&text)
                        }],
                        &[LineHighlightColRange {
                            start_col,
                            end_col,
                            tone: HighlightTone::Match,
                        }],
                        background_style,
                    );
                    let painted_text = painted
                        .iter()
                        .map(|span| span.text.as_str())
                        .collect::<String>();
                    assert_eq!(painted_text, text);
                    assert_eq!(measure_text_width(&painted_text), width);
                }
            }
        }
    }

    #[test]
    fn every_utf16_mark_pair_preserves_rendered_geometry() {
        for line in HOSTILE_LINES {
            let file = one_line_change("placeholder", line);
            let expanded = expand_diff_tabs(line, DEFAULT_TAB_WIDTH, 0);
            let original_width = measure_text_width(&expanded);
            let units = utf16_len(line);
            for start in 0..units {
                for end in start + 1..=units {
                    let Some(index) = build_line_highlight_paint_index(
                        &file,
                        &[mark(
                            ReviewSide::New,
                            1,
                            start as u64,
                            end as u64,
                            HighlightTone::Match,
                        )],
                        DEFAULT_TAB_WIDTH,
                        None,
                    ) else {
                        continue;
                    };
                    let painted = apply_line_highlights_to_spans(
                        &[RenderSpan {
                            foreground: Some("#ffffff".into()),
                            ..span(&expanded)
                        }],
                        index.get(ReviewSide::New, 1).unwrap(),
                        background_style,
                    );
                    let painted_text = painted
                        .iter()
                        .map(|span| span.text.as_str())
                        .collect::<String>();
                    assert_eq!(painted_text, expanded);
                    assert_eq!(measure_text_width(&painted_text), original_width);
                }
            }
        }
    }
}
