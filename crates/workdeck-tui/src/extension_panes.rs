//! Pure terminal-cell planning for native extension panes.

use ratatui::layout::Rect;
use std::collections::{BTreeMap, BTreeSet};
use workdeck_extension_api::{PanePlacement, PaneRegistration, extension_pane_size};

/// One cell between a resizable edge pane and its neighbor.
pub const EXTENSION_PANE_DIVIDER_SIZE: u16 = 1;
/// Smallest review height preserved while edge panes are open or resized.
pub const MIN_EXTENSION_REVIEW_HEIGHT: u16 = 5;

#[derive(Debug, Clone, PartialEq)]
pub struct ExtensionPaneSpec {
    pub key: String,
    pub pane: PaneRegistration,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlannedExtensionPane {
    pub key: String,
    pub pane: PaneRegistration,
    pub bounds: Rect,
    pub divider: Option<Rect>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExtensionPaneLayoutPlan {
    pub panes: Vec<PlannedExtensionPane>,
    pub review_bounds: Rect,
    pub omitted_keys: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
struct AxisSize {
    target: u16,
    min: u16,
    max: u16,
    fixed: bool,
}

fn axis_size(spec: &ExtensionPaneSpec, area: Rect, overrides: &BTreeMap<String, u16>) -> AxisSize {
    let requested = extension_pane_size(&spec.pane, None);
    let min = requested.min.unwrap_or(1);
    let max = requested.max.unwrap_or(u16::MAX);
    let axis = if matches!(
        spec.pane.placement,
        PanePlacement::Left | PanePlacement::Right
    ) {
        area.width
    } else {
        area.height
    };
    let automatic = requested.fraction.map_or(requested.preferred, |fraction| {
        (f64::from(axis) * fraction)
            .round()
            .clamp(0.0, f64::from(u16::MAX)) as u16
    });
    AxisSize {
        target: overrides.get(&spec.key).copied().unwrap_or(automatic),
        min,
        max,
        fixed: min == max,
    }
}

/// Plan exact pane, divider, and remaining-review rectangles without invoking extension code.
#[must_use]
pub fn plan_extension_panes(
    panes: &[ExtensionPaneSpec],
    open_keys: &BTreeSet<String>,
    overrides: &BTreeMap<String, u16>,
    area: Rect,
    min_review_width: u16,
    min_review_height: u16,
) -> ExtensionPaneLayoutPlan {
    let mut left = area.x;
    let mut right = area.right();
    let mut top = area.y;
    let mut bottom = area.bottom();
    let mut planned = BTreeMap::new();
    let mut omitted_keys = Vec::new();

    for spec in panes.iter().filter(|spec| {
        open_keys.contains(&spec.key)
            && matches!(
                spec.pane.placement,
                PanePlacement::Left | PanePlacement::Right
            )
    }) {
        let size = axis_size(spec, area, overrides);
        let divider_size = if size.fixed {
            0
        } else {
            EXTENSION_PANE_DIVIDER_SIZE
        };
        let remaining = right
            .saturating_sub(left)
            .saturating_sub(min_review_width)
            .saturating_sub(divider_size);
        let width = size.target.max(size.min).min(size.max).min(remaining);
        if width < size.min {
            omitted_keys.push(spec.key.clone());
            continue;
        }
        let (bounds, divider) = match spec.pane.placement {
            PanePlacement::Left => {
                let bounds = Rect::new(left, area.y, width, area.height);
                let divider = (divider_size > 0)
                    .then(|| Rect::new(left.saturating_add(width), area.y, 1, area.height));
                left = left.saturating_add(width).saturating_add(divider_size);
                (bounds, divider)
            }
            PanePlacement::Right => {
                let x = right.saturating_sub(width);
                let bounds = Rect::new(x, area.y, width, area.height);
                let divider = (divider_size > 0)
                    .then(|| Rect::new(x.saturating_sub(1), area.y, 1, area.height));
                right = right.saturating_sub(width).saturating_sub(divider_size);
                (bounds, divider)
            }
            PanePlacement::Top | PanePlacement::Bottom => unreachable!(),
        };
        planned.insert(
            spec.key.clone(),
            PlannedExtensionPane {
                key: spec.key.clone(),
                pane: spec.pane.clone(),
                bounds,
                divider,
            },
        );
    }

    for spec in panes.iter().filter(|spec| {
        open_keys.contains(&spec.key)
            && matches!(
                spec.pane.placement,
                PanePlacement::Top | PanePlacement::Bottom
            )
    }) {
        let size = axis_size(spec, area, overrides);
        let divider_size = if size.fixed {
            0
        } else {
            EXTENSION_PANE_DIVIDER_SIZE
        };
        let remaining = bottom
            .saturating_sub(top)
            .saturating_sub(min_review_height)
            .saturating_sub(divider_size);
        let height = size.target.max(size.min).min(size.max).min(remaining);
        if height < size.min {
            omitted_keys.push(spec.key.clone());
            continue;
        }
        let width = right.saturating_sub(left);
        let (bounds, divider) = match spec.pane.placement {
            PanePlacement::Top => {
                let bounds = Rect::new(left, top, width, height);
                let divider = (divider_size > 0)
                    .then(|| Rect::new(left, top.saturating_add(height), width, 1));
                top = top.saturating_add(height).saturating_add(divider_size);
                (bounds, divider)
            }
            PanePlacement::Bottom => {
                let y = bottom.saturating_sub(height);
                let bounds = Rect::new(left, y, width, height);
                let divider =
                    (divider_size > 0).then(|| Rect::new(left, y.saturating_sub(1), width, 1));
                bottom = bottom.saturating_sub(height).saturating_sub(divider_size);
                (bounds, divider)
            }
            PanePlacement::Left | PanePlacement::Right => unreachable!(),
        };
        planned.insert(
            spec.key.clone(),
            PlannedExtensionPane {
                key: spec.key.clone(),
                pane: spec.pane.clone(),
                bounds,
                divider,
            },
        );
    }

    ExtensionPaneLayoutPlan {
        panes: panes
            .iter()
            .filter_map(|spec| planned.remove(&spec.key))
            .collect(),
        review_bounds: Rect::new(
            left,
            top,
            right.saturating_sub(left),
            bottom.saturating_sub(top),
        ),
        omitted_keys,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_extension_api::ExtensionPaneSize;

    fn pane(id: &str, placement: PanePlacement, size: ExtensionPaneSize) -> ExtensionPaneSpec {
        ExtensionPaneSpec {
            key: format!("demo:{id}"),
            pane: PaneRegistration {
                id: id.into(),
                title: id.into(),
                placement,
                default_open: false,
                preferred_size: None,
                width: matches!(placement, PanePlacement::Left | PanePlacement::Right)
                    .then_some(size.clone()),
                height: matches!(placement, PanePlacement::Top | PanePlacement::Bottom)
                    .then_some(size),
            },
        }
    }

    #[test]
    fn plans_vertical_edges_before_horizontal_edges_and_preserves_registration_order() {
        let panes = vec![
            pane(
                "right",
                PanePlacement::Right,
                ExtensionPaneSize {
                    preferred: 28,
                    min: Some(18),
                    max: Some(44),
                    fraction: None,
                },
            ),
            pane(
                "top",
                PanePlacement::Top,
                ExtensionPaneSize {
                    preferred: 2,
                    min: Some(2),
                    max: Some(2),
                    fraction: None,
                },
            ),
            pane(
                "bottom",
                PanePlacement::Bottom,
                ExtensionPaneSize {
                    preferred: 2,
                    min: Some(2),
                    max: Some(2),
                    fraction: None,
                },
            ),
        ];
        let open = panes.iter().map(|pane| pane.key.clone()).collect();
        let plan = plan_extension_panes(
            &panes,
            &open,
            &BTreeMap::new(),
            Rect::new(4, 3, 100, 20),
            20,
            MIN_EXTENSION_REVIEW_HEIGHT,
        );
        assert_eq!(plan.review_bounds, Rect::new(4, 5, 71, 16));
        assert_eq!(plan.panes[0].bounds, Rect::new(76, 3, 28, 20));
        assert_eq!(plan.panes[0].divider, Some(Rect::new(75, 3, 1, 20)));
        assert_eq!(plan.panes[1].bounds, Rect::new(4, 3, 71, 2));
        assert_eq!(plan.panes[1].divider, None);
        assert_eq!(plan.panes[2].bounds, Rect::new(4, 21, 71, 2));
    }

    #[test]
    fn omits_a_pane_that_would_violate_the_review_minimum() {
        let spec = pane(
            "side",
            PanePlacement::Right,
            ExtensionPaneSize {
                preferred: 28,
                min: Some(18),
                max: Some(44),
                fraction: None,
            },
        );
        let open = BTreeSet::from([spec.key.clone()]);
        let plan = plan_extension_panes(
            &[spec],
            &open,
            &BTreeMap::new(),
            Rect::new(0, 0, 38, 10),
            20,
            5,
        );
        assert_eq!(plan.omitted_keys, ["demo:side"]);
        assert!(plan.panes.is_empty());
        assert_eq!(plan.review_bounds, Rect::new(0, 0, 38, 10));
    }
}
