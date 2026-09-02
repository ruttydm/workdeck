//! Native syntax highlighting and Hunk/Pierre-compatible theme registration.
//!
//! Theme normalization and content addressing translate the behavior pinned through Hunk's
//! `@pierre/diffs` 1.3.5, `@pierre/theming` 1.0.1, Shiki 3.23.0, and Pierre Theme 2.0.0. The
//! source packages and complete licenses are recorded in `THIRD_PARTY_NOTICES` and
//! `third_party/themes`.

use crate::compact_highlight::{decode_compact_syntax_lines, encode_compact_syntax_lines};
use crate::{
    CompactHighlightedDiff, HIGHLIGHT_WORKER_PROTOCOL_VERSION, HighlightLineArrays,
    HighlightWorkerClient, HighlightWorkerInput, HighlightWorkerResponse, HighlightedDiffCache,
    HighlightedDiffCode, alias_context_highlight_lines, bundled_theme_assets::BUNDLED_THEME_ASSETS,
    compact_highlighted_diff_byte_length, create_source_backed_highlight_plan,
    remap_source_backed_highlight,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::str::FromStr;
use syntect::easy::HighlightLines;
use syntect::highlighting::{
    Color as SyntectColor, FontStyle, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSet,
};
use syntect::parsing::SyntaxSet;
use workdeck_core::{
    DiffFile, DiffHunk, DiffLineKind, FileChangeKind, FileFlags, FileSourceSnapshots, FileStats,
    SemanticReviewFile, bundled_shiki_theme_is_light, project_review_file, resolve_legacy_theme_id,
};

const HIGHLIGHT_WORKER_CACHE_REVISION: u32 = 2;
pub const PIERRE_LIGHT_THEME: &str = "pierre-light";
pub const PIERRE_DARK_THEME: &str = "pierre-dark";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TextMateTheme {
    name: String,
    #[serde(default)]
    r#type: Option<String>,
    #[serde(default)]
    bg: Option<String>,
    #[serde(default)]
    fg: Option<String>,
    #[serde(default)]
    colors: HashMap<String, serde_json::Value>,
    #[serde(default)]
    token_colors: Option<Vec<TextMateThemeRule>>,
    #[serde(default)]
    settings: Option<Vec<TextMateThemeRule>>,
}

#[derive(Debug, Clone, Deserialize)]
struct TextMateThemeRule {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    scope: Option<TextMateScopes>,
    #[serde(default)]
    settings: Option<TextMateRuleSettings>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum TextMateScopes {
    One(String),
    Many(Vec<String>),
}

impl TextMateScopes {
    fn selectors(&self) -> String {
        match self {
            Self::One(scope) => scope.clone(),
            Self::Many(scopes) => scopes.join(", "),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TextMateRuleSettings {
    #[serde(default)]
    foreground: Option<String>,
    #[serde(default)]
    background: Option<String>,
    #[serde(default)]
    font_style: Option<String>,
}

impl TextMateTheme {
    /// Apply the data-shape normalization performed by Shiki 3.23.0's `normalizeTheme`.
    fn normalize_shiki(mut self) -> Self {
        if self.settings.is_none() {
            self.settings = self.token_colors.take();
        }
        self.r#type.get_or_insert_with(|| "dark".into());
        self.settings.get_or_insert_default();

        let global = self.settings.as_ref().and_then(|settings| {
            settings
                .iter()
                .find(|setting| setting.name.is_none() && setting.scope.is_none())
                .and_then(|setting| setting.settings.as_ref())
        });
        if self.fg.is_none() {
            self.fg = global
                .and_then(|settings| settings.foreground.clone())
                .or_else(|| self.color("editor.foreground"))
                .or_else(|| {
                    Some(if self.r#type.as_deref() == Some("light") {
                        "#333333".into()
                    } else {
                        "#bbbbbb".into()
                    })
                });
        }
        if self.bg.is_none() {
            self.bg = global
                .and_then(|settings| settings.background.clone())
                .or_else(|| self.color("editor.background"))
                .or_else(|| {
                    Some(if self.r#type.as_deref() == Some("light") {
                        "#fffffe".into()
                    } else {
                        "#1e1e1e".into()
                    })
                });
        }

        let has_leading_global = self
            .settings
            .as_ref()
            .and_then(|settings| settings.first())
            .is_some_and(|setting| setting.settings.is_some() && setting.scope.is_none());
        if !has_leading_global {
            self.settings
                .as_mut()
                .expect("settings initialized above")
                .insert(
                    0,
                    TextMateThemeRule {
                        name: None,
                        scope: None,
                        settings: Some(TextMateRuleSettings {
                            foreground: self.fg.clone(),
                            background: self.bg.clone(),
                            font_style: None,
                        }),
                    },
                );
        }
        self
    }

    fn color(&self, key: &str) -> Option<String> {
        self.colors
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    }

    fn append_scope_overrides(&mut self, scope_overrides: &[(String, String)]) {
        let settings = self.settings.get_or_insert_default();
        settings.extend(
            scope_overrides
                .iter()
                .map(|(scope, foreground)| TextMateThemeRule {
                    name: None,
                    scope: Some(TextMateScopes::One(scope.clone())),
                    settings: Some(TextMateRuleSettings {
                        foreground: Some(foreground.clone()),
                        ..TextMateRuleSettings::default()
                    }),
                }),
        );
    }

    fn compile_syntect(&self) -> Theme {
        let mut theme = Theme {
            name: Some(self.name.clone()),
            ..Theme::default()
        };
        theme.settings.foreground = self.fg.as_deref().and_then(syntect_color_from_hex);
        theme.settings.background = self.bg.as_deref().and_then(syntect_color_from_hex);
        theme.scopes = self
            .settings
            .as_deref()
            .unwrap_or_default()
            .iter()
            // Shiki/TextMate resolves equal-specificity rules by latest declaration. Syntect
            // retains the first equal score, so reversing is the exact precedence adapter.
            .rev()
            .filter_map(compile_textmate_rule)
            .collect();
        theme
    }
}

fn compile_textmate_rule(rule: &TextMateThemeRule) -> Option<ThemeItem> {
    let scope = rule.scope.as_ref()?.selectors();
    // Syntect has no direct-child combinator; collapsing it to an adjacent scope-stack match is
    // the closest lossless representation its selector model can hold. The exact source selector
    // remains retained in `TextMateTheme` for a future renderer adapter with child semantics.
    let syntect_scope = scope.replace(" > ", " ");
    let settings = rule.settings.as_ref()?;
    Some(ThemeItem {
        scope: ScopeSelectors::from_str(&syntect_scope).ok()?,
        style: StyleModifier {
            foreground: settings
                .foreground
                .as_deref()
                .and_then(syntect_color_from_hex),
            background: settings
                .background
                .as_deref()
                .and_then(syntect_color_from_hex),
            font_style: settings.font_style.as_deref().map(syntect_font_style),
        },
    })
}

fn syntect_font_style(value: &str) -> FontStyle {
    let mut style = FontStyle::empty();
    for item in value.split_whitespace() {
        match item {
            "bold" => style |= FontStyle::BOLD,
            "italic" => style |= FontStyle::ITALIC,
            "underline" => style |= FontStyle::UNDERLINE,
            // `normal` and `regular` explicitly clear inherited styles. Syntect does not carry
            // TextMate's strikethrough bit, but `Some(empty)` still preserves that clearing.
            "normal" | "regular" | "strikethrough" => {}
            _ => {}
        }
    }
    style
}

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

/// Syntax output for both source sides represented by one visible diff row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightedDiffLine {
    pub deletion: Option<HighlightedLine>,
    pub addition: Option<HighlightedLine>,
}

impl HighlightedDiffLine {
    #[must_use]
    pub fn for_stack(&self, kind: DiffLineKind) -> Option<&HighlightedLine> {
        match kind {
            DiffLineKind::Deletion => self.deletion.as_ref(),
            DiffLineKind::Addition | DiffLineKind::Context => {
                self.addition.as_ref().or(self.deletion.as_ref())
            }
        }
    }
}

pub type HighlightedHunk = Vec<HighlightedDiffLine>;
pub type HighlightedFile = Vec<HighlightedHunk>;

/// Bounds highlighter payload bytes retained between renderer frames.
pub const MAX_WORKER_HIGHLIGHT_CACHE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone)]
struct HighlightWorkerCacheEntry {
    bytes: usize,
    payload: CompactHighlightedDiff,
}

/// Byte-bounded LRU that returns deep clones and retains its own payloads.
#[derive(Debug)]
struct HighlightWorkerCache {
    entries: HashMap<String, HighlightWorkerCacheEntry>,
    lru: VecDeque<String>,
    max_bytes: usize,
    cached_bytes: usize,
}

impl HighlightWorkerCache {
    fn new(max_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            lru: VecDeque::new(),
            max_bytes: max_bytes.max(1),
            cached_bytes: 0,
        }
    }

    fn get(&mut self, cache_key: &str) -> Option<CompactHighlightedDiff> {
        let payload = self.entries.get(cache_key)?.payload.clone();
        self.promote(cache_key);
        Some(payload)
    }

    /// Retain a worker-owned clone and evict least-recently-used entries over budget.
    fn set(&mut self, cache_key: String, payload: &CompactHighlightedDiff) -> bool {
        let bytes = compact_highlighted_diff_byte_length(payload);
        if bytes > self.max_bytes {
            return false;
        }

        if let Some(previous) = self.entries.remove(&cache_key) {
            self.cached_bytes = self.cached_bytes.saturating_sub(previous.bytes);
            self.remove_from_lru(&cache_key);
        }
        self.entries.insert(
            cache_key.clone(),
            HighlightWorkerCacheEntry {
                bytes,
                payload: payload.clone(),
            },
        );
        self.lru.push_back(cache_key);
        self.cached_bytes = self.cached_bytes.saturating_add(bytes);

        while self.cached_bytes > self.max_bytes {
            let Some(least_recently_used) = self.lru.pop_front() else {
                return false;
            };
            if let Some(evicted) = self.entries.remove(&least_recently_used) {
                self.cached_bytes = self.cached_bytes.saturating_sub(evicted.bytes);
            }
        }
        true
    }

    fn promote(&mut self, cache_key: &str) {
        self.remove_from_lru(cache_key);
        self.lru.push_back(cache_key.to_owned());
    }

    fn remove_from_lru(&mut self, cache_key: &str) {
        if let Some(index) = self.lru.iter().position(|key| key == cache_key) {
            self.lru.remove(index);
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.lru.clear();
        self.cached_bytes = 0;
    }

    #[cfg(test)]
    fn cached_bytes(&self) -> usize {
        self.cached_bytes
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HighlightAppearance {
    Dark,
    Light,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HighlightWorkerIdentity<'a> {
    alias_context: bool,
    appearance: HighlightAppearance,
    language: &'a str,
    metadata: HighlightWorkerMetadata<'a>,
    revision: u32,
    theme: &'a str,
}

/// Provider-neutral equivalent of Pierre's worker-render metadata.
///
/// Mount-local file addresses and agent annotations are deliberately absent:
/// neither changes syntax output. Every field that can change rendered diff
/// spans or geometry participates directly in the SHA-256 payload.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HighlightWorkerMetadata<'a> {
    path: &'a str,
    previous_path: Option<&'a str>,
    change_kind: FileChangeKind,
    stats: FileStats,
    flags: FileFlags,
    patch: &'a str,
    split_row_count: usize,
    stack_row_count: usize,
    hunks: &'a [DiffHunk],
    sources: &'a FileSourceSnapshots,
}

/// Hash all worker-render inputs so no caller-supplied compact key can collide.
#[must_use]
pub fn highlight_worker_cache_key(
    file: &DiffFile,
    alias_context: bool,
    appearance: HighlightAppearance,
    language: &str,
    theme: &str,
) -> String {
    let payload = HighlightWorkerIdentity {
        alias_context,
        appearance,
        language,
        metadata: HighlightWorkerMetadata {
            path: &file.path,
            previous_path: file.previous_path.as_deref(),
            change_kind: file.change_kind,
            stats: file.stats,
            flags: file.flags,
            patch: &file.patch,
            split_row_count: file.split_row_count,
            stack_row_count: file.stack_row_count,
            hunks: &file.hunks,
            sources: &file.sources,
        },
        revision: HIGHLIGHT_WORKER_CACHE_REVISION,
        theme,
    };
    let encoded = serde_json::to_vec(&payload).expect("highlight identity is JSON serializable");
    format!("{:x}", Sha256::digest(encoded))
}

/// TextMate highlighter backed by syntect's Oniguruma-compatible engine.
///
/// Cache identity includes full renderer metadata, alias mode, appearance, language, and theme.
/// It never uses the mount-local runtime id, preventing fuzzy reloads from reusing another file's
/// spans.
#[derive(Debug)]
pub struct HighlightCache {
    syntaxes: SyntaxSet,
    themes: ThemeSet,
    textmate_themes: HashMap<String, TextMateTheme>,
    theme_appearances: HashMap<String, HighlightAppearance>,
    entries: HighlightedDiffCache,
    worker_entries: HighlightWorkerCache,
    worker_client: HighlightWorkerClient,
}

impl Default for HighlightCache {
    fn default() -> Self {
        Self {
            syntaxes: SyntaxSet::load_defaults_newlines(),
            themes: ThemeSet::load_defaults(),
            textmate_themes: HashMap::new(),
            theme_appearances: HashMap::new(),
            entries: HighlightedDiffCache::default(),
            worker_entries: HighlightWorkerCache::new(MAX_WORKER_HIGHLIGHT_CACHE_BYTES),
            worker_client: HighlightWorkerClient::new(),
        }
    }
}

impl HighlightCache {
    fn ensure_bundled_theme_loaded(&mut self, theme_name: &str) -> bool {
        if self.textmate_themes.contains_key(theme_name) {
            return true;
        }
        let Some((expected_name, source)) = BUNDLED_THEME_ASSETS
            .iter()
            .find(|(name, _)| *name == theme_name)
        else {
            return false;
        };
        let theme = serde_json::from_str::<TextMateTheme>(source)
            .unwrap_or_else(|error| panic!("invalid bundled theme {expected_name}: {error}"))
            .normalize_shiki();
        assert_eq!(
            &theme.name, expected_name,
            "bundled theme asset name does not match its manifest id"
        );
        let appearance = if theme.r#type.as_deref() == Some("light") {
            HighlightAppearance::Light
        } else {
            HighlightAppearance::Dark
        };
        self.themes
            .themes
            .insert(theme_name.into(), theme.compile_syntect());
        self.theme_appearances.insert(theme_name.into(), appearance);
        self.textmate_themes.insert(theme_name.into(), theme);
        true
    }

    /// Register a content-addressed TextMate theme and return its stable cache identity.
    pub fn ensure_syntax_highlight_theme_registered(
        &mut self,
        appearance: HighlightAppearance,
        base_theme: Option<&str>,
        scope_overrides: &[(String, String)],
    ) -> String {
        let theme_name = syntax_highlight_theme_name(appearance, base_theme, scope_overrides);
        if scope_overrides.is_empty() || self.themes.themes.contains_key(&theme_name) {
            return theme_name;
        }
        let base_theme_name = base_theme.unwrap_or(match appearance {
            HighlightAppearance::Light => PIERRE_LIGHT_THEME,
            HighlightAppearance::Dark => PIERRE_DARK_THEME,
        });
        let fallback = match appearance {
            HighlightAppearance::Light => PIERRE_LIGHT_THEME,
            HighlightAppearance::Dark => PIERRE_DARK_THEME,
        };
        if !self.ensure_bundled_theme_loaded(base_theme_name) {
            self.ensure_bundled_theme_loaded(fallback);
        }
        let Some(mut textmate_theme) = self
            .textmate_themes
            .get(base_theme_name)
            .or_else(|| self.textmate_themes.get(fallback))
            .cloned()
        else {
            return theme_name;
        };
        textmate_theme.name.clone_from(&theme_name);
        textmate_theme.append_scope_overrides(scope_overrides);
        let theme = textmate_theme.compile_syntect();
        self.theme_appearances
            .insert(theme_name.clone(), appearance);
        self.themes.themes.insert(theme_name.clone(), theme);
        self.textmate_themes
            .insert(theme_name.clone(), textmate_theme);
        theme_name
    }

    pub fn highlight_with_syntax_theme(
        &mut self,
        file: &DiffFile,
        appearance: HighlightAppearance,
        base_theme: Option<&str>,
        scope_overrides: &[(String, String)],
    ) -> HighlightedFile {
        let theme_name =
            self.ensure_syntax_highlight_theme_registered(appearance, base_theme, scope_overrides);
        self.highlight(file, &theme_name)
    }

    pub fn highlight(&mut self, file: &DiffFile, theme: &str) -> HighlightedFile {
        let theme = resolve_legacy_theme_id(Some(theme)).unwrap_or(theme);
        self.ensure_bundled_theme_loaded(theme);
        let language = file.language.clone().unwrap_or_default();
        let appearance = self
            .theme_appearances
            .get(theme)
            .copied()
            .unwrap_or_else(|| {
                if bundled_shiki_theme_is_light(Some(theme))
                    .unwrap_or_else(|| theme.to_ascii_lowercase().contains("light"))
                {
                    HighlightAppearance::Light
                } else {
                    HighlightAppearance::Dark
                }
            });
        let metadata = project_review_file(file, &file.key, 0);
        let source_plan = create_source_backed_highlight_plan(
            &metadata,
            file.sources
                .old
                .as_ref()
                .map(|source| source.content.as_str()),
            file.sources
                .new
                .as_ref()
                .map(|source| source.content.as_str()),
        );
        let alias_context = source_plan.is_none();
        let key = highlight_worker_cache_key(file, alias_context, appearance, &language, theme);
        if let Some(cached) = self.entries.get(&key) {
            return cached.highlighted;
        }
        self.worker_client.ensure_builtin_backend();
        let request_id = self.worker_client.enqueue(HighlightWorkerInput {
            alias_context,
            metadata: metadata.clone(),
            appearance,
            language: language.clone(),
            theme: theme.to_owned(),
        });
        let dispatched = self.worker_client.dispatch_next();
        debug_assert_eq!(
            dispatched.as_ref().map(|request| request.id),
            Some(request_id)
        );

        if let Some(cached) = self.worker_entries.get(&key) {
            let response = self
                .worker_client
                .handle_response(HighlightWorkerResponse::Success {
                    version: HIGHLIGHT_WORKER_PROTOCOL_VERSION,
                    id: request_id,
                    code: Box::new(cached),
                });
            let cached = response
                .and_then(|outcome| outcome.settlement.result.ok())
                .expect("native highlight worker accepts its own protocol response");
            if let Ok(sides) = decode_compact_syntax_lines(
                &cached,
                &metadata.deletion_lines,
                &metadata.addition_lines,
            ) {
                let highlighted = assemble_highlighted_file(file, &metadata, &sides);
                self.entries
                    .set(key, highlighted_diff_code(file, highlighted.clone()));
                return highlighted;
            }
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
            .or_else(|| match appearance {
                HighlightAppearance::Light => self.themes.themes.get("InspiredGitHub"),
                HighlightAppearance::Dark => self.themes.themes.get("base16-ocean.dark"),
            })
            .or_else(|| self.themes.themes.values().next())
        else {
            self.worker_client
                .handle_response(HighlightWorkerResponse::Failure {
                    version: HIGHLIGHT_WORKER_PROTOCOL_VERSION,
                    id: request_id,
                    message: "No native syntax theme is available.".into(),
                });
            return plain_file(file);
        };
        let highlight_metadata = source_plan
            .as_ref()
            .map_or(&metadata, |plan| &plan.metadata);
        let highlighted_sides =
            highlight_metadata_sides(highlight_metadata, syntax, theme, &self.syntaxes);
        let visible_sides = if let Some(plan) = &source_plan {
            remap_source_backed_highlight(plan, &highlighted_sides)
        } else {
            let mut visible = highlighted_sides;
            alias_context_highlight_lines(&metadata, &mut visible);
            visible
        };
        let compact_result =
            encode_compact_syntax_lines(&visible_sides).map_err(|error| error.to_string());
        let response = match compact_result {
            Ok(compact) => HighlightWorkerResponse::Success {
                version: HIGHLIGHT_WORKER_PROTOCOL_VERSION,
                id: request_id,
                code: Box::new(compact),
            },
            Err(message) => HighlightWorkerResponse::Failure {
                version: HIGHLIGHT_WORKER_PROTOCOL_VERSION,
                id: request_id,
                message,
            },
        };
        let compact = self
            .worker_client
            .handle_response(response)
            .and_then(|outcome| outcome.settlement.result.ok());
        let cached_sides = compact
            .as_ref()
            .and_then(|payload| {
                decode_compact_syntax_lines(
                    payload,
                    &metadata.deletion_lines,
                    &metadata.addition_lines,
                )
                .ok()
            })
            .unwrap_or(visible_sides);
        let highlighted = assemble_highlighted_file(file, &metadata, &cached_sides);
        if let Some(compact) = &compact {
            self.worker_entries.set(key.clone(), compact);
        }
        self.entries
            .set(key, highlighted_diff_code(file, highlighted.clone()));
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
        self.worker_entries.clear();
    }

    /// Release the logical native worker boundary and reject any outstanding jobs.
    pub fn dispose_worker(&mut self) {
        self.worker_client.dispose();
    }
}

fn highlight_metadata_sides(
    metadata: &SemanticReviewFile,
    syntax: &syntect::parsing::SyntaxReference,
    theme: &Theme,
    syntaxes: &SyntaxSet,
) -> HighlightLineArrays<HighlightedLine> {
    HighlightLineArrays {
        deletion_lines: highlight_source_side(&metadata.deletion_lines, syntax, theme, syntaxes),
        addition_lines: highlight_source_side(&metadata.addition_lines, syntax, theme, syntaxes),
    }
}

fn highlight_source_side(
    lines: &[String],
    syntax: &syntect::parsing::SyntaxReference,
    theme: &Theme,
    syntaxes: &SyntaxSet,
) -> Vec<Option<HighlightedLine>> {
    let mut highlighter = HighlightLines::new(syntax, theme);
    lines
        .iter()
        .map(|source| {
            Some(
                highlighter
                    .highlight_line(source, syntaxes)
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
                            .collect()
                    })
                    .unwrap_or_else(|_| vec![plain_token(source.trim_end_matches('\n'))]),
            )
        })
        .collect()
}

fn assemble_highlighted_file(
    file: &DiffFile,
    metadata: &SemanticReviewFile,
    sides: &HighlightLineArrays<HighlightedLine>,
) -> HighlightedFile {
    file.hunks
        .iter()
        .enumerate()
        .map(|(hunk_index, hunk)| {
            let mut deletion_line_index = metadata
                .hunks
                .get(hunk_index)
                .map_or(0, |hunk| hunk.deletion_line_index);
            let mut addition_line_index = metadata
                .hunks
                .get(hunk_index)
                .map_or(0, |hunk| hunk.addition_line_index);
            hunk.lines
                .iter()
                .map(|line| {
                    let deletion = if line.old_line.is_some() {
                        let highlighted = sides
                            .deletion_lines
                            .get(deletion_line_index)
                            .cloned()
                            .flatten();
                        deletion_line_index = deletion_line_index.saturating_add(1);
                        highlighted
                    } else {
                        None
                    };
                    let addition = if line.new_line.is_some() {
                        let highlighted = sides
                            .addition_lines
                            .get(addition_line_index)
                            .cloned()
                            .flatten();
                        addition_line_index = addition_line_index.saturating_add(1);
                        highlighted
                    } else {
                        None
                    };
                    HighlightedDiffLine { deletion, addition }
                })
                .collect()
        })
        .collect()
}

fn highlighted_diff_code(file: &DiffFile, highlighted: HighlightedFile) -> HighlightedDiffCode {
    let (deletion_line_count, addition_line_count) =
        file.hunks.iter().flat_map(|hunk| &hunk.lines).fold(
            (0_usize, 0_usize),
            |(deletions, additions), line| match line.kind {
                workdeck_core::DiffLineKind::Context => {
                    (deletions.saturating_add(1), additions.saturating_add(1))
                }
                workdeck_core::DiffLineKind::Deletion => (deletions.saturating_add(1), additions),
                workdeck_core::DiffLineKind::Addition => (deletions, additions.saturating_add(1)),
            },
        );
    HighlightedDiffCode::new(highlighted, deletion_line_count, addition_line_count)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SyntaxThemeFingerprint<'a> {
    base_theme_name: &'a str,
    ordered_overrides: Vec<(&'a str, String)>,
}

/// Content-address one base theme plus precedence-sensitive TextMate rules.
#[must_use]
pub fn syntax_highlight_theme_name(
    appearance: HighlightAppearance,
    base_theme: Option<&str>,
    scope_overrides: &[(String, String)],
) -> String {
    let base_theme_name = base_theme.unwrap_or(match appearance {
        HighlightAppearance::Light => PIERRE_LIGHT_THEME,
        HighlightAppearance::Dark => PIERRE_DARK_THEME,
    });
    if scope_overrides.is_empty() {
        return base_theme_name.into();
    }
    let payload = SyntaxThemeFingerprint {
        base_theme_name,
        ordered_overrides: scope_overrides
            .iter()
            .map(|(scope, color)| (scope.as_str(), color.to_ascii_lowercase()))
            .collect(),
    };
    let encoded = serde_json::to_vec(&payload).expect("syntax theme identity is JSON serializable");
    let fingerprint = format!("{:x}", Sha256::digest(encoded));
    format!("workdeck-custom-{}", &fingerprint[..16])
}

fn syntect_color_from_hex(value: &str) -> Option<SyntectColor> {
    fn nibble(value: u8) -> Option<u8> {
        match value {
            b'0'..=b'9' => Some(value - b'0'),
            b'a'..=b'f' => Some(value - b'a' + 10),
            b'A'..=b'F' => Some(value - b'A' + 10),
            _ => None,
        }
    }
    fn pair(first: u8, second: u8) -> Option<u8> {
        Some(nibble(first)? * 16 + nibble(second)?)
    }

    let value = value.strip_prefix('#')?.as_bytes();
    let (r, g, b, a) = match value.len() {
        3 => (
            nibble(value[0])? * 17,
            nibble(value[1])? * 17,
            nibble(value[2])? * 17,
            u8::MAX,
        ),
        4 => (
            nibble(value[0])? * 17,
            nibble(value[1])? * 17,
            nibble(value[2])? * 17,
            nibble(value[3])? * 17,
        ),
        6 | 8 => (
            pair(value[0], value[1])?,
            pair(value[2], value[3])?,
            pair(value[4], value[5])?,
            if value.len() == 8 {
                pair(value[6], value[7])?
            } else {
                u8::MAX
            },
        ),
        _ => return None,
    };
    Some(SyntectColor { r, g, b, a })
}

fn plain_file(file: &DiffFile) -> HighlightedFile {
    file.hunks
        .iter()
        .map(|hunk| {
            hunk.lines
                .iter()
                .map(|line| {
                    let highlighted = vec![plain_token(&line.content)];
                    HighlightedDiffLine {
                        deletion: line.old_line.is_some().then(|| highlighted.clone()),
                        addition: line.new_line.is_some().then_some(highlighted),
                    }
                })
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
    use workdeck_core::{ChangesetSource, FileSourceSnapshots, SourceOrigin, SourceSnapshot};

    fn identity_file(after: &str, name: &str) -> DiffFile {
        parse_patch(
            &format!(
                "diff --git a/{name} b/{name}\n--- a/{name}\n+++ b/{name}\n@@ -1 +1 @@\n-const answer = 41;\n+{after}"
            ),
            "identity",
            name,
            ChangesetSource::Patch {
                label: "identity".into(),
            },
        )
        .unwrap()
        .files
        .remove(0)
    }

    fn test_highlight_payload(line_count: usize) -> CompactHighlightedDiff {
        encode_compact_syntax_lines(&HighlightLineArrays {
            deletion_lines: vec![None; line_count],
            addition_lines: (0..line_count)
                .map(|index| {
                    Some(vec![SyntaxToken {
                        text: format!("line-{index}"),
                        foreground: SyntaxColor {
                            red: 1,
                            green: 2,
                            blue: 3,
                        },
                        bold: false,
                        italic: false,
                        underline: false,
                    }])
                })
                .collect(),
        })
        .unwrap()
    }

    #[test]
    fn worker_cache_returns_an_isolated_clone_without_losing_its_retained_payload() {
        let mut cache = HighlightWorkerCache::new(MAX_WORKER_HIGHLIGHT_CACHE_BYTES);
        let payload = test_highlight_payload(1);
        assert!(cache.set("first".into(), &payload));

        let mut first_response = cache.get("first").unwrap();
        assert!(!std::ptr::eq(
            first_response.addition.starts.as_ptr(),
            payload.addition.starts.as_ptr()
        ));
        first_response.addition.starts[0] = 99;
        assert_eq!(cache.get("first").unwrap().addition.starts[0], 0);
    }

    #[test]
    fn worker_cache_evicts_the_least_recently_used_payload_under_budget() {
        let payload = test_highlight_payload(1);
        let payload_bytes = compact_highlighted_diff_byte_length(&payload);
        let mut cache = HighlightWorkerCache::new(payload_bytes * 2);

        assert!(cache.set("first".into(), &test_highlight_payload(1)));
        assert!(cache.set("second".into(), &test_highlight_payload(1)));
        assert!(cache.get("first").is_some());
        assert!(cache.set("third".into(), &test_highlight_payload(1)));

        assert!(cache.get("first").is_some());
        assert!(cache.get("second").is_none());
        assert!(cache.get("third").is_some());
    }

    #[test]
    fn worker_cache_skips_oversized_payloads_without_evicting_a_resident() {
        let payload = test_highlight_payload(1);
        let mut cache = HighlightWorkerCache::new(compact_highlighted_diff_byte_length(&payload));
        assert!(cache.set("fitting".into(), &payload));
        assert!(!cache.set("oversized".into(), &test_highlight_payload(2)));

        assert!(cache.get("fitting").is_some());
        assert!(cache.get("oversized").is_none());
    }

    #[test]
    fn worker_cache_releases_a_replaced_payloads_previous_byte_charge() {
        let payload = test_highlight_payload(1);
        let mut cache =
            HighlightWorkerCache::new(compact_highlighted_diff_byte_length(&payload) * 2);

        assert!(cache.set("reloaded".into(), &test_highlight_payload(1)));
        assert!(cache.set("reloaded".into(), &test_highlight_payload(2)));
        assert!(cache.set("kept".into(), &test_highlight_payload(1)));

        assert!(cache.get("reloaded").is_none());
        assert!(cache.get("kept").is_some());
        assert_eq!(
            cache.cached_bytes(),
            compact_highlighted_diff_byte_length(&payload)
        );
    }

    #[test]
    fn native_highlight_cache_reconstructs_text_from_review_sources() {
        let file = identity_file("const answer = '🦀';\n", "example.ts");
        let mut cache = HighlightCache::default();
        let first = cache.highlight(&file, PIERRE_DARK_THEME);
        assert!(!cache.worker_entries.entries.is_empty());
        let compact = cache
            .worker_entries
            .entries
            .values()
            .next()
            .unwrap()
            .payload
            .clone();
        assert!(
            compact
                .foreground_palette
                .iter()
                .all(|color| color.starts_with('#'))
        );

        // Force the second call through the worker-owned compact cache instead of the terminal
        // line cache. The reconstructed rows must use the authoritative diff text, including a
        // non-BMP scalar whose compact range occupies two UTF-16 columns.
        cache.entries.clear();
        let second = cache.highlight(&file, PIERRE_DARK_THEME);
        assert_eq!(second, first);
        let reconstructed = second[0][1]
            .addition
            .as_ref()
            .unwrap()
            .iter()
            .map(|token| token.text.as_str())
            .collect::<String>();
        assert_eq!(reconstructed, "const answer = '🦀';");
    }

    #[test]
    fn worker_cache_identity_matches_equivalent_full_inputs() {
        let first = identity_file("const answer = 42;\n", "example.ts");
        let second = identity_file("const answer = 42;\n", "example.ts");
        assert_eq!(
            highlight_worker_cache_key(
                &first,
                true,
                HighlightAppearance::Dark,
                "typescript",
                "pierre-dark",
            ),
            highlight_worker_cache_key(
                &second,
                true,
                HighlightAppearance::Dark,
                "typescript",
                "pierre-dark",
            )
        );
    }

    #[test]
    fn worker_cache_identity_changes_for_text_and_every_render_input() {
        let base = identity_file("const answer = 42;\n", "example.ts");
        let same_length_text = identity_file("const answer = 24;\n", "example.ts");
        let renamed = identity_file("const answer = 42;\n", "example.js");
        let key = |file: &DiffFile,
                   alias_context: bool,
                   appearance: HighlightAppearance,
                   language: &str,
                   theme: &str| {
            highlight_worker_cache_key(file, alias_context, appearance, language, theme)
        };
        let base_key = key(
            &base,
            true,
            HighlightAppearance::Dark,
            "typescript",
            "pierre-dark",
        );
        assert_ne!(
            key(
                &same_length_text,
                true,
                HighlightAppearance::Dark,
                "typescript",
                "pierre-dark",
            ),
            base_key
        );
        assert_ne!(
            key(
                &base,
                false,
                HighlightAppearance::Dark,
                "typescript",
                "pierre-dark",
            ),
            base_key
        );
        assert_ne!(
            key(
                &base,
                true,
                HighlightAppearance::Light,
                "typescript",
                "pierre-light",
            ),
            base_key
        );
        assert_ne!(
            key(
                &base,
                true,
                HighlightAppearance::Dark,
                "javascript",
                "pierre-dark",
            ),
            base_key
        );
        assert_ne!(
            key(
                &renamed,
                true,
                HighlightAppearance::Dark,
                "typescript",
                "pierre-dark",
            ),
            base_key
        );
    }

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

    #[test]
    fn ui_cache_charges_context_lines_on_both_diff_sides() {
        let file = parse_patch(
            "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,2 +1,2 @@\n context\n-old\n+new\n",
            "cache-cost",
            "cache-cost",
            ChangesetSource::Patch {
                label: "cache-cost".into(),
            },
        )
        .unwrap()
        .files
        .remove(0);
        let code = highlighted_diff_code(&file, Vec::new());
        assert_eq!(code.deletion_line_count, 2);
        assert_eq!(code.addition_line_count, 2);
        assert_eq!(code.retained_line_count(), 4);
    }

    #[test]
    fn syntax_theme_identity_includes_precedence_sensitive_scope_order() {
        let broad_first = vec![
            ("comment".into(), "#111111".into()),
            ("comment, string".into(), "#222222".into()),
        ];
        let broad_last = vec![
            ("comment, string".into(), "#222222".into()),
            ("comment".into(), "#111111".into()),
        ];
        assert_ne!(
            syntax_highlight_theme_name(
                HighlightAppearance::Dark,
                Some("github-dark-default"),
                &broad_first,
            ),
            syntax_highlight_theme_name(
                HighlightAppearance::Dark,
                Some("github-dark-default"),
                &broad_last,
            )
        );
        assert_eq!(
            syntax_highlight_theme_name(
                HighlightAppearance::Dark,
                Some("github-dark-default"),
                &broad_first,
            ),
            syntax_highlight_theme_name(
                HighlightAppearance::Dark,
                Some("github-dark-default"),
                &[
                    ("comment".into(), "#111111".into()),
                    ("comment, string".into(), "#222222".into()),
                ],
            )
        );
        assert_eq!(
            syntax_highlight_theme_name(
                HighlightAppearance::Dark,
                Some("github-dark-default"),
                &broad_first,
            ),
            "workdeck-custom-35128eff5a5bf474"
        );
    }

    #[test]
    fn syntax_theme_defaults_and_registration_use_native_theme_storage() {
        assert_eq!(
            syntax_highlight_theme_name(HighlightAppearance::Light, None, &[]),
            PIERRE_LIGHT_THEME
        );
        assert_eq!(
            syntax_highlight_theme_name(HighlightAppearance::Dark, None, &[]),
            PIERRE_DARK_THEME
        );
        let mut cache = HighlightCache::default();
        let name = cache.ensure_syntax_highlight_theme_registered(
            HighlightAppearance::Light,
            Some("github-light-default"),
            &[("keyword.control".into(), "#AABBCC".into())],
        );
        assert!(name.starts_with("workdeck-custom-"));
        assert_eq!(name.len(), "workdeck-custom-".len() + 16);
        assert!(cache.themes.themes.contains_key(&name));
        assert_eq!(
            cache.theme_appearances.get(&name),
            Some(&HighlightAppearance::Light)
        );
        assert_eq!(
            cache.ensure_syntax_highlight_theme_registered(
                HighlightAppearance::Light,
                Some("github-light-default"),
                &[("keyword.control".into(), "#aabbcc".into())],
            ),
            name
        );
        let registered = cache.textmate_themes.get(&name).unwrap();
        let last = registered.settings.as_ref().unwrap().last().unwrap();
        assert_eq!(last.scope.as_ref().unwrap().selectors(), "keyword.control");
        assert_eq!(
            last.settings.as_ref().unwrap().foreground.as_deref(),
            Some("#AABBCC")
        );
    }

    #[test]
    fn all_pinned_shiki_and_pierre_theme_payloads_compile_without_dropped_rules() {
        let mut cache = HighlightCache::default();
        assert!(cache.textmate_themes.is_empty());
        for (theme_id, _) in BUNDLED_THEME_ASSETS {
            assert!(cache.ensure_bundled_theme_loaded(theme_id));
        }
        assert_eq!(cache.textmate_themes.len(), 67);
        assert_eq!(cache.theme_appearances.len(), 67);
        for theme_id in workdeck_core::BUNDLED_SHIKI_THEME_IDS {
            assert!(cache.textmate_themes.contains_key(*theme_id), "{theme_id}");
        }
        for theme_id in [PIERRE_LIGHT_THEME, PIERRE_DARK_THEME] {
            assert!(cache.textmate_themes.contains_key(theme_id));
        }

        for (theme_id, textmate) in &cache.textmate_themes {
            let expected_rules = textmate
                .settings
                .as_deref()
                .unwrap_or_default()
                .iter()
                .filter(|rule| rule.scope.is_some() && rule.settings.is_some())
                .count();
            let compiled_rules = cache.themes.themes[theme_id].scopes.len();
            assert_eq!(compiled_rules, expected_rules, "{theme_id}");
        }
    }

    #[test]
    fn shiki_normalization_preserves_exact_defaults_rules_and_alpha_colors() {
        let mut cache = HighlightCache::default();
        assert!(cache.ensure_bundled_theme_loaded("github-dark-default"));
        let github = &cache.textmate_themes["github-dark-default"];
        assert_eq!(github.fg.as_deref(), Some("#e6edf3"));
        assert_eq!(github.bg.as_deref(), Some("#0d1117"));
        assert_eq!(github.settings.as_ref().unwrap().len(), 50);
        assert!(github.token_colors.is_none());
        assert_eq!(
            cache.themes.themes["github-dark-default"]
                .settings
                .foreground,
            Some(SyntectColor {
                r: 0xe6,
                g: 0xed,
                b: 0xf3,
                a: 0xff,
            })
        );
        assert_eq!(
            syntect_color_from_hex("#D50"),
            Some(SyntectColor {
                r: 0xdd,
                g: 0x55,
                b: 0x00,
                a: 0xff,
            })
        );
        assert_eq!(
            syntect_color_from_hex("#565869AA"),
            Some(SyntectColor {
                r: 0x56,
                g: 0x58,
                b: 0x69,
                a: 0xaa,
            })
        );
        assert_eq!(syntect_color_from_hex("#\u{e9}00"), None);
    }

    #[test]
    fn native_highlighter_uses_the_selected_pinned_theme_payload() {
        let file = identity_file("const answer = true;\n", "example.rs");
        let mut cache = HighlightCache::default();
        let github = cache.highlight(&file, "github-dark-default");
        let github_added = github[0][1].addition.as_ref().unwrap();
        assert_eq!(
            github_added
                .iter()
                .map(|token| token.text.as_str())
                .collect::<String>(),
            "const answer = true;"
        );
        let github_keyword = github_added
            .iter()
            .find(|token| token.text == "const")
            .unwrap();
        assert_eq!(
            github_keyword.foreground,
            SyntaxColor {
                red: 0xff,
                green: 0x7b,
                blue: 0x72,
            }
        );

        let pierre = cache.highlight(&file, PIERRE_DARK_THEME);
        let pierre_keyword = pierre[0][1]
            .addition
            .as_ref()
            .unwrap()
            .iter()
            .find(|token| token.text == "const")
            .unwrap();
        assert_eq!(
            pierre_keyword.foreground,
            SyntaxColor {
                red: 0xd5,
                green: 0x68,
                blue: 0xea,
            }
        );

        let custom = cache.highlight_with_syntax_theme(
            &file,
            HighlightAppearance::Dark,
            Some("github-dark-default"),
            &[("storage.type".into(), "#123456".into())],
        );
        let custom_keyword = custom[0][1]
            .addition
            .as_ref()
            .unwrap()
            .iter()
            .find(|token| token.text == "const")
            .unwrap();
        assert_eq!(
            custom_keyword.foreground,
            SyntaxColor {
                red: 0x12,
                green: 0x34,
                blue: 0x56,
            }
        );
    }

    #[test]
    fn source_backed_highlight_preserves_independent_context_grammar_states() {
        let mut file = parse_patch(
            "diff --git a/state.rs b/state.rs\n--- a/state.rs\n+++ b/state.rs\n@@ -2,2 +2,2 @@\n same\n-old\n+new\n",
            "source-state",
            "source-state",
            ChangesetSource::Patch {
                label: "source-state".into(),
            },
        )
        .unwrap()
        .files
        .remove(0);
        file.set_sources(FileSourceSnapshots {
            old: Some(SourceSnapshot::new(
                "/*\nsame\nold\n".into(),
                SourceOrigin::Revision {
                    revision: "HEAD".into(),
                },
                true,
            )),
            new: Some(SourceSnapshot::new(
                "let prefix = 1;\nsame\nnew\n".into(),
                SourceOrigin::WorkingTree,
                true,
            )),
        });

        let mut cache = HighlightCache::default();
        let source_backed = cache.highlight(&file, "base16-ocean.dark");
        let context = &source_backed[0][0];
        assert_ne!(context.deletion, context.addition);
        assert_eq!(
            context
                .deletion
                .as_ref()
                .unwrap()
                .iter()
                .map(|token| token.text.as_str())
                .collect::<String>(),
            "same"
        );
        assert_eq!(
            context
                .addition
                .as_ref()
                .unwrap()
                .iter()
                .map(|token| token.text.as_str())
                .collect::<String>(),
            "same"
        );

        file.set_sources(FileSourceSnapshots::default());
        let fragment = cache.highlight(&file, "base16-ocean.dark");
        assert_eq!(fragment[0][0].deletion, fragment[0][0].addition);
    }

    #[test]
    fn complete_source_metadata_uses_absolute_hunk_line_indexes() {
        let mut file = parse_patch(
            "diff --git a/complete.rs b/complete.rs\n--- a/complete.rs\n+++ b/complete.rs\n@@ -2,2 +2,2 @@\n same\n-old\n+new\n",
            "complete-source",
            "complete-source",
            ChangesetSource::Patch {
                label: "complete-source".into(),
            },
        )
        .unwrap()
        .files
        .remove(0);
        file.flags.partial = false;
        file.set_sources(FileSourceSnapshots {
            old: Some(SourceSnapshot::new(
                "prefix\nsame\nold\n".into(),
                SourceOrigin::Revision {
                    revision: "HEAD".into(),
                },
                true,
            )),
            new: Some(SourceSnapshot::new(
                "prefix\nsame\nnew\n".into(),
                SourceOrigin::WorkingTree,
                true,
            )),
        });

        let highlighted = HighlightCache::default().highlight(&file, "base16-ocean.dark");
        let text = |line: &HighlightedLine| {
            line.iter()
                .map(|token| token.text.as_str())
                .collect::<String>()
        };
        assert_eq!(text(highlighted[0][0].deletion.as_ref().unwrap()), "same");
        assert_eq!(text(highlighted[0][1].deletion.as_ref().unwrap()), "old");
        assert_eq!(text(highlighted[0][2].addition.as_ref().unwrap()), "new");
    }
}
