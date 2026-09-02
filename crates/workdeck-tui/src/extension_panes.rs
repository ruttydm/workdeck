//! Pure terminal-cell planning for native extension panes.

use ratatui::layout::Rect;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use workdeck_core::DiffFile;
use workdeck_extension_api::{
    PanePlacement, PaneRegistration, WORKDECK_FILES_PANE_KEY, WORKDECK_VENDOR_EXTENSION_ID,
    bundled_files_pane, extension_pane_size,
};

use crate::ExtensionCurrentLinePaint;

/// One cell between a resizable edge pane and its neighbor.
pub const EXTENSION_PANE_DIVIDER_SIZE: u16 = 1;
/// Pointer target centered over the visible one-cell divider.
pub const PANE_DIVIDER_HIT_AREA_SIZE: u16 = 5;
pub const PANE_DIVIDER_HIT_AREA_OFFSET: u16 = PANE_DIVIDER_HIT_AREA_SIZE / 2;
/// Smallest review height preserved while edge panes are open or resized.
pub const MIN_EXTENSION_REVIEW_HEIGHT: u16 = 5;

static NEXT_PANE_REGISTRATION_ID: AtomicU64 = AtomicU64::new(1);

/// One extension-owned pane registration with reload-sensitive object identity.
#[derive(Debug, Clone, PartialEq)]
pub struct RegisteredExtensionPane {
    pub identity: u64,
    pub extension_id: String,
    pub pane: PaneRegistration,
}

impl RegisteredExtensionPane {
    #[must_use]
    pub fn new(extension_id: impl Into<String>, pane: PaneRegistration) -> Arc<Self> {
        Arc::new(Self {
            identity: NEXT_PANE_REGISTRATION_ID.fetch_add(1, Ordering::Relaxed),
            extension_id: extension_id.into(),
            pane,
        })
    }

    #[must_use]
    pub fn key(&self) -> String {
        format!("{}:{}", self.extension_id, self.pane.id)
    }
}

/// One pane offered to the current review session.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionPane {
    pub key: String,
    pub registered: Arc<RegisteredExtensionPane>,
    pub placement: PanePlacement,
    pub title: String,
    pub default_open: bool,
}

/// Compose the built-in Files registration before native registrations.
///
/// First registration wins for both qualified pane keys and replacement targets, matching Hunk's
/// registry resolver. Extension-application diagnostics retain ownership of skipped registrations.
#[must_use]
pub fn build_session_panes(extension_panes: &[Arc<RegisteredExtensionPane>]) -> Vec<SessionPane> {
    let bundled =
        RegisteredExtensionPane::new(WORKDECK_VENDOR_EXTENSION_ID, bundled_files_pane().clone());
    let mut claimed_keys = BTreeSet::new();
    let mut claimed_replacements = BTreeSet::new();
    let mut resolved = Vec::new();
    for registered in std::iter::once(&bundled).chain(extension_panes) {
        let key = registered.key();
        if claimed_keys.contains(&key)
            || registered
                .pane
                .replaces
                .as_ref()
                .is_some_and(|target| claimed_replacements.contains(target))
        {
            continue;
        }
        claimed_keys.insert(key);
        if let Some(target) = &registered.pane.replaces {
            claimed_replacements.insert(target.clone());
        }
        resolved.push(Arc::clone(registered));
    }

    let replacements = resolved
        .iter()
        .filter_map(|registered| registered.pane.replaces.clone())
        .collect::<BTreeSet<_>>();
    resolved
        .into_iter()
        .map(|registered| {
            let key = registered.key();
            let pane = &registered.pane;
            SessionPane {
                placement: pane.placement,
                title: if pane.title.is_empty() {
                    pane.id.clone()
                } else {
                    pane.title.clone()
                },
                default_open: !replacements.contains(&key)
                    && (key == WORKDECK_FILES_PANE_KEY
                        || pane.default_open
                        || pane.replaces.is_some()),
                key,
                registered,
            }
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneOpenState {
    pub known: Vec<String>,
    pub open: Vec<String>,
}

#[must_use]
pub fn initial_pane_open_state(panes: &[SessionPane]) -> Arc<PaneOpenState> {
    Arc::new(PaneOpenState {
        known: panes.iter().map(|pane| pane.key.clone()).collect(),
        open: panes
            .iter()
            .filter(|pane| pane.default_open)
            .map(|pane| pane.key.clone())
            .collect(),
    })
}

/// Retain existing choices by stable key while applying defaults to newly registered panes.
#[must_use]
pub fn reconcile_pane_open_state(
    panes: &[SessionPane],
    state: &Arc<PaneOpenState>,
) -> Arc<PaneOpenState> {
    let keys = panes
        .iter()
        .map(|pane| pane.key.clone())
        .collect::<Vec<_>>();
    let known = state
        .known
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let open = state
        .open
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let next_open = panes
        .iter()
        .filter(|pane| {
            if known.contains(pane.key.as_str()) {
                open.contains(pane.key.as_str())
            } else {
                pane.default_open
            }
        })
        .map(|pane| pane.key.clone())
        .collect::<Vec<_>>();
    if keys == state.known && next_open == state.open {
        return Arc::clone(state);
    }
    Arc::new(PaneOpenState {
        known: keys,
        open: next_open,
    })
}

/// Resolve the pane currently filling a named slot, preserving the deepest open fallback.
#[must_use]
pub fn resolve_pane_slot_key(
    panes: &[SessionPane],
    slot_key: &str,
    open_keys: &BTreeSet<String>,
    quarantined_registration_ids: &BTreeSet<u64>,
) -> String {
    let mut replacement_by_target = BTreeMap::new();
    for pane in panes {
        if let Some(target) = &pane.registered.pane.replaces
            && !quarantined_registration_ids.contains(&pane.registered.identity)
        {
            replacement_by_target.insert(target.as_str(), pane);
        }
    }

    let mut replacements = Vec::new();
    let mut visited = BTreeSet::new();
    let mut target = slot_key;
    while visited.insert(target) {
        let Some(replacement) = replacement_by_target.get(target).copied() else {
            break;
        };
        replacements.push(replacement);
        target = &replacement.key;
    }
    if let Some(open) = replacements
        .iter()
        .rev()
        .find(|replacement| open_keys.contains(&replacement.key))
    {
        return open.key.clone();
    }
    if open_keys.contains(slot_key) {
        return slot_key.to_owned();
    }
    replacements
        .last()
        .map_or_else(|| slot_key.to_owned(), |pane| pane.key.clone())
}

/// Resolve a bare extension-local pane id or an already-qualified pane key.
#[must_use]
pub fn resolve_pane_key(panes: &[SessionPane], extension_id: &str, id: &str) -> Option<String> {
    let key = if id.contains(':') {
        id.to_owned()
    } else {
        format!("{extension_id}:{id}")
    };
    panes.iter().any(|pane| pane.key == key).then_some(key)
}

/// Immutable review state supplied to a native pane availability callback.
#[derive(Debug, Clone, Copy)]
pub struct PaneAvailabilityContext<'a> {
    pub files: &'a [DiffFile],
    pub selected_file_id: Option<&'a str>,
    pub selected_hunk_index: Option<usize>,
    pub placement: PanePlacement,
    pub current_line: Option<&'a ExtensionCurrentLinePaint>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PaneAvailabilityFailure {
    pub pane: SessionPane,
    pub error: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PaneAvailabilityProbe {
    pub available_registration_ids: BTreeSet<u64>,
    pub failures: Vec<PaneAvailabilityFailure>,
}

/// Probe availability before geometry planning, without mutating or quarantining host state.
///
/// The evaluator is the native transport seam for registrations that declare `available`; this
/// keeps extension execution out of the pure rectangle planner.
pub fn probe_extension_pane_availability<F>(
    panes: &[SessionPane],
    files: &[DiffFile],
    selected_file_id: Option<&str>,
    selected_hunk_index: Option<usize>,
    current_line: Option<&ExtensionCurrentLinePaint>,
    retain_current_line_registration_ids: &BTreeSet<u64>,
    mut evaluate: F,
) -> PaneAvailabilityProbe
where
    F: FnMut(&RegisteredExtensionPane, PaneAvailabilityContext<'_>) -> Result<bool, String>,
{
    let mut probe = PaneAvailabilityProbe::default();
    for pane in panes {
        let registration = &pane.registered;
        if registration.pane.current_line
            && retain_current_line_registration_ids.contains(&registration.identity)
        {
            probe
                .available_registration_ids
                .insert(registration.identity);
            continue;
        }
        if !registration.pane.available {
            probe
                .available_registration_ids
                .insert(registration.identity);
            continue;
        }
        let context = PaneAvailabilityContext {
            files,
            selected_file_id,
            selected_hunk_index,
            placement: pane.placement,
            current_line: registration
                .pane
                .current_line
                .then_some(current_line)
                .flatten(),
        };
        match evaluate(registration, context) {
            Ok(true) => {
                probe
                    .available_registration_ids
                    .insert(registration.identity);
            }
            Ok(false) => {}
            Err(error) => probe.failures.push(PaneAvailabilityFailure {
                pane: pane.clone(),
                error,
            }),
        }
    }
    probe
}

/// Expand the visible divider to Hunk's five-cell pointer target on its resize axis.
#[must_use]
pub const fn pane_divider_hit_area(divider: Rect, placement: PanePlacement) -> Rect {
    if matches!(placement, PanePlacement::Left | PanePlacement::Right) {
        Rect::new(
            divider.x.saturating_sub(PANE_DIVIDER_HIT_AREA_OFFSET),
            divider.y,
            PANE_DIVIDER_HIT_AREA_SIZE,
            divider.height,
        )
    } else {
        Rect::new(
            divider.x,
            divider.y.saturating_sub(PANE_DIVIDER_HIT_AREA_OFFSET),
            divider.width,
            PANE_DIVIDER_HIT_AREA_SIZE,
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExtensionPaneSpec {
    pub key: String,
    pub pane: PaneRegistration,
}

impl From<&SessionPane> for ExtensionPaneSpec {
    fn from(value: &SessionPane) -> Self {
        Self {
            key: value.key.clone(),
            pane: value.registered.pane.clone(),
        }
    }
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
    use workdeck_core::ReviewSide;
    use workdeck_diff::{DiffRow, SplitLineCell, SplitLineKind};
    use workdeck_extension_api::ExtensionPaneSize;

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

    fn current_line_paint() -> ExtensionCurrentLinePaint {
        let stable_key = "line:0:new:1";
        let cursor = LineCursor {
            file_id: "alpha".into(),
            hunk_index: 0,
            stable_key: stable_key.into(),
            target: LineCursorTarget {
                side: ReviewSide::New,
                line: 1,
            },
            expanded_gap_key: None,
        };
        let empty = SplitLineCell {
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
                    file_id: "alpha".into(),
                    hunk_index: 0,
                    left: empty.clone(),
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
                replaces: None,
                current_line: false,
                available: false,
            },
        }
    }

    #[test]
    fn frozen_hunk_extension_pane_oracles_cover_both_pinned_trees() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/extension-panes.json"
        ))
        .unwrap();
        assert_eq!(oracle["baselineOracle"]["passed"], 16);
        assert_eq!(oracle["baselineOracle"]["expectations"], 46);
        assert_eq!(oracle["stableOracle"]["passed"], 12);
        assert_eq!(oracle["stableOracle"]["expectations"], 37);
        assert_eq!(oracle["authoritative"], "baseline");
    }

    #[test]
    fn offers_the_bundled_files_pane_before_user_panes() {
        let user = registration("meta", "extra");
        let panes = build_session_panes(&[user]);
        assert_eq!(
            panes
                .iter()
                .map(|pane| pane.key.as_str())
                .collect::<Vec<_>>(),
            [WORKDECK_FILES_PANE_KEY, "meta:extra"]
        );
        assert_eq!(
            panes
                .iter()
                .map(|pane| pane.default_open)
                .collect::<Vec<_>>(),
            [true, false]
        );
    }

    #[test]
    fn a_replacement_changes_only_the_initial_bundled_files_default() {
        let mut replacement = registration("meta", "files");
        Arc::get_mut(&mut replacement).unwrap().pane.replaces =
            Some(WORKDECK_FILES_PANE_KEY.into());
        let panes = build_session_panes(&[replacement]);
        assert_eq!(
            panes
                .iter()
                .map(|pane| (pane.key.as_str(), pane.default_open))
                .collect::<Vec<_>>(),
            [(WORKDECK_FILES_PANE_KEY, false), ("meta:files", true)]
        );
    }

    #[test]
    fn replacement_defaults_apply_to_any_registered_pane_key() {
        let mut base = registration("meta", "base");
        Arc::get_mut(&mut base).unwrap().pane.default_open = true;
        let mut replacement = registration("other", "replacement");
        Arc::get_mut(&mut replacement).unwrap().pane.replaces = Some("meta:base".into());
        let panes = build_session_panes(&[base, replacement]);
        assert!(
            !panes
                .iter()
                .find(|pane| pane.key == "meta:base")
                .unwrap()
                .default_open
        );
        assert!(
            panes
                .iter()
                .find(|pane| pane.key == "other:replacement")
                .unwrap()
                .default_open
        );
    }

    #[test]
    fn preserves_open_choices_across_reloads_and_applies_new_defaults() {
        let mut extra = registration("meta", "extra");
        Arc::get_mut(&mut extra).unwrap().pane.default_open = true;
        let before = build_session_panes(&[extra]);
        let initial = initial_pane_open_state(&before);
        let closed = Arc::new(PaneOpenState {
            known: initial.known.clone(),
            open: vec![WORKDECK_FILES_PANE_KEY.into()],
        });

        let mut reloaded_extra = registration("meta", "extra");
        Arc::get_mut(&mut reloaded_extra).unwrap().pane.default_open = true;
        let mut fresh = registration("meta", "fresh");
        Arc::get_mut(&mut fresh).unwrap().pane.default_open = true;
        let after = build_session_panes(&[reloaded_extra, fresh]);
        let reconciled = reconcile_pane_open_state(&after, &closed);
        assert_eq!(reconciled.open, [WORKDECK_FILES_PANE_KEY, "meta:fresh"]);
        assert!(Arc::ptr_eq(
            &reconciled,
            &reconcile_pane_open_state(&after, &reconciled)
        ));
    }

    #[test]
    fn resolves_a_named_pane_slot_to_its_open_owner_or_fallback() {
        let mut replacement = registration("meta", "files");
        let replacement_id = replacement.identity;
        Arc::get_mut(&mut replacement).unwrap().pane.replaces =
            Some(WORKDECK_FILES_PANE_KEY.into());
        let panes = build_session_panes(&[replacement]);
        let open_replacement = BTreeSet::from(["meta:files".into()]);
        assert_eq!(
            resolve_pane_slot_key(
                &panes,
                WORKDECK_FILES_PANE_KEY,
                &open_replacement,
                &BTreeSet::new()
            ),
            "meta:files"
        );
        assert_eq!(
            resolve_pane_slot_key(
                &panes,
                WORKDECK_FILES_PANE_KEY,
                &BTreeSet::from([WORKDECK_FILES_PANE_KEY.into()]),
                &BTreeSet::new()
            ),
            WORKDECK_FILES_PANE_KEY
        );
        assert_eq!(
            resolve_pane_slot_key(
                &panes,
                WORKDECK_FILES_PANE_KEY,
                &BTreeSet::new(),
                &BTreeSet::new()
            ),
            "meta:files"
        );
        assert_eq!(
            resolve_pane_slot_key(
                &panes,
                WORKDECK_FILES_PANE_KEY,
                &open_replacement,
                &BTreeSet::from([replacement_id])
            ),
            WORKDECK_FILES_PANE_KEY
        );
    }

    #[test]
    fn follows_named_replacement_chains_to_the_pane_filling_the_slot() {
        let mut first = registration("meta", "files");
        Arc::get_mut(&mut first).unwrap().pane.replaces = Some(WORKDECK_FILES_PANE_KEY.into());
        let mut second = registration("other", "files");
        Arc::get_mut(&mut second).unwrap().pane.replaces = Some("meta:files".into());
        let panes = build_session_panes(&[first, second]);
        assert_eq!(
            resolve_pane_slot_key(
                &panes,
                WORKDECK_FILES_PANE_KEY,
                &BTreeSet::from(["other:files".into()]),
                &BTreeSet::new()
            ),
            "other:files"
        );
        assert_eq!(
            resolve_pane_slot_key(
                &panes,
                WORKDECK_FILES_PANE_KEY,
                &BTreeSet::from(["meta:files".into()]),
                &BTreeSet::new()
            ),
            "meta:files"
        );
        assert_eq!(
            resolve_pane_slot_key(
                &panes,
                WORKDECK_FILES_PANE_KEY,
                &BTreeSet::new(),
                &BTreeSet::new()
            ),
            "other:files"
        );
    }

    #[test]
    fn resolves_bare_ids_locally_and_qualified_ids_exactly() {
        let panes =
            build_session_panes(&[registration("meta", "extra"), registration("meta", "files")]);
        assert_eq!(
            resolve_pane_key(&panes, "meta", "extra").as_deref(),
            Some("meta:extra")
        );
        assert_eq!(
            resolve_pane_key(&panes, "meta", "files").as_deref(),
            Some("meta:files")
        );
        assert_eq!(
            resolve_pane_key(&panes, "meta", WORKDECK_FILES_PANE_KEY).as_deref(),
            Some(WORKDECK_FILES_PANE_KEY)
        );
        assert_eq!(resolve_pane_key(&panes, "other", "files"), None);
        assert_eq!(
            resolve_pane_key(&panes, "other", "meta:extra").as_deref(),
            Some("meta:extra")
        );
    }

    #[test]
    fn plans_all_four_edges_around_one_review_rectangle() {
        let panes = [
            pane(
                "left",
                PanePlacement::Left,
                ExtensionPaneSize {
                    preferred: 20,
                    min: Some(20),
                    max: Some(20),
                    fraction: None,
                },
            ),
            pane(
                "right",
                PanePlacement::Right,
                ExtensionPaneSize {
                    preferred: 15,
                    min: Some(15),
                    max: Some(15),
                    fraction: None,
                },
            ),
            pane(
                "top",
                PanePlacement::Top,
                ExtensionPaneSize {
                    preferred: 4,
                    min: Some(4),
                    max: Some(4),
                    fraction: None,
                },
            ),
            pane(
                "bottom",
                PanePlacement::Bottom,
                ExtensionPaneSize {
                    preferred: 3,
                    min: Some(3),
                    max: Some(3),
                    fraction: None,
                },
            ),
        ];
        let open = panes.iter().map(|pane| pane.key.clone()).collect();
        let plan = plan_extension_panes(
            &panes,
            &open,
            &BTreeMap::new(),
            Rect::new(0, 0, 100, 30),
            40,
            5,
        );
        assert_eq!(plan.review_bounds, Rect::new(20, 4, 65, 23));
        assert_eq!(
            plan.panes
                .iter()
                .map(|pane| pane.pane.placement)
                .collect::<Vec<_>>(),
            [
                PanePlacement::Left,
                PanePlacement::Right,
                PanePlacement::Top,
                PanePlacement::Bottom
            ]
        );
    }

    #[test]
    fn separates_commit_phase_availability_from_pure_geometry_planning() {
        let mut registered = registration("a", "detail");
        {
            let pane = &mut Arc::get_mut(&mut registered).unwrap().pane;
            pane.placement = PanePlacement::Bottom;
            pane.height = Some(ExtensionPaneSize {
                preferred: 3,
                min: Some(3),
                max: Some(3),
                fraction: None,
            });
            pane.current_line = true;
            pane.available = true;
        }
        let panes = build_session_panes(&[Arc::clone(&registered)]);
        let mut allowed = false;
        let mut calls = 0;
        let unavailable = probe_extension_pane_availability(
            &panes,
            &[],
            None,
            None,
            None,
            &BTreeSet::new(),
            |_, context| {
                calls += 1;
                Ok(allowed && context.current_line.is_some())
            },
        );
        assert!(
            !unavailable
                .available_registration_ids
                .contains(&registered.identity)
        );
        assert_eq!(calls, 1);

        allowed = true;
        let paint = current_line_paint();
        let restored = probe_extension_pane_availability(
            &panes,
            &[],
            None,
            None,
            Some(&paint),
            &BTreeSet::new(),
            |_, context| {
                calls += 1;
                Ok(allowed && context.current_line.is_some())
            },
        );
        assert!(
            restored
                .available_registration_ids
                .contains(&registered.identity)
        );

        let calls_before_pending = calls;
        let retained = probe_extension_pane_availability(
            &panes,
            &[],
            None,
            None,
            None,
            &BTreeSet::from([registered.identity]),
            |_, _| {
                calls += 1;
                Ok(false)
            },
        );
        assert!(
            retained
                .available_registration_ids
                .contains(&registered.identity)
        );
        assert_eq!(calls, calls_before_pending);
    }

    #[test]
    fn does_not_retain_a_same_key_replacement_by_stale_registration_identity() {
        let mut previous = registration("a", "detail");
        {
            let pane = &mut Arc::get_mut(&mut previous).unwrap().pane;
            pane.current_line = true;
            pane.available = true;
        }
        let mut replacement = registration("a", "detail");
        {
            let pane = &mut Arc::get_mut(&mut replacement).unwrap().pane;
            pane.current_line = true;
            pane.available = true;
        }
        let panes = build_session_panes(&[Arc::clone(&replacement)]);
        let mut calls = 0;
        let probe = probe_extension_pane_availability(
            &panes,
            &[],
            None,
            None,
            None,
            &BTreeSet::from([previous.identity]),
            |_, _| {
                calls += 1;
                Ok(false)
            },
        );
        assert_eq!(calls, 1);
        assert!(
            !probe
                .available_registration_ids
                .contains(&replacement.identity)
        );
    }

    #[test]
    fn returns_availability_failures_without_quarantining_or_notifying() {
        let mut throwing = registration("a", "throwing");
        Arc::get_mut(&mut throwing).unwrap().pane.available = true;
        let mut asynchronous = registration("a", "async");
        Arc::get_mut(&mut asynchronous).unwrap().pane.available = true;
        let panes = build_session_panes(&[throwing, asynchronous]);
        let probe = probe_extension_pane_availability(
            &panes,
            &[],
            None,
            None,
            None,
            &BTreeSet::new(),
            |registered, _| {
                Err(if registered.pane.id == "throwing" {
                    "availability exploded".into()
                } else {
                    "available() must return a boolean synchronously".into()
                })
            },
        );
        assert_eq!(probe.available_registration_ids.len(), 1);
        assert_eq!(
            probe
                .failures
                .iter()
                .map(|failure| failure.error.as_str())
                .collect::<Vec<_>>(),
            [
                "availability exploded",
                "available() must return a boolean synchronously"
            ]
        );
    }

    #[test]
    fn resolves_responsive_targets_from_the_full_body_axis_before_manual_overrides() {
        let panes = [
            pane(
                "left",
                PanePlacement::Left,
                ExtensionPaneSize {
                    preferred: 34,
                    min: Some(22),
                    max: Some(56),
                    fraction: Some(0.16),
                },
            ),
            pane(
                "top",
                PanePlacement::Top,
                ExtensionPaneSize {
                    preferred: 8,
                    min: Some(3),
                    max: Some(12),
                    fraction: Some(0.25),
                },
            ),
        ];
        let open = panes.iter().map(|pane| pane.key.clone()).collect();
        let plan = |width, overrides: &BTreeMap<String, u16>| {
            plan_extension_panes(&panes, &open, overrides, Rect::new(0, 0, width, 40), 48, 5)
        };
        assert_eq!(
            plan(100, &BTreeMap::new())
                .panes
                .iter()
                .map(|pane| (pane.bounds.width, pane.bounds.height))
                .collect::<Vec<_>>(),
            [(22, 40), (77, 10)]
        );
        assert_eq!(
            plan(238, &BTreeMap::new())
                .panes
                .iter()
                .map(|pane| (pane.bounds.width, pane.bounds.height))
                .collect::<Vec<_>>(),
            [(38, 40), (199, 10)]
        );
        assert_eq!(plan(400, &BTreeMap::new()).panes[0].bounds.width, 56);
        assert_eq!(
            plan(238, &BTreeMap::from([("demo:left".into(), 47)])).panes[0]
                .bounds
                .width,
            47
        );
    }

    #[test]
    fn rounds_fractional_cells_before_bounds_and_allocates_competing_panes_in_order() {
        let panes = [
            pane(
                "one",
                PanePlacement::Left,
                ExtensionPaneSize {
                    preferred: 20,
                    min: Some(10),
                    max: Some(80),
                    fraction: Some(0.6),
                },
            ),
            pane(
                "two",
                PanePlacement::Left,
                ExtensionPaneSize {
                    preferred: 20,
                    min: Some(10),
                    max: Some(80),
                    fraction: Some(0.6),
                },
            ),
            pane(
                "three",
                PanePlacement::Left,
                ExtensionPaneSize {
                    preferred: 20,
                    min: Some(20),
                    max: Some(80),
                    fraction: Some(0.6),
                },
            ),
        ];
        let open = panes.iter().map(|pane| pane.key.clone()).collect();
        let plan = plan_extension_panes(
            &panes,
            &open,
            &BTreeMap::new(),
            Rect::new(0, 0, 100, 30),
            20,
            5,
        );
        assert_eq!(
            plan.panes
                .iter()
                .map(|pane| (pane.key.as_str(), pane.bounds.width))
                .collect::<Vec<_>>(),
            [("demo:one", 60), ("demo:two", 18)]
        );
        assert_eq!(plan.omitted_keys, ["demo:three"]);

        let half = pane(
            "half",
            PanePlacement::Left,
            ExtensionPaneSize {
                preferred: 10,
                min: Some(1),
                max: Some(50),
                fraction: Some(0.1),
            },
        );
        let half_plan = plan_extension_panes(
            std::slice::from_ref(&half),
            &BTreeSet::from([half.key.clone()]),
            &BTreeMap::new(),
            Rect::new(0, 0, 225, 30),
            20,
            5,
        );
        assert_eq!(half_plan.panes[0].bounds.width, 23);
    }

    #[test]
    fn sizes_top_and_bottom_fractional_panes_from_the_full_body_height() {
        let panes = [
            pane(
                "top",
                PanePlacement::Top,
                ExtensionPaneSize {
                    preferred: 8,
                    min: Some(3),
                    max: Some(20),
                    fraction: Some(0.25),
                },
            ),
            pane(
                "bottom",
                PanePlacement::Bottom,
                ExtensionPaneSize {
                    preferred: 8,
                    min: Some(3),
                    max: Some(20),
                    fraction: Some(0.25),
                },
            ),
        ];
        let open = panes.iter().map(|pane| pane.key.clone()).collect();
        let heights = |height| {
            plan_extension_panes(
                &panes,
                &open,
                &BTreeMap::new(),
                Rect::new(0, 0, 100, height),
                20,
                5,
            )
            .panes
            .iter()
            .map(|pane| pane.bounds.height)
            .collect::<Vec<_>>()
        };
        assert_eq!(heights(40), [10, 10]);
        assert_eq!(heights(60), [15, 15]);
    }

    #[test]
    fn uses_explicit_height_overrides_and_reserves_only_resizable_dividers() {
        let top = pane(
            "top",
            PanePlacement::Top,
            ExtensionPaneSize {
                preferred: 4,
                min: Some(2),
                max: Some(8),
                fraction: None,
            },
        );
        let plan = plan_extension_panes(
            std::slice::from_ref(&top),
            &BTreeSet::from([top.key.clone()]),
            &BTreeMap::from([(top.key.clone(), 7)]),
            Rect::new(0, 0, 100, 20),
            40,
            5,
        );
        assert_eq!(plan.panes[0].bounds, Rect::new(0, 0, 100, 7));
        assert_eq!(plan.panes[0].divider, Some(Rect::new(0, 7, 100, 1)));
        assert_eq!(plan.review_bounds, Rect::new(0, 8, 100, 12));
    }

    #[test]
    fn omits_later_panes_when_minimum_review_bounds_are_exhausted() {
        let panes = ["one", "two", "three"].map(|id| {
            pane(
                id,
                PanePlacement::Left,
                ExtensionPaneSize {
                    preferred: 30,
                    min: Some(20),
                    max: None,
                    fraction: None,
                },
            )
        });
        let open = panes.iter().map(|pane| pane.key.clone()).collect();
        let plan = plan_extension_panes(
            &panes,
            &open,
            &BTreeMap::new(),
            Rect::new(0, 0, 110, 30),
            48,
            5,
        );
        assert_eq!(
            plan.panes
                .iter()
                .map(|pane| pane.key.as_str())
                .collect::<Vec<_>>(),
            ["demo:one", "demo:two"]
        );
        assert_eq!(plan.omitted_keys, ["demo:three"]);
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

    #[test]
    fn expands_divider_pointer_targets_to_five_cells_on_the_resize_axis() {
        assert_eq!(
            pane_divider_hit_area(Rect::new(20, 3, 1, 12), PanePlacement::Right),
            Rect::new(18, 3, 5, 12)
        );
        assert_eq!(
            pane_divider_hit_area(Rect::new(4, 10, 30, 1), PanePlacement::Bottom),
            Rect::new(4, 8, 30, 5)
        );
    }
}
