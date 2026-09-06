//! Committed-review lifecycle publication for native extensions.
//!
//! This is the state-machine port of Hunk's `useExtensionReviewEvents` hook. Ratatui has no React
//! effect lifecycle, so registry, review, and projection identities are explicit and selection
//! settlement is driven by the application tick.

use std::time::{Duration, Instant};

use workdeck_extension_api::{
    ExtensionDiffFile, ExtensionLayoutMode, ExtensionLifecycleEvent, ExtensionResolvedLayout,
    ExtensionReviewSnapshotNote,
};
use workdeck_review::diff_extension_review_notes;

/// Trailing delay that collapses rapid navigation into one settled selection event.
pub const SELECTION_CHANGED_DEBOUNCE: Duration = Duration::from_millis(150);
pub const SELECTION_CHANGED_DEBOUNCE_MS: u64 = 150;

/// Immutable facts committed by the current Ratatui review frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionReviewEventFacts {
    pub registry_generation: u64,
    /// Changes whenever reload replaces the public file objects, even if their content is equal.
    pub review_projection_generation: u64,
    pub review_generation: String,
    pub review_notes: Vec<ExtensionReviewSnapshotNote>,
    pub filter: String,
    pub layout_mode: ExtensionLayoutMode,
    pub resolved_layout: ExtensionResolvedLayout,
    pub selected_file: Option<ExtensionDiffFile>,
    pub selected_file_id: Option<String>,
    pub selected_hunk_index: Option<usize>,
    pub theme_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SelectionInput {
    registry_generation: u64,
    review_projection_generation: u64,
    selected_file: Option<ExtensionDiffFile>,
    selected_file_id: Option<String>,
    selected_hunk_index: Option<usize>,
}

impl From<&ExtensionReviewEventFacts> for SelectionInput {
    fn from(facts: &ExtensionReviewEventFacts) -> Self {
        Self {
            registry_generation: facts.registry_generation,
            review_projection_generation: facts.review_projection_generation,
            selected_file: facts.selected_file.clone(),
            selected_file_id: facts.selected_file_id.clone(),
            selected_hunk_index: facts
                .selected_file_id
                .as_ref()
                .and(facts.selected_hunk_index),
        }
    }
}

#[derive(Debug, Clone)]
struct PendingSelection {
    token: u64,
    deadline: Instant,
    input: SelectionInput,
}

#[derive(Debug, Clone)]
struct ProjectionBaseline<T> {
    registry_generation: u64,
    value: T,
}

#[derive(Debug, Clone)]
struct ReportedNotes {
    registry_generation: u64,
    review_generation: String,
    notes: Vec<ExtensionReviewSnapshotNote>,
}

/// Pure lifecycle state machine shared by the real TUI and deterministic parity tests.
#[derive(Debug, Default)]
pub struct ExtensionReviewEventController {
    active_registry_generation: Option<u64>,
    next_selection_token: u64,
    last_selection_input: Option<SelectionInput>,
    pending_selection: Option<PendingSelection>,
    last_viewed_file: Option<(u64, u64, String)>,
    last_viewed_hunk: Option<(u64, String, usize)>,
    reported_notes: Option<ReportedNotes>,
    reported_filter: Option<ProjectionBaseline<String>>,
    reported_layout: Option<ProjectionBaseline<(ExtensionLayoutMode, ExtensionResolvedLayout)>>,
    reported_theme: Option<ProjectionBaseline<String>>,
}

impl ExtensionReviewEventController {
    /// Commit current facts, returning immediate note/filter/layout/theme changes.
    ///
    /// Selection attention is scheduled separately and returned by `settle_due` after the exact
    /// trailing debounce. A replacement registry establishes fresh baselines without describing
    /// its initial values as changes.
    pub fn update(
        &mut self,
        facts: &ExtensionReviewEventFacts,
        now: Instant,
    ) -> Vec<ExtensionLifecycleEvent> {
        if self.active_registry_generation != Some(facts.registry_generation) {
            self.replace_registry(facts, now);
            return Vec::new();
        }

        let mut events = self.sync_notes(facts);
        if self.reported_filter.as_ref().is_some_and(|reported| {
            reported.registry_generation == facts.registry_generation
                && reported.value != facts.filter
        }) {
            events.push(ExtensionLifecycleEvent::FilterChanged {
                filter: facts.filter.clone(),
            });
        }
        self.reported_filter = Some(ProjectionBaseline {
            registry_generation: facts.registry_generation,
            value: facts.filter.clone(),
        });

        let layout = (facts.layout_mode, facts.resolved_layout);
        if self.reported_layout.as_ref().is_some_and(|reported| {
            reported.registry_generation == facts.registry_generation && reported.value != layout
        }) {
            events.push(ExtensionLifecycleEvent::LayoutChanged {
                mode: facts.layout_mode,
                layout: facts.resolved_layout,
            });
        }
        self.reported_layout = Some(ProjectionBaseline {
            registry_generation: facts.registry_generation,
            value: layout,
        });

        if self.reported_theme.as_ref().is_some_and(|reported| {
            reported.registry_generation == facts.registry_generation
                && reported.value != facts.theme_id
        }) {
            events.push(ExtensionLifecycleEvent::ThemeChanged {
                theme_id: facts.theme_id.clone(),
            });
        }
        self.reported_theme = Some(ProjectionBaseline {
            registry_generation: facts.registry_generation,
            value: facts.theme_id.clone(),
        });

        let selection = SelectionInput::from(facts);
        if self.last_selection_input.as_ref() != Some(&selection) {
            self.schedule_selection(selection, now);
        }
        events
    }

    /// Deliver the current settled selection once its trailing deadline has elapsed.
    pub fn settle_due(&mut self, now: Instant) -> Vec<ExtensionLifecycleEvent> {
        let Some(pending) = self.pending_selection.as_ref() else {
            return Vec::new();
        };
        if now < pending.deadline {
            return Vec::new();
        }
        let token = pending.token;
        self.settle_selection(token)
    }

    /// Invalidate all pending work when the mounted review is torn down.
    pub fn unmount(&mut self) {
        self.next_selection_token = self.next_selection_token.saturating_add(1);
        self.active_registry_generation = None;
        self.pending_selection = None;
        self.last_selection_input = None;
    }

    #[doc(hidden)]
    #[must_use]
    pub fn pending_selection_token(&self) -> Option<u64> {
        self.pending_selection.as_ref().map(|pending| pending.token)
    }

    #[doc(hidden)]
    pub fn settle_selection(&mut self, token: u64) -> Vec<ExtensionLifecycleEvent> {
        let Some(pending) = self.pending_selection.take() else {
            return Vec::new();
        };
        if pending.token != token
            || self.active_registry_generation != Some(pending.input.registry_generation)
        {
            if pending.token != token {
                self.pending_selection = Some(pending);
            }
            return Vec::new();
        }

        let input = pending.input;
        let mut events = vec![ExtensionLifecycleEvent::SelectionChanged {
            file_id: input.selected_file_id.clone(),
            hunk_index: input.selected_hunk_index,
        }];
        if let Some(file) = input.selected_file {
            let file_identity = (
                input.registry_generation,
                input.review_projection_generation,
                file.id.clone(),
            );
            if self.last_viewed_file.as_ref() != Some(&file_identity) {
                self.last_viewed_file = Some(file_identity);
                events.push(ExtensionLifecycleEvent::FileViewed {
                    file: file.clone(),
                    hunk_index: input.selected_hunk_index,
                });
            }
            if let Some(hunk_index) = input.selected_hunk_index {
                let hunk_identity = (input.registry_generation, file.id.clone(), hunk_index);
                if self.last_viewed_hunk.as_ref() != Some(&hunk_identity) {
                    self.last_viewed_hunk = Some(hunk_identity);
                    events.push(ExtensionLifecycleEvent::HunkViewed { file, hunk_index });
                }
            }
        }
        events
    }

    fn replace_registry(&mut self, facts: &ExtensionReviewEventFacts, now: Instant) {
        self.active_registry_generation = Some(facts.registry_generation);
        self.reported_notes = Some(ReportedNotes {
            registry_generation: facts.registry_generation,
            review_generation: facts.review_generation.clone(),
            notes: facts.review_notes.clone(),
        });
        self.reported_filter = Some(ProjectionBaseline {
            registry_generation: facts.registry_generation,
            value: facts.filter.clone(),
        });
        self.reported_layout = Some(ProjectionBaseline {
            registry_generation: facts.registry_generation,
            value: (facts.layout_mode, facts.resolved_layout),
        });
        self.reported_theme = Some(ProjectionBaseline {
            registry_generation: facts.registry_generation,
            value: facts.theme_id.clone(),
        });
        self.schedule_selection(SelectionInput::from(facts), now);
    }

    fn schedule_selection(&mut self, input: SelectionInput, now: Instant) {
        self.next_selection_token = self.next_selection_token.saturating_add(1);
        self.last_selection_input = Some(input.clone());
        self.pending_selection = Some(PendingSelection {
            token: self.next_selection_token,
            deadline: now + SELECTION_CHANGED_DEBOUNCE,
            input,
        });
    }

    fn sync_notes(&mut self, facts: &ExtensionReviewEventFacts) -> Vec<ExtensionLifecycleEvent> {
        let events = self
            .reported_notes
            .as_ref()
            .filter(|reported| {
                reported.registry_generation == facts.registry_generation
                    && reported.review_generation == facts.review_generation
            })
            .map_or_else(Vec::new, |reported| {
                diff_extension_review_notes(&reported.notes, &facts.review_notes)
                    .into_iter()
                    .map(|change| ExtensionLifecycleEvent::NoteChanged { change })
                    .collect()
            });
        self.reported_notes = Some(ReportedNotes {
            registry_generation: facts.registry_generation,
            review_generation: facts.review_generation.clone(),
            notes: facts.review_notes.clone(),
        });
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_core::{ReviewNoteSource, ReviewSide};
    use workdeck_extension_api::{
        ExtensionDiffStats, ExtensionReviewNoteResolution, ExtensionReviewSnapshotNoteAnchor,
    };

    fn file(id: &str, patch: &str) -> ExtensionDiffFile {
        ExtensionDiffFile {
            id: id.into(),
            path: format!("{id}.rs"),
            previous_path: None,
            patch: patch.into(),
            language: Some("rust".into()),
            stats: ExtensionDiffStats {
                additions: 1,
                deletions: 1,
            },
            metadata: serde_json::json!({ "hunks": [] }),
            change_type: Some(workdeck_extension_api::ExtensionVcsFileChangeType::Change),
            stats_truncated: false,
            hunks: Vec::new(),
            agent: None,
            is_untracked: false,
            is_binary: false,
            is_too_large: false,
        }
    }

    fn note(id: &str, summary: &str) -> ExtensionReviewSnapshotNote {
        ExtensionReviewSnapshotNote {
            id: id.into(),
            parent_id: None,
            source: ReviewNoteSource::User,
            original_source: None,
            file_key: "alpha".into(),
            anchor: ExtensionReviewSnapshotNoteAnchor {
                old_range: None,
                new_range: Some([1, 1]),
                preferred: None,
                intersecting_hunk_indices: vec![0],
                owner_hunk_index: Some(0),
            },
            summary: summary.into(),
            rationale: None,
            markup: None,
            title: None,
            author: None,
            created_at: None,
            updated_at: None,
            editable: true,
            tags: Vec::new(),
            confidence: None,
            resolution: ExtensionReviewNoteResolution::Active,
        }
    }

    fn facts(
        registry: u64,
        projection: u64,
        selected: Option<ExtensionDiffFile>,
    ) -> ExtensionReviewEventFacts {
        ExtensionReviewEventFacts {
            registry_generation: registry,
            review_projection_generation: projection,
            review_generation: format!("review:{projection}"),
            review_notes: Vec::new(),
            filter: String::new(),
            layout_mode: ExtensionLayoutMode::Auto,
            resolved_layout: ExtensionResolvedLayout::Split,
            selected_file_id: selected.as_ref().map(|file| file.id.clone()),
            selected_hunk_index: selected.as_ref().map(|_| 0),
            selected_file: selected,
            theme_id: "github-dark-default".into(),
        }
    }

    fn names(events: Vec<ExtensionLifecycleEvent>) -> Vec<String> {
        events
            .into_iter()
            .map(|event| event.into_parts().0)
            .collect()
    }

    #[test]
    fn debounces_initial_selection_and_suppresses_initial_projection_events() {
        let now = Instant::now();
        let mut controller = ExtensionReviewEventController::default();
        assert!(
            controller
                .update(&facts(1, 1, Some(file("alpha", "one"))), now)
                .is_empty()
        );
        assert!(
            controller
                .settle_due(now + Duration::from_millis(149))
                .is_empty()
        );
        assert_eq!(
            names(controller.settle_due(now + SELECTION_CHANGED_DEBOUNCE)),
            ["selection_changed", "file_viewed", "hunk_viewed"]
        );
        assert_eq!(SELECTION_CHANGED_DEBOUNCE_MS, 150);
    }

    #[test]
    fn collapses_rapid_selection_changes_and_rejects_the_replaced_token() {
        let now = Instant::now();
        let mut controller = ExtensionReviewEventController::default();
        let alpha = facts(1, 1, Some(file("alpha", "one")));
        controller.update(&alpha, now);
        let stale = controller.pending_selection_token().unwrap();
        let beta = facts(1, 1, Some(file("beta", "two")));
        controller.update(&beta, now + Duration::from_millis(10));
        assert!(controller.settle_selection(stale).is_empty());
        let current = controller.pending_selection_token().unwrap();
        let events = controller.settle_selection(current);
        assert_eq!(
            names(events.clone()),
            ["selection_changed", "file_viewed", "hunk_viewed"]
        );
        assert_eq!(events[0].clone().into_parts().1["fileId"], "beta");
    }

    #[test]
    fn replacement_file_projection_reemits_file_viewed_but_not_the_same_hunk() {
        let now = Instant::now();
        let mut controller = ExtensionReviewEventController::default();
        controller.update(&facts(1, 1, Some(file("alpha", "one"))), now);
        controller.settle_due(now + SELECTION_CHANGED_DEBOUNCE);
        controller.update(&facts(1, 2, Some(file("alpha", "two"))), now);
        assert_eq!(
            names(controller.settle_due(now + SELECTION_CHANGED_DEBOUNCE)),
            ["selection_changed", "file_viewed"]
        );
    }

    #[test]
    fn same_file_hunk_change_does_not_reemit_file_viewed() {
        let now = Instant::now();
        let mut controller = ExtensionReviewEventController::default();
        let mut current = facts(1, 1, Some(file("alpha", "one")));
        controller.update(&current, now);
        controller.settle_due(now + SELECTION_CHANGED_DEBOUNCE);
        current.selected_hunk_index = Some(2);
        controller.update(&current, now);
        assert_eq!(
            names(controller.settle_due(now + SELECTION_CHANGED_DEBOUNCE)),
            ["selection_changed", "hunk_viewed"]
        );
    }

    #[test]
    fn empty_selection_emits_no_view_attention_events() {
        let now = Instant::now();
        let mut controller = ExtensionReviewEventController::default();
        controller.update(&facts(1, 1, Some(file("alpha", "one"))), now);
        controller.settle_due(now + SELECTION_CHANGED_DEBOUNCE);
        controller.update(&facts(1, 1, None), now);
        let events = controller.settle_due(now + SELECTION_CHANGED_DEBOUNCE);
        assert_eq!(names(events.clone()), ["selection_changed"]);
        assert_eq!(
            events[0].clone().into_parts().1,
            serde_json::json!({"fileId": null, "hunkIndex": null})
        );
    }

    #[test]
    fn publishes_only_committed_filter_layout_and_theme_changes_in_order() {
        let now = Instant::now();
        let mut controller = ExtensionReviewEventController::default();
        let mut current = facts(1, 1, Some(file("alpha", "one")));
        controller.update(&current, now);
        assert!(controller.update(&current, now).is_empty());
        current.filter = "src/".into();
        current.layout_mode = ExtensionLayoutMode::Stack;
        current.resolved_layout = ExtensionResolvedLayout::Stack;
        current.theme_id = "github-light-default".into();
        assert_eq!(
            names(controller.update(&current, now)),
            ["filter_changed", "layout_changed", "theme_changed"]
        );
    }

    #[test]
    fn replacement_registry_retires_delayed_work_and_reestablishes_view_attention() {
        let now = Instant::now();
        let mut controller = ExtensionReviewEventController::default();
        controller.update(&facts(1, 1, Some(file("alpha", "one"))), now);
        let stale = controller.pending_selection_token().unwrap();
        controller.update(&facts(2, 1, Some(file("alpha", "one"))), now);
        assert!(controller.settle_selection(stale).is_empty());
        let current = controller.pending_selection_token().unwrap();
        assert_eq!(
            names(controller.settle_selection(current)),
            ["selection_changed", "file_viewed", "hunk_viewed"]
        );
    }

    #[test]
    fn replacement_registry_seeds_changed_projection_values_before_reporting_later_changes() {
        let now = Instant::now();
        let mut controller = ExtensionReviewEventController::default();
        controller.update(&facts(1, 1, Some(file("alpha", "one"))), now);
        let mut replacement = facts(2, 1, Some(file("alpha", "one")));
        replacement.filter = "src/".into();
        replacement.layout_mode = ExtensionLayoutMode::Stack;
        replacement.resolved_layout = ExtensionResolvedLayout::Stack;
        replacement.theme_id = "github-light-default".into();
        assert!(controller.update(&replacement, now).is_empty());
        replacement.filter = "test/".into();
        replacement.layout_mode = ExtensionLayoutMode::Auto;
        replacement.resolved_layout = ExtensionResolvedLayout::Split;
        replacement.theme_id = "github-dark-default".into();
        assert_eq!(
            names(controller.update(&replacement, now)),
            ["filter_changed", "layout_changed", "theme_changed"]
        );
    }

    #[test]
    fn saved_note_changes_emit_only_within_one_registry_and_review_generation() {
        let now = Instant::now();
        let mut controller = ExtensionReviewEventController::default();
        let mut current = facts(1, 1, Some(file("alpha", "one")));
        current.review_notes = vec![note("user:1", "before")];
        assert!(controller.update(&current, now).is_empty());
        current.review_notes = vec![note("user:1", "after"), note("live:1", "agent")];
        let events = controller.update(&current, now);
        assert_eq!(names(events.clone()), ["note_changed", "note_changed"]);
        assert_eq!(events[0].clone().into_parts().1["kind"], "updated");
        assert_eq!(events[1].clone().into_parts().1["kind"], "created");

        current.review_generation = "review:2".into();
        current.review_notes = vec![note("other:1", "replacement")];
        assert!(controller.update(&current, now).is_empty());
    }

    #[test]
    fn unmount_invalidates_pending_selection_and_strict_replay_uses_a_new_token() {
        let now = Instant::now();
        let mut controller = ExtensionReviewEventController::default();
        let current = facts(1, 1, Some(file("alpha", "one")));
        controller.update(&current, now);
        let stale = controller.pending_selection_token().unwrap();
        controller.unmount();
        assert!(controller.settle_selection(stale).is_empty());
        controller.update(&current, now);
        let replay = controller.pending_selection_token().unwrap();
        assert_ne!(stale, replay);
    }

    #[test]
    fn frozen_review_event_hook_oracle_records_the_executed_baseline() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../port/hunk/oracles/extension-review-events.json"
        )))
        .unwrap();
        assert_eq!(
            oracle["baselines"][0]["commit"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(oracle["baselines"][0]["tests"], 16);
        assert_eq!(oracle["baselines"][0]["passed"], 16);
        assert_eq!(oracle["baselines"][0]["expect_calls"], 62);
        assert_eq!(oracle["stable_presence"], false);
        assert_eq!(oracle["test_mapping"].as_array().unwrap().len(), 16);
    }

    #[test]
    fn side_type_remains_shared_with_review_navigation() {
        assert_eq!(serde_json::to_value(ReviewSide::Old).unwrap(), "old");
    }
}
