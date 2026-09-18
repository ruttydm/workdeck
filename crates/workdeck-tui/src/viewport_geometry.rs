//! Measured review-section geometry consumed by viewport anchoring and selection.

use std::collections::HashMap;

use crate::VerticalBounds;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewportRowBounds {
    pub key: String,
    pub stable_key: String,
    pub stable_keys: Vec<String>,
    pub expanded_gap_key: Option<String>,
    pub top: usize,
    pub height: usize,
}

/// The viewport-facing subset of one measured diff section.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ViewportSectionGeometry {
    pub body_height: usize,
    pub hunk_bounds: HashMap<usize, VerticalBounds>,
    pub row_bounds: Vec<ViewportRowBounds>,
    row_bounds_by_key: HashMap<String, usize>,
    row_bounds_by_stable_key: HashMap<String, usize>,
}

impl ViewportSectionGeometry {
    #[must_use]
    pub fn new(
        body_height: usize,
        row_bounds: Vec<ViewportRowBounds>,
        hunk_bounds: HashMap<usize, VerticalBounds>,
    ) -> Self {
        let mut row_bounds_by_key = HashMap::with_capacity(row_bounds.len());
        let mut row_bounds_by_stable_key = HashMap::with_capacity(row_bounds.len());
        for (index, bounds) in row_bounds.iter().enumerate() {
            // Hunk's key map is last-write-wins, while aliases retain their first owner.
            row_bounds_by_key.insert(bounds.key.clone(), index);
            for stable_key in &bounds.stable_keys {
                row_bounds_by_stable_key
                    .entry(stable_key.clone())
                    .or_insert(index);
            }
        }
        Self {
            body_height,
            hunk_bounds,
            row_bounds,
            row_bounds_by_key,
            row_bounds_by_stable_key,
        }
    }

    #[must_use]
    pub fn bounds_for_key(&self, key: &str) -> Option<&ViewportRowBounds> {
        self.row_bounds_by_key
            .get(key)
            .and_then(|index| self.row_bounds.get(*index))
    }

    #[must_use]
    pub fn bounds_for_stable_key(&self, stable_key: &str) -> Option<&ViewportRowBounds> {
        self.row_bounds_by_stable_key
            .get(stable_key)
            .and_then(|index| self.row_bounds.get(*index))
    }
}
