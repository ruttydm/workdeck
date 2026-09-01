//! Immutable line-highlight maps consumed by the review painter.

use std::collections::BTreeMap;
use std::sync::Arc;
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

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_core::ReviewSide;
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
}
