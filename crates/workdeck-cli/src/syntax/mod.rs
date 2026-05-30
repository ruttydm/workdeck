use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use std::path::Path;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Style as SynStyle, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

const LIGHT_DIFF_TEXT_COLOR: Color = Color::Rgb(32, 38, 35);
const LIGHT_DIFF_ADD_BACKGROUND: Color = Color::Rgb(220, 246, 226);
const LIGHT_DIFF_REMOVE_BACKGROUND: Color = Color::Rgb(255, 224, 224);
const LIGHT_DIFF_HUNK_COLOR: Color = Color::Rgb(0, 95, 135);
const LIGHT_DIFF_SECTION_COLOR: Color = Color::Rgb(128, 96, 0);
const DARK_DIFF_ADD_TEXT_COLOR: Color = Color::Rgb(176, 239, 197);
const DARK_DIFF_REMOVE_TEXT_COLOR: Color = Color::Rgb(255, 196, 196);
const DARK_DIFF_ADD_BACKGROUND: Color = Color::Rgb(8, 46, 28);
const DARK_DIFF_REMOVE_BACKGROUND: Color = Color::Rgb(58, 18, 24);
const DARK_DIFF_HUNK_COLOR: Color = Color::Rgb(132, 204, 255);
const DARK_DIFF_SECTION_COLOR: Color = Color::Rgb(236, 201, 117);

pub struct SyntaxHighlighter {
    syntaxes: SyntaxSet,
    themes: ThemeSet,
    theme_mode: ThemeMode,
}

impl Default for SyntaxHighlighter {
    fn default() -> Self {
        Self::new("auto")
    }
}

impl SyntaxHighlighter {
    pub fn new(theme: &str) -> Self {
        Self {
            syntaxes: SyntaxSet::load_defaults_newlines(),
            themes: ThemeSet::load_defaults(),
            theme_mode: ThemeMode::from_config(theme),
        }
    }

    pub fn highlight(&self, path: &Path, content: &str, max_lines: usize) -> Text<'static> {
        if path.extension().and_then(|ext| ext.to_str()) == Some("diff")
            || content.starts_with("diff --git")
            || content.starts_with("# staged")
            || content.starts_with("# unstaged")
        {
            return highlight_diff(content, max_lines, self.theme_mode);
        }

        let syntax = syntax_for_path(&self.syntaxes, path);

        let theme = self.theme_mode.theme_names().iter().find_map(|name| {
            self.themes
                .themes
                .get(*name)
                .or_else(|| self.themes.themes.values().next())
        });

        let Some(theme) = theme else {
            return plain_text(content, max_lines);
        };

        let mut highlighter = HighlightLines::new(syntax, theme);
        let mut lines = Vec::new();
        for line in LinesWithEndings::from(content).take(max_lines) {
            match highlighter.highlight_line(line, &self.syntaxes) {
                Ok(ranges) => {
                    lines.push(Line::from(
                        ranges
                            .into_iter()
                            .map(|(style, text)| {
                                Span::styled(text.to_string(), syn_style(style, self.theme_mode))
                            })
                            .collect::<Vec<_>>(),
                    ));
                }
                Err(_) => lines.push(Line::from(line.to_string())),
            }
        }
        Text::from(lines)
    }
}

fn syntax_for_path<'a>(
    syntaxes: &'a SyntaxSet,
    path: &Path,
) -> &'a syntect::parsing::SyntaxReference {
    let extension = path.extension().and_then(|ext| ext.to_str());
    if extension == Some("vue") {
        return syntaxes
            .find_syntax_by_extension("vue")
            .or_else(|| syntaxes.find_syntax_by_extension("html"))
            .or_else(|| syntaxes.find_syntax_by_name("HTML"))
            .unwrap_or_else(|| syntaxes.find_syntax_plain_text());
    }

    extension
        .and_then(|ext| syntaxes.find_syntax_by_extension(ext))
        .unwrap_or_else(|| syntaxes.find_syntax_plain_text())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThemeMode {
    Light,
    Dark,
}

impl ThemeMode {
    fn from_config(theme: &str) -> Self {
        match theme.trim().to_ascii_lowercase().as_str() {
            "dark" | "base16-ocean.dark" | "solarized-dark" | "solarized (dark)" => Self::Dark,
            "light" | "base16-ocean.light" | "solarized-light" | "solarized (light)"
            | "inspiredgithub" => Self::Light,
            _ => detect_terminal_theme().unwrap_or(Self::Dark),
        }
    }

    fn theme_names(self) -> &'static [&'static str] {
        match self {
            Self::Light => &["base16-ocean.light", "Solarized (light)", "InspiredGitHub"],
            Self::Dark => &["base16-ocean.dark", "Solarized (dark)"],
        }
    }
}

fn detect_terminal_theme() -> Option<ThemeMode> {
    let colorfgbg = std::env::var("COLORFGBG").ok()?;
    let background = colorfgbg.rsplit(';').next()?.parse::<u8>().ok()?;
    Some(if background <= 6 {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    })
}

pub fn plain_text(content: &str, max_lines: usize) -> Text<'static> {
    Text::from(
        content
            .lines()
            .take(max_lines)
            .map(|line| Line::from(line.to_string()))
            .collect::<Vec<_>>(),
    )
}

fn highlight_diff(content: &str, max_lines: usize, theme_mode: ThemeMode) -> Text<'static> {
    let palette = DiffPalette::for_theme(theme_mode);
    Text::from(
        content
            .lines()
            .take(max_lines)
            .map(|line| {
                let style = if line.starts_with('+') && !line.starts_with("+++") {
                    palette.add.style()
                } else if line.starts_with('-') && !line.starts_with("---") {
                    palette.remove.style()
                } else if line.starts_with("@@") {
                    palette.hunk.style()
                } else if line.starts_with('#') {
                    palette.section.style()
                } else {
                    Style::default()
                };
                Line::from(Span::styled(line.to_string(), style)).style(style)
            })
            .collect::<Vec<_>>(),
    )
}

#[derive(Debug, Clone, Copy)]
struct DiffPalette {
    add: DiffLineStyle,
    remove: DiffLineStyle,
    hunk: DiffLineStyle,
    section: DiffLineStyle,
}

#[derive(Debug, Clone, Copy)]
struct DiffLineStyle {
    fg: Color,
    bg: Option<Color>,
    modifier: Modifier,
}

impl DiffLineStyle {
    fn background(fg: Color, bg: Color) -> Self {
        Self {
            fg,
            bg: Some(bg),
            modifier: Modifier::empty(),
        }
    }

    fn bold(fg: Color) -> Self {
        Self {
            fg,
            bg: None,
            modifier: Modifier::BOLD,
        }
    }

    fn style(self) -> Style {
        let style = Style::default().fg(self.fg);
        let style = if let Some(bg) = self.bg {
            style.bg(bg)
        } else {
            style
        };
        style.add_modifier(self.modifier)
    }
}

impl DiffPalette {
    fn for_theme(theme_mode: ThemeMode) -> Self {
        match theme_mode {
            ThemeMode::Light => Self {
                add: DiffLineStyle::background(LIGHT_DIFF_TEXT_COLOR, LIGHT_DIFF_ADD_BACKGROUND),
                remove: DiffLineStyle::background(
                    LIGHT_DIFF_TEXT_COLOR,
                    LIGHT_DIFF_REMOVE_BACKGROUND,
                ),
                hunk: DiffLineStyle::bold(LIGHT_DIFF_HUNK_COLOR),
                section: DiffLineStyle::bold(LIGHT_DIFF_SECTION_COLOR),
            },
            ThemeMode::Dark => Self {
                add: DiffLineStyle::background(DARK_DIFF_ADD_TEXT_COLOR, DARK_DIFF_ADD_BACKGROUND),
                remove: DiffLineStyle::background(
                    DARK_DIFF_REMOVE_TEXT_COLOR,
                    DARK_DIFF_REMOVE_BACKGROUND,
                ),
                hunk: DiffLineStyle::bold(DARK_DIFF_HUNK_COLOR),
                section: DiffLineStyle::bold(DARK_DIFF_SECTION_COLOR),
            },
        }
    }
}

fn syn_style(style: SynStyle, theme_mode: ThemeMode) -> Style {
    let (r, g, b) = readable_rgb(
        style.foreground.r,
        style.foreground.g,
        style.foreground.b,
        theme_mode,
    );
    Style::default().fg(Color::Rgb(r, g, b))
}

fn readable_rgb(r: u8, g: u8, b: u8, theme_mode: ThemeMode) -> (u8, u8, u8) {
    let luma = luma(r, g, b);
    match theme_mode {
        ThemeMode::Light if luma > 180 => scale_rgb(r, g, b, 0.45),
        ThemeMode::Dark if luma < 75 => lift_rgb(r, g, b, 90),
        _ => (r, g, b),
    }
}

fn luma(r: u8, g: u8, b: u8) -> u32 {
    ((299 * r as u32) + (587 * g as u32) + (114 * b as u32)) / 1000
}

fn scale_rgb(r: u8, g: u8, b: u8, factor: f32) -> (u8, u8, u8) {
    (
        (r as f32 * factor).round() as u8,
        (g as f32 * factor).round() as u8,
        (b as f32 * factor).round() as u8,
    )
}

fn lift_rgb(r: u8, g: u8, b: u8, minimum: u8) -> (u8, u8, u8) {
    (r.max(minimum), g.max(minimum), b.max(minimum))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_lines_get_styles() {
        let text = highlight_diff("+added\n-removed\n@@ hunk\n# staged", 10, ThemeMode::Light);
        let add_style = text.lines[0].spans[0].style;
        let remove_style = text.lines[1].spans[0].style;
        let hunk_style = text.lines[2].spans[0].style;
        let section_style = text.lines[3].spans[0].style;

        assert_eq!(text.lines.len(), 4);
        assert_eq!(add_style.fg, Some(LIGHT_DIFF_TEXT_COLOR));
        assert_eq!(add_style.bg, Some(LIGHT_DIFF_ADD_BACKGROUND));
        assert_eq!(remove_style.fg, Some(LIGHT_DIFF_TEXT_COLOR));
        assert_eq!(remove_style.bg, Some(LIGHT_DIFF_REMOVE_BACKGROUND));
        assert_eq!(hunk_style.fg, Some(LIGHT_DIFF_HUNK_COLOR));
        assert_eq!(section_style.fg, Some(LIGHT_DIFF_SECTION_COLOR));
        assert_eq!(hunk_style.bg, None);
        assert_eq!(section_style.bg, None);
    }

    #[test]
    fn diff_highlighting_uses_light_and_dark_palettes() {
        let light =
            SyntaxHighlighter::new("light").highlight(Path::new("change.diff"), "+a\n-b\n", 10);
        let dark =
            SyntaxHighlighter::new("dark").highlight(Path::new("change.diff"), "+a\n-b\n", 10);

        assert_eq!(
            light.lines[0].spans[0].style.fg,
            Some(LIGHT_DIFF_TEXT_COLOR)
        );
        assert_eq!(
            light.lines[0].spans[0].style.bg,
            Some(LIGHT_DIFF_ADD_BACKGROUND)
        );
        assert_eq!(
            light.lines[1].spans[0].style.bg,
            Some(LIGHT_DIFF_REMOVE_BACKGROUND)
        );
        assert_eq!(
            dark.lines[0].spans[0].style.fg,
            Some(DARK_DIFF_ADD_TEXT_COLOR)
        );
        assert_eq!(
            dark.lines[1].spans[0].style.fg,
            Some(DARK_DIFF_REMOVE_TEXT_COLOR)
        );
        assert_eq!(
            dark.lines[0].spans[0].style.bg,
            Some(DARK_DIFF_ADD_BACKGROUND)
        );
        assert_ne!(
            light.lines[0].spans[0].style.bg,
            dark.lines[0].spans[0].style.bg
        );
        assert_ne!(
            light.lines[1].spans[0].style.bg,
            dark.lines[1].spans[0].style.bg
        );
    }

    #[test]
    fn diff_palettes_are_readable_on_light_and_dark_backgrounds() {
        let light = DiffPalette::for_theme(ThemeMode::Light);
        let dark = DiffPalette::for_theme(ThemeMode::Dark);

        for line_style in [light.add, light.remove] {
            assert!(color_luma(line_style.fg) < 90);
            assert!(color_luma(line_style.bg.unwrap()) > 210);
        }
        for line_style in [dark.add, dark.remove] {
            assert!(color_luma(line_style.fg) > 180);
            assert!(color_luma(line_style.bg.unwrap()) < 90);
            assert!(color_luma(line_style.fg) - color_luma(line_style.bg.unwrap()) > 130);
        }
    }

    #[test]
    fn dark_diff_palette_distinguishes_added_and_removed_lines() {
        let dark =
            SyntaxHighlighter::new("dark").highlight(Path::new("change.diff"), "+a\n-b\n", 10);

        assert_ne!(
            dark.lines[0].spans[0].style.fg,
            dark.lines[1].spans[0].style.fg
        );
        assert_ne!(
            dark.lines[0].spans[0].style.bg,
            dark.lines[1].spans[0].style.bg
        );
    }

    #[test]
    fn syntax_highlighting_does_not_force_theme_backgrounds() {
        let highlighter = SyntaxHighlighter::default();
        let text = highlighter.highlight(Path::new("main.rs"), "fn main() {}\n", 10);

        assert_eq!(text.lines.len(), 1);
        assert!(
            text.lines
                .iter()
                .flat_map(|line| &line.spans)
                .all(|span| span.style.bg.is_none())
        );
    }

    #[test]
    fn vue_files_fall_back_to_html_syntax() {
        let syntaxes = SyntaxSet::load_defaults_newlines();
        let syntax = syntax_for_path(&syntaxes, Path::new("Component.vue"));

        assert_ne!(syntax.name, syntaxes.find_syntax_plain_text().name);
    }

    #[test]
    fn light_theme_clamps_near_white_foregrounds() {
        let (r, g, b) = readable_rgb(245, 245, 245, ThemeMode::Light);

        assert!(luma(r, g, b) < 180);
    }

    #[test]
    fn dark_theme_lifts_near_black_foregrounds() {
        let (r, g, b) = readable_rgb(10, 20, 30, ThemeMode::Dark);

        assert!(luma(r, g, b) >= 90);
    }

    fn color_luma(color: Color) -> u32 {
        match color {
            Color::Rgb(r, g, b) => luma(r, g, b),
            _ => 0,
        }
    }
}
