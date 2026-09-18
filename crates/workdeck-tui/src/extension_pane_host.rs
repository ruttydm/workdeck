//! Registration-scoped containment for native extension pane rendering.
//!
//! This is the Ratatui/native-process counterpart of Hunk's
//! `src/ui/components/panes/ExtensionPane.tsx` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. React component identity becomes the
//! monotonically assigned native registration identity; the host remains responsible for exact
//! pane geometry, clipping, semantic theme projection, guarded navigation, warnings, and safe
//! fallback selection.

use workdeck_extension_api::{
    ExtensionPaintTheme, ExtensionPaneView, PanePlacement, ViewNode, ViewStyle,
    WORKDECK_FILES_PANE_KEY,
};

use crate::RegisteredExtensionPane;

/// The renderer-independent fallback selected after one registration fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionPaneFallback {
    /// The caller supplied Hunk's `onRenderFailure` callback and owns recovery.
    None,
    /// Even the bundled Files pane must be able to report failure without invoking itself again.
    FilesPaneUnavailable,
    /// Standalone hosts fall back to the host-owned Files sidebar.
    BuiltInFilesPane,
}

/// Complete observable result of containing one pane render failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionPaneRenderFailure {
    pub warning: String,
    pub fallback: ExtensionPaneFallback,
    pub invoke_render_failure_callback: bool,
}

/// State equivalent to Hunk's registration-sensitive React error boundary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtensionPaneErrorBoundaryState {
    registered_identity: Option<u64>,
    failed: bool,
}

impl ExtensionPaneErrorBoundaryState {
    /// Reconcile the mounted registration and report whether its view may be invoked.
    ///
    /// A fresh object with unchanged extension and pane ids receives a fresh native identity, so
    /// it clears the previous failure exactly as Hunk's reference comparison does.
    pub fn prepare_render(&mut self, registered: &RegisteredExtensionPane) -> bool {
        if self.registered_identity != Some(registered.identity) {
            self.registered_identity = Some(registered.identity);
            self.failed = false;
        }
        !self.failed
    }

    /// Contain the first failure for the current identity. React's boundary calls `componentDidCatch`
    /// once and then paints fallback; repeated frame attempts therefore produce no duplicate warning.
    pub fn report_failure(
        &mut self,
        registered: &RegisteredExtensionPane,
        error: impl std::fmt::Display,
        has_render_failure_callback: bool,
    ) -> Option<ExtensionPaneRenderFailure> {
        let may_render = self.prepare_render(registered);
        if !may_render {
            return None;
        }
        self.failed = true;
        Some(extension_pane_render_failure(
            registered,
            error,
            has_render_failure_callback,
        ))
    }

    #[must_use]
    pub const fn failed(&self) -> bool {
        self.failed
    }
}

/// Choose Hunk's exact fallback and attributed notification text.
#[must_use]
pub fn extension_pane_render_failure(
    registered: &RegisteredExtensionPane,
    error: impl std::fmt::Display,
    has_render_failure_callback: bool,
) -> ExtensionPaneRenderFailure {
    let files_pane = registered.key() == WORKDECK_FILES_PANE_KEY;
    let fallback = if has_render_failure_callback {
        ExtensionPaneFallback::None
    } else if files_pane {
        ExtensionPaneFallback::FilesPaneUnavailable
    } else {
        ExtensionPaneFallback::BuiltInFilesPane
    };
    let fallback_notice = if has_render_failure_callback || files_pane {
        ""
    } else {
        " • using the built-in files pane"
    };
    ExtensionPaneRenderFailure {
        warning: format!(
            "Extension {} pane \"{}\" failed rendering • {error}{fallback_notice}",
            registered.extension_id, registered.pane.id
        ),
        fallback,
        invoke_render_failure_callback: has_render_failure_callback,
    }
}

/// Renderer-independent output used when the bundled Files painter itself fails.
#[must_use]
pub fn files_pane_unavailable_view(registered: &RegisteredExtensionPane) -> ExtensionPaneView {
    ExtensionPaneView {
        extension_id: registered.extension_id.clone(),
        pane: registered.pane.clone(),
        content: ViewNode::Text {
            text: "Files pane unavailable".into(),
            style: ViewStyle {
                foreground: Some("muted".into()),
                ..ViewStyle::default()
            },
        },
    }
}

/// Native memo inputs corresponding to Hunk's explicit `memo` comparator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionPaneMemoSignature {
    pub registration_identity: u64,
    /// Stable identities of each public file projection, in order.
    pub file_identities: Vec<String>,
    pub selected_file_id: Option<String>,
    pub selected_hunk_index: Option<usize>,
    pub placement: PanePlacement,
    /// Object/revision identity, not merely the theme name.
    pub theme_identity: u64,
    pub width: u16,
    pub height: u16,
    pub show_top_chrome: bool,
    pub keybindings_identity: u64,
    pub current_line_identity: Option<String>,
}

/// Return whether Hunk's host comparator would reuse the mounted pane output.
#[must_use]
pub fn extension_pane_host_props_equal(
    previous: &ExtensionPaneMemoSignature,
    next: &ExtensionPaneMemoSignature,
    next_pane_requests_current_line: bool,
) -> bool {
    previous.registration_identity == next.registration_identity
        && previous.file_identities == next.file_identities
        && previous.selected_file_id == next.selected_file_id
        && previous.selected_hunk_index == next.selected_hunk_index
        && previous.placement == next.placement
        && previous.theme_identity == next.theme_identity
        && previous.width == next.width
        && previous.height == next.height
        && previous.show_top_chrome == next.show_top_chrome
        && previous.keybindings_identity == next.keybindings_identity
        && (!next_pane_requests_current_line
            || previous.current_line_identity == next.current_line_identity)
}

/// Exact fixed host rectangle and bundled Files chrome choices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionPaneBoxPlan {
    pub width: u16,
    pub height: u16,
    pub background: String,
    pub clip_overflow: bool,
    pub column: bool,
    pub top_border: bool,
    pub border_color: Option<String>,
    pub padding_top: u16,
    pub padding_bottom: u16,
}

#[must_use]
pub fn plan_extension_pane_box(
    registered: &RegisteredExtensionPane,
    theme: &ExtensionPaintTheme,
    width: u16,
    height: u16,
    show_top_chrome: bool,
) -> ExtensionPaneBoxPlan {
    let files_chrome = registered.key() == WORKDECK_FILES_PANE_KEY;
    ExtensionPaneBoxPlan {
        width,
        height,
        background: theme.panel.clone(),
        clip_overflow: true,
        column: true,
        top_border: files_chrome && show_top_chrome,
        border_color: files_chrome.then(|| theme.border.clone()),
        padding_top: u16::from(files_chrome && show_top_chrome),
        padding_bottom: u16::from(files_chrome),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NavigableFile, guard_extension_select_hunk};
    use workdeck_extension_api::{
        ExtensionPaneSize, ExtensionThemeAppearance, PaneRegistration,
        WORKDECK_VENDOR_EXTENSION_ID, bundled_files_pane,
    };

    fn pane(id: &str) -> std::sync::Arc<RegisteredExtensionPane> {
        RegisteredExtensionPane::new(
            "probe",
            PaneRegistration {
                id: id.into(),
                title: String::new(),
                placement: PanePlacement::Left,
                default_open: false,
                preferred_size: None,
                width: Some(ExtensionPaneSize {
                    preferred: 34,
                    min: Some(22),
                    max: None,
                    fraction: None,
                }),
                height: None,
                replaces: None,
                current_line: false,
                available: false,
            },
        )
    }

    fn theme() -> ExtensionPaintTheme {
        ExtensionPaintTheme {
            appearance: ExtensionThemeAppearance::Dark,
            background: "#000000".into(),
            panel: "#111111".into(),
            panel_alt: "#222222".into(),
            border: "#333333".into(),
            accent: "#444444".into(),
            accent_muted: "#555555".into(),
            text: "#ffffff".into(),
            muted: "#888888".into(),
            selected_hunk: "#222244".into(),
            badge_added: "#00ff00".into(),
            badge_removed: "#ff0000".into(),
            badge_neutral: "#888888".into(),
            file_new: "#00aa00".into(),
            file_deleted: "#aa0000".into(),
            file_renamed: "#ffff00".into(),
            file_modified: "#ffaa00".into(),
            file_untracked: "#00ffff".into(),
            note_border: "#333355".into(),
        }
    }

    #[test]
    fn frozen_extension_pane_oracle_records_both_pinned_baselines() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/extension-pane.json"
        ))
        .unwrap();
        assert_eq!(
            oracle["baseline"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(oracle["stable"], "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd");
        assert_eq!(oracle["baselineTestOracle"]["passed"], 3);
        assert_eq!(oracle["stableTestOracle"]["passed"], 3);
    }

    #[test]
    fn refuses_garbage_hunk_indices_and_clamps_the_rest_into_the_file_range() {
        let files = [
            NavigableFile {
                id: "alpha",
                hunk_count: 2,
            },
            NavigableFile {
                id: "beta",
                hunk_count: 1,
            },
        ];
        for invalid in [Some(f64::NAN), Some(f64::INFINITY)] {
            assert!(
                guard_extension_select_hunk("probe", &files, true, "alpha", invalid)
                    .unwrap_err()
                    .0
                    .contains("invalid hunk index")
            );
        }
        assert!(
            guard_extension_select_hunk("probe", &files, true, "missing", Some(0.0))
                .unwrap_err()
                .0
                .contains("unknown file id \"missing\"")
        );
        let selected = [Some(99.0), Some(-5.0), Some(0.75)]
            .into_iter()
            .map(|index| {
                guard_extension_select_hunk("probe", &files, true, "alpha", index)
                    .unwrap()
                    .hunk_index
            })
            .collect::<Vec<_>>();
        assert_eq!(selected, [1, 0, 0]);
    }

    #[test]
    fn the_bundled_files_pane_has_a_renderer_independent_safe_fallback() {
        let registered = RegisteredExtensionPane::new(
            WORKDECK_VENDOR_EXTENSION_ID,
            bundled_files_pane().clone(),
        );
        let mut boundary = ExtensionPaneErrorBoundaryState::default();
        assert!(boundary.prepare_render(&registered));
        let failure = boundary
            .report_failure(&registered, "files exploded", false)
            .unwrap();
        assert_eq!(
            failure.fallback,
            ExtensionPaneFallback::FilesPaneUnavailable
        );
        assert_eq!(
            failure.warning,
            "Extension workdeck pane \"files\" failed rendering • files exploded"
        );
        assert!(!failure.warning.contains("using the built-in files pane"));
        let fallback = files_pane_unavailable_view(&registered);
        assert_eq!(
            fallback.content,
            ViewNode::Text {
                text: "Files pane unavailable".into(),
                style: ViewStyle {
                    foreground: Some("muted".into()),
                    ..ViewStyle::default()
                },
            }
        );
        assert!(!boundary.prepare_render(&registered));
        assert!(
            boundary
                .report_failure(&registered, "again", false)
                .is_none()
        );
    }

    #[test]
    fn a_fresh_registration_clears_the_failed_boundary_under_unchanged_ids() {
        let broken = pane("probe-view");
        let fixed = pane("probe-view");
        assert_eq!(broken.key(), fixed.key());
        assert_ne!(broken.identity, fixed.identity);

        let mut boundary = ExtensionPaneErrorBoundaryState::default();
        let failure = boundary
            .report_failure(&broken, "sidebar exploded", false)
            .unwrap();
        assert_eq!(failure.fallback, ExtensionPaneFallback::BuiltInFilesPane);
        assert_eq!(
            failure.warning,
            "Extension probe pane \"probe-view\" failed rendering • sidebar exploded • using the built-in files pane"
        );
        assert!(boundary.failed());
        assert!(boundary.prepare_render(&fixed));
        assert!(!boundary.failed());
    }

    #[test]
    fn callback_owned_recovery_has_no_recursive_fallback_and_warns_once() {
        let registered = pane("replacement");
        let mut boundary = ExtensionPaneErrorBoundaryState::default();
        let failure = boundary.report_failure(&registered, "boom", true).unwrap();
        assert_eq!(failure.fallback, ExtensionPaneFallback::None);
        assert!(failure.invoke_render_failure_callback);
        assert!(!failure.warning.contains("using the built-in files pane"));
        assert!(boundary.report_failure(&registered, "boom", true).is_none());
    }

    #[test]
    fn memo_contract_ignores_current_line_until_the_pane_opts_in() {
        let mut previous = ExtensionPaneMemoSignature {
            registration_identity: 7,
            file_identities: vec!["alpha@1".into(), "beta@1".into()],
            selected_file_id: Some("alpha".into()),
            selected_hunk_index: Some(0),
            placement: PanePlacement::Left,
            theme_identity: 2,
            width: 30,
            height: 20,
            show_top_chrome: true,
            keybindings_identity: 4,
            current_line_identity: Some("alpha:new:1".into()),
        };
        let mut next = previous.clone();
        next.current_line_identity = Some("alpha:new:2".into());
        assert!(extension_pane_host_props_equal(&previous, &next, false));
        assert!(!extension_pane_host_props_equal(&previous, &next, true));

        previous.file_identities.swap(0, 1);
        assert!(!extension_pane_host_props_equal(&previous, &next, false));
    }

    #[test]
    fn fixed_box_plan_applies_top_chrome_only_to_the_bundled_files_pane() {
        let files = RegisteredExtensionPane::new(
            WORKDECK_VENDOR_EXTENSION_ID,
            bundled_files_pane().clone(),
        );
        let files_plan = plan_extension_pane_box(&files, &theme(), 30, 20, true);
        assert_eq!((files_plan.width, files_plan.height), (30, 20));
        assert!(files_plan.clip_overflow && files_plan.column && files_plan.top_border);
        assert_eq!(files_plan.padding_top, 1);
        assert_eq!(files_plan.padding_bottom, 1);

        let ordinary = plan_extension_pane_box(&pane("ordinary"), &theme(), 30, 20, true);
        assert!(!ordinary.top_border);
        assert_eq!(ordinary.border_color, None);
        assert_eq!((ordinary.padding_top, ordinary.padding_bottom), (0, 0));
    }
}
