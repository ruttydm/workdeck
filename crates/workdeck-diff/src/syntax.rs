use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::mem::size_of;
use std::path::Path;
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, ThemeSet};
use syntect::parsing::SyntaxSet;
use workdeck_core::{
    DiffFile, DiffHunk, FileChangeKind, FileFlags, FileSourceSnapshots, FileStats,
    bundled_shiki_theme_is_light, resolve_legacy_theme_id,
};

const HIGHLIGHT_WORKER_CACHE_REVISION: u32 = 1;

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

/// Bounds highlighter payload bytes retained between renderer frames.
pub const MAX_WORKER_HIGHLIGHT_CACHE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone)]
struct HighlightWorkerCacheEntry {
    bytes: usize,
    payload: HighlightedFile,
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

    fn get(&mut self, cache_key: &str) -> Option<HighlightedFile> {
        let payload = self.entries.get(cache_key)?.payload.clone();
        self.promote(cache_key);
        Some(payload)
    }

    /// Retain a worker-owned clone and evict least-recently-used entries over budget.
    fn set(&mut self, cache_key: String, payload: &HighlightedFile) -> bool {
        let bytes = highlighted_file_byte_length(payload);
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

    fn len(&self) -> usize {
        self.entries.len()
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
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

fn highlighted_file_byte_length(payload: &HighlightedFile) -> usize {
    let container_bytes = size_of::<HighlightedFile>()
        .saturating_add(payload.len().saturating_mul(size_of::<HighlightedHunk>()));
    payload.iter().fold(container_bytes, |file_bytes, hunk| {
        let hunk_bytes = size_of::<HighlightedHunk>()
            .saturating_add(hunk.len().saturating_mul(size_of::<HighlightedLine>()));
        file_bytes.saturating_add(hunk.iter().fold(hunk_bytes, |line_bytes, line| {
            line_bytes.saturating_add(line.iter().fold(0, |token_bytes, token| {
                token_bytes
                    .saturating_add(size_of::<SyntaxToken>())
                    .saturating_add(token.text.len())
            }))
        }))
    })
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
    entries: HighlightWorkerCache,
}

impl Default for HighlightCache {
    fn default() -> Self {
        Self {
            syntaxes: SyntaxSet::load_defaults_newlines(),
            themes: ThemeSet::load_defaults(),
            entries: HighlightWorkerCache::new(MAX_WORKER_HIGHLIGHT_CACHE_BYTES),
        }
    }
}

impl HighlightCache {
    pub fn highlight(&mut self, file: &DiffFile, theme: &str) -> HighlightedFile {
        let theme = resolve_legacy_theme_id(Some(theme)).unwrap_or(theme);
        let language = file.language.clone().unwrap_or_default();
        let appearance = if bundled_shiki_theme_is_light(Some(theme))
            .unwrap_or_else(|| theme.to_ascii_lowercase().contains("light"))
        {
            HighlightAppearance::Light
        } else {
            HighlightAppearance::Dark
        };
        let key = highlight_worker_cache_key(file, false, appearance, &language, theme);
        if let Some(cached) = self.entries.get(&key) {
            return cached;
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
        self.entries.set(key, &highlighted);
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

    fn test_highlight_payload(line_count: usize) -> HighlightedFile {
        vec![
            (0..line_count)
                .map(|index| {
                    vec![SyntaxToken {
                        text: format!("line-{index}"),
                        foreground: SyntaxColor {
                            red: 1,
                            green: 2,
                            blue: 3,
                        },
                        bold: false,
                        italic: false,
                        underline: false,
                    }]
                })
                .collect(),
        ]
    }

    #[test]
    fn worker_cache_returns_an_isolated_clone_without_losing_its_retained_payload() {
        let mut cache = HighlightWorkerCache::new(MAX_WORKER_HIGHLIGHT_CACHE_BYTES);
        let payload = test_highlight_payload(1);
        assert!(cache.set("first".into(), &payload));

        let mut first_response = cache.get("first").unwrap();
        assert!(!std::ptr::eq(
            first_response[0][0].as_ptr(),
            payload[0][0].as_ptr()
        ));
        first_response[0][0][0].text = "transferred-away".into();
        assert_eq!(cache.get("first").unwrap()[0][0][0].text, "line-0");
    }

    #[test]
    fn worker_cache_evicts_the_least_recently_used_payload_under_budget() {
        let payload = test_highlight_payload(1);
        let payload_bytes = highlighted_file_byte_length(&payload);
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
        let mut cache = HighlightWorkerCache::new(highlighted_file_byte_length(&payload));
        assert!(cache.set("fitting".into(), &payload));
        assert!(!cache.set("oversized".into(), &test_highlight_payload(2)));

        assert!(cache.get("fitting").is_some());
        assert!(cache.get("oversized").is_none());
    }

    #[test]
    fn worker_cache_releases_a_replaced_payloads_previous_byte_charge() {
        let payload = test_highlight_payload(1);
        let mut cache = HighlightWorkerCache::new(highlighted_file_byte_length(&payload) * 2);

        assert!(cache.set("reloaded".into(), &test_highlight_payload(1)));
        assert!(cache.set("reloaded".into(), &test_highlight_payload(2)));
        assert!(cache.set("kept".into(), &test_highlight_payload(1)));

        assert!(cache.get("reloaded").is_none());
        assert!(cache.get("kept").is_some());
        assert_eq!(cache.cached_bytes(), highlighted_file_byte_length(&payload));
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
}
