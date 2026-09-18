//! Exhaustive source accounting for Hunk's continuous review pane.
//!
//! `DiffPane.tsx` is the largest review-surface source file in the pinned Hunk tree.  It is not
//! copied into Workdeck or executed by a JavaScript runtime.  This verifier reads both pinned
//! blobs through `git show`, checks their complete byte identity, and then checks an explicit
//! function/contract/test projection into the native Ratatui review shell.  The projection is
//! deliberately finer grained than a single "renderer exists" marker: scrolling, windowing,
//! notes, copy gestures, source expansion, highlighting, line cursors, and rendering each have
//! their own native surface and executable parity evidence.

use anyhow::{Context, Result, ensure};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;

const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
const STABLE: &str = "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd";
const SOURCE_PATH: &str = "src/ui/components/panes/DiffPane.tsx";
const BASELINE_BYTES: usize = 106_437;
const BASELINE_LINES: usize = 2_835;
const BASELINE_SHA256: &str = "2f3c6f032b94f96f8d176def74821336bf96d1c876d5d942141cbe18a83b5f19";
const STABLE_BYTES: usize = 96_299;
const STABLE_LINES: usize = 2_557;
const STABLE_SHA256: &str = "d1c6dca60aec1b1a7aa1e0228e5b5280550fb55e1738b7af7ee35c7d89309d0a";

struct NativeMarker {
    path: &'static str,
    marker: &'static str,
}

struct FunctionContract {
    name: &'static str,
    native: &'static [NativeMarker],
    tests: &'static [&'static str],
}

// Every top-level function in the main tree is accounted for.  The two stable-only helpers are
// retained below as well: stable-v0.20.1 predates the current owned Ratatui viewport/capture
// paths, so their behavior is represented by the corresponding native policies rather than by a
// dead compatibility function.
const FUNCTION_CONTRACTS: &[FunctionContract] = &[
    FunctionContract {
        name: "storedReviewNoteMetadata",
        native: &[
            NativeMarker {
                path: "crates/workdeck-tui/src/lib.rs",
                marker: "fn comments_with_thread_draft(",
            },
            NativeMarker {
                path: "crates/workdeck-review/src/semantic_selectors.rs",
                marker: "select_visible_threaded_stored_review_notes",
            },
        ],
        tests: &[
            "crates/workdeck-review/src/semantic_selectors.rs#tests::stored_notes_preserve_collection_order_resolution_and_exclude_drafts",
            "crates/workdeck-review/src/semantic_selectors.rs#tests::reply_trees_are_depth_first_and_visible_connectors_collapse_hidden_parents",
        ],
    },
    FunctionContract {
        name: "storedReviewNoteActions",
        native: &[
            NativeMarker {
                path: "crates/workdeck-tui/src/lib.rs",
                marker: "fn handle_note_mouse(",
            },
            NativeMarker {
                path: "crates/workdeck-review/src/semantic_intents.rs",
                marker: "fn plan_note_removal(",
            },
        ],
        tests: &[
            "crates/workdeck-tui/src/lib.rs#tests::mouse_delete_rejects_parent_notes_without_mutation",
            "crates/workdeck-tui/src/lib.rs#tests::user_note_composer_edits_and_replies_with_stable_public_identity",
        ],
    },
    FunctionContract {
        name: "resetOpenTuiScrollAccumulators",
        native: &[
            NativeMarker {
                path: "crates/workdeck-tui/src/lib.rs",
                marker: "fn handle_horizontal_mouse_scroll(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/lib.rs",
                marker: "mouse_scroll_accumulator = 0.0",
            },
        ],
        tests: &[
            "crates/workdeck-tui/src/lib.rs#tests::shifted_and_native_horizontal_wheel_events_never_move_the_vertical_viewport",
            "crates/workdeck-tui/src/lib.rs#tests::wrapped_review_leaves_shifted_wheel_available_for_vertical_scrolling",
        ],
    },
    FunctionContract {
        name: "clampVerticalScrollTop",
        native: &[
            NativeMarker {
                path: "crates/workdeck-tui/src/lib.rs",
                marker: "fn current_review_content_height(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/lib.rs",
                marker: "let max_scroll = if self.review_geometry_published.get()",
            },
        ],
        tests: &[
            "crates/workdeck-tui/src/lib.rs#tests::scroll_wheel_at_eof_does_not_accumulate_invisible_overscroll",
            "crates/workdeck-tui/src/lib.rs#tests::scroll_short_final_file_allows_upward_movement_after_navigation",
        ],
    },
    FunctionContract {
        name: "streamRowBoundsAt",
        native: &[
            NativeMarker {
                path: "crates/workdeck-tui/src/lib.rs",
                marker: "fn current_review_rows_with_options(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/diff_section_geometry.rs",
                marker: "row_bounds_by_stable_key",
            },
        ],
        tests: &[
            "crates/workdeck-tui/src/lib.rs#tests::viewport_split_rows_preserve_visible_styles_and_complete_geometry",
            "crates/workdeck-tui/src/lib.rs#tests::note_after_paging_preserves_the_visible_review_anchor",
        ],
    },
    FunctionContract {
        name: "buildAdjacentPrefetchFileIds",
        native: &[NativeMarker {
            path: "crates/workdeck-tui/src/highlight_prefetch.rs",
            marker: "pub fn adjacent_highlight_prefetch_ids(",
        }],
        tests: &[
            "crates/workdeck-tui/src/highlight_prefetch.rs#tests::frozen_pins_match_prefetch_policy_including_halo_and_selection_edges",
        ],
    },
    FunctionContract {
        name: "buildHighlightPrefetchFileIds",
        native: &[
            NativeMarker {
                path: "crates/workdeck-tui/src/highlight_prefetch.rs",
                marker: "pub fn highlight_prefetch_ids(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/highlight_prefetch.rs",
                marker: "RapidScrollPrefetch",
            },
        ],
        tests: &[
            "crates/workdeck-tui/src/highlight_prefetch.rs#tests::frozen_pins_match_prefetch_policy_including_halo_and_selection_edges",
            "crates/workdeck-tui/src/lib.rs#tests::live_plain_split_prefetch_requests_halo_not_every_file_and_follows_eof_jump",
        ],
    },
    FunctionContract {
        name: "DiffPane",
        native: &[
            NativeMarker {
                path: "crates/workdeck-tui/src/lib.rs",
                marker: "fn render_review(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/diff_section_body.rs",
                marker: "pub fn paint_diff_section_body(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/diff_section_geometry.rs",
                marker: "pub fn estimate_diff_section_body_rows(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/file_render_window.rs",
                marker: "pub fn build_file_render_window(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/review_render_plan.rs",
                marker: "fn build_inline_visible_note_placements",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/copy_selection.rs",
                marker: "pub fn render_copy_selection_text(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/line_cursors.rs",
                marker: "pub fn build_line_cursors(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/viewport_anchor.rs",
                marker: "pub fn find_viewport_row_anchor(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/viewport_selection.rs",
                marker: "pub fn find_viewport_centered_hunk_target(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/public_review.rs",
                marker: "pub fn render_workdeck_review_stream(",
            },
        ],
        tests: &[
            "crates/workdeck-tui/src/lib.rs#tests::next_hunk_gives_destination_file_the_review_header_after_scrolling",
            "crates/workdeck-tui/src/lib.rs#tests::scroll_pinned_header_handoff_keeps_the_viewport_lane_stable",
            "crates/workdeck-tui/src/lib.rs#tests::cross_file_hunk_sequence_preserves_destination_header_and_backward_target",
            "crates/workdeck-tui/src/lib.rs#tests::annotation_toggle_shows_notes_for_both_files_in_current_viewport",
            "crates/workdeck-tui/src/lib.rs#tests::review_top_chrome_padding_matches_pinned_header_hits_and_reserved_rows",
            "crates/workdeck-tui/src/lib.rs#tests::pinned_rapid_navigation_and_wheel_rendering_settles",
            "crates/workdeck-tui/src/public_review/tests.rs#renders_reusable_file_header_and_multi_file_review_stream_primitives",
        ],
    },
    FunctionContract {
        name: "estimateInitialRenderViewportHeight",
        native: &[NativeMarker {
            path: "crates/workdeck-tui/src/ui_geometry.rs",
            marker: "pub fn estimate_initial_render_viewport_height(",
        }],
        tests: &[
            "crates/workdeck-tui/src/ui_geometry.rs#tests::initial_viewport_subtracts_pane_screen_top_from_renderer_height",
            "crates/workdeck-tui/src/ui_geometry.rs#tests::initial_viewport_never_returns_empty_while_geometry_is_unknown",
        ],
    },
    FunctionContract {
        name: "retainCopySelectionCapture",
        native: &[
            NativeMarker {
                path: "crates/workdeck-tui/src/mouse_capture.rs",
                marker: "pub struct MouseCapture<T>",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/lib.rs",
                marker: "fn handle_copy_selection_mouse_at(",
            },
        ],
        tests: &[
            "crates/workdeck-tui/src/mouse_capture.rs#tests::capture_targets_a_persistent_identity_and_release_clears_it",
            "crates/workdeck-tui/src/lib.rs#tests::nested_row_mouse_action_claims_parent_selection_event",
        ],
    },
];

const SOURCE_MARKERS: &[&str] = &[
    "useLayoutEffect",
    "useEffect",
    "measureDiffSectionGeometry",
    "buildInStreamFileHeaderHeights",
    "buildFileSectionLayouts",
    "buildFileRenderWindow",
    "visibleBodyBoundsByFile",
    "resolveReviewRevealNoteId",
    "computeHunkRevealScrollTop",
    "computeLineRevealScrollTop",
    "computeLineAlignmentScrollTop",
    "findViewportCenteredHunkTarget",
    "prefetchHighlightedDiff",
    "DiffSection",
    "VerticalScrollbar",
    "onMouseDown",
    "onMouseDrag",
    "onMouseDragEnd",
    "onMouseScroll",
    "onMouseUp",
    "showAgentNotes",
    "showLineNumbers",
    "showHunkHeaders",
    "copySelectionDrag",
    "selectedFileTopAlignRequestId",
    "selectedHunkRevealRequestId",
    "layoutToggleRequestId",
    "scrollEdgeRequest",
    "onViewportCenteredHunkChange",
    "onViewportLineCursorChange",
    "onCurrentLinePaintChange",
    "scrollChildIntoView",
];

const ORACLE_FIXTURES: &[&str] = &[
    "port/hunk/oracles/review-mouse-ownership.json",
    "port/hunk/oracles/copy-selection.json",
    "port/hunk/oracles/file-render-window.json",
    "port/hunk/oracles/highlight-prefetch-policy.json",
    "port/hunk/oracles/review-render-plan.json",
    "port/hunk/oracles/review-row-geometry.json",
    "port/hunk/oracles/diff-section-geometry.json",
    "port/hunk/oracles/diff-section-body.json",
    "port/hunk/oracles/viewport-navigation.json",
    "port/hunk/oracles/app-host-wrap-anchor.json",
    "port/hunk/oracles/app-host-layout-anchor.json",
    "port/hunk/oracles/app-host-destination-header.json",
    "port/hunk/oracles/app-host-deep-note.json",
    "port/hunk/oracles/app-host-viewport-notes.json",
    "port/hunk/oracles/app-host-horizontal-wheel.json",
    "port/hunk/oracles/app-host-arrow-scroll.json",
    "port/hunk/oracles/app-host-scroll-regression.json",
    "port/hunk/oracles/app-host-cross-file-sequence.json",
    "port/hunk/oracles/app-host-down-selection.json",
    "port/hunk/oracles/app-host-wheel-selection.json",
    "port/hunk/oracles/app-host-content-edge-authority.json",
    "port/hunk/oracles/app-host-draft-blur.json",
    "port/hunk/oracles/app-host-draft-burst.json",
    "port/hunk/oracles/app-host-draft-cjk.json",
    "port/hunk/oracles/app-host-draft-focus.json",
];

fn source_function_names(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let line = line
                .strip_prefix("export function ")
                .or_else(|| line.strip_prefix("function "))?;
            let end = line.find('(').or_else(|| line.find('{'))?;
            Some(line[..end].trim().to_owned())
        })
        .collect()
}

fn source_const_names(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| {
            // Only column-zero declarations are module constants. Local `const` bindings inside
            // the component are implementation details and must not make the source contract
            // appear complete through an accidental name match.
            let line = line
                .strip_prefix("const ")
                .or_else(|| line.strip_prefix("export const "))?;
            let end = line
                .find(':')
                .or_else(|| line.find('='))
                .or_else(|| line.find(' '))?;
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
        .with_context(|| format!("DiffPane mapping lacks a Rust test anchor: {item}"))?;
    let source = if let Some(source) = sources.get(path) {
        source
    } else {
        let text = fs::read_to_string(repo.join(path))
            .with_context(|| format!("read DiffPane Rust evidence {path}"))?;
        let parsed = syn::parse_file(&text)
            .with_context(|| format!("parse DiffPane Rust evidence {path}"))?;
        sources.entry(path.to_owned()).or_insert(parsed)
    };
    ensure!(
        crate::rust_items_have_test(&source.items, Some(anchor)),
        "DiffPane mapping references a missing executable Rust test: {item}"
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
        "pinned DiffPane {commit} changed size: {} != {bytes}",
        source.len()
    );
    ensure!(
        source.split(|byte| *byte == b'\n').count() == lines + 1,
        "pinned DiffPane {commit} changed line count"
    );
    ensure!(
        format!("{:x}", Sha256::digest(&source)) == sha,
        "pinned DiffPane {commit} changed SHA-256"
    );
    String::from_utf8(source).context("pinned DiffPane is not UTF-8")
}

fn verify_oracle(repo: &Path, path: &str) -> Result<()> {
    let bytes =
        fs::read(repo.join(path)).with_context(|| format!("read DiffPane oracle {path}"))?;
    let value: Value =
        serde_json::from_slice(&bytes).with_context(|| format!("parse DiffPane oracle {path}"))?;
    let encoded = value.to_string();
    ensure!(
        encoded.contains(BASELINE) && encoded.contains(STABLE),
        "DiffPane oracle {path} does not identify both pinned trees"
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
                let text = fs::read_to_string(repo.join(marker.path))
                    .with_context(|| format!("read DiffPane native surface {}", marker.path))?;
                contents.entry(marker.path).or_insert(text)
            };
            ensure!(
                source.contains(marker.marker),
                "DiffPane native surface {} is missing {:?} for {}",
                marker.path,
                marker.marker,
                contract.name
            );
        }
    }
    Ok(())
}

/// Verify the complete pinned DiffPane source blob and its executable native projection.
pub(crate) fn verify(repo: &Path, baseline: &str) -> Result<()> {
    ensure!(
        baseline == BASELINE,
        "DiffPane verifier received unexpected baseline {baseline}"
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
        "storedReviewNoteMetadata",
        "storedReviewNoteActions",
        "resetOpenTuiScrollAccumulators",
        "clampVerticalScrollTop",
        "streamRowBoundsAt",
        "buildAdjacentPrefetchFileIds",
        "buildHighlightPrefetchFileIds",
        "DiffPane",
    ];
    ensure!(
        baseline_functions == expected_baseline_functions,
        "pinned DiffPane baseline function surface changed: {baseline_functions:?}"
    );
    let expected_constants = [
        "EMPTY_VISIBLE_AGENT_NOTES",
        "EMPTY_EXPANDED_GAP_KEYS",
        "EMPTY_EXPANDED_GAPS_BY_FILE_ID",
        "EMPTY_FILE_VIEWS",
        "EMPTY_LINE_HIGHLIGHTS",
        "EMPTY_SOURCE_STATUS_BY_FILE_ID",
        "NOOP_TOGGLE_GAP",
    ];
    ensure!(
        source_const_names(&source) == expected_constants,
        "pinned DiffPane constants changed"
    );

    let stable_functions = source_function_names(&stable);
    let expected_stable_functions = [
        "resetOpenTuiScrollAccumulators",
        "clampVerticalScrollTop",
        "estimateInitialRenderViewportHeight",
        "streamRowBoundsAt",
        "buildAdjacentPrefetchFileIds",
        "buildHighlightPrefetchFileIds",
        "retainCopySelectionCapture",
        "DiffPane",
    ];
    ensure!(
        stable_functions == expected_stable_functions,
        "pinned stable DiffPane function surface changed: {stable_functions:?}"
    );
    for marker in SOURCE_MARKERS {
        ensure!(
            source.contains(marker),
            "pinned DiffPane source is missing required contract marker {marker:?}"
        );
    }

    let contract_names = FUNCTION_CONTRACTS
        .iter()
        .map(|contract| contract.name)
        .collect::<Vec<_>>();
    let expected_names = baseline_functions
        .iter()
        .map(String::as_str)
        .chain([
            "estimateInitialRenderViewportHeight",
            "retainCopySelectionCapture",
        ])
        .collect::<Vec<_>>();
    let mut unique = BTreeSet::new();
    ensure!(
        contract_names.iter().all(|name| unique.insert(*name)),
        "DiffPane contract table repeats a source function"
    );
    ensure!(
        expected_names
            .iter()
            .all(|name| contract_names.contains(name)),
        "DiffPane contract table omits a pinned function"
    );
    let mut rust_sources = HashMap::new();
    for contract in FUNCTION_CONTRACTS {
        ensure!(
            !contract.native.is_empty() && !contract.tests.is_empty(),
            "DiffPane contract {} has no native surface or executable evidence",
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

    let migration = fs::read_to_string(repo.join("docs/diff-pane-migration.md"))
        .context("read DiffPane migration documentation")?;
    for marker in [
        SOURCE_PATH,
        "106,437",
        "96,299",
        "non-overlapping",
        "Ratatui",
        "copy selection",
        "pinned-header",
        "No TypeScript source mirror",
    ] {
        ensure!(
            migration.contains(marker),
            "DiffPane migration documentation is missing {marker:?}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_ratatui_review_pane_replaces_both_pinned_source_contracts() {
        let repo = crate::repo_root().unwrap();
        verify(&repo, BASELINE).unwrap();
    }
}
