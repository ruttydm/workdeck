//! Renderer-neutral vocabulary for every built-in review command.

use workdeck_core::SemanticReviewLineAddress;

use crate::{
    ReviewSelectionScope, SemanticReviewIntent, SemanticReviewState,
    select_active_editable_review_note_id, select_active_replyable_review_note_id,
    select_normalized_semantic_selection, select_review_gap_for_selection,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppCommandLocus {
    Semantic,
    ClientLocal,
    HostOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppCommandCategory {
    App,
    Review,
    View,
}

impl AppCommandCategory {
    #[must_use]
    pub const fn id_segment(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Review => "review",
            Self::View => "view",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalCommandDirection {
    Up,
    Down,
}

impl VerticalCommandDirection {
    #[must_use]
    pub const fn delta(self) -> isize {
        match self {
            Self::Up => -1,
            Self::Down => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppCommandReviewEffect {
    MoveSelection {
        scope: ReviewSelectionScope,
        direction: VerticalCommandDirection,
    },
    ToggleNoteVisibility,
    StartDraft,
    StartEditActive,
    StartReplyActive,
    ToggleSelectedGap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppCommandCatalogEntry {
    pub id: &'static str,
    pub aliases: &'static [&'static str],
    pub title: &'static str,
    pub category: AppCommandCategory,
    pub default_keys: &'static [&'static str],
    pub locus: AppCommandLocus,
    pub vertical_direction: Option<VerticalCommandDirection>,
    pub review: Option<AppCommandReviewEffect>,
    pub public_to_extensions: bool,
    pub closes_menu: bool,
}

const fn command(
    id: &'static str,
    title: &'static str,
    category: AppCommandCategory,
    default_keys: &'static [&'static str],
    locus: AppCommandLocus,
) -> AppCommandCatalogEntry {
    AppCommandCatalogEntry {
        id,
        aliases: &[],
        title,
        category,
        default_keys,
        locus,
        vertical_direction: None,
        review: None,
        public_to_extensions: true,
        closes_menu: false,
    }
}

const fn closing(mut entry: AppCommandCatalogEntry) -> AppCommandCatalogEntry {
    entry.closes_menu = true;
    entry
}

const fn alias(
    mut entry: AppCommandCatalogEntry,
    aliases: &'static [&'static str],
) -> AppCommandCatalogEntry {
    entry.aliases = aliases;
    entry
}

const fn vertical(
    mut entry: AppCommandCatalogEntry,
    direction: VerticalCommandDirection,
) -> AppCommandCatalogEntry {
    entry.vertical_direction = Some(direction);
    entry
}

const fn semantic(
    mut entry: AppCommandCatalogEntry,
    effect: AppCommandReviewEffect,
) -> AppCommandCatalogEntry {
    entry.review = Some(effect);
    entry
}

use AppCommandCategory::{App, Review, View};
use AppCommandLocus::{ClientLocal, HostOnly, Semantic};
use VerticalCommandDirection::{Down, Up};

/// Order is dispatch order and therefore part of the public command contract.
pub const APP_COMMAND_CATALOG: &[AppCommandCatalogEntry] = &[
    command(
        "workdeck.review.jumpToBottom",
        "Jump to end",
        Review,
        &["G", "end"],
        ClientLocal,
    ),
    command(
        "workdeck.review.jumpToTop",
        "Jump to start",
        Review,
        &["g", "home"],
        ClientLocal,
    ),
    command("workdeck.app.quit", "Quit", App, &["q"], HostOnly),
    closing(command(
        "workdeck.app.toggleHelp",
        "Toggle help",
        App,
        &["?"],
        ClientLocal,
    )),
    closing(command(
        "workdeck.app.openAgentSkill",
        "Show agent skill",
        App,
        &[],
        HostOnly,
    )),
    command(
        "workdeck.app.toggleFocusArea",
        "Switch focus between files and filter",
        App,
        &["tab"],
        ClientLocal,
    ),
    command(
        "workdeck.review.focusFilter",
        "Focus the file filter",
        Review,
        &["/"],
        ClientLocal,
    ),
    closing(semantic(
        command(
            "workdeck.review.startNote",
            "Add a review note",
            Review,
            &["c"],
            Semantic,
        ),
        AppCommandReviewEffect::StartDraft,
    )),
    closing(semantic(
        command(
            "workdeck.review.editActiveNote",
            "Edit active review note",
            Review,
            &["E"],
            Semantic,
        ),
        AppCommandReviewEffect::StartEditActive,
    )),
    closing(semantic(
        command(
            "workdeck.review.replyToActiveNote",
            "Reply to active review note",
            Review,
            &["R"],
            Semantic,
        ),
        AppCommandReviewEffect::StartReplyActive,
    )),
    vertical(
        command(
            "workdeck.review.pageDown",
            "Scroll down one page",
            Review,
            &["pagedown", "space", "f"],
            ClientLocal,
        ),
        Down,
    ),
    vertical(
        command(
            "workdeck.review.pageUp",
            "Scroll up one page",
            Review,
            &["pageup", "b", "shift+space"],
            ClientLocal,
        ),
        Up,
    ),
    vertical(
        command(
            "workdeck.review.halfPageDown",
            "Scroll down half a page",
            Review,
            &["d", "ctrl+d"],
            ClientLocal,
        ),
        Down,
    ),
    vertical(
        command(
            "workdeck.review.halfPageUp",
            "Scroll up half a page",
            Review,
            &["u", "ctrl+u"],
            ClientLocal,
        ),
        Up,
    ),
    vertical(
        command(
            "workdeck.review.stepDown",
            "Scroll down one row",
            Review,
            &["down", "j"],
            ClientLocal,
        ),
        Down,
    ),
    vertical(
        command(
            "workdeck.review.stepUp",
            "Scroll up one row",
            Review,
            &["up", "k"],
            ClientLocal,
        ),
        Up,
    ),
    command(
        "workdeck.review.scrollCodeLeft",
        "Scroll code left",
        Review,
        &["left", "shift+left"],
        ClientLocal,
    ),
    command(
        "workdeck.review.scrollCodeRight",
        "Scroll code right",
        Review,
        &["right", "shift+right"],
        ClientLocal,
    ),
    command(
        "workdeck.review.alignCurrentLineTop",
        "Align current line to top",
        Review,
        &[],
        ClientLocal,
    ),
    command(
        "workdeck.review.alignCurrentLineCenter",
        "Align current line to center",
        Review,
        &[],
        ClientLocal,
    ),
    command(
        "workdeck.review.alignCurrentLineBottom",
        "Align current line to bottom",
        Review,
        &[],
        ClientLocal,
    ),
    closing(command(
        "workdeck.view.cursorLineRow",
        "Highlight the current row",
        View,
        &[],
        ClientLocal,
    )),
    closing(command(
        "workdeck.view.cursorLineNumber",
        "Mark the current line number",
        View,
        &[],
        ClientLocal,
    )),
    closing(command(
        "workdeck.view.cursorLineOff",
        "Hide the current-line marker",
        View,
        &[],
        ClientLocal,
    )),
    closing(command(
        "workdeck.view.layoutSplit",
        "Split layout",
        View,
        &["1"],
        ClientLocal,
    )),
    closing(command(
        "workdeck.view.layoutStack",
        "Stack layout",
        View,
        &["2"],
        ClientLocal,
    )),
    closing(command(
        "workdeck.view.layoutAuto",
        "Auto layout",
        View,
        &["0"],
        ClientLocal,
    )),
    closing(command(
        "workdeck.view.applyFilePresentationToAllMatching",
        "Apply the current file presentation to all matching files",
        View,
        &[],
        ClientLocal,
    )),
    closing(alias(
        command(
            "workdeck.view.toggleFilesPane",
            "Toggle files pane",
            View,
            &["s"],
            ClientLocal,
        ),
        &["workdeck.view.toggleSidebar"],
    )),
    closing(command(
        "workdeck.app.refresh",
        "Refresh the review",
        App,
        &["r"],
        HostOnly,
    )),
    closing(command(
        "workdeck.view.openThemeSelector",
        "Choose theme",
        View,
        &["t"],
        ClientLocal,
    )),
    closing(semantic(
        command(
            "workdeck.view.toggleAgentNotes",
            "Toggle agent notes",
            View,
            &["a"],
            Semantic,
        ),
        AppCommandReviewEffect::ToggleNoteVisibility,
    )),
    closing(command(
        "workdeck.view.toggleLineNumbers",
        "Toggle line numbers",
        View,
        &["l"],
        ClientLocal,
    )),
    closing(command(
        "workdeck.view.toggleLineWrap",
        "Toggle line wrapping",
        View,
        &["w"],
        ClientLocal,
    )),
    closing(command(
        "workdeck.view.toggleMenuBar",
        "Toggle menu bar",
        View,
        &["M"],
        ClientLocal,
    )),
    closing(command(
        "workdeck.view.toggleHunkHeaders",
        "Toggle hunk headers",
        View,
        &["m"],
        ClientLocal,
    )),
    closing(command(
        "workdeck.view.toggleCopyDecorations",
        "Toggle copy decorations",
        View,
        &[],
        ClientLocal,
    )),
    closing(semantic(
        command(
            "workdeck.review.toggleHunkGap",
            "Expand or collapse context for the selected hunk",
            Review,
            &["z"],
            Semantic,
        ),
        AppCommandReviewEffect::ToggleSelectedGap,
    )),
    closing(command(
        "workdeck.review.editSelectedFile",
        "Open the selected file in your editor",
        Review,
        &["e"],
        HostOnly,
    )),
    closing(vertical(
        semantic(
            command(
                "workdeck.review.previousHunk",
                "Previous hunk",
                Review,
                &["["],
                Semantic,
            ),
            AppCommandReviewEffect::MoveSelection {
                scope: ReviewSelectionScope::Hunk,
                direction: Up,
            },
        ),
        Up,
    )),
    closing(vertical(
        semantic(
            command(
                "workdeck.review.nextHunk",
                "Next hunk",
                Review,
                &["]"],
                Semantic,
            ),
            AppCommandReviewEffect::MoveSelection {
                scope: ReviewSelectionScope::Hunk,
                direction: Down,
            },
        ),
        Down,
    )),
    closing(vertical(
        semantic(
            command(
                "workdeck.review.previousFile",
                "Previous file",
                Review,
                &[","],
                Semantic,
            ),
            AppCommandReviewEffect::MoveSelection {
                scope: ReviewSelectionScope::File,
                direction: Up,
            },
        ),
        Up,
    )),
    closing(vertical(
        semantic(
            command(
                "workdeck.review.nextFile",
                "Next file",
                Review,
                &["."],
                Semantic,
            ),
            AppCommandReviewEffect::MoveSelection {
                scope: ReviewSelectionScope::File,
                direction: Down,
            },
        ),
        Down,
    )),
    closing(vertical(
        semantic(
            command(
                "workdeck.review.previousAnnotatedHunk",
                "Previous annotated hunk",
                Review,
                &["{"],
                Semantic,
            ),
            AppCommandReviewEffect::MoveSelection {
                scope: ReviewSelectionScope::AnnotatedHunk,
                direction: Up,
            },
        ),
        Up,
    )),
    closing(vertical(
        semantic(
            command(
                "workdeck.review.nextAnnotatedHunk",
                "Next annotated hunk",
                Review,
                &["}"],
                Semantic,
            ),
            AppCommandReviewEffect::MoveSelection {
                scope: ReviewSelectionScope::AnnotatedHunk,
                direction: Down,
            },
        ),
        Down,
    )),
    closing(vertical(
        semantic(
            command(
                "workdeck.review.previousAnnotatedFile",
                "Previous annotated file",
                Review,
                &[],
                Semantic,
            ),
            AppCommandReviewEffect::MoveSelection {
                scope: ReviewSelectionScope::AnnotatedFile,
                direction: Up,
            },
        ),
        Up,
    )),
    closing(vertical(
        semantic(
            command(
                "workdeck.review.nextAnnotatedFile",
                "Next annotated file",
                Review,
                &[],
                Semantic,
            ),
            AppCommandReviewEffect::MoveSelection {
                scope: ReviewSelectionScope::AnnotatedFile,
                direction: Down,
            },
        ),
        Down,
    )),
];

#[must_use]
pub fn app_command_catalog_entry(id: &str) -> Option<&'static AppCommandCatalogEntry> {
    APP_COMMAND_CATALOG
        .iter()
        .find(|entry| entry.id == id || entry.aliases.contains(&id))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppCommandNoteLocation {
    pub file_key: String,
    pub hunk_index: usize,
}

pub struct AppCommandLoweringContext<'a> {
    pub count: usize,
    pub state: &'a SemanticReviewState,
    pub note_target: Option<SemanticReviewLineAddress>,
    pub note_location: Option<AppCommandNoteLocation>,
}

#[must_use]
pub fn lower_app_command_to_review_intent(
    entry: &AppCommandCatalogEntry,
    context: AppCommandLoweringContext<'_>,
) -> Option<SemanticReviewIntent> {
    match entry.review? {
        AppCommandReviewEffect::MoveSelection { scope, direction } => {
            Some(SemanticReviewIntent::Move {
                scope,
                delta: direction
                    .delta()
                    .saturating_mul(isize::try_from(context.count).unwrap_or(isize::MAX)),
            })
        }
        AppCommandReviewEffect::ToggleNoteVisibility => Some(
            SemanticReviewIntent::SetNoteVisibility(!context.state.show_agent_notes),
        ),
        AppCommandReviewEffect::StartDraft => {
            let location = context.note_location.or_else(|| {
                let selection = select_normalized_semantic_selection(context.state);
                Some(AppCommandNoteLocation {
                    file_key: selection.file_key?,
                    hunk_index: selection.hunk_index,
                })
            })?;
            Some(SemanticReviewIntent::StartDraft {
                file_key: location.file_key,
                hunk_index: isize::try_from(location.hunk_index).unwrap_or(isize::MAX),
                target: context.note_target,
                reveal: None,
            })
        }
        AppCommandReviewEffect::StartEditActive => Some(SemanticReviewIntent::StartEdit {
            note_id: select_active_editable_review_note_id(context.state)?,
            reveal: None,
        }),
        AppCommandReviewEffect::StartReplyActive => Some(SemanticReviewIntent::StartReply {
            note_id: select_active_replyable_review_note_id(context.state)?,
            reveal: None,
        }),
        AppCommandReviewEffect::ToggleSelectedGap => {
            let target = select_review_gap_for_selection(context.state)?;
            Some(SemanticReviewIntent::ToggleExpansion {
                file_key: target.file_key,
                gap_id: target.gap_id,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use workdeck_core::{ReviewNoteSource, ReviewSide, SemanticReviewLineAddress};

    use super::*;
    use crate::{
        ReviewNoteResolution, ReviewStoredNote, SemanticReviewSelection,
        semantic_test_support::{document, document_with_sources, note},
    };

    fn entry(id: &str) -> &'static AppCommandCatalogEntry {
        app_command_catalog_entry(id).unwrap_or_else(|| panic!("missing command {id}"))
    }

    fn context(state: &SemanticReviewState, count: usize) -> AppCommandLoweringContext<'_> {
        AppCommandLoweringContext {
            count,
            state,
            note_target: None,
            note_location: None,
        }
    }

    #[test]
    fn every_command_has_one_id_under_its_category_and_semantic_effect_invariant() {
        let ids = APP_COMMAND_CATALOG
            .iter()
            .map(|command| command.id)
            .collect::<BTreeSet<_>>();
        assert_eq!(ids.len(), APP_COMMAND_CATALOG.len());
        assert_eq!(APP_COMMAND_CATALOG.len(), 47);
        for command in APP_COMMAND_CATALOG {
            assert!(
                command
                    .id
                    .starts_with(&format!("workdeck.{}.", command.category.id_segment()))
            );
            assert!(!command.title.is_empty());
            assert_eq!(
                command.locus == AppCommandLocus::Semantic,
                command.review.is_some(),
                "{}",
                command.id
            );
            assert!(command.public_to_extensions);
        }
    }

    #[test]
    fn aliases_are_unique_noncanonical_and_resolve_to_the_canonical_entry() {
        let canonical = APP_COMMAND_CATALOG
            .iter()
            .map(|command| command.id)
            .collect::<BTreeSet<_>>();
        let aliases = APP_COMMAND_CATALOG
            .iter()
            .flat_map(|command| command.aliases.iter().copied())
            .collect::<Vec<_>>();
        assert_eq!(
            aliases.iter().copied().collect::<BTreeSet<_>>().len(),
            aliases.len()
        );
        assert!(aliases.iter().all(|alias| !canonical.contains(alias)));
        for command in APP_COMMAND_CATALOG {
            for alias in command.aliases {
                assert_eq!(app_command_catalog_entry(alias), Some(command));
            }
        }
        assert_eq!(
            entry("workdeck.view.toggleSidebar").id,
            "workdeck.view.toggleFilesPane"
        );
    }

    #[test]
    fn host_only_list_is_exact() {
        assert_eq!(
            APP_COMMAND_CATALOG
                .iter()
                .filter(|command| command.locus == AppCommandLocus::HostOnly)
                .map(|command| command.id)
                .collect::<Vec<_>>(),
            [
                "workdeck.app.quit",
                "workdeck.app.openAgentSkill",
                "workdeck.app.refresh",
                "workdeck.review.editSelectedFile",
            ]
        );
    }

    #[test]
    fn navigation_and_note_visibility_lower_from_declared_effects() {
        let state = SemanticReviewState::new(document(&[("alpha", 1)]), false);
        assert_eq!(
            lower_app_command_to_review_intent(
                entry("workdeck.review.nextHunk"),
                context(&state, 1)
            ),
            Some(SemanticReviewIntent::Move {
                scope: ReviewSelectionScope::Hunk,
                delta: 1
            })
        );
        assert_eq!(
            lower_app_command_to_review_intent(
                entry("workdeck.review.previousHunk"),
                context(&state, 3)
            ),
            Some(SemanticReviewIntent::Move {
                scope: ReviewSelectionScope::Hunk,
                delta: -3
            })
        );
        assert_eq!(
            lower_app_command_to_review_intent(
                entry("workdeck.review.previousAnnotatedFile"),
                context(&state, 2)
            ),
            Some(SemanticReviewIntent::Move {
                scope: ReviewSelectionScope::AnnotatedFile,
                delta: -2
            })
        );
        assert_eq!(
            lower_app_command_to_review_intent(
                entry("workdeck.view.toggleAgentNotes"),
                context(&state, 1)
            ),
            Some(SemanticReviewIntent::SetNoteVisibility(true))
        );
        let visible = SemanticReviewState::new(document(&[("alpha", 1)]), true);
        assert_eq!(
            lower_app_command_to_review_intent(
                entry("workdeck.view.toggleAgentNotes"),
                context(&visible, 1)
            ),
            Some(SemanticReviewIntent::SetNoteVisibility(false))
        );
    }

    #[test]
    fn local_and_host_commands_lower_to_nothing() {
        let state = SemanticReviewState::new(document(&[("alpha", 1)]), false);
        for id in ["workdeck.view.toggleFilesPane", "workdeck.app.quit"] {
            assert!(lower_app_command_to_review_intent(entry(id), context(&state, 1)).is_none());
        }
    }

    #[test]
    fn new_note_uses_selection_optional_line_or_explicit_affordance_location() {
        let state = SemanticReviewState::new(document(&[("alpha", 1), ("beta", 2)]), false);
        assert_eq!(
            lower_app_command_to_review_intent(
                entry("workdeck.review.startNote"),
                context(&state, 1)
            ),
            Some(SemanticReviewIntent::StartDraft {
                file_key: "alpha".into(),
                hunk_index: 0,
                target: None,
                reveal: None,
            })
        );
        assert_eq!(
            lower_app_command_to_review_intent(
                entry("workdeck.review.startNote"),
                AppCommandLoweringContext {
                    note_target: Some(SemanticReviewLineAddress {
                        side: ReviewSide::New,
                        line: 21
                    }),
                    note_location: Some(AppCommandNoteLocation {
                        file_key: "beta".into(),
                        hunk_index: 1
                    }),
                    ..context(&state, 1)
                },
            ),
            Some(SemanticReviewIntent::StartDraft {
                file_key: "beta".into(),
                hunk_index: 1,
                target: Some(SemanticReviewLineAddress {
                    side: ReviewSide::New,
                    line: 21
                }),
                reveal: None,
            })
        );
    }

    #[test]
    fn edit_and_reply_use_active_note_policies() {
        let mut state = SemanticReviewState::new(document(&[("alpha", 1)]), false);
        state.live_notes.push(ReviewStoredNote {
            note: note("live-1", "alpha", ReviewNoteSource::Agent),
            resolution: ReviewNoteResolution::Active,
        });
        let mut user = note("user-1", "alpha", ReviewNoteSource::User);
        user.parent_id = Some("live-1".into());
        state.user_notes.push(ReviewStoredNote {
            note: user,
            resolution: ReviewNoteResolution::Active,
        });
        assert_eq!(
            lower_app_command_to_review_intent(
                entry("workdeck.review.editActiveNote"),
                context(&state, 1)
            ),
            Some(SemanticReviewIntent::StartEdit {
                note_id: "user-1".into(),
                reveal: None
            })
        );
        assert_eq!(
            lower_app_command_to_review_intent(
                entry("workdeck.review.replyToActiveNote"),
                context(&state, 1)
            ),
            Some(SemanticReviewIntent::StartReply {
                note_id: "user-1".into(),
                reveal: None
            })
        );
    }

    #[test]
    fn gap_toggle_uses_shared_selection_policy_and_targetless_effects_are_noops() {
        let mut state = SemanticReviewState::new(
            document_with_sources(&[("alpha", Some("source:alpha"), true)]),
            false,
        );
        let file = std::sync::Arc::make_mut(&mut state.document)
            .files
            .first_mut()
            .unwrap();
        file.hunks[0].collapsed_before = 1;
        file.hunks[0].addition_start = 2;
        file.hunks[0].deletion_start = 2;
        file.addition_lines = vec!["context".into(), "change".into()];
        file.deletion_lines = file.addition_lines.clone();
        assert_eq!(
            lower_app_command_to_review_intent(
                entry("workdeck.review.toggleHunkGap"),
                context(&state, 1)
            ),
            Some(SemanticReviewIntent::ToggleExpansion {
                file_key: "alpha".into(),
                gap_id: "before:0".into()
            })
        );

        let empty = SemanticReviewState::new(document(&[]), false);
        assert!(
            lower_app_command_to_review_intent(
                entry("workdeck.review.startNote"),
                context(&empty, 1)
            )
            .is_none()
        );
        assert!(
            lower_app_command_to_review_intent(
                entry("workdeck.review.toggleHunkGap"),
                context(&empty, 1)
            )
            .is_none()
        );
        assert!(
            lower_app_command_to_review_intent(
                entry("workdeck.review.editActiveNote"),
                context(&empty, 1)
            )
            .is_none()
        );
        assert!(
            lower_app_command_to_review_intent(
                entry("workdeck.review.replyToActiveNote"),
                context(&empty, 1)
            )
            .is_none()
        );
    }

    #[test]
    fn note_location_overrides_an_empty_selection_like_an_addressed_affordance() {
        let mut state = SemanticReviewState::new(document(&[]), false);
        state.selection = SemanticReviewSelection {
            file_key: None,
            hunk_index: 0,
        };
        assert!(matches!(
            lower_app_command_to_review_intent(
                entry("workdeck.review.startNote"),
                AppCommandLoweringContext {
                    note_location: Some(AppCommandNoteLocation { file_key: "detached".into(), hunk_index: 4 }),
                    ..context(&state, 1)
                },
            ),
            Some(SemanticReviewIntent::StartDraft { file_key, hunk_index: 4, .. }) if file_key == "detached"
        ));
    }
}
