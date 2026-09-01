//! Placement-aware native extension pane sizing.

use crate::{PanePlacement, PaneRegistration};
use serde::{Deserialize, Serialize};

/// Requested pane width or height along its docked edge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionPaneSize {
    /// Fixed-cell target and fallback when no responsive fraction is present.
    pub preferred: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fraction: Option<f64>,
}

impl ExtensionPaneSize {
    #[must_use]
    pub const fn fixed(preferred: u16) -> Self {
        Self {
            preferred,
            min: None,
            max: None,
            fraction: None,
        }
    }
}

pub const DEFAULT_VERTICAL_PANE_WIDTH: ExtensionPaneSize = ExtensionPaneSize {
    preferred: 34,
    min: Some(22),
    max: None,
    fraction: None,
};
pub const DEFAULT_HORIZONTAL_PANE_HEIGHT: ExtensionPaneSize = ExtensionPaneSize {
    preferred: 8,
    min: Some(3),
    max: None,
    fraction: None,
};

/// Report whether placement occupies a vertical edge and is width-sized.
#[must_use]
pub const fn is_vertical_pane_placement(placement: PanePlacement) -> bool {
    matches!(placement, PanePlacement::Left | PanePlacement::Right)
}

/// Resolve the host default for the dimension implied by placement.
#[must_use]
pub const fn default_extension_pane_size(placement: PanePlacement) -> ExtensionPaneSize {
    if is_vertical_pane_placement(placement) {
        DEFAULT_VERTICAL_PANE_WIDTH
    } else {
        DEFAULT_HORIZONTAL_PANE_HEIGHT
    }
}

/// Read the placement-appropriate request, retaining the former Workdeck fixed
/// size as a compatibility fallback before applying Hunk's defaults.
#[must_use]
pub fn extension_pane_size(
    pane: &PaneRegistration,
    placement: Option<PanePlacement>,
) -> ExtensionPaneSize {
    let placement = placement.unwrap_or(pane.placement);
    let requested = if is_vertical_pane_placement(placement) {
        pane.width.as_ref()
    } else {
        pane.height.as_ref()
    };
    requested.cloned().unwrap_or_else(|| {
        pane.preferred_size
            .map(ExtensionPaneSize::fixed)
            .unwrap_or_else(|| default_extension_pane_size(placement))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(placement: PanePlacement) -> PaneRegistration {
        PaneRegistration {
            id: "probe".into(),
            title: "Probe".into(),
            placement,
            preferred_size: None,
            width: None,
            height: None,
        }
    }

    #[test]
    fn placement_selects_hunks_exact_default_dimension() {
        assert_eq!(
            default_extension_pane_size(PanePlacement::Left),
            ExtensionPaneSize {
                preferred: 34,
                min: Some(22),
                max: None,
                fraction: None,
            }
        );
        assert_eq!(
            default_extension_pane_size(PanePlacement::Bottom),
            ExtensionPaneSize {
                preferred: 8,
                min: Some(3),
                max: None,
                fraction: None,
            }
        );
    }

    #[test]
    fn only_the_dimension_matching_effective_placement_is_read() {
        let mut pane = pane(PanePlacement::Left);
        pane.width = Some(ExtensionPaneSize {
            preferred: 40,
            min: Some(20),
            max: Some(60),
            fraction: Some(0.25),
        });
        pane.height = Some(ExtensionPaneSize::fixed(11));
        assert_eq!(
            extension_pane_size(&pane, None),
            pane.width.clone().unwrap()
        );
        assert_eq!(
            extension_pane_size(&pane, Some(PanePlacement::Top)),
            pane.height.clone().unwrap()
        );
    }

    #[test]
    fn legacy_fixed_size_precedes_the_placement_default() {
        let mut pane = pane(PanePlacement::Right);
        pane.preferred_size = Some(29);
        assert_eq!(
            extension_pane_size(&pane, None),
            ExtensionPaneSize::fixed(29)
        );
    }
}
