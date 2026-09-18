//! Cursor paint inputs matched against stable planned-row identities.

use workdeck_core::ReviewSide;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorHighlightStyle {
    Row,
    Number,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorHighlight {
    /// Render-plan anchor shared with reveal and viewport lookups.
    pub stable_key: String,
    pub style: CursorHighlightStyle,
    /// Split-row half carrying the cursor and any note created from it.
    pub side: ReviewSide,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedRowIdentity {
    pub stable_key: String,
    pub stable_alias_keys: Vec<String>,
}

/// Report whether one planned row carries the stable anchor under the cursor.
#[must_use]
pub fn planned_row_matches_cursor(
    row: &PlannedRowIdentity,
    cursor: Option<&CursorHighlight>,
) -> bool {
    cursor.is_some_and(|cursor| {
        row.stable_key == cursor.stable_key
            || row
                .stable_alias_keys
                .iter()
                .any(|alias| alias == &cursor.stable_key)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cursor() -> CursorHighlight {
        CursorHighlight {
            stable_key: "line:new:2".into(),
            style: CursorHighlightStyle::Row,
            side: ReviewSide::New,
        }
    }

    #[test]
    fn matches_canonical_and_alias_stable_keys() {
        let cursor = cursor();
        assert!(planned_row_matches_cursor(
            &PlannedRowIdentity {
                stable_key: cursor.stable_key.clone(),
                stable_alias_keys: Vec::new(),
            },
            Some(&cursor),
        ));
        assert!(planned_row_matches_cursor(
            &PlannedRowIdentity {
                stable_key: "line:old:2".into(),
                stable_alias_keys: vec![cursor.stable_key.clone()],
            },
            Some(&cursor),
        ));
    }

    #[test]
    fn rejects_absent_and_unrelated_cursors() {
        let cursor = cursor();
        assert!(!planned_row_matches_cursor(
            &PlannedRowIdentity {
                stable_key: "line:new:3".into(),
                stable_alias_keys: Vec::new(),
            },
            Some(&cursor),
        ));
        assert!(!planned_row_matches_cursor(
            &PlannedRowIdentity {
                stable_key: cursor.stable_key.clone(),
                stable_alias_keys: Vec::new(),
            },
            None,
        ));
    }
}
