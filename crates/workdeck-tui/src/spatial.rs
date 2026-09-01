use std::collections::HashMap;

/// One vertical extent measured in terminal rows within one coordinate space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerticalBounds {
    pub top: usize,
    pub height: usize,
}

/// One selected column extent on a row, in inclusive review-stream columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CopySelectedRowRange {
    pub start_col: usize,
    pub end_col: usize,
}

/// Shared geometry for one file section body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionGeometry<THunkBounds> {
    pub body_height: usize,
    pub hunk_anchor_rows: HashMap<usize, usize>,
    pub hunk_bounds: HashMap<usize, THunkBounds>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spatial_models_retain_inclusive_columns_and_one_coordinate_space() {
        let geometry = SectionGeometry {
            body_height: 12,
            hunk_anchor_rows: HashMap::from([(0, 3)]),
            hunk_bounds: HashMap::from([(0, VerticalBounds { top: 3, height: 4 })]),
        };
        assert_eq!(geometry.hunk_anchor_rows[&0], 3);
        assert_eq!(
            geometry.hunk_bounds[&0],
            VerticalBounds { top: 3, height: 4 }
        );
        assert_eq!(
            CopySelectedRowRange {
                start_col: 2,
                end_col: 4,
            },
            CopySelectedRowRange {
                start_col: 2,
                end_col: 4,
            }
        );
    }
}
