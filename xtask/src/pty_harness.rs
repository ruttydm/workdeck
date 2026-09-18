//! Exhaustive source accounting for Hunk's PTY integration harness.
//!
//! Hunk's helper launches a Bun/Tuistory process, drives raw mouse and keyboard bytes, builds
//! temporary repositories, and inspects terminal cells.  Workdeck keeps the behavior in the
//! Rust `qwertty-term-vt`/PTY harness used by `terminal_pager`; the upstream TypeScript helper is
//! read only through its pinned Git blobs and is never executed or mirrored.

use anyhow::{Context, Result, ensure};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;

const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
const STABLE: &str = "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd";
const SOURCE_PATH: &str = "test/pty/harness.ts";
const BASELINE_BYTES: usize = 36_240;
const BASELINE_LINES: usize = 1_146;
const BASELINE_SHA256: &str = "83653303ff81ef7ba00a5bacb3eab2832ce8fc8b4f87c376d0d1eed37d834b51";
const STABLE_BYTES: usize = 35_035;
const STABLE_LINES: usize = 1_113;
const STABLE_SHA256: &str = "1f19ec9777ac4cc3487158e3c7b78bfd6008bc18ad702342ad9912f5e084fd10";

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
        name: "resolveBunExecutable",
        native: &[NativeMarker {
            path: "crates/workdeck-cli/tests/terminal_pager.rs",
            marker: "struct Session {",
        }],
        tests: &[
            "crates/workdeck-cli/tests/terminal_pager/harness.rs#snapshot_string_helpers_match_both_pinned_oracles",
        ],
    },
    FunctionContract {
        name: "loadTuistory",
        native: &[
            NativeMarker {
                path: "crates/workdeck-cli/tests/terminal_pager.rs",
                marker: "qwertty_term_vt::stream::{Stream, TerminalHandler}",
            },
            NativeMarker {
                path: "crates/workdeck-cli/tests/terminal_pager.rs",
                marker: "fn wait_for(",
            },
        ],
        tests: &[
            "crates/workdeck-cli/tests/terminal_pager.rs#explicit_pager_hides_chrome_and_pages_forward_on_space",
            "crates/workdeck-cli/tests/terminal_pager.rs#general_pager_accepts_terminal_mouse_wheel",
        ],
    },
    FunctionContract {
        name: "sleep",
        native: &[NativeMarker {
            path: "crates/workdeck-cli/tests/terminal_pager.rs",
            marker: "std::thread::sleep",
        }],
        tests: &[
            "crates/workdeck-cli/tests/terminal_pager/harness.rs#mouse_drag_interpolates_all_five_source_steps_in_both_directions",
        ],
    },
    FunctionContract {
        name: "measureKeyScroll",
        native: &[NativeMarker {
            path: "crates/workdeck-cli/tests/terminal_pager.rs",
            marker: "fn wheel_until(",
        }],
        tests: &[
            "crates/workdeck-cli/tests/terminal_pager.rs#general_pager_navigates_to_bottom_clamped_final_file_and_back",
            "crates/workdeck-cli/tests/terminal_pager.rs#pager_half_page_page_up_and_content_jumps",
        ],
    },
    FunctionContract {
        name: "moveMouse",
        native: &[
            NativeMarker {
                path: "crates/workdeck-cli/tests/terminal_pager.rs",
                marker: "fn move_mouse(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/lib.rs",
                marker: "pub fn handle_mouse_event(",
            },
        ],
        tests: &[
            "crates/workdeck-cli/tests/terminal_pager.rs#real_terminal_gap_click_expands_and_collapses_source_in_both_layouts",
            "crates/workdeck-cli/tests/terminal_pager.rs#clicked_add_note_can_cancel_and_save_with_mouse_controls",
        ],
    },
    FunctionContract {
        name: "revealAddNoteAffordance",
        native: &[
            NativeMarker {
                path: "crates/workdeck-cli/tests/terminal_pager.rs",
                marker: "fn click_label(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/diff_section_body.rs",
                marker: "add_note",
            },
        ],
        tests: &[
            "crates/workdeck-cli/tests/terminal_pager.rs#clicked_add_note_can_cancel_and_save_with_mouse_controls",
            "crates/workdeck-cli/tests/terminal_pager.rs#stack_add_note_affordance_saves_clicked_target",
        ],
    },
    FunctionContract {
        name: "dragMouse",
        native: &[
            NativeMarker {
                path: "crates/workdeck-cli/tests/terminal_pager/harness.rs",
                marker: "pub(super) fn drag_mouse(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/copy_selection.rs",
                marker: "CopySelectionDrag",
            },
        ],
        tests: &[
            "crates/workdeck-cli/tests/terminal_pager/harness.rs#mouse_drag_interpolates_all_five_source_steps_in_both_directions",
            "crates/workdeck-tui/src/lib.rs#tests::nested_row_mouse_action_claims_parent_selection_event",
        ],
    },
    FunctionContract {
        name: "rightmostColumnOf",
        native: &[NativeMarker {
            path: "crates/workdeck-cli/tests/terminal_pager/harness.rs",
            marker: "pub(super) fn rightmost_column_of(",
        }],
        tests: &[
            "crates/workdeck-cli/tests/terminal_pager/harness.rs#snapshot_string_helpers_match_both_pinned_oracles",
        ],
    },
    FunctionContract {
        name: "rowCellBackgrounds",
        native: &[
            NativeMarker {
                path: "crates/workdeck-cli/tests/terminal_pager.rs",
                marker: "parser.terminal().snapshot()",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/lib.rs",
                marker: "Buffer::empty",
            },
        ],
        tests: &[
            "crates/workdeck-cli/tests/terminal_pager.rs#saved_notes_support_clickable_threaded_edit_reply_and_delete",
            "crates/workdeck-tui/src/lib.rs#tests::native_cursor_paint_blends_each_ratatui_surface_and_number_mode_stops_at_the_gutter",
        ],
    },
    FunctionContract {
        name: "lineIndexOf",
        native: &[NativeMarker {
            path: "crates/workdeck-cli/tests/terminal_pager/harness.rs",
            marker: "pub(super) fn line_index_of(",
        }],
        tests: &[
            "crates/workdeck-cli/tests/terminal_pager/harness.rs#snapshot_string_helpers_match_both_pinned_oracles",
            "crates/workdeck-cli/tests/terminal_pager.rs#user_notes_draft_and_save_inline_with_newline_geometry",
        ],
    },
    FunctionContract {
        name: "revealAddNoteNear",
        native: &[NativeMarker {
            path: "crates/workdeck-cli/tests/terminal_pager.rs",
            marker: "fn reveal_note_actions(",
        }],
        tests: &[
            "crates/workdeck-cli/tests/terminal_pager.rs#clicked_add_note_owns_keyboard_cancel_and_save",
            "crates/workdeck-cli/tests/terminal_pager.rs#multiple_clicked_notes_survive_on_one_hunk",
        ],
    },
    FunctionContract {
        name: "revealAddNoteOnRow",
        native: &[NativeMarker {
            path: "crates/workdeck-cli/tests/terminal_pager.rs",
            marker: "fn open_on_row(",
        }],
        tests: &[
            "crates/workdeck-cli/tests/terminal_pager.rs#context_click_overrides_keyboard_cursor_without_moving_target_row",
            "crates/workdeck-cli/tests/terminal_pager.rs#deletion_only_add_note_affordance_saves_old_side",
        ],
    },
    FunctionContract {
        name: "writeText",
        native: &[NativeMarker {
            path: "crates/workdeck-cli/tests/terminal_pager/harness.rs",
            marker: "std::fs::write(root.path().join",
        }],
        tests: &[
            "crates/workdeck-cli/tests/terminal_pager/harness.rs#direct_file_pair_bytes_match_both_frozen_upstream_oracles",
            "crates/workdeck-cli/tests/terminal_pager/harness.rs#repository_factory_commits_baseline_and_prepared_entries_before_changes",
        ],
    },
    FunctionContract {
        name: "shellQuote",
        native: &[NativeMarker {
            path: "crates/workdeck-cli/tests/terminal_pager.rs",
            marker: "Command::new(\"git\")",
        }],
        tests: &[
            "crates/workdeck-cli/tests/terminal_pager.rs#real_git_review_defers_source_until_expansion_in_both_layouts",
        ],
    },
    FunctionContract {
        name: "createNumberedExportLines",
        native: &[NativeMarker {
            path: "crates/workdeck-cli/tests/terminal_pager.rs",
            marker: "fn patch(lines: usize)",
        }],
        tests: &[
            "crates/workdeck-cli/tests/terminal_pager.rs#explicit_pager_hides_chrome_and_pages_forward_on_space",
            "crates/workdeck-cli/tests/terminal_pager.rs#general_pager_navigates_to_bottom_clamped_final_file_and_back",
        ],
    },
    FunctionContract {
        name: "runGit",
        native: &[NativeMarker {
            path: "crates/workdeck-cli/tests/terminal_pager/harness.rs",
            marker: "pub(super) fn git(",
        }],
        tests: &[
            "crates/workdeck-cli/tests/terminal_pager/harness.rs#repository_factory_commits_baseline_and_prepared_entries_before_changes",
            "crates/workdeck-cli/tests/terminal_pager.rs#real_git_review_defers_source_until_expansion_in_both_layouts",
        ],
    },
    FunctionContract {
        name: "createPtyHarness",
        native: &[
            NativeMarker {
                path: "crates/workdeck-cli/tests/terminal_pager.rs",
                marker: "struct Session {",
            },
            NativeMarker {
                path: "crates/workdeck-cli/tests/terminal_pager.rs",
                marker: "fn launch_in_config(",
            },
            NativeMarker {
                path: "crates/workdeck-cli/tests/terminal_lifecycle.rs",
                marker: "fn pty_pair()",
            },
        ],
        tests: &[
            "crates/workdeck-cli/tests/terminal_pager.rs#draft_save_accepts_tmux_csi_u_bytes_through_real_terminal_input",
            "crates/workdeck-cli/tests/terminal_pager.rs#stdin_patch_accepts_terminal_mouse_wheel",
            "crates/workdeck-cli/tests/terminal_pager.rs#piped_stdin_still_allows_concrete_theme_app_terminal_input",
            "crates/workdeck-cli/tests/terminal_lifecycle.rs#exits_cleanly_when_host_closes_pty_master",
            "crates/workdeck-cli/tests/terminal_pager.rs#session_attention_highlight_reveals_and_paints_exact_range_then_clears_and_navigates",
        ],
    },
];

const SOURCE_MARKERS: &[&str] = &[
    "spawnSync",
    "mkdtempSync",
    "writeFileSync",
    "rmSync",
    "tmpdir",
    "sourceEntrypoint",
    "tuistoryIdleDelayMs",
    "HUNK_TEST_EXECUTABLE",
    "process.versions.bun",
    "loadTuistory",
    "waitIdle",
    "waitForText",
    "writeRaw",
    "getTerminalData",
    "spans",
    "ChangedFileSpec",
    "createPtyHarness",
    "createLongWrapFilePair",
    "createWideCharacterFilePair",
    "createTabbedFilePair",
    "createDeletionOnlyFilePair",
    "createAgentFilePair",
    "createGapAnnotatedAgentFilePair",
    "createExpandableContextFilePair",
    "createScrollableFilePair",
    "createWatchFilePair",
    "createNumberedExportLines",
    "runGit",
];

fn source_function_names(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| {
            let line = line
                .strip_prefix("export async function ")
                .or_else(|| line.strip_prefix("export function "))
                .or_else(|| line.strip_prefix("async function "))
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
            let line = line
                .strip_prefix("export interface ")
                .or_else(|| line.strip_prefix("interface "))?;
            let end = line.find('{').or_else(|| line.find('<'))?;
            Some(line[..end].trim().to_owned())
        })
        .collect()
}

fn source_const_names(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| {
            let line = line.strip_prefix("const ")?;
            let end = [line.find(':'), line.find('=')]
                .into_iter()
                .flatten()
                .min()?;
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
        .with_context(|| format!("PTY harness mapping lacks a Rust test anchor: {item}"))?;
    let source = if let Some(source) = sources.get(path) {
        source
    } else {
        let text = fs::read_to_string(repo.join(path))
            .with_context(|| format!("read PTY harness Rust evidence {path}"))?;
        let parsed = syn::parse_file(&text)
            .with_context(|| format!("parse PTY harness Rust evidence {path}"))?;
        sources.entry(path.to_owned()).or_insert(parsed)
    };
    ensure!(
        crate::rust_items_have_test(&source.items, Some(anchor)),
        "PTY harness mapping references a missing executable Rust test: {item}"
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
        "pinned PTY harness {commit} changed size: {} != {bytes}",
        source.len()
    );
    ensure!(
        source.split(|byte| *byte == b'\n').count() == lines + 1,
        "pinned PTY harness {commit} changed line count"
    );
    ensure!(
        format!("{:x}", Sha256::digest(&source)) == sha,
        "pinned PTY harness {commit} changed SHA-256"
    );
    String::from_utf8(source).context("pinned PTY harness is not UTF-8")
}

fn verify_oracle(repo: &Path, path: &str) -> Result<()> {
    let bytes = fs::read(repo.join(path)).with_context(|| format!("read PTY oracle {path}"))?;
    let value: Value =
        serde_json::from_slice(&bytes).with_context(|| format!("parse PTY oracle {path}"))?;
    let encoded = value.to_string();
    ensure!(
        encoded.contains(BASELINE) && encoded.contains(STABLE),
        "PTY oracle {path} does not identify both pinned trees"
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
                    .with_context(|| format!("read PTY native surface {}", marker.path))?;
                contents.entry(marker.path).or_insert(text)
            };
            ensure!(
                source.contains(marker.marker),
                "PTY native surface {} is missing {:?} for {}",
                marker.path,
                marker.marker,
                contract.name
            );
        }
    }
    Ok(())
}

/// Verify both pinned PTY harness blobs and their native executable projection.
pub(crate) fn verify(repo: &Path, baseline: &str) -> Result<()> {
    ensure!(
        baseline == BASELINE,
        "PTY harness verifier received unexpected baseline {baseline}"
    );
    let source = verify_source_blob(
        repo,
        BASELINE,
        BASELINE_BYTES,
        BASELINE_LINES,
        BASELINE_SHA256,
    )?;
    let stable = verify_source_blob(repo, STABLE, STABLE_BYTES, STABLE_LINES, STABLE_SHA256)?;

    let expected_functions = [
        "resolveBunExecutable",
        "loadTuistory",
        "sleep",
        "measureKeyScroll",
        "moveMouse",
        "revealAddNoteAffordance",
        "dragMouse",
        "rightmostColumnOf",
        "rowCellBackgrounds",
        "lineIndexOf",
        "revealAddNoteNear",
        "revealAddNoteOnRow",
        "writeText",
        "shellQuote",
        "createNumberedExportLines",
        "runGit",
        "createPtyHarness",
    ];
    ensure!(
        source_function_names(&source) == expected_functions,
        "pinned PTY baseline function surface changed: {:?}",
        source_function_names(&source)
    );
    ensure!(
        source_function_names(&stable) == expected_functions,
        "pinned stable PTY function surface changed: {:?}",
        source_function_names(&stable)
    );
    ensure!(
        source_interface_names(&source) == ["ChangedFileSpec"],
        "pinned PTY baseline interface surface changed"
    );
    ensure!(
        source_interface_names(&stable) == ["ChangedFileSpec"],
        "pinned stable PTY interface surface changed"
    );
    ensure!(
        source_const_names(&source)
            == [
                "integrationDir",
                "repoRoot",
                "sourceEntrypoint",
                "tuistoryIdleDelayMs",
                "bunExecutable",
                "explicitHunkExecutable",
            ],
        "pinned PTY baseline constants changed"
    );
    ensure!(
        source_const_names(&stable)
            == [
                "integrationDir",
                "repoRoot",
                "sourceEntrypoint",
                "bunExecutable",
                "explicitHunkExecutable",
            ],
        "pinned stable PTY constants changed"
    );
    for marker in SOURCE_MARKERS {
        ensure!(
            source.contains(marker),
            "pinned PTY source is missing required contract marker {marker:?}"
        );
    }

    let contract_names = FUNCTION_CONTRACTS
        .iter()
        .map(|contract| contract.name)
        .collect::<Vec<_>>();
    let mut unique = BTreeSet::new();
    ensure!(
        contract_names.iter().all(|name| unique.insert(*name)),
        "PTY harness contract table repeats a source function"
    );
    ensure!(
        expected_functions
            .iter()
            .all(|name| contract_names.contains(name)),
        "PTY harness contract table omits a pinned function"
    );
    let mut rust_sources = HashMap::new();
    for contract in FUNCTION_CONTRACTS {
        ensure!(
            !contract.native.is_empty() && !contract.tests.is_empty(),
            "PTY harness contract {} has no native surface or executable evidence",
            contract.name
        );
        for test in contract.tests {
            verify_rust_anchor(repo, test, &mut rust_sources)?;
        }
    }
    verify_native_surface(repo)?;
    for oracle in [
        "port/hunk/oracles/pty-harness-snapshot-helpers.json",
        "port/hunk/oracles/pty-harness-file-pairs.json",
        "port/hunk/oracles/app-host-scroll-regression.json",
        "port/hunk/oracles/app-host-wrap-frames.json",
        "port/hunk/oracles/app-host-draft-csi-u.json",
        "port/hunk/oracles/app-host-draft-blur.json",
        "port/hunk/oracles/app-host-deep-note.json",
    ] {
        verify_oracle(repo, oracle)?;
    }

    let migration = fs::read_to_string(repo.join("docs/pty-harness-migration.md"))
        .context("read PTY harness migration documentation")?;
    for marker in [
        SOURCE_PATH,
        "36,240",
        "35,035",
        "non-overlapping",
        "qwertty",
        "mouse",
        "repository fixtures",
        "No TypeScript source mirror",
    ] {
        ensure!(
            migration.contains(marker),
            "PTY harness migration documentation is missing {marker:?}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_rust_pty_harness_replaces_both_pinned_sources() {
        let repo = crate::repo_root().unwrap();
        verify(&repo, BASELINE).unwrap();
    }
}
