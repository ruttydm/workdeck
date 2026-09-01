//! Responsive viewport policy for the unified review canvas.

use crate::LayoutMode;

const AUTO_SPLIT_MIN_WIDTH: u16 = 120;
const SIDEBAR_VIEWPORT_MIN_WIDTH: u16 = 160;
const FULL_VIEWPORT_MIN_WIDTH: u16 = 220;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponsiveViewport {
    Full,
    Medium,
    Tight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResponsiveLayout {
    pub viewport: ResponsiveViewport,
    pub layout: LayoutMode,
    pub show_sidebar: bool,
}

/// Resolve layout and sidebar visibility from independent terminal-width thresholds.
#[must_use]
pub const fn resolve_responsive_layout(
    requested_layout: LayoutMode,
    viewport_width: u16,
) -> ResponsiveLayout {
    let viewport = if viewport_width >= FULL_VIEWPORT_MIN_WIDTH {
        ResponsiveViewport::Full
    } else if viewport_width >= SIDEBAR_VIEWPORT_MIN_WIDTH {
        ResponsiveViewport::Medium
    } else {
        ResponsiveViewport::Tight
    };
    let layout = match requested_layout {
        LayoutMode::Auto if viewport_width >= AUTO_SPLIT_MIN_WIDTH => LayoutMode::Split,
        LayoutMode::Auto => LayoutMode::Stack,
        explicit => explicit,
    };
    ResponsiveLayout {
        viewport,
        layout,
        show_sidebar: !matches!(viewport, ResponsiveViewport::Tight),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIGHT_WIDTH: u16 = 100;
    const NARROW_SPLIT_WIDTH: u16 = 140;
    const MEDIUM_WIDTH: u16 = 180;
    const FULL_WIDTH: u16 = 240;

    #[test]
    fn auto_chooses_stack_without_sidebar_on_tight_terminals() {
        assert_eq!(
            resolve_responsive_layout(LayoutMode::Auto, TIGHT_WIDTH),
            ResponsiveLayout {
                viewport: ResponsiveViewport::Tight,
                layout: LayoutMode::Stack,
                show_sidebar: false,
            }
        );
    }

    #[test]
    fn auto_keeps_split_after_sidebar_disappears_on_narrow_terminals() {
        assert_eq!(
            resolve_responsive_layout(LayoutMode::Auto, NARROW_SPLIT_WIDTH),
            ResponsiveLayout {
                viewport: ResponsiveViewport::Tight,
                layout: LayoutMode::Split,
                show_sidebar: false,
            }
        );
    }

    #[test]
    fn auto_chooses_split_with_compact_sidebar_on_medium_terminals() {
        assert_eq!(
            resolve_responsive_layout(LayoutMode::Auto, MEDIUM_WIDTH),
            ResponsiveLayout {
                viewport: ResponsiveViewport::Medium,
                layout: LayoutMode::Split,
                show_sidebar: true,
            }
        );
    }

    #[test]
    fn auto_chooses_split_with_sidebar_on_full_terminals() {
        assert_eq!(
            resolve_responsive_layout(LayoutMode::Auto, FULL_WIDTH),
            ResponsiveLayout {
                viewport: ResponsiveViewport::Full,
                layout: LayoutMode::Split,
                show_sidebar: true,
            }
        );
    }

    #[test]
    fn explicit_split_survives_tight_terminal_while_sidebar_hides() {
        assert_eq!(
            resolve_responsive_layout(LayoutMode::Split, TIGHT_WIDTH),
            ResponsiveLayout {
                viewport: ResponsiveViewport::Tight,
                layout: LayoutMode::Split,
                show_sidebar: false,
            }
        );
    }

    #[test]
    fn explicit_stack_survives_full_terminal_while_sidebar_shows() {
        assert_eq!(
            resolve_responsive_layout(LayoutMode::Stack, FULL_WIDTH),
            ResponsiveLayout {
                viewport: ResponsiveViewport::Full,
                layout: LayoutMode::Stack,
                show_sidebar: true,
            }
        );
    }

    #[test]
    fn explicit_modes_show_sidebar_at_medium_and_full_widths() {
        for (mode, width) in [
            (LayoutMode::Split, MEDIUM_WIDTH),
            (LayoutMode::Stack, MEDIUM_WIDTH),
            (LayoutMode::Split, FULL_WIDTH),
        ] {
            assert!(resolve_responsive_layout(mode, width).show_sidebar);
        }
    }

    #[test]
    fn viewport_buckets_change_exactly_at_medium_and_full_minimums() {
        assert_eq!(
            resolve_responsive_layout(LayoutMode::Auto, 159).viewport,
            ResponsiveViewport::Tight
        );
        assert_eq!(
            resolve_responsive_layout(LayoutMode::Auto, 160).viewport,
            ResponsiveViewport::Medium
        );
        assert_eq!(
            resolve_responsive_layout(LayoutMode::Auto, 219).viewport,
            ResponsiveViewport::Medium
        );
        assert_eq!(
            resolve_responsive_layout(LayoutMode::Auto, 220).viewport,
            ResponsiveViewport::Full
        );
    }

    #[test]
    fn sidebar_cutoff_does_not_change_split_layout() {
        assert_eq!(
            resolve_responsive_layout(LayoutMode::Auto, 159),
            ResponsiveLayout {
                viewport: ResponsiveViewport::Tight,
                layout: LayoutMode::Split,
                show_sidebar: false,
            }
        );
        assert_eq!(
            resolve_responsive_layout(LayoutMode::Auto, 160),
            ResponsiveLayout {
                viewport: ResponsiveViewport::Medium,
                layout: LayoutMode::Split,
                show_sidebar: true,
            }
        );
    }

    #[test]
    fn auto_switches_from_stack_to_split_at_its_independent_cutoff() {
        assert_eq!(
            resolve_responsive_layout(LayoutMode::Auto, 119).layout,
            LayoutMode::Stack
        );
        assert_eq!(
            resolve_responsive_layout(LayoutMode::Auto, 120).layout,
            LayoutMode::Split
        );
    }
}
