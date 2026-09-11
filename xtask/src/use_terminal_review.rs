//! Exhaustive source accounting for Hunk's terminal review controller.
//!
//! The React hook is the composition boundary between Hunk's semantic review store and its
//! terminal pane.  Workdeck owns the same boundary in Rust: the semantic store and selectors
//! live in `workdeck-review`, the source/reveal/cursor controller in `workdeck-tui`, and the
//! authenticated session bridge consumes the same state.  This verifier reads the two pinned
//! Hunk blobs with `git show`; no TypeScript mirror is kept in the final tree.

use anyhow::{Context, Result, ensure};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;

const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
const STABLE: &str = "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd";
const SOURCE_PATH: &str = "src/ui/hooks/useTerminalReview.ts";
const BASELINE_BYTES: usize = 57_589;
const BASELINE_LINES: usize = 1_570;
const BASELINE_SHA256: &str = "7ebddab5d2cba4676dfe6d3a16decf568ea3fef80836a5d18b35fde7480c289a";
const STABLE_BYTES: usize = 55_549;
const STABLE_LINES: usize = 1_510;
const STABLE_SHA256: &str = "6996dda23b5e622f15149072ff82bcd39d8a0f21ab12300fb07d708aefd3333a";

struct NativeMarker {
    path: &'static str,
    marker: &'static str,
}

struct FunctionContract {
    name: &'static str,
    native: &'static [NativeMarker],
    tests: &'static [&'static str],
}

const FUNCTION_CONTRACTS: &[FunctionContract] = &[
    FunctionContract {
        name: "mergeAnnotationMaps",
        native: &[
            NativeMarker {
                path: "crates/workdeck-tui/src/public_review.rs",
                marker: "pub fn merge_file_annotations_by_file_id(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/review_state_helpers.rs",
                marker: "pub fn build_review_stream_state(",
            },
        ],
        tests: &[
            "crates/workdeck-tui/src/review_state_helpers.rs#tests::stream_merges_file_id_keyed_live_annotations_without_mutating_input",
            "crates/workdeck-review/src/review_note_mapping.rs#tests::threaded_grouping_preserves_visible_depth_replies_and_guides",
        ],
    },
    FunctionContract {
        name: "useReviewStoreSnapshot",
        native: &[
            NativeMarker {
                path: "crates/workdeck-review/src/semantic_store.rs",
                marker: "pub struct SemanticReviewStore",
            },
            NativeMarker {
                path: "crates/workdeck-review/src/semantic_store.rs",
                marker: "pub fn subscribe<F>(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/lib.rs",
                marker: "pub struct ReviewApp",
            },
        ],
        tests: &[
            "crates/workdeck-review/src/semantic_store.rs#tests::publishes_new_snapshots_and_returns_the_state_just_produced",
            "crates/workdeck-review/src/semantic_store.rs#tests::unsubscribe_stops_notifications",
            "crates/workdeck-review/src/semantic_store.rs#tests::semantic_noop_preserves_snapshot_identity_and_skips_notification",
        ],
    },
    FunctionContract {
        name: "withMissingNoteMessage",
        native: &[
            NativeMarker {
                path: "crates/workdeck-review/src/semantic_intents.rs",
                marker: "ReviewIntentPlanningErrorCode::NoteNotFound",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/lib.rs",
                marker: "fn handle_note_mouse(",
            },
        ],
        tests: &[
            "crates/workdeck-review/src/semantic_intents.rs#tests::removal_targets_owner_collection_and_refuses_parents_with_replies",
            "crates/workdeck-tui/src/lib.rs#tests::mouse_delete_rejects_parent_notes_without_mutation",
        ],
    },
    FunctionContract {
        name: "revealRequestFor",
        native: &[
            NativeMarker {
                path: "crates/workdeck-review/src/semantic_state.rs",
                marker: "pub struct ReviewRevealRequest",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/lib.rs",
                marker: "fn scroll_to_reveal(",
            },
            NativeMarker {
                path: "crates/workdeck-review/src/semantic_intents.rs",
                marker: "REVIEW_DRAFT_START_REVEAL",
            },
        ],
        tests: &[
            "crates/workdeck-tui/src/lib.rs#tests::session_comment_navigation_reveals_deep_inline_note_and_returns_hunk",
            "crates/workdeck-tui/src/review_state_helpers.rs#tests::selection_reconciliation_ignores_filters_and_has_no_reveal_side_effect",
            "crates/workdeck-review/src/semantic_intents.rs#tests::file_jump_defaults_to_header_accepts_override_and_anchor_never_reveals",
        ],
    },
    FunctionContract {
        name: "sameLineCursor",
        native: &[
            NativeMarker {
                path: "crates/workdeck-tui/src/line_cursors.rs",
                marker: "pub struct LineCursor",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/line_cursors.rs",
                marker: "shares_storage_with",
            },
        ],
        tests: &[
            "crates/workdeck-tui/src/line_cursors.rs#tests::equivalent_remeasurement_reuses_storage_and_stabilizer_skips_same_measurement",
            "crates/workdeck-tui/src/line_cursors.rs#tests::synthetic_note_cursor_uses_the_canonical_side_anchor",
        ],
    },
    FunctionContract {
        name: "useTerminalReview",
        native: &[
            NativeMarker {
                path: "crates/workdeck-core/src/semantic.rs",
                marker: "pub fn project_review_document(",
            },
            NativeMarker {
                path: "crates/workdeck-review/src/semantic_selectors.rs",
                marker: "pub fn review_file_keys_with_retired_content(",
            },
            NativeMarker {
                path: "crates/workdeck-review/src/semantic_selectors.rs",
                marker: "pub fn select_threaded_stored_review_notes(",
            },
            NativeMarker {
                path: "crates/workdeck-review/src/semantic_selectors.rs",
                marker: "pub fn select_visible_threaded_stored_review_notes(",
            },
            NativeMarker {
                path: "crates/workdeck-review/src/annotations.rs",
                marker: "pub fn build_review_annotation_index(",
            },
            NativeMarker {
                path: "crates/workdeck-review/src/semantic_intents.rs",
                marker: "pub fn plan_semantic_review_intent(",
            },
            NativeMarker {
                path: "crates/workdeck-review/src/semantic_intents.rs",
                marker: "pub fn apply_semantic_review_intent(",
            },
            NativeMarker {
                path: "crates/workdeck-review/src/command_catalog.rs",
                marker: "pub fn lower_app_command_to_review_intent(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/review_state_helpers.rs",
                marker: "pub fn build_review_stream_state(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/line_cursors.rs",
                marker: "pub fn build_line_cursors(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/line_cursors.rs",
                marker: "pub fn find_line_cursor_at(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/line_cursors.rs",
                marker: "pub fn find_next_line_cursor(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/source_controller.rs",
                marker: "pub(super) fn start_source_load(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/source_presentation.rs",
                marker: "pub fn expanded_status<'a>(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/lib.rs",
                marker: "pub struct ReviewApp",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/lib.rs",
                marker: "fn move_selection(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/lib.rs",
                marker: "fn toggle_source_gap(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/lib.rs",
                marker: "fn open_note_composer(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/lib.rs",
                marker: "fn save_note_composer(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/session_review_controller.rs",
                marker: "fn markup_feedback(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/session_review_controller.rs",
                marker: "fn current_navigation_result(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/public_review.rs",
                marker: "pub fn merge_file_annotations_by_file_id(",
            },
            NativeMarker {
                path: "crates/workdeck-review/src/review_note_mapping.rs",
                marker: "pub fn group_threaded_stored_notes_by_file_id<T",
            },
        ],
        tests: &[
            "crates/workdeck-tui/src/lib.rs#tests::manual_reload_replaces_changed_content_and_invalidates_the_old_syntax_cache",
            "crates/workdeck-tui/src/lib.rs#tests::reload_preserves_and_clears_attention_marks_when_content_is_unchanged",
            "crates/workdeck-tui/src/lib.rs#tests::reload_rekeys_painted_attention_marks_when_file_runtime_identity_shifts",
            "crates/workdeck-tui/src/lib.rs#tests::source_gaps_are_collapsed_by_default_and_expand_from_snapshots",
            "crates/workdeck-tui/src/lib.rs#tests::user_note_composer_edits_and_replies_with_stable_public_identity",
            "crates/workdeck-tui/src/lib.rs#tests::session_comment_navigation_keeps_active_filter_and_visible_files",
            "crates/workdeck-tui/src/lib.rs#tests::session_comment_navigation_reveals_deep_inline_note_and_returns_hunk",
            "crates/workdeck-tui/src/lib.rs#tests::native_line_highlight_refresh_actions_update_live_epochs_and_notices",
            "crates/workdeck-tui/src/lib.rs#tests::pinned_rapid_navigation_and_wheel_rendering_settles",
            "crates/workdeck-tui/src/lib.rs#tests::frozen_app_host_reload_oracle_maps_both_pins_and_all_source_tests",
            "crates/workdeck-tui/src/lib.rs#tests::review_shortcuts_change_layout_and_navigation",
            "crates/workdeck-tui/src/lib.rs#tests::expanded_gap_rows_are_reachable_by_cursor_and_note",
            "crates/workdeck-tui/src/source_controller.rs#tests::gap_toggle_starts_a_worker_and_completion_reaches_the_live_rows",
            "crates/workdeck-tui/src/source_controller.rs#tests::cross_file_beta_line_reveal_resolves_before_another_frame",
            "crates/workdeck-tui/src/source_controller.rs#tests::counted_alpha_cursor_movement_reveals_nearest_row_and_carries_hunk_selection",
            "crates/workdeck-tui/src/source_controller.rs#tests::alpha_keyboard_note_actions_restore_the_note_line_target",
            "crates/workdeck-tui/src/session_review_controller.rs#tests::comment_batches_preflight_every_target_and_commit_atomically",
            "crates/workdeck-tui/src/session_review_controller.rs#tests::live_comment_markup_requires_launch_experimental_authority",
            "crates/workdeck-tui/src/session_review_controller.rs#tests::agent_highlights_share_validation_limits_and_clear_counts",
            "crates/workdeck-tui/src/review_state_helpers.rs#tests::stream_merges_file_id_keyed_live_annotations_without_mutating_input",
            "crates/workdeck-review/src/review_note_mapping.rs#tests::threaded_grouping_preserves_visible_depth_replies_and_guides",
            "crates/workdeck-review/src/semantic_store.rs#tests::publishes_new_snapshots_and_returns_the_state_just_produced",
            "crates/workdeck-review/src/semantic_intents.rs#tests::apply_commits_complete_plan_and_planning_failure_preserves_snapshot_identity",
        ],
    },
];

const SOURCE_MARKERS: &[&str] = &[
    "useDeferredValue",
    "useLayoutEffect",
    "useEffect",
    "createReviewStore",
    "projectReviewDocument",
    "reviewFileKeysWithRetiredContent",
    "selectThreadedStoredReviewNotes",
    "selectVisibleThreadedStoredReviewNotes",
    "buildReviewStreamState",
    "buildReviewAnnotationIndex",
    "applyReviewIntent",
    "lowerAppCommandToReviewIntent",
    "findLineCursorAt",
    "findNextLineCursor",
    "startSourceLoad",
    "getFullText",
    "applyGapToggle",
    "addLiveComment",
    "addLiveCommentBatch",
    "clearLiveComments",
    "navigateToLocation",
    "addAgentLineHighlight",
    "clearAgentLineHighlights",
    "markupFeedback",
    "stmlEnabled",
    "startUserNote",
    "startUserNoteEdit",
    "startUserNoteReply",
    "saveDraftNote",
    "removeUserNote",
    "stateRevision",
    "selectedFileTopAlignRequestId",
    "selectedHunkRevealRequestId",
    "anchorSelection",
    "anchorLineCursor",
    "moveLineCursor",
    "moveSelection",
    "revealLine",
    "toggleAgentNotes",
    "sourceStatusByFileId",
    "reviewNoteSummaries",
    "lineCursorRevealRequest",
    "agentLineHighlightsByFileId",
];

const STABLE_ONLY_MARKERS: &[&str] = &["mergeAnnotationMaps", "sameLineCursor"];

const ORACLE_FIXTURES: &[&str] = &[
    "port/hunk/oracles/review-state.json",
    "port/hunk/oracles/review-note-mapping.json",
    "port/hunk/oracles/review-triage.json",
    "port/hunk/oracles/line-cursors.json",
    "port/hunk/oracles/source-capability-identity.json",
    "port/hunk/oracles/file-source-execution.json",
    "port/hunk/oracles/app-host-selection.json",
    "port/hunk/oracles/app-host-filter-selection.json",
    "port/hunk/oracles/app-host-reload.json",
    "port/hunk/oracles/app-host-reload-comments.json",
    "port/hunk/oracles/app-host-reload-root.json",
    "port/hunk/oracles/app-host-reload-experimental.json",
    "port/hunk/oracles/app-host-cursor-line.json",
    "port/hunk/oracles/app-host-deep-note.json",
    "port/hunk/oracles/app-host-draft-blur.json",
    "port/hunk/oracles/app-host-draft-burst.json",
    "port/hunk/oracles/app-host-draft-cjk.json",
    "port/hunk/oracles/app-host-draft-focus.json",
    "port/hunk/oracles/app-host-viewport-notes.json",
    "port/hunk/oracles/app-host-cross-file-sequence.json",
    "port/hunk/oracles/app-host-destination-header.json",
    "port/hunk/oracles/app-host-content-edge-authority.json",
];

fn source_function_names(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let line = line
                .strip_prefix("export function ")
                .or_else(|| line.strip_prefix("function "))?;
            let end = [line.find('('), line.find('{'), line.find('<')]
                .into_iter()
                .flatten()
                .min()?;
            Some(line[..end].trim().to_owned())
        })
        .collect()
}

fn source_interface_names(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let line = line
                .strip_prefix("export interface ")
                .or_else(|| line.strip_prefix("interface "))?;
            let end = line.find('{').or_else(|| line.find('<'))?;
            Some(line[..end].trim().to_owned())
        })
        .collect()
}

fn source_type_names(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let line = line
                .strip_prefix("export type ")
                .or_else(|| line.strip_prefix("type "))?;
            let end = line.find('=').or_else(|| line.find('<'))?;
            Some(line[..end].trim().to_owned())
        })
        .collect()
}

fn source_const_names(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| {
            let line = line
                .strip_prefix("const ")
                .or_else(|| line.strip_prefix("export const "))?;
            let end = line.find(':').or_else(|| line.find('='))?;
            Some(line[..end].trim().to_owned())
        })
        .collect()
}

fn verify_rust_anchor(
    repo: &Path,
    item: &str,
    sources: &mut HashMap<String, syn::File>,
) -> Result<()> {
    let (path, anchor) = item
        .split_once('#')
        .with_context(|| format!("terminal review mapping lacks a Rust test anchor: {item}"))?;
    let source = if let Some(source) = sources.get(path) {
        source
    } else {
        let text = fs::read_to_string(repo.join(path))
            .with_context(|| format!("read terminal review Rust evidence {path}"))?;
        let parsed = syn::parse_file(&text)
            .with_context(|| format!("parse terminal review Rust evidence {path}"))?;
        sources.entry(path.to_owned()).or_insert(parsed)
    };
    ensure!(
        crate::rust_items_have_test(&source.items, Some(anchor)),
        "terminal review mapping references a missing executable Rust test: {item}"
    );
    Ok(())
}

fn verify_source_blob(
    repo: &Path,
    commit: &str,
    bytes: usize,
    lines: usize,
    sha: &str,
) -> Result<String> {
    let source = crate::git_stdout_bytes(repo, ["show", &format!("{commit}:{SOURCE_PATH}")])?;
    ensure!(
        source.len() == bytes,
        "pinned terminal review hook {commit} changed size: {} != {bytes}",
        source.len()
    );
    ensure!(
        source.split(|byte| *byte == b'\n').count() == lines + 1,
        "pinned terminal review hook {commit} changed line count"
    );
    ensure!(
        format!("{:x}", Sha256::digest(&source)) == sha,
        "pinned terminal review hook {commit} changed SHA-256"
    );
    String::from_utf8(source).context("pinned terminal review hook is not UTF-8")
}

fn verify_oracle(repo: &Path, path: &str) -> Result<()> {
    let bytes =
        fs::read(repo.join(path)).with_context(|| format!("read terminal review oracle {path}"))?;
    let value: Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse terminal review oracle {path}"))?;
    let encoded = value.to_string();
    ensure!(
        encoded.contains(BASELINE) && encoded.contains(STABLE),
        "terminal review oracle {path} does not identify both pinned trees"
    );
    Ok(())
}

fn verify_native_surface(repo: &Path) -> Result<()> {
    let mut contents = BTreeMap::new();
    for contract in FUNCTION_CONTRACTS {
        for marker in contract.native {
            let source = if let Some(source) = contents.get(marker.path) {
                source
            } else {
                let text = fs::read_to_string(repo.join(marker.path)).with_context(|| {
                    format!("read terminal review native surface {}", marker.path)
                })?;
                contents.entry(marker.path).or_insert(text)
            };
            ensure!(
                source.contains(marker.marker),
                "terminal review native surface {} is missing {:?} for {}",
                marker.path,
                marker.marker,
                contract.name
            );
        }
    }
    Ok(())
}

/// Verify both pinned terminal-review hook blobs and their executable native projection.
pub(crate) fn verify(repo: &Path, baseline: &str) -> Result<()> {
    ensure!(
        baseline == BASELINE,
        "terminal review verifier received unexpected baseline {baseline}"
    );
    let source = verify_source_blob(
        repo,
        BASELINE,
        BASELINE_BYTES,
        BASELINE_LINES,
        BASELINE_SHA256,
    )?;
    let stable = verify_source_blob(repo, STABLE, STABLE_BYTES, STABLE_LINES, STABLE_SHA256)?;

    let baseline_functions = source_function_names(&source);
    let expected_baseline_functions = [
        "useReviewStoreSnapshot",
        "withMissingNoteMessage",
        "revealRequestFor",
        "useTerminalReview",
    ];
    ensure!(
        baseline_functions == expected_baseline_functions,
        "pinned terminal review baseline function surface changed: {baseline_functions:?}"
    );
    ensure!(
        source_interface_names(&source)
            == [
                "SourceLoadRequest",
                "LineCursorRevealRequest",
                "ReviewSelectionOptions",
                "TerminalReview",
                "AgentNoteGeometrySnapshot",
            ],
        "pinned terminal review baseline interface surface changed"
    );
    ensure!(
        source_type_names(&source) == ["RevealedLineResult"],
        "pinned terminal review type surface changed"
    );
    ensure!(
        source_const_names(&source) == ["EMPTY_AGENT_LINE_HIGHLIGHTS"],
        "pinned terminal review constant surface changed"
    );

    let stable_functions = source_function_names(&stable);
    ensure!(
        stable_functions
            == [
                "mergeAnnotationMaps",
                "useReviewStoreSnapshot",
                "withMissingNoteMessage",
                "revealRequestFor",
                "sameLineCursor",
                "useTerminalReview",
            ],
        "pinned stable terminal review function surface changed: {stable_functions:?}"
    );
    ensure!(
        source_interface_names(&stable)
            == [
                "SourceLoadRequest",
                "LineCursorRevealRequest",
                "ReviewSelectionOptions",
                "TerminalReview",
                "AgentNoteGeometrySnapshot",
            ],
        "pinned stable terminal review interface surface changed"
    );
    ensure!(
        source_type_names(&stable) == ["RevealedLineResult"],
        "pinned stable terminal review type surface changed"
    );
    ensure!(
        source_const_names(&stable) == ["EMPTY_AGENT_LINE_HIGHLIGHTS"],
        "pinned stable terminal review constants changed"
    );

    for marker in SOURCE_MARKERS {
        ensure!(
            source.contains(marker),
            "pinned terminal review source is missing required contract marker {marker:?}"
        );
    }
    for marker in STABLE_ONLY_MARKERS {
        ensure!(
            stable.contains(marker),
            "pinned stable terminal review source is missing stable-only marker {marker:?}"
        );
    }

    let contract_names = FUNCTION_CONTRACTS
        .iter()
        .map(|contract| contract.name)
        .collect::<Vec<_>>();
    let expected_names = baseline_functions
        .iter()
        .map(String::as_str)
        .chain(["mergeAnnotationMaps", "sameLineCursor"])
        .collect::<Vec<_>>();
    let mut unique = BTreeSet::new();
    ensure!(
        contract_names.iter().all(|name| unique.insert(*name)),
        "terminal review contract table repeats a source function"
    );
    ensure!(
        expected_names
            .iter()
            .all(|name| contract_names.contains(name)),
        "terminal review contract table omits a pinned function"
    );
    let mut rust_sources = HashMap::new();
    for contract in FUNCTION_CONTRACTS {
        ensure!(
            !contract.native.is_empty() && !contract.tests.is_empty(),
            "terminal review contract {} has no native surface or executable evidence",
            contract.name
        );
        for test in contract.tests {
            verify_rust_anchor(repo, test, &mut rust_sources)?;
        }
    }
    verify_native_surface(repo)?;
    for oracle in ORACLE_FIXTURES {
        verify_oracle(repo, oracle)?;
    }

    let migration = fs::read_to_string(repo.join("docs/use-terminal-review-migration.md"))
        .context("read terminal review migration documentation")?;
    for marker in [
        SOURCE_PATH,
        "57,589",
        "55,549",
        "non-overlapping",
        "semantic store",
        "source loading",
        "line cursor",
        "session bridge",
        "No TypeScript source mirror",
    ] {
        ensure!(
            migration.contains(marker),
            "terminal review migration documentation is missing {marker:?}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_rust_terminal_review_replaces_both_pinned_hook_contracts() {
        let repo = crate::repo_root().unwrap();
        verify(&repo, BASELINE).unwrap();
    }
}
