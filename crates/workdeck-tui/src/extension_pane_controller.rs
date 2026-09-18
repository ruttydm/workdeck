//! Committed lifecycle controller for native extension panes.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use workdeck_extension_api::{
    ExtensionDiffFile, PanePlacement, WORKDECK_FILES_PANE_KEY, extension_pane_size,
};

use crate::{
    ExtensionCurrentLinePaint, ExtensionCurrentLinePaintState, ExtensionCurrentLinePaintStatus,
    ExtensionCurrentLinePaintUpdate, ExtensionPaneLayoutPlan, ExtensionPaneSpec,
    PaneAvailabilityContext, PaneOpenState, RegisteredExtensionPane, SessionPane,
    apply_extension_current_line_paint_update, extension_current_line_paint_matches_cursor,
    initial_pane_open_state, plan_extension_panes, probe_extension_pane_availability,
    reconcile_pane_open_state, resolve_pane_key, resolve_pane_slot_key,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitialSidebarVisibility {
    Auto,
    Hidden,
    Visible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneResizeAxis {
    Width,
    Height,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneSizeOverride {
    pub axis: PaneResizeAxis,
    pub size: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivePaneResize {
    pub key: String,
    pub registration_identity: u64,
    pub placement: PanePlacement,
    pub origin: u16,
    pub start_size: u16,
    pub max_size: u16,
    pub min_size: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneControlOperation {
    Open,
    Close,
    Toggle,
    IsOpen,
}

impl PaneControlOperation {
    fn label(self) -> &'static str {
        match self {
            Self::Open => "panes.open",
            Self::Close => "panes.close",
            Self::Toggle => "panes.toggle",
            Self::IsOpen => "panes.isOpen",
        }
    }
}

/// Owned equivalent of Hunk's committed React pane-controller state.
#[derive(Debug, Clone)]
pub struct ExtensionPaneControllerState {
    session_panes: Vec<SessionPane>,
    open_state: Arc<PaneOpenState>,
    size_overrides: BTreeMap<String, PaneSizeOverride>,
    resize: Option<ActivePaneResize>,
    sidebar_visible: bool,
    force_sidebar_open: bool,
    responsive_shows_sidebar: bool,
    can_force_show_sidebar: bool,
    body_width: u16,
    body_height: u16,
    min_review_width: u16,
    min_review_height: u16,
    current_line_cursor: Option<(String, String)>,
    current_line_paint_state: Arc<ExtensionCurrentLinePaintState>,
    retained_current_line_registration_ids: BTreeSet<u64>,
    available_registration_ids: BTreeSet<u64>,
    quarantined_registration_ids: BTreeSet<u64>,
    reported_availability_failure_ids: BTreeSet<u64>,
    warnings: Vec<String>,
    layout: ExtensionPaneLayoutPlan,
}

impl ExtensionPaneControllerState {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        session_panes: Vec<SessionPane>,
        initial_sidebar: InitialSidebarVisibility,
        pager_mode: bool,
        responsive_shows_sidebar: bool,
        can_force_show_sidebar: bool,
        body_width: u16,
        body_height: u16,
        min_review_width: u16,
        min_review_height: u16,
    ) -> Self {
        let mut open_state = initial_pane_open_state(&session_panes);
        if initial_sidebar == InitialSidebarVisibility::Hidden {
            let open = open_state.open.iter().cloned().collect::<BTreeSet<_>>();
            let files_key = resolve_pane_slot_key(
                &session_panes,
                WORKDECK_FILES_PANE_KEY,
                &open,
                &BTreeSet::new(),
            );
            open_state = Arc::new(PaneOpenState {
                known: open_state.known.clone(),
                open: open_state
                    .open
                    .iter()
                    .filter(|key| **key != files_key)
                    .cloned()
                    .collect(),
            });
        }
        let mut state = Self {
            session_panes,
            open_state,
            size_overrides: BTreeMap::new(),
            resize: None,
            sidebar_visible: !pager_mode,
            force_sidebar_open: !pager_mode && initial_sidebar == InitialSidebarVisibility::Visible,
            responsive_shows_sidebar,
            can_force_show_sidebar,
            body_width,
            body_height,
            min_review_width,
            min_review_height,
            current_line_cursor: None,
            current_line_paint_state: Arc::new(ExtensionCurrentLinePaintState::default()),
            retained_current_line_registration_ids: BTreeSet::new(),
            available_registration_ids: BTreeSet::new(),
            quarantined_registration_ids: BTreeSet::new(),
            reported_availability_failure_ids: BTreeSet::new(),
            warnings: Vec::new(),
            layout: ExtensionPaneLayoutPlan::default(),
        };
        state.replan();
        state
    }

    #[must_use]
    pub fn session_panes(&self) -> &[SessionPane] {
        &self.session_panes
    }

    #[must_use]
    pub fn open_keys(&self) -> &[String] {
        &self.open_state.open
    }

    #[must_use]
    pub const fn layout(&self) -> &ExtensionPaneLayoutPlan {
        &self.layout
    }

    #[must_use]
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }

    #[must_use]
    pub fn current_line_paint(&self) -> Option<&ExtensionCurrentLinePaint> {
        extension_current_line_paint_matches_cursor(
            &self.current_line_paint_state,
            self.current_line_cursor
                .as_ref()
                .map(|(file, key)| (file.as_str(), key.as_str())),
        )
        .then_some(self.current_line_paint_state.paint.as_ref())
        .flatten()
    }

    fn current_line_paint_pending(&self) -> bool {
        self.current_line_paint_state.status == ExtensionCurrentLinePaintStatus::Pending
            || (self.current_line_paint_state.status == ExtensionCurrentLinePaintStatus::Ready
                && self.current_line_paint().is_none())
    }

    #[must_use]
    pub fn current_line_paint_requested(&self) -> bool {
        self.session_panes.iter().any(|pane| {
            self.open_state.open.contains(&pane.key) && pane.registered.pane.current_line
        })
    }

    pub fn set_current_line_cursor(&mut self, cursor: Option<(String, String)>) {
        self.current_line_cursor = cursor;
        self.replan();
    }

    pub fn update_current_line_paint(&mut self, update: ExtensionCurrentLinePaintUpdate) {
        self.current_line_paint_state =
            apply_extension_current_line_paint_update(&self.current_line_paint_state, update);
        self.replan();
    }

    pub fn set_geometry(
        &mut self,
        body_width: u16,
        body_height: u16,
        responsive_shows_sidebar: bool,
        can_force_show_sidebar: bool,
    ) {
        self.body_width = body_width;
        self.body_height = body_height;
        self.responsive_shows_sidebar = responsive_shows_sidebar;
        self.can_force_show_sidebar = can_force_show_sidebar;
        self.replan();
    }

    pub fn reconcile_session_panes(&mut self, panes: Vec<SessionPane>) {
        self.open_state = reconcile_pane_open_state(&panes, &self.open_state);
        self.session_panes = panes;
        self.available_registration_ids.retain(|identity| {
            self.session_panes
                .iter()
                .any(|pane| pane.registered.identity == *identity)
        });
        self.quarantined_registration_ids.retain(|identity| {
            self.session_panes
                .iter()
                .any(|pane| pane.registered.identity == *identity)
        });
        self.replan();
    }

    fn sidebar_area_visible(&self) -> bool {
        self.sidebar_visible
            && (self.responsive_shows_sidebar
                || (self.force_sidebar_open && self.can_force_show_sidebar))
    }

    fn effective_open_keys(&self) -> Vec<String> {
        let sidebar_visible = self.sidebar_area_visible();
        let mut effective = self
            .open_state
            .open
            .iter()
            .filter(|key| {
                let pane = self.session_panes.iter().find(|pane| &pane.key == *key);
                sidebar_visible
                    || !pane.is_some_and(|pane| {
                        matches!(pane.placement, PanePlacement::Left | PanePlacement::Right)
                    })
            })
            .cloned()
            .collect::<Vec<_>>();
        let failed_files_replacement = self.session_panes.iter().any(|pane| {
            self.open_state.open.contains(&pane.key)
                && pane.registered.pane.replaces.as_deref() == Some(WORKDECK_FILES_PANE_KEY)
                && self
                    .quarantined_registration_ids
                    .contains(&pane.registered.identity)
        });
        if failed_files_replacement
            && sidebar_visible
            && !effective.iter().any(|key| key == WORKDECK_FILES_PANE_KEY)
        {
            effective.push(WORKDECK_FILES_PANE_KEY.into());
        }
        effective
    }

    pub fn probe_availability<F>(
        &mut self,
        files: &[ExtensionDiffFile],
        selected_file_id: Option<&str>,
        selected_hunk_index: Option<usize>,
        evaluate: F,
    ) where
        F: FnMut(&RegisteredExtensionPane, PaneAvailabilityContext<'_>) -> Result<bool, String>,
    {
        let effective = self.effective_open_keys();
        let candidate_panes = self
            .session_panes
            .iter()
            .filter(|pane| {
                effective.contains(&pane.key)
                    && !self
                        .quarantined_registration_ids
                        .contains(&pane.registered.identity)
            })
            .cloned()
            .collect::<Vec<_>>();
        let retained = if self.current_line_paint_pending() {
            &self.retained_current_line_registration_ids
        } else {
            &BTreeSet::new()
        };
        let probe = probe_extension_pane_availability(
            &candidate_panes,
            files,
            selected_file_id,
            selected_hunk_index,
            self.current_line_paint(),
            retained,
            evaluate,
        );
        self.available_registration_ids = probe.available_registration_ids;
        for failure in probe.failures {
            let identity = failure.pane.registered.identity;
            self.quarantined_registration_ids.insert(identity);
            if self.reported_availability_failure_ids.insert(identity) {
                self.warnings.push(format!(
                    "Extension {} pane \"{}\" availability failed • {}",
                    failure.pane.registered.extension_id,
                    failure.pane.registered.pane.id,
                    failure.error
                ));
            }
        }
        self.replan();
    }

    fn accepted_open_keys(&self) -> BTreeSet<String> {
        let pending = self.current_line_paint_pending();
        self.effective_open_keys()
            .into_iter()
            .filter(|key| {
                let Some(pane) = self.session_panes.iter().find(|pane| pane.key == *key) else {
                    return false;
                };
                let identity = pane.registered.identity;
                if self.quarantined_registration_ids.contains(&identity) {
                    return false;
                }
                (pending
                    && self
                        .retained_current_line_registration_ids
                        .contains(&identity))
                    || !pane.registered.pane.available
                    || self.available_registration_ids.contains(&identity)
            })
            .collect()
    }

    fn matching_axis_overrides(&self) -> BTreeMap<String, u16> {
        self.session_panes
            .iter()
            .filter_map(|pane| {
                let axis = if matches!(pane.placement, PanePlacement::Left | PanePlacement::Right) {
                    PaneResizeAxis::Width
                } else {
                    PaneResizeAxis::Height
                };
                self.size_overrides
                    .get(&pane.key)
                    .filter(|value| value.axis == axis)
                    .map(|value| (pane.key.clone(), value.size))
            })
            .collect()
    }

    fn replan(&mut self) {
        let specs = self
            .session_panes
            .iter()
            .map(ExtensionPaneSpec::from)
            .collect::<Vec<_>>();
        self.layout = plan_extension_panes(
            &specs,
            &self.accepted_open_keys(),
            &self.matching_axis_overrides(),
            ratatui::layout::Rect::new(0, 0, self.body_width, self.body_height),
            self.min_review_width,
            self.min_review_height,
        );
        if self.resize.as_ref().is_some_and(|resize| {
            !self.session_panes.iter().any(|pane| {
                pane.key == resize.key
                    && pane.registered.identity == resize.registration_identity
                    && pane.placement == resize.placement
                    && self
                        .layout
                        .panes
                        .iter()
                        .any(|planned| planned.key == pane.key && planned.divider.is_some())
            })
        }) {
            self.resize = None;
        }
        if !self.current_line_paint_pending() {
            self.retained_current_line_registration_ids = self
                .layout
                .panes
                .iter()
                .filter_map(|planned| {
                    self.session_panes
                        .iter()
                        .find(|pane| pane.key == planned.key && pane.registered.pane.current_line)
                        .map(|pane| pane.registered.identity)
                })
                .collect();
        }
    }

    fn set_open(&mut self, key: &str, open: bool) {
        let currently_open = self
            .open_state
            .open
            .iter()
            .any(|candidate| candidate == key);
        if currently_open == open {
            return;
        }
        if !open && self.resize.as_ref().is_some_and(|resize| resize.key == key) {
            self.resize = None;
        }
        let mut keys = self.open_state.open.clone();
        if open {
            keys.push(key.into());
        } else {
            keys.retain(|candidate| candidate != key);
        }
        self.open_state = Arc::new(PaneOpenState {
            known: self.open_state.known.clone(),
            open: keys,
        });
        self.replan();
    }

    fn reveal_sidebar_area(&mut self) {
        self.sidebar_visible = true;
        if !self.responsive_shows_sidebar && self.can_force_show_sidebar {
            self.force_sidebar_open = true;
        }
    }

    pub fn pane_control(
        &mut self,
        extension_id: &str,
        operation: PaneControlOperation,
        id: &str,
        lease_live: bool,
    ) -> bool {
        if !lease_live {
            if operation != PaneControlOperation::IsOpen {
                self.warnings.push(format!(
                    "Extension {extension_id} {} ignored — the review session was reloaded",
                    operation.label()
                ));
            }
            return false;
        }
        let Some(key) = resolve_pane_key(&self.session_panes, extension_id, id) else {
            if operation != PaneControlOperation::IsOpen {
                self.warnings.push(format!(
                    "Extension {extension_id} {} targeted unknown pane \"{id}\"",
                    operation.label()
                ));
            }
            return false;
        };
        let is_open = self.open_state.open.contains(&key);
        match operation {
            PaneControlOperation::Open => {
                self.set_open(&key, true);
                if self.session_panes.iter().any(|pane| {
                    pane.key == key
                        && matches!(pane.placement, PanePlacement::Left | PanePlacement::Right)
                }) {
                    self.reveal_sidebar_area();
                    self.replan();
                }
                true
            }
            PaneControlOperation::Close => {
                self.set_open(&key, false);
                true
            }
            PaneControlOperation::Toggle => {
                self.set_open(&key, !is_open);
                if !is_open
                    && self.session_panes.iter().any(|pane| {
                        pane.key == key
                            && matches!(pane.placement, PanePlacement::Left | PanePlacement::Right)
                    })
                {
                    self.reveal_sidebar_area();
                    self.replan();
                }
                true
            }
            PaneControlOperation::IsOpen => is_open,
        }
    }

    pub fn toggle_files_pane(&mut self) {
        let visible_keys = self
            .layout
            .panes
            .iter()
            .map(|pane| pane.key.clone())
            .collect::<BTreeSet<_>>();
        let visible_files_key = resolve_pane_slot_key(
            &self.session_panes,
            WORKDECK_FILES_PANE_KEY,
            &visible_keys,
            &self.quarantined_registration_ids,
        );
        if visible_keys.contains(&visible_files_key)
            && visible_files_key == WORKDECK_FILES_PANE_KEY
            && !self
                .open_state
                .open
                .contains(&WORKDECK_FILES_PANE_KEY.to_owned())
            && let Some(failed) = self.session_panes.iter().find(|pane| {
                self.open_state.open.contains(&pane.key)
                    && pane.registered.pane.replaces.as_deref() == Some(WORKDECK_FILES_PANE_KEY)
                    && self
                        .quarantined_registration_ids
                        .contains(&pane.registered.identity)
            })
        {
            let key = failed.key.clone();
            self.set_open(&key, false);
            return;
        }

        let logical = self
            .open_state
            .open
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let files_key = resolve_pane_slot_key(
            &self.session_panes,
            WORKDECK_FILES_PANE_KEY,
            &logical,
            &self.quarantined_registration_ids,
        );
        let uses_sidebar = self.session_panes.iter().any(|pane| {
            pane.key == files_key
                && matches!(pane.placement, PanePlacement::Left | PanePlacement::Right)
        });
        if uses_sidebar && !self.sidebar_area_visible() {
            self.set_open(&files_key, true);
            self.reveal_sidebar_area();
            self.replan();
        } else {
            let open = self.open_state.open.contains(&files_key);
            self.set_open(&files_key, !open);
        }
    }

    pub fn report_pane_render_failure(&mut self, registration_identity: u64) {
        let replacement_key = self.session_panes.iter().find_map(|pane| {
            (pane.registered.identity == registration_identity).then(|| {
                (
                    pane.key.clone(),
                    pane.registered.pane.replaces.as_deref() == Some(WORKDECK_FILES_PANE_KEY),
                )
            })
        });
        self.quarantined_registration_ids
            .insert(registration_identity);
        if let Some((key, true)) = replacement_key {
            self.set_open(&key, false);
            self.set_open(WORKDECK_FILES_PANE_KEY, true);
            self.reveal_sidebar_area();
        }
        self.replan();
    }

    pub fn begin_resize(
        &mut self,
        key: &str,
        registration_identity: u64,
        left_button: bool,
        x: u16,
        y: u16,
    ) -> bool {
        if !left_button {
            return false;
        }
        let Some(session) = self
            .session_panes
            .iter()
            .find(|pane| pane.key == key && pane.registered.identity == registration_identity)
        else {
            return false;
        };
        let Some(planned) = self
            .layout
            .panes
            .iter()
            .find(|planned| planned.key == key && planned.divider.is_some())
        else {
            return false;
        };
        let vertical = matches!(
            session.placement,
            PanePlacement::Left | PanePlacement::Right
        );
        let requested = extension_pane_size(&session.registered.pane, Some(session.placement));
        let current_size = if vertical {
            planned.bounds.width
        } else {
            planned.bounds.height
        };
        let review_size = if vertical {
            self.layout.review_bounds.width
        } else {
            self.layout.review_bounds.height
        };
        let review_minimum = if vertical {
            self.min_review_width
        } else {
            self.min_review_height
        };
        self.resize = Some(ActivePaneResize {
            key: key.into(),
            registration_identity,
            placement: session.placement,
            origin: if vertical { x } else { y },
            start_size: current_size,
            max_size: current_size
                .saturating_add(review_size.saturating_sub(review_minimum))
                .min(requested.max.unwrap_or(u16::MAX)),
            min_size: requested.min.unwrap_or(1),
        });
        true
    }

    pub fn update_resize(&mut self, x: u16, y: u16) -> bool {
        let Some(resize) = self.resize.clone() else {
            return false;
        };
        let Some(planned) = self.layout.panes.iter().find(|planned| {
            planned.key == resize.key
                && planned.divider.is_some()
                && self.session_panes.iter().any(|pane| {
                    pane.key == resize.key
                        && pane.registered.identity == resize.registration_identity
                        && pane.placement == resize.placement
                })
        }) else {
            self.resize = None;
            return false;
        };
        let vertical = matches!(resize.placement, PanePlacement::Left | PanePlacement::Right);
        let current_size = if vertical {
            planned.bounds.width
        } else {
            planned.bounds.height
        };
        let review_size = if vertical {
            self.layout.review_bounds.width
        } else {
            self.layout.review_bounds.height
        };
        let review_minimum = if vertical {
            self.min_review_width
        } else {
            self.min_review_height
        };
        let current_max = current_size.saturating_add(review_size.saturating_sub(review_minimum));
        let position = if vertical { x } else { y };
        let delta = if matches!(
            resize.placement,
            PanePlacement::Right | PanePlacement::Bottom
        ) {
            i32::from(resize.origin) - i32::from(position)
        } else {
            i32::from(position) - i32::from(resize.origin)
        };
        let next = (i32::from(resize.start_size) + delta).clamp(
            i32::from(resize.min_size),
            i32::from(resize.max_size.min(current_max)),
        ) as u16;
        self.size_overrides.insert(
            resize.key,
            PaneSizeOverride {
                axis: if vertical {
                    PaneResizeAxis::Width
                } else {
                    PaneResizeAxis::Height
                },
                size: next,
            },
        );
        self.replan();
        true
    }

    pub fn end_resize(&mut self) -> bool {
        self.resize.take().is_some()
    }

    #[must_use]
    pub fn resizing_pane_key(&self) -> Option<&str> {
        self.resize.as_ref().map(|resize| resize.key.as_str())
    }

    #[must_use]
    pub fn files_pane_visible(&self) -> bool {
        let visible = self
            .layout
            .panes
            .iter()
            .map(|pane| pane.key.clone())
            .collect::<BTreeSet<_>>();
        let owner = resolve_pane_slot_key(
            &self.session_panes,
            WORKDECK_FILES_PANE_KEY,
            &visible,
            &self.quarantined_registration_ids,
        );
        visible.contains(&owner)
    }

    #[must_use]
    pub fn render_sidebar(&self) -> bool {
        self.layout.panes.iter().any(|pane| {
            matches!(
                pane.pane.placement,
                PanePlacement::Left | PanePlacement::Right
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_core::ReviewSide;
    use workdeck_diff::{DiffRow, SplitLineCell, SplitLineKind};
    use workdeck_extension_api::{ExtensionPaneSize, PaneRegistration};

    use crate::{
        CurrentLinePlannedRow, CurrentLineRowPlan, LineCursor, LineCursorTarget,
        create_extension_current_line_paint, resolve_theme,
    };

    fn registration(extension_id: &str, id: &str) -> Arc<RegisteredExtensionPane> {
        RegisteredExtensionPane::new(
            extension_id,
            PaneRegistration {
                id: id.into(),
                title: id.into(),
                placement: PanePlacement::Left,
                default_open: false,
                preferred_size: None,
                width: None,
                height: None,
                replaces: None,
                current_line: false,
                available: false,
            },
        )
    }

    fn controller(
        registrations: &[Arc<RegisteredExtensionPane>],
        initial_sidebar: InitialSidebarVisibility,
    ) -> ExtensionPaneControllerState {
        ExtensionPaneControllerState::new(
            crate::build_session_panes(registrations),
            initial_sidebar,
            false,
            true,
            true,
            100,
            30,
            48,
            5,
        )
    }

    fn current_line_paint() -> ExtensionCurrentLinePaint {
        let stable_key = "line:0:new:1";
        let cursor = LineCursor {
            file_id: "file".into(),
            hunk_index: 0,
            stable_key: stable_key.into(),
            target: LineCursorTarget {
                side: ReviewSide::New,
                line: 1,
            },
            expanded_gap_key: None,
        };
        let blank = SplitLineCell {
            kind: SplitLineKind::Empty,
            sign: " ".into(),
            line_number: None,
            move_kind: None,
            spans: Vec::new(),
        };
        let plan = CurrentLineRowPlan {
            planned_rows: vec![CurrentLinePlannedRow {
                stable_key: stable_key.into(),
                stable_alias_keys: Vec::new(),
                row: DiffRow::SplitLine {
                    key: "split:0".into(),
                    file_id: "file".into(),
                    hunk_index: 0,
                    left: blank,
                    right: SplitLineCell {
                        kind: SplitLineKind::Addition,
                        sign: "+".into(),
                        line_number: Some(1),
                        move_kind: None,
                        spans: Vec::new(),
                    },
                    is_expansion_row: false,
                    expanded_gap_key: None,
                },
            }],
            line_number_digits: 1,
        };
        create_extension_current_line_paint(
            &cursor,
            &plan,
            true,
            0,
            &resolve_theme(Some("github-dark-default"), None, &[]),
        )
        .unwrap()
    }

    fn pane_visible(controller: &ExtensionPaneControllerState, key: &str) -> bool {
        controller.layout().panes.iter().any(|pane| pane.key == key)
    }

    #[test]
    fn frozen_hunk_pane_controller_oracle_is_main_only_and_complete() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/extension-pane-controller.json"
        ))
        .unwrap();
        assert_eq!(oracle["baselineOracle"]["passed"], 13);
        assert_eq!(oracle["baselineOracle"]["expectations"], 54);
        assert_eq!(oracle["stableOracle"]["status"], "absent");
        assert_eq!(oracle["authoritative"], "baseline");
    }

    #[test]
    fn probes_availability_after_commit_and_keeps_false_panes_logically_open() {
        let mut detail = registration("meta", "detail");
        {
            let pane = &mut Arc::get_mut(&mut detail).unwrap().pane;
            pane.placement = PanePlacement::Bottom;
            pane.height = Some(ExtensionPaneSize {
                preferred: 3,
                min: Some(3),
                max: Some(3),
                fraction: None,
            });
            pane.default_open = true;
            pane.available = true;
        }
        let mut controller = controller(&[detail], InitialSidebarVisibility::Auto);
        let mut available = false;
        let mut calls = 0;
        controller.probe_availability(&[], None, None, |_, _| {
            calls += 1;
            Ok(available)
        });
        assert_eq!(calls, 1);
        assert!(!pane_visible(&controller, "meta:detail"));
        assert!(
            controller
                .open_keys()
                .iter()
                .any(|key| key == "meta:detail")
        );
        assert!(controller.warnings().is_empty());

        available = true;
        controller.probe_availability(&[], Some("next"), None, |_, _| {
            calls += 1;
            Ok(available)
        });
        assert_eq!(calls, 2);
        assert!(pane_visible(&controller, "meta:detail"));
    }

    #[test]
    fn quarantines_failed_replacement_warns_once_and_restores_a_fresh_registration() {
        let mut broken = registration("meta", "files");
        {
            let pane = &mut Arc::get_mut(&mut broken).unwrap().pane;
            pane.replaces = Some(WORKDECK_FILES_PANE_KEY.into());
            pane.available = true;
        }
        let mut controller = controller(&[broken], InitialSidebarVisibility::Auto);
        let mut calls = 0;
        controller.probe_availability(&[], None, None, |_, _| {
            calls += 1;
            Err("availability exploded".into())
        });
        assert_eq!(calls, 1);
        assert_eq!(
            controller.warnings(),
            ["Extension meta pane \"files\" availability failed • availability exploded"]
        );
        assert!(pane_visible(&controller, WORKDECK_FILES_PANE_KEY));
        controller.probe_availability(&[], Some("changed"), None, |_, _| {
            calls += 1;
            Ok(true)
        });
        assert_eq!(calls, 1);
        assert_eq!(controller.warnings().len(), 1);

        let mut healthy = registration("meta", "files");
        {
            let pane = &mut Arc::get_mut(&mut healthy).unwrap().pane;
            pane.replaces = Some(WORKDECK_FILES_PANE_KEY.into());
            pane.available = true;
        }
        controller.reconcile_session_panes(crate::build_session_panes(&[healthy]));
        controller.probe_availability(&[], None, None, |_, _| Ok(true));
        assert!(pane_visible(&controller, "meta:files"));
    }

    #[test]
    fn toggles_off_a_built_in_files_fallback_injected_after_availability_failure() {
        let mut broken = registration("meta", "files");
        {
            let pane = &mut Arc::get_mut(&mut broken).unwrap().pane;
            pane.replaces = Some(WORKDECK_FILES_PANE_KEY.into());
            pane.available = true;
        }
        let mut controller = controller(&[broken], InitialSidebarVisibility::Auto);
        controller.probe_availability(&[], None, None, |_, _| Err("exploded".into()));
        assert!(controller.files_pane_visible());
        controller.toggle_files_pane();
        assert!(!controller.files_pane_visible());
        controller.toggle_files_pane();
        assert!(controller.files_pane_visible());
    }

    #[test]
    fn hidden_initial_files_slot_preserves_independent_side_panes_and_toggle_restores_replacement()
    {
        let mut activity = registration("activity-test", "activity");
        {
            let pane = &mut Arc::get_mut(&mut activity).unwrap().pane;
            pane.placement = PanePlacement::Right;
            pane.default_open = true;
        }
        let mut replacement = registration("replacement-test", "files");
        {
            let pane = &mut Arc::get_mut(&mut replacement).unwrap().pane;
            pane.default_open = true;
            pane.replaces = Some(WORKDECK_FILES_PANE_KEY.into());
        }
        let mut controller = controller(
            &[Arc::clone(&activity), Arc::clone(&replacement)],
            InitialSidebarVisibility::Hidden,
        );
        controller.set_geometry(240, 30, true, true);
        assert!(pane_visible(&controller, &activity.key()));
        assert!(!pane_visible(&controller, &replacement.key()));

        controller.toggle_files_pane();
        assert!(pane_visible(&controller, &activity.key()));
        assert!(
            pane_visible(&controller, &replacement.key()),
            "planned panes: {:?}; open: {:?}",
            controller
                .layout()
                .panes
                .iter()
                .map(|pane| pane.key.as_str())
                .collect::<Vec<_>>(),
            controller.open_keys(),
        );
    }

    #[test]
    fn publishes_newly_registered_default_open_state_immediately() {
        let mut controller = controller(&[], InitialSidebarVisibility::Auto);
        let mut fresh = registration("meta", "fresh");
        {
            let pane = &mut Arc::get_mut(&mut fresh).unwrap().pane;
            pane.placement = PanePlacement::Bottom;
            pane.height = Some(ExtensionPaneSize {
                preferred: 2,
                min: Some(2),
                max: Some(2),
                fraction: None,
            });
            pane.default_open = true;
        }
        controller.reconcile_session_panes(crate::build_session_panes(&[fresh]));
        assert!(controller.open_keys().iter().any(|key| key == "meta:fresh"));
        assert!(pane_visible(&controller, "meta:fresh"));
    }

    #[test]
    fn retires_captured_controls_and_reveals_only_authorized_side_panes() {
        let extra = registration("meta", "extra");
        let mut controller = ExtensionPaneControllerState::new(
            crate::build_session_panes(&[extra]),
            InitialSidebarVisibility::Auto,
            false,
            false,
            true,
            100,
            30,
            48,
            5,
        );
        assert!(!controller.render_sidebar());
        assert!(controller.pane_control("meta", PaneControlOperation::Open, "extra", true));
        assert!(controller.render_sidebar());
        assert!(controller.pane_control("meta", PaneControlOperation::IsOpen, "extra", true));
        assert!(!controller.pane_control("meta", PaneControlOperation::Close, "extra", false));
        assert!(!controller.pane_control("meta", PaneControlOperation::IsOpen, "extra", false));
        assert!(
            controller
                .warnings()
                .last()
                .unwrap()
                .contains("panes.close ignored")
        );
    }

    #[test]
    fn falls_back_after_replacement_render_failure_without_retaining_open_choice() {
        let mut replacement = registration("meta", "files");
        let identity = replacement.identity;
        Arc::get_mut(&mut replacement).unwrap().pane.replaces =
            Some(WORKDECK_FILES_PANE_KEY.into());
        let mut controller = controller(&[replacement], InitialSidebarVisibility::Auto);
        assert!(pane_visible(&controller, "meta:files"));
        controller.report_pane_render_failure(identity);
        assert!(pane_visible(&controller, WORKDECK_FILES_PANE_KEY));
        assert!(!controller.pane_control("meta", PaneControlOperation::IsOpen, "files", true));
    }

    #[test]
    fn retains_accepted_current_line_pane_while_replacement_paint_is_pending() {
        let mut detail = registration("meta", "line");
        {
            let pane = &mut Arc::get_mut(&mut detail).unwrap().pane;
            pane.placement = PanePlacement::Bottom;
            pane.height = Some(ExtensionPaneSize {
                preferred: 3,
                min: Some(3),
                max: Some(3),
                fraction: None,
            });
            pane.default_open = true;
            pane.current_line = true;
            pane.available = true;
        }
        let mut controller = controller(&[detail], InitialSidebarVisibility::Auto);
        assert!(controller.current_line_paint_requested());
        assert!(!pane_visible(&controller, "meta:line"));

        let paint = current_line_paint();
        controller.set_current_line_cursor(Some(("file".into(), "line:0:new:1".into())));
        controller.update_current_line_paint(ExtensionCurrentLinePaintUpdate::Ready {
            file_id: "file".into(),
            cursor_key: "line:0:new:1".into(),
            paint: Box::new(paint),
        });
        let mut calls = 0;
        controller.probe_availability(&[], None, None, |_, context| {
            calls += 1;
            Ok(context.current_line.is_some())
        });
        assert!(pane_visible(&controller, "meta:line"));

        controller.update_current_line_paint(ExtensionCurrentLinePaintUpdate::Pending);
        let before_pending = calls;
        controller.probe_availability(&[], None, None, |_, _| {
            calls += 1;
            Ok(false)
        });
        assert!(pane_visible(&controller, "meta:line"));
        assert_eq!(calls, before_pending);

        let mut replacement = registration("meta", "line");
        let replacement_id = replacement.identity;
        {
            let pane = &mut Arc::get_mut(&mut replacement).unwrap().pane;
            pane.placement = PanePlacement::Bottom;
            pane.height = Some(ExtensionPaneSize {
                preferred: 3,
                min: Some(3),
                max: Some(3),
                fraction: None,
            });
            pane.default_open = true;
            pane.current_line = true;
            pane.available = true;
        }
        controller.reconcile_session_panes(crate::build_session_panes(&[replacement]));
        let mut replacement_calls = 0;
        controller.probe_availability(&[], None, None, |registered, context| {
            if registered.identity == replacement_id {
                replacement_calls += 1;
                assert!(context.current_line.is_none());
            }
            Ok(false)
        });
        assert_eq!(replacement_calls, 1);
        assert!(!pane_visible(&controller, "meta:line"));
    }

    fn responsive_right() -> Arc<RegisteredExtensionPane> {
        let mut right = registration("meta", "right");
        {
            let pane = &mut Arc::get_mut(&mut right).unwrap().pane;
            pane.placement = PanePlacement::Right;
            pane.width = Some(ExtensionPaneSize {
                preferred: 20,
                min: Some(10),
                max: Some(50),
                fraction: Some(0.2),
            });
            pane.default_open = true;
        }
        right
    }

    #[test]
    fn tracks_responsive_sizes_until_drag_establishes_a_restorable_override() {
        let right = responsive_right();
        let identity = right.identity;
        let mut controller = controller(&[right], InitialSidebarVisibility::Hidden);
        assert_eq!(controller.layout().panes[0].bounds.width, 20);
        controller.set_geometry(150, 30, true, true);
        assert_eq!(controller.layout().panes[0].bounds.width, 30);
        let divider = controller.layout().panes[0].divider.unwrap();
        assert!(controller.begin_resize("meta:right", identity, true, divider.x, divider.y));
        assert!(controller.update_resize(divider.x.saturating_sub(10), divider.y));
        assert_eq!(controller.layout().panes[0].bounds.width, 40);
        assert!(controller.end_resize());
        controller.set_geometry(80, 30, true, true);
        assert_eq!(controller.layout().panes[0].bounds.width, 31);
        controller.set_geometry(150, 30, true, true);
        assert_eq!(controller.layout().panes[0].bounds.width, 40);
    }

    #[test]
    fn does_not_reinterpret_same_key_width_override_after_pane_moves_to_row_axis() {
        let right = responsive_right();
        let identity = right.identity;
        let mut controller = controller(&[right], InitialSidebarVisibility::Hidden);
        let divider = controller.layout().panes[0].divider.unwrap();
        assert!(controller.begin_resize("meta:right", identity, true, divider.x, divider.y));
        assert!(controller.update_resize(divider.x.saturating_sub(10), divider.y));
        assert_eq!(controller.layout().panes[0].bounds.width, 30);
        controller.end_resize();

        let mut bottom = registration("meta", "right");
        {
            let pane = &mut Arc::get_mut(&mut bottom).unwrap().pane;
            pane.placement = PanePlacement::Bottom;
            pane.height = Some(ExtensionPaneSize {
                preferred: 5,
                min: Some(3),
                max: Some(12),
                fraction: None,
            });
            pane.default_open = true;
        }
        controller.reconcile_session_panes(crate::build_session_panes(&[bottom]));
        assert_eq!(controller.layout().panes[0].bounds.height, 5);
    }

    #[test]
    fn resizes_right_panes_inverted_and_cancels_after_terminal_shrink() {
        let right = responsive_right();
        let identity = right.identity;
        let mut controller = controller(&[right], InitialSidebarVisibility::Hidden);
        let planned = controller.layout().panes[0].clone();
        let divider = planned.divider.unwrap();
        assert!(controller.begin_resize("meta:right", identity, true, divider.x, divider.y));
        assert!(controller.update_resize(divider.x.saturating_sub(8), divider.y));
        assert_eq!(
            controller.layout().panes[0].bounds.width,
            planned.bounds.width + 8
        );
        controller.set_geometry(55, 30, true, true);
        assert_eq!(controller.resizing_pane_key(), None);
        assert!(!controller.update_resize(0, 0));
    }

    #[test]
    fn resizes_bottom_panes_with_inverted_row_axis_movement() {
        let mut bottom = registration("meta", "bottom");
        let identity = bottom.identity;
        {
            let pane = &mut Arc::get_mut(&mut bottom).unwrap().pane;
            pane.placement = PanePlacement::Bottom;
            pane.height = Some(ExtensionPaneSize {
                preferred: 5,
                min: Some(3),
                max: Some(12),
                fraction: None,
            });
            pane.default_open = true;
        }
        let mut controller = controller(&[bottom], InitialSidebarVisibility::Hidden);
        let planned = controller.layout().panes[0].clone();
        let divider = planned.divider.unwrap();
        assert!(controller.begin_resize("meta:bottom", identity, true, divider.x, divider.y));
        assert!(controller.update_resize(divider.x, divider.y.saturating_sub(4)));
        assert_eq!(
            controller.layout().panes[0].bounds.height,
            planned.bounds.height + 4
        );
    }

    #[test]
    fn cancels_active_drag_when_controls_close_its_pane() {
        let mut extra = registration("meta", "extra");
        let identity = extra.identity;
        {
            let pane = &mut Arc::get_mut(&mut extra).unwrap().pane;
            pane.default_open = true;
            pane.width = Some(ExtensionPaneSize {
                preferred: 24,
                min: Some(10),
                max: Some(40),
                fraction: None,
            });
        }
        let mut controller = controller(&[extra], InitialSidebarVisibility::Hidden);
        let divider = controller.layout().panes[0].divider.unwrap();
        assert!(controller.begin_resize("meta:extra", identity, true, divider.x, divider.y));
        assert_eq!(controller.resizing_pane_key(), Some("meta:extra"));
        assert!(controller.pane_control("meta", PaneControlOperation::Close, "extra", true));
        assert_eq!(controller.resizing_pane_key(), None);
        assert!(!pane_visible(&controller, "meta:extra"));
    }

    #[test]
    fn cancels_drag_when_reload_replaces_registration_behind_same_key() {
        let mut first = registration("meta", "extra");
        let identity = first.identity;
        {
            let pane = &mut Arc::get_mut(&mut first).unwrap().pane;
            pane.default_open = true;
            pane.width = Some(ExtensionPaneSize {
                preferred: 24,
                min: Some(10),
                max: Some(40),
                fraction: None,
            });
        }
        let mut controller = controller(&[first], InitialSidebarVisibility::Hidden);
        let divider = controller.layout().panes[0].divider.unwrap();
        assert!(controller.begin_resize("meta:extra", identity, true, divider.x, divider.y));
        assert_eq!(controller.resizing_pane_key(), Some("meta:extra"));

        let mut replacement = registration("meta", "extra");
        {
            let pane = &mut Arc::get_mut(&mut replacement).unwrap().pane;
            pane.default_open = true;
            pane.width = Some(ExtensionPaneSize {
                preferred: 24,
                min: Some(10),
                max: Some(40),
                fraction: None,
            });
        }
        controller.reconcile_session_panes(crate::build_session_panes(&[replacement]));
        assert_eq!(controller.resizing_pane_key(), None);
    }
}
