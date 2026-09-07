//! Shared highlighted-diff cache, prefetch, and committed-snapshot ownership.
//!
//! This is a native Rust reimplementation of Hunk's `src/ui/diff/useHighlightedDiff.ts` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. React effects become an explicit coordinator:
//! render-time reads are side-effect free, prefetch starts at most one logical request per key,
//! and late/retryable completions cannot poison the shared cache.

use std::collections::HashMap;

use workdeck_core::{DiffFile, DiffLineKind, review_digest};
use workdeck_diff::{
    HighlightAppearance, HighlightCache, HighlightedDiffCache, HighlightedDiffCode,
    HighlightedFile, SourceHighlightTheme, syntax_highlight_theme_name,
};

use crate::{AppTheme, ThemeAppearance};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HighlightSourceProviderIdentity {
    PatchOnly,
    Versioned(String),
    Unversioned(u64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightCompletion {
    pub code: HighlightedDiffCode,
    pub retryable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightRequestState {
    Cached,
    Existing(u64),
    Started(u64),
}

#[derive(Debug, Default)]
pub struct HighlightedDiffCoordinator {
    cache: HighlightedDiffCache,
    in_flight: HashMap<String, u64>,
    next_request_id: u64,
}

impl HighlightedDiffCoordinator {
    #[must_use]
    pub fn begin(&mut self, cache_key: &str) -> HighlightRequestState {
        if self.cache.get(cache_key).is_some() {
            return HighlightRequestState::Cached;
        }
        if let Some(request_id) = self.in_flight.get(cache_key) {
            return HighlightRequestState::Existing(*request_id);
        }
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        self.in_flight
            .insert(cache_key.into(), self.next_request_id);
        HighlightRequestState::Started(self.next_request_id)
    }

    #[must_use]
    pub fn read(&mut self, cache_key: &str) -> Option<HighlightedDiffCode> {
        self.cache.get(cache_key)
    }

    #[must_use]
    pub fn peek(&self, cache_key: &str) -> Option<&HighlightedDiffCode> {
        self.cache.peek(cache_key)
    }

    /// Commit only the still-active request. Retryable plain-row fallbacks remain visible to the
    /// caller but deliberately do not occupy the shared cache.
    pub fn commit(
        &mut self,
        cache_key: &str,
        request_id: u64,
        completion: &HighlightCompletion,
    ) -> bool {
        if self.in_flight.get(cache_key) != Some(&request_id) {
            return false;
        }
        self.in_flight.remove(cache_key);
        if !completion.retryable {
            self.cache.set(cache_key.into(), completion.code.clone());
        }
        true
    }

    pub fn clear(&mut self) {
        self.cache.clear();
        self.in_flight.clear();
    }

    #[must_use]
    pub fn is_in_flight(&self, cache_key: &str) -> bool {
        self.in_flight.contains_key(cache_key)
    }
}

fn appearance(theme: &AppTheme) -> HighlightAppearance {
    match theme.appearance {
        ThemeAppearance::Light => HighlightAppearance::Light,
        ThemeAppearance::Dark => HighlightAppearance::Dark,
    }
}

fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}

/// Borrow content directly instead of allocating a JSON value tree for every cache lookup.
/// Declaration order intentionally matches the existing ordered JSON fingerprint format.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct HighlightFingerprintMetadata<'a> {
    path: &'a str,
    previous_path: Option<&'a str>,
    change_kind: &'a workdeck_core::FileChangeKind,
    stats: &'a workdeck_core::FileStats,
    flags: &'a workdeck_core::FileFlags,
    split_row_count: usize,
    stack_row_count: usize,
    hunks: &'a [workdeck_core::DiffHunk],
    sources: &'a workdeck_core::FileSourceSnapshots,
}

fn highlight_fingerprint_metadata(file: &DiffFile) -> String {
    serde_json::to_string(&HighlightFingerprintMetadata {
        path: &file.path,
        previous_path: file.previous_path.as_deref(),
        change_kind: &file.change_kind,
        stats: &file.stats,
        flags: &file.flags,
        split_row_count: file.split_row_count,
        stack_row_count: file.stack_row_count,
        hunks: &file.hunks,
        sources: &file.sources,
    })
    .expect("highlight fingerprint metadata is JSON serializable")
}

/// Hash every diff-content input that can change the native highlighted result.
#[must_use]
pub fn highlighted_content_fingerprint(file: &DiffFile) -> String {
    let metadata = highlight_fingerprint_metadata(file);
    let fingerprint_input = format!(
        "{}:{}{}:{}",
        utf16_len(&file.patch),
        file.patch,
        utf16_len(&metadata),
        metadata
    );
    review_digest(fingerprint_input.as_bytes())
}

fn derived_source_provider(file: &DiffFile) -> HighlightSourceProviderIdentity {
    if !file.flags.partial {
        return HighlightSourceProviderIdentity::PatchOnly;
    }
    file.source_identity
        .as_ref()
        .map_or(HighlightSourceProviderIdentity::PatchOnly, |identity| {
            HighlightSourceProviderIdentity::Versioned(identity.clone())
        })
}

fn source_provider_fingerprint(provider: &HighlightSourceProviderIdentity) -> String {
    match provider {
        HighlightSourceProviderIdentity::PatchOnly => "patch-only".into(),
        HighlightSourceProviderIdentity::Versioned(cache_key) => {
            format!("source-cache:{}:{cache_key}", utf16_len(cache_key))
        }
        HighlightSourceProviderIdentity::Unversioned(id) => format!("source:{id}"),
    }
}

/// Cache key containing theme, syntax theme, mounted file, language, complete content, and source
/// provider identity. The explicit-provider form preserves Hunk's unversioned-provider boundary;
/// normal Workdeck callers use embedded, content-addressed source snapshots.
#[must_use]
pub fn highlighted_diff_cache_key_with_provider(
    theme: &AppTheme,
    file: &DiffFile,
    provider: &HighlightSourceProviderIdentity,
) -> String {
    let syntax_theme = syntax_highlight_theme_name(
        appearance(theme),
        theme.syntax_theme.as_deref(),
        &theme.syntax_scope_overrides,
    );
    let file_id = if file.runtime_id.is_empty() {
        &file.key
    } else {
        &file.runtime_id
    };
    format!(
        "{}:{syntax_theme}:{file_id}:{}:{}:{}",
        theme.id,
        file.language.as_deref().unwrap_or("text"),
        highlighted_content_fingerprint(file),
        source_provider_fingerprint(provider)
    )
}

#[must_use]
pub fn highlighted_diff_cache_key(theme: &AppTheme, file: &DiffFile) -> String {
    highlighted_diff_cache_key_with_provider(theme, file, &derived_source_provider(file))
}

fn highlighted_diff_code(file: &DiffFile, highlighted: HighlightedFile) -> HighlightedDiffCode {
    let (deletion_lines, addition_lines) = file.hunks.iter().flat_map(|hunk| &hunk.lines).fold(
        (0_usize, 0_usize),
        |(deletions, additions), line| {
            (
                deletions.saturating_add(usize::from(line.kind != DiffLineKind::Addition)),
                additions.saturating_add(usize::from(line.kind != DiffLineKind::Deletion)),
            )
        },
    );
    HighlightedDiffCode::new(highlighted, deletion_lines, addition_lines)
}

/// Read the best already-available result without promoting shared-cache recency.
#[must_use]
pub fn resolve_highlighted_snapshot(
    coordinator: &HighlightedDiffCoordinator,
    appearance_cache_key: Option<&str>,
    highlighted: Option<&HighlightedDiffCode>,
    highlighted_cache_key: Option<&str>,
) -> Option<HighlightedDiffCode> {
    let appearance_cache_key = appearance_cache_key?;
    if highlighted_cache_key == Some(appearance_cache_key) {
        return highlighted.cloned();
    }
    coordinator.peek(appearance_cache_key).cloned()
}

#[derive(Debug, Default)]
pub struct HighlightedDiffRuntime {
    engine: HighlightCache,
    coordinator: HighlightedDiffCoordinator,
}

impl Drop for HighlightedDiffRuntime {
    fn drop(&mut self) {
        self.engine.dispose_worker();
    }
}

impl HighlightedDiffRuntime {
    pub fn dispose_worker(&mut self) {
        self.engine.dispose_worker();
    }

    /// Queue or poll one shared highlight request. `offload_large_diff = false` intentionally keeps
    /// inline highlighting as the default; callers opt into the native worker for eligible files.
    pub fn prefetch_highlighted_diff(
        &mut self,
        file: &DiffFile,
        theme: &AppTheme,
        offload_large_diff: bool,
    ) -> Option<HighlightedDiffCode> {
        let cache_key = highlighted_diff_cache_key(theme, file);
        let request = self.coordinator.begin(&cache_key);
        if request == HighlightRequestState::Cached {
            return self.coordinator.read(&cache_key);
        }
        let request_id = match request {
            HighlightRequestState::Existing(request_id)
            | HighlightRequestState::Started(request_id) => request_id,
            HighlightRequestState::Cached => unreachable!(),
        };
        let highlighted = if offload_large_diff {
            self.engine.highlight_with_syntax_theme_live(
                file,
                appearance(theme),
                theme.syntax_theme.as_deref(),
                &theme.syntax_scope_overrides,
            )?
        } else {
            self.engine.highlight_with_syntax_theme(
                file,
                appearance(theme),
                theme.syntax_theme.as_deref(),
                &theme.syntax_scope_overrides,
            )
        };
        let completion = HighlightCompletion {
            code: highlighted_diff_code(file, highlighted),
            retryable: false,
        };
        self.coordinator.commit(&cache_key, request_id, &completion);
        Some(completion.code)
    }

    #[must_use]
    pub fn resolve_snapshot(
        &self,
        file: Option<&DiffFile>,
        theme: &AppTheme,
        highlighted: Option<&HighlightedDiffCode>,
        highlighted_cache_key: Option<&str>,
    ) -> Option<HighlightedDiffCode> {
        let appearance_cache_key = file.map(|file| highlighted_diff_cache_key(theme, file));
        resolve_highlighted_snapshot(
            &self.coordinator,
            appearance_cache_key.as_deref(),
            highlighted,
            highlighted_cache_key,
        )
    }

    pub fn engine_mut(&mut self) -> &mut HighlightCache {
        &mut self.engine
    }

    pub fn coordinator_mut(&mut self) -> &mut HighlightedDiffCoordinator {
        &mut self.coordinator
    }

    pub fn clear(&mut self) {
        self.engine.clear();
        self.coordinator.clear();
    }

    pub fn highlight_source_with_syntax_theme(
        &mut self,
        file: &DiffFile,
        text: &str,
        theme: SourceHighlightTheme<'_>,
    ) -> workdeck_diff::HighlightedSourceCode {
        self.engine
            .highlight_source_with_syntax_theme(file, text, theme)
    }

    pub fn highlight_source_with_syntax_theme_live(
        &mut self,
        file: &DiffFile,
        text: &str,
        theme: SourceHighlightTheme<'_>,
        should_load_highlight: bool,
    ) -> Option<workdeck_diff::HighlightedSourceCode> {
        self.engine.highlight_source_with_syntax_theme_live(
            file,
            text,
            theme,
            should_load_highlight,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_core::{ChangesetSource, FileSourceSnapshots, SourceOrigin, SourceSnapshot};
    use workdeck_diff::{
        FileComparisonOptions, FileSnapshot, HighlightedDiffLine, SyntaxColor, SyntaxToken,
        diff_from_file_snapshots, parse_patch,
    };

    use crate::resolve_theme;

    fn file(before: &str, after: &str, id: &str, path: &str) -> DiffFile {
        let mut file = diff_from_file_snapshots(
            FileSnapshot {
                cache_key: "before",
                contents: before,
                name: path,
            },
            FileSnapshot {
                cache_key: "after",
                contents: after,
                name: path,
            },
            FileComparisonOptions { context_radius: 0 },
        )
        .unwrap();
        file.runtime_id = id.into();
        file.flags.partial = false;
        file.set_sources(FileSourceSnapshots {
            old: Some(SourceSnapshot::new(
                before.into(),
                SourceOrigin::File { path: path.into() },
                true,
            )),
            new: Some(SourceSnapshot::new(
                after.into(),
                SourceOrigin::File { path: path.into() },
                true,
            )),
        });
        file
    }

    #[test]
    fn borrowed_fingerprint_preserves_every_serialized_byte_and_digest() {
        let mut plain = file("old\n", "new\n", "plain", "src/plain.ts");
        plain.sources = FileSourceSnapshots::default();
        plain.flags.partial = true;
        let mut unicode = file("old\r\n\"雪\"\n", "new\n😀\t\\\n", "unicode", "src/雪😀.ts");
        unicode.previous_path = Some("before/\"雪\".ts".into());
        unicode.patch = "patch\r\n\"\t😀\u{2028}\u{0085}".into();
        unicode.flags.binary = true;
        unicode.flags.too_large = true;
        unicode.stats.truncated = true;
        unicode.change_kind = workdeck_core::FileChangeKind::Renamed;
        let mut empty = plain.clone();
        empty.hunks.clear();
        empty.path.clear();
        empty.patch.clear();
        for file in [plain, unicode, empty] {
            let legacy = serde_json::json!({
                "path": file.path, "previousPath": file.previous_path,
                "changeKind": file.change_kind, "stats": file.stats, "flags": file.flags,
                "splitRowCount": file.split_row_count, "stackRowCount": file.stack_row_count,
                "hunks": file.hunks, "sources": file.sources,
            })
            .to_string();
            assert_eq!(highlight_fingerprint_metadata(&file), legacy);
            let input = format!(
                "{}:{}{}:{}",
                utf16_len(&file.patch),
                file.patch,
                utf16_len(&legacy),
                legacy
            );
            assert_eq!(
                highlighted_content_fingerprint(&file),
                review_digest(input.as_bytes())
            );
        }
    }

    fn theme() -> AppTheme {
        resolve_theme(Some("github-dark-default"), None, &[])
    }

    fn rendered_text(code: &HighlightedDiffCode) -> String {
        code.highlighted
            .iter()
            .flatten()
            .flat_map(|line| line.deletion.iter().chain(line.addition.iter()))
            .flatten()
            .map(|token| token.text.as_str())
            .collect()
    }

    fn dummy_code(text: &str) -> HighlightedDiffCode {
        HighlightedDiffCode::new(
            vec![vec![HighlightedDiffLine {
                deletion: None,
                addition: Some(vec![SyntaxToken {
                    text: text.into(),
                    foreground: SyntaxColor {
                        red: 1,
                        green: 2,
                        blue: 3,
                    },
                    bold: false,
                    italic: false,
                    underline: false,
                }]),
            }]],
            0,
            1,
        )
    }

    #[test]
    fn does_not_reuse_stale_highlights_for_full_patch_sampling_collisions() {
        let first_patch = format!("{}a{}", "x".repeat(96), "x".repeat(415));
        let second_patch = format!("{}b{}", "x".repeat(96), "x".repeat(415));
        let sampled = |patch: &str| {
            let mid = patch.len() / 2;
            format!(
                "{}:{}:{}:{}",
                patch.len(),
                &patch[..64],
                &patch[mid..mid + 64],
                &patch[patch.len() - 64..]
            )
        };
        assert_eq!(sampled(&first_patch), sampled(&second_patch));

        let mut first = file(
            "const marker = \"base\";\n",
            "const marker = \"one\";\n",
            "adversarial-patch-cache",
            "adversarial.ts",
        );
        first.patch = first_patch;
        let mut patch_only_change = first.clone();
        patch_only_change.patch = second_patch.clone();
        let mut second = file(
            "const marker = \"base\";\n",
            "const marker = \"two\";\n",
            "adversarial-patch-cache",
            "adversarial.ts",
        );
        second.patch = second_patch;
        let theme = theme();
        assert_ne!(
            highlighted_diff_cache_key(&theme, &first),
            highlighted_diff_cache_key(&theme, &patch_only_change)
        );
        assert_ne!(
            highlighted_diff_cache_key(&theme, &first),
            highlighted_diff_cache_key(&theme, &second)
        );

        let mut runtime = HighlightedDiffRuntime::default();
        let first_highlight = runtime
            .prefetch_highlighted_diff(&first, &theme, false)
            .unwrap();
        let second_highlight = runtime
            .prefetch_highlighted_diff(&second, &theme, false)
            .unwrap();
        let first_text = rendered_text(&first_highlight);
        let second_text = rendered_text(&second_highlight);
        assert!(first_text.contains("one"));
        assert!(second_text.contains("two"));
        assert!(!second_text.contains("one"));
    }

    #[test]
    fn unversioned_partial_source_providers_have_instance_identity() {
        let mut file = file("old\n", "new\n", "cache", "cache.ts");
        file.flags.partial = true;
        file.sources = FileSourceSnapshots::default();
        file.source_identity = None;
        let theme = theme();
        let first = HighlightSourceProviderIdentity::Unversioned(1);
        let second = HighlightSourceProviderIdentity::Unversioned(2);
        assert_eq!(
            highlighted_diff_cache_key_with_provider(&theme, &file, &first),
            highlighted_diff_cache_key_with_provider(&theme, &file, &first)
        );
        assert_ne!(
            highlighted_diff_cache_key_with_provider(&theme, &file, &first),
            highlighted_diff_cache_key_with_provider(&theme, &file, &second)
        );
        file.flags.partial = false;
        assert_eq!(
            highlighted_diff_cache_key_with_provider(
                &theme,
                &file,
                &HighlightSourceProviderIdentity::PatchOnly,
            ),
            highlighted_diff_cache_key_with_provider(
                &theme,
                &file,
                &HighlightSourceProviderIdentity::PatchOnly,
            )
        );
    }

    #[test]
    fn inline_is_default_and_worker_offload_requires_explicit_request() {
        let contents = (0..40)
            .map(|index| format!("export const line{index} = {index};"))
            .collect::<Vec<_>>()
            .join("\n");
        let large = file("", &format!("{contents}\n"), "large", "large.ts");
        let theme = theme();
        let mut inline = HighlightedDiffRuntime::default();
        assert!(
            inline
                .prefetch_highlighted_diff(&large, &theme, false)
                .is_some()
        );

        let mut offloaded = HighlightedDiffRuntime::default();
        assert!(
            offloaded
                .prefetch_highlighted_diff(&large, &theme, true)
                .is_none()
        );
        let key = highlighted_diff_cache_key(&theme, &large);
        assert!(offloaded.coordinator.is_in_flight(&key));
        assert!(matches!(
            offloaded.coordinator.begin(&key),
            HighlightRequestState::Existing(_)
        ));
        let mut settled = None;
        for _ in 0..10_000 {
            settled = offloaded.prefetch_highlighted_diff(&large, &theme, true);
            if settled.is_some() {
                break;
            }
            std::thread::yield_now();
        }
        assert!(settled.is_some(), "native highlight worker did not settle");
    }

    #[test]
    fn retryable_and_stale_completions_never_poison_shared_cache() {
        let mut coordinator = HighlightedDiffCoordinator::default();
        let HighlightRequestState::Started(first_id) = coordinator.begin("key") else {
            panic!("first request must start");
        };
        let retryable = HighlightCompletion {
            code: dummy_code("plain"),
            retryable: true,
        };
        assert!(coordinator.commit("key", first_id, &retryable));
        assert!(coordinator.peek("key").is_none());

        let HighlightRequestState::Started(second_id) = coordinator.begin("key") else {
            panic!("retry must start a new request");
        };
        assert_ne!(first_id, second_id);
        let completed = HighlightCompletion {
            code: dummy_code("highlighted"),
            retryable: false,
        };
        assert!(!coordinator.commit("key", first_id, &completed));
        assert!(coordinator.peek("key").is_none());
        assert!(coordinator.commit("key", second_id, &completed));
        assert_eq!(coordinator.peek("key"), Some(&completed.code));
    }

    #[test]
    fn equivalent_versioned_sources_reuse_keys_and_changed_versions_do_not() {
        let file = file("old\n", "new\n", "cache", "cache.ts");
        let theme = theme();
        let first = HighlightSourceProviderIdentity::Versioned("snapshot-1".into());
        let equivalent = HighlightSourceProviderIdentity::Versioned("snapshot-1".into());
        let changed = HighlightSourceProviderIdentity::Versioned("snapshot-2".into());
        assert_eq!(
            highlighted_diff_cache_key_with_provider(&theme, &file, &first),
            highlighted_diff_cache_key_with_provider(&theme, &file, &equivalent)
        );
        assert_ne!(
            highlighted_diff_cache_key_with_provider(&theme, &file, &first),
            highlighted_diff_cache_key_with_provider(&theme, &file, &changed)
        );
    }

    #[test]
    fn render_snapshot_reads_are_side_effect_free_and_current_state_wins() {
        let mut coordinator = HighlightedDiffCoordinator::default();
        let HighlightRequestState::Started(id) = coordinator.begin("cached") else {
            panic!("request must start");
        };
        let cached = HighlightCompletion {
            code: dummy_code("cached"),
            retryable: false,
        };
        assert!(coordinator.commit("cached", id, &cached));
        assert_eq!(
            resolve_highlighted_snapshot(&coordinator, Some("cached"), None, None),
            Some(cached.code.clone())
        );
        let current = dummy_code("current");
        assert_eq!(
            resolve_highlighted_snapshot(
                &coordinator,
                Some("cached"),
                Some(&current),
                Some("cached"),
            ),
            Some(current)
        );
        assert_eq!(
            resolve_highlighted_snapshot(&coordinator, None, Some(&cached.code), Some("cached")),
            None
        );
    }

    #[test]
    fn source_provider_key_is_derived_from_native_snapshot_identity() {
        let mut file = parse_patch(
            "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n",
            "patch",
            "Patch",
            ChangesetSource::Patch {
                label: "patch".into(),
            },
        )
        .unwrap()
        .files
        .remove(0);
        let theme = theme();
        let patch_only = highlighted_diff_cache_key(&theme, &file);
        file.set_sources(FileSourceSnapshots {
            old: Some(SourceSnapshot::new(
                "old\n".into(),
                SourceOrigin::Revision {
                    revision: "old".into(),
                },
                true,
            )),
            new: Some(SourceSnapshot::new(
                "new\n".into(),
                SourceOrigin::WorkingTree,
                true,
            )),
        });
        assert_ne!(patch_only, highlighted_diff_cache_key(&theme, &file));
    }

    #[test]
    fn frozen_baseline_cache_key_vectors_are_retained() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/highlighted-diff-runtime.json"
        ))
        .unwrap();
        let vectors = &oracle["baselineCacheKeyVectors"];
        assert_eq!(
            vectors["first"],
            "github-dark-default:github-dark-default:adversarial-patch-cache:typescript:\
             e26f0da38cbc67dfc330e6738c8fad1a41b4578f621646f5ef2a6b42743e0ba1:patch-only"
                .replace(char::is_whitespace, "")
        );
        assert_ne!(vectors["first"], vectors["patchChanged"]);
        assert_eq!(vectors["versioned"], vectors["versionedEquivalent"]);
        assert_ne!(vectors["versioned"], vectors["versionedChanged"]);
    }
}
