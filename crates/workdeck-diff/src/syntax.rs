use std::collections::HashMap;
use std::path::Path;
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, ThemeSet};
use syntect::parsing::SyntaxSet;
use workdeck_core::DiffFile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyntaxColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxToken {
    pub text: String,
    pub foreground: SyntaxColor,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

pub type HighlightedLine = Vec<SyntaxToken>;
pub type HighlightedHunk = Vec<HighlightedLine>;
pub type HighlightedFile = Vec<HighlightedHunk>;
type HighlightKey = (String, String, String);

/// TextMate highlighter backed by syntect's Oniguruma-compatible engine.
///
/// Cache identity includes semantic content, language, and theme. It never uses the mount-local
/// runtime id, preventing fuzzy reloads from reusing another file's spans.
#[derive(Debug)]
pub struct HighlightCache {
    syntaxes: SyntaxSet,
    themes: ThemeSet,
    entries: HashMap<HighlightKey, HighlightedFile>,
}

impl Default for HighlightCache {
    fn default() -> Self {
        Self {
            syntaxes: SyntaxSet::load_defaults_newlines(),
            themes: ThemeSet::load_defaults(),
            entries: HashMap::new(),
        }
    }
}

impl HighlightCache {
    pub fn highlight(&mut self, file: &DiffFile, theme: &str) -> HighlightedFile {
        let language = file.language.clone().unwrap_or_default();
        let key = (
            file.content_identity.clone(),
            language.clone(),
            theme.to_owned(),
        );
        if let Some(cached) = self.entries.get(&key) {
            return cached.clone();
        }
        let syntax = self
            .syntaxes
            .find_syntax_by_token(&language)
            .or_else(|| {
                Path::new(&file.path)
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .and_then(|extension| self.syntaxes.find_syntax_by_extension(extension))
            })
            .unwrap_or_else(|| self.syntaxes.find_syntax_plain_text());
        let Some(theme) = self
            .themes
            .themes
            .get(theme)
            .or_else(|| self.themes.themes.get("base16-ocean.dark"))
            .or_else(|| self.themes.themes.values().next())
        else {
            return plain_file(file);
        };
        let mut highlighter = HighlightLines::new(syntax, theme);
        let highlighted = file
            .hunks
            .iter()
            .map(|hunk| {
                hunk.lines
                    .iter()
                    .map(|line| {
                        let source = format!("{}\n", line.content);
                        highlighter
                            .highlight_line(&source, &self.syntaxes)
                            .map(|ranges| {
                                ranges
                                    .into_iter()
                                    .map(|(style, text)| SyntaxToken {
                                        text: text.trim_end_matches('\n').to_owned(),
                                        foreground: SyntaxColor {
                                            red: style.foreground.r,
                                            green: style.foreground.g,
                                            blue: style.foreground.b,
                                        },
                                        bold: style.font_style.contains(FontStyle::BOLD),
                                        italic: style.font_style.contains(FontStyle::ITALIC),
                                        underline: style.font_style.contains(FontStyle::UNDERLINE),
                                    })
                                    .filter(|token| !token.text.is_empty())
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_else(|_| vec![plain_token(&line.content)])
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        self.entries.insert(key, highlighted.clone());
        highlighted
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

fn plain_file(file: &DiffFile) -> Vec<Vec<Vec<SyntaxToken>>> {
    file.hunks
        .iter()
        .map(|hunk| {
            hunk.lines
                .iter()
                .map(|line| vec![plain_token(&line.content)])
                .collect()
        })
        .collect()
}

fn plain_token(text: &str) -> SyntaxToken {
    SyntaxToken {
        text: text.to_owned(),
        foreground: SyntaxColor {
            red: 201,
            green: 209,
            blue: 217,
        },
        bold: false,
        italic: false,
        underline: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_patch;
    use workdeck_core::ChangesetSource;

    #[test]
    fn cache_identity_uses_content_not_runtime_mount_ids() {
        let mut first = parse_patch(
            "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-let a = 1;\n+let a = true;\n",
            "first",
            "first",
            ChangesetSource::Patch { label: "first".into() },
        )
        .unwrap();
        let mut second = parse_patch(
            "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-let a = 1;\n+let a = \"text\";\n",
            "second",
            "second",
            ChangesetSource::Patch { label: "second".into() },
        )
        .unwrap();
        first.files[0].runtime_id = "shared-runtime-id".into();
        second.files[0].runtime_id = "shared-runtime-id".into();
        assert_eq!(first.files[0].runtime_id, second.files[0].runtime_id);
        assert_ne!(
            first.files[0].content_identity,
            second.files[0].content_identity
        );
        let mut cache = HighlightCache::default();
        cache.highlight(&first.files[0], "base16-ocean.dark");
        cache.highlight(&second.files[0], "base16-ocean.dark");
        assert_eq!(cache.len(), 2);
    }
}
