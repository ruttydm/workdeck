//! Immutable line-highlight maps consumed by the review painter.

use std::collections::BTreeMap;
use std::sync::Arc;
use workdeck_core::Changeset;
use workdeck_extension_api::ValidatedLineHighlight;

#[derive(Debug, Clone, Default)]
pub struct LineHighlightMap(Arc<BTreeMap<String, Arc<[ValidatedLineHighlight]>>>);

impl LineHighlightMap {
    #[must_use]
    pub fn from_entries(
        entries: impl IntoIterator<Item = (String, Vec<ValidatedLineHighlight>)>,
    ) -> Self {
        Self(Arc::new(
            entries
                .into_iter()
                .map(|(file_id, marks)| (file_id, Arc::from(marks)))
                .collect(),
        ))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    pub fn get(&self, file_id: &str) -> Option<&[ValidatedLineHighlight]> {
        self.0.get(file_id).map(AsRef::as_ref)
    }

    #[must_use]
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// Merge base and overlay marks in paint order without mutating either input.
///
/// Overlay marks append after base marks, so overlaps paint last. If either
/// side is empty the other map allocation is retained unchanged, preserving
/// downstream row-memoization identity.
#[must_use]
pub fn merge_line_highlight_maps(
    base: &LineHighlightMap,
    overlay: &LineHighlightMap,
) -> LineHighlightMap {
    if overlay.is_empty() {
        return base.clone();
    }
    if base.is_empty() {
        return overlay.clone();
    }

    let mut merged = (*base.0).clone();
    for (file_id, marks) in overlay.0.iter() {
        if let Some(existing) = merged.get(file_id) {
            let mut combined = Vec::with_capacity(existing.len() + marks.len());
            combined.extend_from_slice(existing);
            combined.extend_from_slice(marks);
            merged.insert(file_id.clone(), Arc::from(combined));
        } else {
            merged.insert(file_id.clone(), Arc::clone(marks));
        }
    }
    LineHighlightMap(Arc::new(merged))
}

/// Carry agent attention marks across one immutable review reload.
///
/// Stable file keys locate replacements. Exact content identity is required
/// before the existing mark allocation is re-keyed to the new runtime ID.
/// Removed or changed files lose their marks instead of painting stale text.
#[must_use]
pub fn carry_over_line_highlights(
    marks_by_file_id: &LineHighlightMap,
    previous: &Changeset,
    next: &Changeset,
) -> LineHighlightMap {
    let mut carried = BTreeMap::new();
    if marks_by_file_id.is_empty() {
        return LineHighlightMap(Arc::new(carried));
    }

    let next_by_key = next
        .files
        .iter()
        .map(|file| (file.key.as_str(), file))
        .collect::<BTreeMap<_, _>>();
    for file in &previous.files {
        let Some(marks) = marks_by_file_id.0.get(&file.runtime_id) else {
            continue;
        };
        if marks.is_empty() {
            continue;
        }
        let Some(replacement) = next_by_key.get(file.key.as_str()) else {
            continue;
        };
        if replacement.content_identity != file.content_identity {
            continue;
        }
        carried.insert(replacement.runtime_id.clone(), Arc::clone(marks));
    }
    LineHighlightMap(Arc::new(carried))
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_core::{ChangesetSource, ReviewSide};
    use workdeck_diff::parse_patch;
    use workdeck_extension_api::HighlightTone;

    fn mark(line: u64, start: u64, end: u64) -> ValidatedLineHighlight {
        ValidatedLineHighlight {
            side: ReviewSide::New,
            line,
            start,
            end,
            tone: HighlightTone::Match,
        }
    }

    fn reloaded_document(files: &[(&str, Option<&str>)], generation: &str) -> Changeset {
        let mut parsed = parse_patch(
            "diff --git a/sample.rs b/sample.rs\n--- a/sample.rs\n+++ b/sample.rs\n@@ -1 +1 @@\n-old\n+new\n",
            format!("reload-{generation}"),
            "reload",
            ChangesetSource::Patch {
                label: "reload".into(),
            },
        )
        .unwrap();
        let template = parsed.files.remove(0);
        parsed.files = files
            .iter()
            .map(|(key, content_identity)| {
                let mut file = template.clone();
                file.key = (*key).into();
                file.runtime_id = format!("{key}:{generation}");
                file.content_identity = content_identity
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("content:{key}"));
                file
            })
            .collect();
        parsed
    }

    #[test]
    fn returns_either_side_unchanged_when_the_other_is_empty() {
        let base = LineHighlightMap::from_entries([("file-1".into(), vec![mark(1, 0, 4)])]);
        let empty = LineHighlightMap::default();

        assert!(merge_line_highlight_maps(&base, &empty).ptr_eq(&base));
        assert!(merge_line_highlight_maps(&empty, &base).ptr_eq(&base));
    }

    #[test]
    fn appends_overlay_marks_after_base_without_mutating_inputs() {
        let base = LineHighlightMap::from_entries([("file-1".into(), vec![mark(1, 0, 4)])]);
        let overlay = LineHighlightMap::from_entries([
            ("file-1".into(), vec![mark(1, 2, 6)]),
            ("file-2".into(), vec![mark(3, 0, 2)]),
        ]);

        let merged = merge_line_highlight_maps(&base, &overlay);
        assert_eq!(
            merged.get("file-1"),
            Some([mark(1, 0, 4), mark(1, 2, 6)].as_slice())
        );
        assert_eq!(merged.get("file-2"), Some([mark(3, 0, 2)].as_slice()));
        assert_eq!(base.get("file-1").map(<[_]>::len), Some(1));
        assert_eq!(overlay.get("file-1").map(<[_]>::len), Some(1));
    }

    #[test]
    fn reload_rekeys_marks_when_content_is_unchanged_and_preserves_mark_identity() {
        let previous = reloaded_document(&[("alpha", None), ("beta", None)], "1");
        let next = reloaded_document(&[("alpha", None), ("beta", None)], "2");
        let marks = LineHighlightMap::from_entries([("alpha:1".into(), vec![mark(1, 0, 4)])]);

        let carried = carry_over_line_highlights(&marks, &previous, &next);
        assert_eq!(carried.get("alpha:2"), marks.get("alpha:1"));
        assert!(Arc::ptr_eq(
            carried.0.get("alpha:2").unwrap(),
            marks.0.get("alpha:1").unwrap(),
        ));
    }

    #[test]
    fn reload_drops_marks_when_content_changes() {
        let previous = reloaded_document(&[("alpha", None)], "1");
        let next = reloaded_document(&[("alpha", Some("content:changed"))], "2");
        let marks = LineHighlightMap::from_entries([("alpha:1".into(), vec![mark(1, 0, 4)])]);

        assert!(carry_over_line_highlights(&marks, &previous, &next).is_empty());
    }

    #[test]
    fn reload_drops_removed_files_and_keeps_surviving_files() {
        let previous = reloaded_document(&[("alpha", None), ("beta", None)], "1");
        let next = reloaded_document(&[("beta", None)], "2");
        let marks = LineHighlightMap::from_entries([
            ("alpha:1".into(), vec![mark(1, 0, 4)]),
            ("beta:1".into(), vec![mark(1, 0, 4)]),
        ]);

        let carried = carry_over_line_highlights(&marks, &previous, &next);
        assert_eq!(carried.len(), 1);
        assert_eq!(carried.get("beta:2"), Some([mark(1, 0, 4)].as_slice()));
    }

    #[test]
    fn reload_returns_an_empty_map_when_there_is_nothing_to_carry() {
        let previous = reloaded_document(&[("alpha", None)], "1");
        let next = reloaded_document(&[("alpha", None)], "2");
        assert!(
            carry_over_line_highlights(&LineHighlightMap::default(), &previous, &next).is_empty()
        );
    }
}
