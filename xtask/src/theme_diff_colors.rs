//! Rust replacement for Hunk's generated theme diff-color harvester.
//!
//! Theme JSON is treated as build input only. The shipped binary uses the
//! generated catalog in workdeck-core; this command proves that catalog is
//! still reproducible from the vendored TextMate/Shiki payloads without a
//! JavaScript runtime.

use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{collections::BTreeMap, fs, path::Path};

const ADDED_TOKENS: &[&str] = &[
    "editorGutter.addedBackground",
    "diffEditor.insertedTextBackground",
    "terminal.ansiGreen",
    "gitDecoration.addedResourceForeground",
];
const REMOVED_TOKENS: &[&str] = &[
    "editorGutter.deletedBackground",
    "diffEditor.removedTextBackground",
    "terminal.ansiRed",
    "gitDecoration.deletedResourceForeground",
];
const MODIFIED_TOKENS: &[&str] = &[
    "editorGutter.modifiedBackground",
    "gitDecoration.modifiedResourceForeground",
    "terminal.ansiBlue",
];
const MIN_ACCENT_SATURATION: f64 = 0.12;
const MIN_BACKGROUND_TOKEN_CONTRAST_DARK: f64 = 3.0;
const MIN_BACKGROUND_TOKEN_CONTRAST_LIGHT: f64 = 2.5;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DiffColors {
    pub added: Option<String>,
    pub removed: Option<String>,
    pub modified: Option<String>,
}

/// Normalize a VS Code token color, dropping alpha without compositing it.
pub(crate) fn normalize_token_color(raw: &str) -> Option<String> {
    let value = raw.trim().to_ascii_lowercase();
    let digits = value.strip_prefix('#')?;
    if !matches!(digits.len(), 3 | 4 | 6 | 8)
        || !digits
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    let digits = &digits[..6.min(digits.len())];
    let expanded = if digits.len() == 3 {
        digits
            .chars()
            .flat_map(|digit| [digit, digit])
            .collect::<String>()
    } else if digits.len() == 4 {
        digits
            .chars()
            .take(3)
            .flat_map(|digit| [digit, digit])
            .collect::<String>()
    } else {
        digits.to_owned()
    };
    Some(format!("#{expanded}"))
}

/// Harvest one theme's semantic added/removed/modified accents.
pub(crate) fn harvest_theme_diff_colors(
    colors: &BTreeMap<String, String>,
    theme_background: &str,
) -> Option<DiffColors> {
    let entry = DiffColors {
        added: first_usable(colors, ADDED_TOKENS, "added", theme_background),
        removed: first_usable(colors, REMOVED_TOKENS, "removed", theme_background),
        modified: first_usable(colors, MODIFIED_TOKENS, "modified", theme_background),
    };
    (entry.added.is_some() || entry.removed.is_some()).then_some(entry)
}

fn first_usable(
    colors: &BTreeMap<String, String>,
    tokens: &[&str],
    slot: &str,
    theme_background: &str,
) -> Option<String> {
    tokens.iter().find_map(|token| {
        let color = normalize_token_color(colors.get(*token)?)?;
        is_usable_accent(slot, token, &color, theme_background).then_some(color)
    })
}

fn is_usable_accent(slot: &str, token: &str, color: &str, theme_background: &str) -> bool {
    let (hue, saturation) = hex_to_hsl(color);
    if saturation < MIN_ACCENT_SATURATION {
        return false;
    }
    if slot == "added" && !(50.0..=200.0).contains(&hue) {
        return false;
    }
    if slot == "removed" && !(hue >= 300.0 || hue <= 30.0) {
        return false;
    }
    if token.starts_with("editorGutter.") || token.starts_with("diffEditor.") {
        let minimum = if relative_luminance(theme_background) > 0.45 {
            MIN_BACKGROUND_TOKEN_CONTRAST_LIGHT
        } else {
            MIN_BACKGROUND_TOKEN_CONTRAST_DARK
        };
        if contrast_ratio(color, theme_background) < minimum {
            return false;
        }
    }
    true
}

fn hex_to_hsl(hex: &str) -> (f64, f64) {
    let red = u8::from_str_radix(&hex[1..3], 16).unwrap_or_default() as f64 / 255.0;
    let green = u8::from_str_radix(&hex[3..5], 16).unwrap_or_default() as f64 / 255.0;
    let blue = u8::from_str_radix(&hex[5..7], 16).unwrap_or_default() as f64 / 255.0;
    let max = red.max(green).max(blue);
    let min = red.min(green).min(blue);
    let delta = max - min;
    let lightness = (max + min) / 2.0;
    if delta == 0.0 {
        return (0.0, 0.0);
    }
    let saturation = delta / (1.0 - (2.0 * lightness - 1.0).abs());
    let hue = if max == red {
        60.0 * (((green - blue) / delta + 6.0) % 6.0)
    } else if max == green {
        60.0 * ((blue - red) / delta + 2.0)
    } else {
        60.0 * ((red - green) / delta + 4.0)
    };
    (hue, saturation)
}

fn relative_luminance(hex: &str) -> f64 {
    let channels = [&hex[1..3], &hex[3..5], &hex[5..7]];
    let linear = channels.map(|channel| {
        let value = u8::from_str_radix(channel, 16).unwrap_or_default() as f64 / 255.0;
        if value <= 0.03928 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    });
    0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2]
}

fn contrast_ratio(first: &str, second: &str) -> f64 {
    let first = relative_luminance(first);
    let second = relative_luminance(second);
    let (lighter, darker) = if first >= second {
        (first, second)
    } else {
        (second, first)
    };
    (lighter + 0.05) / (darker + 0.05)
}

fn expected_colors(theme_id: &str) -> Option<DiffColors> {
    workdeck_core::get_bundled_shiki_theme_diff_colors(Some(theme_id)).map(|colors| DiffColors {
        added: colors.added.map(str::to_owned),
        removed: colors.removed.map(str::to_owned),
        modified: colors.modified.map(str::to_owned),
    })
}

/// Recompute every generated catalog entry from the vendored theme JSON.
pub(crate) fn check(repo: &Path) -> Result<()> {
    let root = repo.join("crates/workdeck-diff/assets/themes");
    let mut checked = 0;
    for theme_id in workdeck_core::BUNDLED_SHIKI_THEME_IDS {
        let path = root.join(format!("{theme_id}.json"));
        let bytes =
            fs::read(&path).with_context(|| format!("read theme asset {}", path.display()))?;
        let value: Value = serde_json::from_slice(&bytes)
            .with_context(|| format!("parse theme asset {}", path.display()))?;
        let colors = value
            .get("colors")
            .and_then(Value::as_object)
            .with_context(|| format!("theme asset {theme_id} has no colors object"))?
            .iter()
            .filter_map(|(key, value)| value.as_str().map(|value| (key.clone(), value.to_owned())))
            .collect::<BTreeMap<_, _>>();
        let background = workdeck_core::get_bundled_shiki_theme_background(Some(theme_id))
            .with_context(|| format!("theme {theme_id} has no background"))?;
        let actual = harvest_theme_diff_colors(&colors, background);
        ensure!(
            actual == expected_colors(theme_id),
            "generated diff colors drifted for {theme_id}: expected {:?}, found {:?}",
            expected_colors(theme_id),
            actual
        );
        checked += 1;
    }
    println!("verified {checked} generated theme diff-color entries");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colors(values: &[(&str, &str)]) -> BTreeMap<String, String> {
        values
            .iter()
            .map(|(key, value)| ((*key).into(), (*value).into()))
            .collect()
    }

    #[test]
    fn normalizes_alpha_and_shorthand_without_compositing() {
        assert_eq!(normalize_token_color("#f0717890"), Some("#f07178".into()));
        assert_eq!(normalize_token_color("#C3E88D60"), Some("#c3e88d".into()));
        assert_eq!(normalize_token_color("#FFF"), Some("#ffffff".into()));
        assert_eq!(normalize_token_color("#abcd"), Some("#aabbcc".into()));
    }

    #[test]
    fn rejects_non_hex_or_wrong_length_values() {
        for value in ["red", "#12345", "rgba(0, 0, 0, 0.5)", "#123456789"] {
            assert_eq!(normalize_token_color(value), None, "{value}");
        }
    }

    #[test]
    fn prefers_gutter_accents_and_strips_blend_alpha() {
        let entry = harvest_theme_diff_colors(
            &colors(&[
                ("editorGutter.addedBackground", "#C3E88D60"),
                ("editorGutter.deletedBackground", "#f0717860"),
                ("editorGutter.modifiedBackground", "#82AAFF60"),
                ("gitDecoration.deletedResourceForeground", "#f0717890"),
            ]),
            "#263238",
        );
        assert_eq!(
            entry,
            Some(DiffColors {
                added: Some("#c3e88d".into()),
                removed: Some("#f07178".into()),
                modified: Some("#82aaff".into()),
            })
        );
    }

    #[test]
    fn rejects_preblended_surfaces_and_falls_through_to_diff_editor() {
        let entry = harvest_theme_diff_colors(
            &colors(&[
                ("editorGutter.addedBackground", "#164846"),
                ("editorGutter.deletedBackground", "#823c41"),
                ("diffEditor.insertedTextBackground", "#41a6b520"),
                ("diffEditor.removedTextBackground", "#db4b4b22"),
            ]),
            "#1a1b26",
        );
        assert_eq!(
            entry,
            Some(DiffColors {
                added: Some("#41a6b5".into()),
                removed: Some("#db4b4b".into()),
                modified: None,
            })
        );
    }

    #[test]
    fn rejects_wrong_hue_desaturated_and_empty_candidates() {
        assert_eq!(
            harvest_theme_diff_colors(
                &colors(&[
                    ("gitDecoration.addedResourceForeground", "#ebdbb2"),
                    ("gitDecoration.deletedResourceForeground", "#cc241d"),
                ]),
                "#282828",
            ),
            Some(DiffColors {
                added: None,
                removed: Some("#cc241d".into()),
                modified: None,
            })
        );
        assert_eq!(
            harvest_theme_diff_colors(
                &colors(&[
                    ("terminal.ansiGreen", "#77cc00"),
                    ("terminal.ansiRed", "#D32F2F"),
                    ("terminal.ansiBlue", "#e0e0e0"),
                ]),
                "#ffffff",
            ),
            Some(DiffColors {
                added: Some("#77cc00".into()),
                removed: Some("#d32f2f".into()),
                modified: None,
            })
        );
        assert_eq!(
            harvest_theme_diff_colors(
                &colors(&[
                    ("gitDecoration.addedResourceForeground", "#ECB22E"),
                    ("gitDecoration.deletedResourceForeground", "#FFF"),
                    ("gitDecoration.modifiedResourceForeground", "#ECB22E"),
                ]),
                "#222222",
            ),
            None
        );
    }

    #[test]
    fn foreground_accents_skip_background_contrast_gate() {
        assert_eq!(
            harvest_theme_diff_colors(
                &colors(&[
                    ("terminal.ansiGreen", "#859900"),
                    ("terminal.ansiRed", "#dc322f"),
                    ("terminal.ansiBlue", "#268bd2"),
                ]),
                "#fdf6e3",
            ),
            Some(DiffColors {
                added: Some("#859900".into()),
                removed: Some("#dc322f".into()),
                modified: Some("#268bd2".into()),
            })
        );
    }

    #[test]
    fn vendored_theme_catalog_is_reproducible_from_rust_inputs() {
        let repo = crate::repo_root().unwrap();
        check(&repo).unwrap();
    }
}
