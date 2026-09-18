//! Hunk-compatible diff-row palettes, rail paint, gutters, and highlight tones.

use std::cell::RefCell;
use std::collections::BTreeMap;

use workdeck_extension_api::HighlightTone;

use crate::{
    AppTheme, TRANSPARENT_BACKGROUND, ThemeAppearance, blend_hex, contrast_ratio,
    hex_color_distance,
};

const INACTIVE_RAIL_BLEND: f64 = 0.35;
const SELECTION_BG_BLEND: f64 = 0.75;
const CURSOR_LINE_BG_BLEND: f64 = 0.2;
const MIN_LINE_HIGHLIGHT_BG_DISTANCE: u16 = 72;
const LINE_HIGHLIGHT_BLEND_STEP: f64 = 0.05;
const LINE_HIGHLIGHT_MAX_BLEND: f64 = 0.85;
const MIN_LINE_HIGHLIGHT_TEXT_CONTRAST: f64 = 3.1;
pub const DEFAULT_DIM_RATIO: f64 = 0.45;
pub const MIN_DIM_TEXT_CONTRAST: f64 = 1.6;
const MAX_DERIVED_STYLE_CACHE_ENTRIES: usize = 4_096;

thread_local! {
    static SELECTION_BACKGROUNDS: RefCell<BTreeMap<String, String>> = const { RefCell::new(BTreeMap::new()) };
    static CURSOR_BACKGROUNDS: RefCell<BTreeMap<String, String>> = const { RefCell::new(BTreeMap::new()) };
    static LINE_HIGHLIGHT_STYLES: RefCell<BTreeMap<String, Option<LineHighlightToneStyle>>> = const { RefCell::new(BTreeMap::new()) };
    static DIM_FOREGROUNDS: RefCell<BTreeMap<String, String>> = const { RefCell::new(BTreeMap::new()) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowCellKind {
    Context,
    Addition,
    Deletion,
    Empty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowCellPalette<'a> {
    pub gutter_background: &'a str,
    pub content_background: &'a str,
    pub sign_color: &'a str,
    pub number_color: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineHighlightToneStyle {
    Colors {
        background: String,
        foreground: Option<String>,
    },
    Dim,
}

#[must_use]
pub const fn diff_rail_marker() -> &'static str {
    "▌"
}

fn cached_background(
    cache: &'static std::thread::LocalKey<RefCell<BTreeMap<String, String>>>,
    key: String,
    calculate: impl FnOnce() -> String,
) -> String {
    cache.with(|cache| {
        if let Some(value) = cache.borrow().get(&key) {
            return value.clone();
        }
        let value = calculate();
        insert_bounded(&mut cache.borrow_mut(), key, value.clone());
        value
    })
}

fn insert_bounded<Value>(cache: &mut BTreeMap<String, Value>, key: String, value: Value) {
    if cache.len() >= MAX_DERIVED_STYLE_CACHE_ENTRIES && !cache.contains_key(&key) {
        cache.clear();
    }
    cache.insert(key, value);
}

#[must_use]
pub fn selection_highlight_background(base_background: &str, theme: &AppTheme) -> String {
    cached_background(
        &SELECTION_BACKGROUNDS,
        format!("{}\0{}", theme.selected_hunk, base_background),
        || blend_hex(&theme.selected_hunk, base_background, SELECTION_BG_BLEND),
    )
}

#[must_use]
pub fn cursor_line_highlight_background(base_background: &str, theme: &AppTheme) -> String {
    let source = if base_background == TRANSPARENT_BACKGROUND {
        match theme.appearance {
            ThemeAppearance::Dark => "#000000",
            ThemeAppearance::Light => "#ffffff",
        }
    } else {
        base_background
    };
    cached_background(
        &CURSOR_BACKGROUNDS,
        format!("{}\0{}\0{}", theme.text, source, theme.appearance as u8),
        || blend_hex(&theme.text, source, CURSOR_LINE_BG_BLEND),
    )
}

#[must_use]
pub fn neutral_rail_color(theme: &AppTheme) -> &str {
    &theme.line_number_fg
}

#[must_use]
pub fn dim_rail_color(color: &str, theme: &AppTheme) -> String {
    blend_hex(color, &theme.panel, INACTIVE_RAIL_BLEND)
}

#[must_use]
pub fn stack_rail_color(kind: RowCellKind, theme: &AppTheme, selected: bool) -> String {
    let color = match kind {
        RowCellKind::Addition => &theme.added_sign_color,
        RowCellKind::Deletion => &theme.removed_sign_color,
        RowCellKind::Context | RowCellKind::Empty => &theme.line_number_fg,
    };
    if selected {
        color.clone()
    } else {
        dim_rail_color(color, theme)
    }
}

#[must_use]
pub fn split_left_rail_color(kind: RowCellKind, theme: &AppTheme, selected: bool) -> String {
    let color = if kind == RowCellKind::Deletion {
        &theme.removed_sign_color
    } else {
        &theme.line_number_fg
    };
    if selected {
        color.clone()
    } else {
        dim_rail_color(color, theme)
    }
}

#[must_use]
pub fn split_right_rail_color(kind: RowCellKind, theme: &AppTheme, selected: bool) -> String {
    let color = if kind == RowCellKind::Addition {
        &theme.added_sign_color
    } else {
        &theme.line_number_fg
    };
    if selected {
        color.clone()
    } else {
        dim_rail_color(color, theme)
    }
}

#[must_use]
pub fn split_cell_palette(kind: RowCellKind, theme: &AppTheme, moved: bool) -> RowCellPalette<'_> {
    match kind {
        RowCellKind::Addition => RowCellPalette {
            gutter_background: if moved {
                &theme.moved_added_bg
            } else {
                &theme.added_bg
            },
            content_background: if moved {
                &theme.moved_added_bg
            } else {
                &theme.added_bg
            },
            sign_color: &theme.added_sign_color,
            number_color: &theme.added_sign_color,
        },
        RowCellKind::Deletion => RowCellPalette {
            gutter_background: if moved {
                &theme.moved_removed_bg
            } else {
                &theme.removed_bg
            },
            content_background: if moved {
                &theme.moved_removed_bg
            } else {
                &theme.removed_bg
            },
            sign_color: &theme.removed_sign_color,
            number_color: &theme.removed_sign_color,
        },
        RowCellKind::Empty => RowCellPalette {
            gutter_background: &theme.line_number_bg,
            content_background: &theme.panel_alt,
            sign_color: &theme.muted,
            number_color: &theme.line_number_fg,
        },
        RowCellKind::Context => RowCellPalette {
            gutter_background: &theme.line_number_bg,
            content_background: &theme.context_bg,
            sign_color: &theme.muted,
            number_color: &theme.line_number_fg,
        },
    }
}

#[must_use]
pub fn stack_cell_palette(kind: RowCellKind, theme: &AppTheme, moved: bool) -> RowCellPalette<'_> {
    debug_assert!(kind != RowCellKind::Empty);
    split_cell_palette(kind, theme, moved)
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
        ThemeAppearance::Dark => "#000000".into(),
        ThemeAppearance::Light => "#ffffff".into(),
    }
}

fn tone_anchor(tone: HighlightTone, theme: &AppTheme) -> &str {
    match tone {
        HighlightTone::Info => &theme.badge_neutral,
        HighlightTone::Warning => &theme.file_modified,
        HighlightTone::Error => &theme.removed_sign_color,
        HighlightTone::Current | HighlightTone::Match | HighlightTone::Dim => &theme.accent,
    }
}

fn strengthen_highlight_background(base: &str, anchor: &str, text_color: &str) -> String {
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

fn tone_cache_key(tone: HighlightTone, base: &str, theme: &AppTheme) -> String {
    format!(
        "{tone:?}\0{base}\0{}\0{}\0{}\0{}\0{}\0{}\0{}",
        theme.appearance as u8,
        theme.background,
        theme.text,
        theme.accent,
        theme.badge_neutral,
        theme.file_modified,
        theme.removed_sign_color,
    )
}

#[must_use]
pub fn line_highlight_tone_style(
    tone: HighlightTone,
    base_background: &str,
    theme: &AppTheme,
) -> Option<LineHighlightToneStyle> {
    let key = tone_cache_key(tone, base_background, theme);
    LINE_HIGHLIGHT_STYLES.with(|cache| {
        if let Some(value) = cache.borrow().get(&key) {
            return value.clone();
        }
        let value = if tone == HighlightTone::Dim {
            Some(LineHighlightToneStyle::Dim)
        } else if tone == HighlightTone::Current && is_hex_theme_color(&theme.text) {
            Some(LineHighlightToneStyle::Colors {
                background: theme.text.clone(),
                foreground: Some(effective_highlight_background(&theme.background, theme)),
            })
        } else {
            let anchor = tone_anchor(tone, theme);
            (is_hex_theme_color(anchor) && is_hex_theme_color(&theme.text)).then(|| {
                LineHighlightToneStyle::Colors {
                    background: strengthen_highlight_background(
                        &effective_highlight_background(base_background, theme),
                        anchor,
                        &theme.text,
                    ),
                    foreground: None,
                }
            })
        };
        insert_bounded(&mut cache.borrow_mut(), key, value.clone());
        value
    })
}

#[must_use]
pub fn dim_span_foreground(
    source_foreground: Option<&str>,
    base_background: &str,
    theme: &AppTheme,
    ratio: f64,
) -> String {
    let key = format!(
        "{}\0{}\0{}\0{}\0{}\0{}\0{}",
        source_foreground.unwrap_or_default(),
        base_background,
        theme.appearance as u8,
        theme.background,
        theme.syntax_colors.default,
        theme.text,
        ratio.to_bits(),
    );
    DIM_FOREGROUNDS.with(|cache| {
        if let Some(value) = cache.borrow().get(&key) {
            return value.clone();
        }
        let background = effective_highlight_background(base_background, theme);
        let fallback = if is_hex_theme_color(&theme.syntax_colors.default) {
            theme.syntax_colors.default.as_str()
        } else if is_hex_theme_color(&theme.text) {
            theme.text.as_str()
        } else {
            match theme.appearance {
                ThemeAppearance::Dark => "#adbac7",
                ThemeAppearance::Light => "#24292f",
            }
        };
        let foreground = source_foreground
            .filter(|color| is_hex_theme_color(color))
            .unwrap_or(fallback);
        let mut result = foreground.to_owned();
        let candidate = blend_hex(foreground, &background, ratio);
        if contrast_ratio(&candidate, &background) >= MIN_DIM_TEXT_CONTRAST {
            result = candidate;
        } else {
            for step in 1..=9 {
                let step_ratio = ratio + f64::from(step) * 0.05;
                if step_ratio > 0.901 {
                    break;
                }
                let strengthened = blend_hex(foreground, &background, step_ratio);
                if contrast_ratio(&strengthened, &background) >= MIN_DIM_TEXT_CONTRAST {
                    result = strengthened;
                    break;
                }
            }
        }
        insert_bounded(&mut cache.borrow_mut(), key, result.clone());
        result
    })
}

#[must_use]
pub fn diff_line_number_text(value: Option<u32>, width: usize) -> String {
    value.map_or_else(|| " ".repeat(width), |value| format!("{value:>width$}"))
}

#[must_use]
pub fn stack_gutter_text(
    sign: char,
    old_line: Option<u32>,
    new_line: Option<u32>,
    line_number_digits: usize,
    show_line_numbers: bool,
) -> String {
    if !show_line_numbers {
        return format!("{sign} ");
    }
    format!(
        "{} {} {sign}",
        diff_line_number_text(old_line, line_number_digits),
        diff_line_number_text(new_line, line_number_digits),
    )
}

#[must_use]
pub fn split_gutter_text(
    sign: char,
    line: Option<u32>,
    line_number_digits: usize,
    show_line_numbers: bool,
) -> String {
    if !show_line_numbers {
        return format!("{sign} ");
    }
    format!("{} {sign}", diff_line_number_text(line, line_number_digits))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{available_themes, resolve_theme, with_transparent_surfaces};

    fn themes() -> Vec<AppTheme> {
        available_themes(&[])
    }

    #[test]
    fn cursor_marker_paints_transparent_context_rows() {
        for id in ["github-dark-dimmed", "github-light-default"] {
            let theme = with_transparent_surfaces(&resolve_theme(Some(id), None, &[]));
            let context = stack_cell_palette(RowCellKind::Context, &theme, false);
            assert_eq!(context.content_background, TRANSPARENT_BACKGROUND);
            assert_ne!(
                cursor_line_highlight_background(context.content_background, &theme),
                TRANSPARENT_BACKGROUND
            );
        }
    }

    #[test]
    fn cursor_marker_keeps_every_builtin_row_readable() {
        for base in themes() {
            for theme in [base.clone(), with_transparent_surfaces(&base)] {
                for kind in [
                    RowCellKind::Context,
                    RowCellKind::Addition,
                    RowCellKind::Deletion,
                ] {
                    let palette = stack_cell_palette(kind, &theme, false);
                    let marked =
                        cursor_line_highlight_background(palette.content_background, &theme);
                    assert!(contrast_ratio(&theme.text, &marked) > 3.0);
                }
            }
        }
    }

    #[test]
    fn cursor_marker_moves_added_and_context_rows_visibly() {
        let theme = resolve_theme(Some("github-dark-dimmed"), None, &[]);
        for kind in [RowCellKind::Context, RowCellKind::Addition] {
            let from = stack_cell_palette(kind, &theme, false).content_background;
            let to = cursor_line_highlight_background(from, &theme);
            assert!(contrast_ratio(&to, from) > 1.2);
        }
    }

    #[test]
    fn tinted_marks_clear_a_visible_distance_on_every_builtin_row() {
        for theme in themes() {
            for kind in [
                RowCellKind::Context,
                RowCellKind::Addition,
                RowCellKind::Deletion,
            ] {
                let base = stack_cell_palette(kind, &theme, false).content_background;
                for tone in [
                    HighlightTone::Match,
                    HighlightTone::Info,
                    HighlightTone::Warning,
                    HighlightTone::Error,
                ] {
                    let Some(LineHighlightToneStyle::Colors { background, .. }) =
                        line_highlight_tone_style(tone, base, &theme)
                    else {
                        panic!("expected a tinted mark");
                    };
                    assert!(hex_color_distance(&background, base) >= 60);
                }
            }
        }
    }

    #[test]
    fn current_mark_is_readable_reverse_video_and_dominates_match() {
        for theme in themes() {
            for kind in [
                RowCellKind::Context,
                RowCellKind::Addition,
                RowCellKind::Deletion,
            ] {
                let base = stack_cell_palette(kind, &theme, false).content_background;
                let Some(LineHighlightToneStyle::Colors {
                    background: match_bg,
                    ..
                }) = line_highlight_tone_style(HighlightTone::Match, base, &theme)
                else {
                    panic!("expected match paint");
                };
                let Some(LineHighlightToneStyle::Colors {
                    background,
                    foreground: Some(foreground),
                }) = line_highlight_tone_style(HighlightTone::Current, base, &theme)
                else {
                    panic!("expected reverse video");
                };
                assert_eq!(background, theme.text);
                assert_eq!(foreground, theme.background);
                assert!(
                    hex_color_distance(&background, base) > hex_color_distance(&match_bg, base)
                );
                assert!(contrast_ratio(&foreground, &background) > 3.0);
            }
        }
    }

    #[test]
    fn tinted_marks_are_visible_and_readable_on_transparent_surfaces() {
        for base in themes() {
            let theme = with_transparent_surfaces(&base);
            let context = stack_cell_palette(RowCellKind::Context, &theme, false);
            assert_eq!(context.content_background, TRANSPARENT_BACKGROUND);
            let assumed = match theme.appearance {
                ThemeAppearance::Dark => "#000000",
                ThemeAppearance::Light => "#ffffff",
            };
            for tone in [
                HighlightTone::Match,
                HighlightTone::Info,
                HighlightTone::Warning,
                HighlightTone::Error,
            ] {
                let Some(LineHighlightToneStyle::Colors { background, .. }) =
                    line_highlight_tone_style(tone, context.content_background, &theme)
                else {
                    panic!("expected a tinted mark");
                };
                assert_ne!(background, TRANSPARENT_BACKGROUND);
                assert!(hex_color_distance(&background, assumed) >= 60);
                assert!(contrast_ratio(&theme.text, &background) > 3.0);
            }
        }
    }

    #[test]
    fn current_mark_stays_reverse_video_on_transparent_surfaces() {
        for base in themes() {
            let theme = with_transparent_surfaces(&base);
            let Some(LineHighlightToneStyle::Colors {
                background,
                foreground: Some(foreground),
            }) = line_highlight_tone_style(HighlightTone::Current, TRANSPARENT_BACKGROUND, &theme)
            else {
                panic!("expected reverse video");
            };
            assert_eq!(background, theme.text);
            assert_ne!(foreground, TRANSPARENT_BACKGROUND);
            assert!(contrast_ratio(&foreground, &background) > 3.0);
        }
    }

    #[test]
    fn all_tinted_context_marks_keep_builtin_theme_text_readable() {
        for theme in themes() {
            let base = stack_cell_palette(RowCellKind::Context, &theme, false).content_background;
            for tone in [
                HighlightTone::Match,
                HighlightTone::Info,
                HighlightTone::Warning,
                HighlightTone::Error,
            ] {
                let Some(LineHighlightToneStyle::Colors { background, .. }) =
                    line_highlight_tone_style(tone, base, &theme)
                else {
                    panic!("expected a tinted mark");
                };
                assert!(contrast_ratio(&theme.text, &background) > 3.0);
            }
        }
    }

    #[test]
    fn dim_tone_moves_syntax_toward_each_row_background_but_stays_readable() {
        for theme in themes() {
            for kind in [
                RowCellKind::Context,
                RowCellKind::Addition,
                RowCellKind::Deletion,
            ] {
                let base = stack_cell_palette(kind, &theme, false).content_background;
                assert_eq!(
                    line_highlight_tone_style(HighlightTone::Dim, base, &theme),
                    Some(LineHighlightToneStyle::Dim)
                );
                let syntax = "#e06c75";
                let dimmed = dim_span_foreground(syntax.into(), base, &theme, DEFAULT_DIM_RATIO);
                let effective = effective_highlight_background(base, &theme);
                assert!(contrast_ratio(&dimmed, &effective) >= MIN_DIM_TEXT_CONTRAST);
                assert!(
                    hex_color_distance(&dimmed, &effective)
                        < hex_color_distance(syntax, &effective)
                );
            }
        }
    }

    #[test]
    fn dim_tone_uses_the_assumed_terminal_surface_for_transparency() {
        for base in themes() {
            let theme = with_transparent_surfaces(&base);
            let background =
                stack_cell_palette(RowCellKind::Context, &theme, false).content_background;
            let assumed = match theme.appearance {
                ThemeAppearance::Dark => "#000000",
                ThemeAppearance::Light => "#ffffff",
            };
            let syntax = "#e06c75";
            let dimmed = dim_span_foreground(Some(syntax), background, &theme, DEFAULT_DIM_RATIO);
            assert!(contrast_ratio(&dimmed, assumed) >= MIN_DIM_TEXT_CONTRAST);
            assert!(hex_color_distance(&dimmed, assumed) < hex_color_distance(syntax, assumed));
        }
    }

    #[test]
    fn rails_palettes_and_gutters_preserve_the_complete_row_contract() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        assert_eq!(diff_rail_marker(), "▌");
        assert_eq!(neutral_rail_color(&theme), theme.line_number_fg);
        assert_eq!(
            stack_rail_color(RowCellKind::Addition, &theme, true),
            theme.added_sign_color
        );
        assert_eq!(
            split_left_rail_color(RowCellKind::Deletion, &theme, true),
            theme.removed_sign_color
        );
        assert_eq!(
            split_right_rail_color(RowCellKind::Addition, &theme, true),
            theme.added_sign_color
        );
        assert_ne!(
            stack_rail_color(RowCellKind::Addition, &theme, false),
            theme.added_sign_color
        );
        let empty = split_cell_palette(RowCellKind::Empty, &theme, false);
        assert_eq!(empty.gutter_background, theme.line_number_bg);
        assert_eq!(empty.content_background, theme.panel_alt);
        let moved = stack_cell_palette(RowCellKind::Addition, &theme, true);
        assert_eq!(moved.content_background, theme.moved_added_bg);
        assert_eq!(
            selection_highlight_background("#000000", &theme),
            blend_hex(&theme.selected_hunk, "#000000", 0.75)
        );
        assert_eq!(diff_line_number_text(None, 3), "   ");
        assert_eq!(diff_line_number_text(Some(7), 3), "  7");
        assert_eq!(
            stack_gutter_text('+', Some(2), Some(3), 3, true),
            "  2   3 +"
        );
        assert_eq!(stack_gutter_text('+', Some(2), Some(3), 3, false), "+ ");
        assert_eq!(split_gutter_text('-', Some(2), 3, true), "  2 -");
        assert_eq!(split_gutter_text('-', Some(2), 3, false), "- ");
    }

    #[test]
    fn value_keyed_caches_separate_mutated_theme_inputs_and_stay_bounded() {
        let mut first = resolve_theme(Some("github-dark-default"), None, &[]);
        let mut syntax_colors = (*first.syntax_colors).clone();
        syntax_colors.default = "not-a-color".into();
        first.syntax_colors = std::sync::Arc::new(syntax_colors);
        first.text = "#ffffff".into();
        let mut second = first.clone();
        second.text = "#ff0000".into();
        assert_ne!(
            dim_span_foreground(None, "#000000", &first, DEFAULT_DIM_RATIO),
            dim_span_foreground(None, "#000000", &second, DEFAULT_DIM_RATIO)
        );

        second.selected_hunk = "#00ff00".into();
        assert_ne!(
            selection_highlight_background("#000000", &first),
            selection_highlight_background("#000000", &second)
        );

        let mut bounded = (0..MAX_DERIVED_STYLE_CACHE_ENTRIES)
            .map(|index| (index.to_string(), index))
            .collect::<BTreeMap<_, _>>();
        insert_bounded(&mut bounded, "next".into(), usize::MAX);
        assert_eq!(bounded.len(), 1);
        assert_eq!(bounded.get("next"), Some(&usize::MAX));
    }

    #[test]
    fn frozen_row_style_oracle_records_both_pinned_trees_and_every_source_test() {
        let oracle: serde_json::Value =
            serde_json::from_str(include_str!("../../../port/hunk/oracles/row-style.json"))
                .unwrap();
        assert_eq!(
            oracle["baseline"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(oracle["stable"], "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd");
        assert_eq!(oracle["baselineOracle"]["passed"], 10);
        assert_eq!(oracle["stableOracle"]["passed"], 8);
        assert_eq!(oracle["testMapping"].as_array().unwrap().len(), 10);
        assert_eq!(oracle["sourceCoverage"][0]["bytes"][0], 0);
        assert_eq!(oracle["sourceCoverage"][6]["bytes"][1], 15_348);
        assert_eq!(oracle["testCoverage"][0]["bytes"][0], 0);
        assert_eq!(oracle["testCoverage"][3]["bytes"][1], 7_384);
    }
}
