//! Exhaustive source accounting for Hunk's top-level `App` composition.
//!
//! Hunk's `App.tsx` is the application-level wiring boundary: review state, keyboard/mouse
//! routing, pane layout, extensions, sessions, themes, reloads, and workspace writes meet there.
//! Workdeck keeps that boundary in the Ratatui `ReviewApp` plus `AppHostController` rather than
//! retaining React/OpenTUI.  The verifier pins both historical blobs and requires an explicit
//! native surface, executable tests, and frozen oracle set for the complete contract.

use anyhow::{Context, Result, ensure};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;

const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
const STABLE: &str = "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd";
const SOURCE_PATH: &str = "src/ui/App.tsx";
const BASELINE_BYTES: usize = 57_514;
const BASELINE_LINES: usize = 1_571;
const BASELINE_SHA256: &str = "76ed422be18d500c905f7f1f5508a222bdef676f9433a51623527bc17fdb3912";
const STABLE_BYTES: usize = 100_153;
const STABLE_LINES: usize = 2_623;
const STABLE_SHA256: &str = "514ed6c4591cdc206479664711beea8ff0079a91604722962f162ab7954703ed";

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
        name: "clamp",
        native: &[
            NativeMarker {
                path: "crates/workdeck-tui/src/lib.rs",
                marker: "fn reconcile_horizontal_offset(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/lib.rs",
                marker: ".min(self.max_horizontal_offset())",
            },
        ],
        tests: &[
            "crates/workdeck-tui/src/lib.rs#tests::filtering_and_reloading_reconcile_horizontal_offset_without_scroll_input",
            "crates/workdeck-tui/src/lib.rs#tests::scroll_wheel_at_eof_does_not_accumulate_invisible_overscroll",
        ],
    },
    FunctionContract {
        name: "withCurrentViewOptions",
        native: &[
            NativeMarker {
                path: "crates/workdeck-tui/src/lib.rs",
                marker: "fn current_view_preferences(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/app_host.rs",
                marker: "pub struct AppHostReloadPlan",
            },
        ],
        tests: &[
            "crates/workdeck-tui/src/app_host.rs#tests::current_review_refresh_uses_live_view_options_and_the_full_commit_gate",
            "crates/workdeck-tui/src/lib.rs#tests::review_shortcuts_change_layout_and_navigation",
        ],
    },
    FunctionContract {
        name: "App",
        native: &[
            NativeMarker {
                path: "crates/workdeck-tui/src/lib.rs",
                marker: "pub struct ReviewApp",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/lib.rs",
                marker: "pub fn handle_key(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/lib.rs",
                marker: "pub fn handle_mouse_event(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/lib.rs",
                marker: "fn apply_builtin_command_action(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/lib.rs",
                marker: "fn render_review(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/lib.rs",
                marker: "fn run_review_inner(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/app_host.rs",
                marker: "pub struct AppHostController",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/app_host.rs",
                marker: "pub fn process_pending",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/app_host.rs",
                marker: "pub fn publish_snapshot",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/extension_pane_controller.rs",
                marker: "pub struct ExtensionPaneControllerState",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/extension_pane_controller.rs",
                marker: "pub fn toggle_files_pane(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/extension_panes.rs",
                marker: "pub fn plan_extension_panes(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/extension_commands.rs",
                marker: "pub fn build_extension_app_commands(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/extension_dialogs.rs",
                marker: "pub(crate) struct ExtensionDialogQueue",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/extension_workspace.rs",
                marker: "pub fn normalize_workspace_write_request(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/extension_navigation.rs",
                marker: "pub fn guard_extension_reveal_line(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/theme_selector_controller.rs",
                marker: "pub struct ThemeController",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/lib.rs",
                marker: "keyboard_mode_controller:",
            },
        ],
        tests: &[
            "crates/workdeck-tui/src/app_host.rs#tests::broker_mutations_run_on_the_owner_thread_and_answer_after_commit",
            "crates/workdeck-tui/src/app_host.rs#tests::queued_reloads_run_to_completion_in_fifo_order",
            "crates/workdeck-tui/src/app_host.rs#tests::current_review_refresh_uses_live_view_options_and_the_full_commit_gate",
            "crates/workdeck-tui/src/app_host.rs#tests::mouse_scroll_bootstrap_matches_both_pinned_source_fixtures",
            "crates/workdeck-tui/src/app_host.rs#tests::file_shortcuts_publish_selection_and_filter_focus_retains_selected_file",
            "crates/workdeck-tui/src/app_host.rs#tests::plain_launch_reload_cannot_enable_markup_and_rejected_comments_leave_no_state",
            "crates/workdeck-tui/src/app_host.rs#tests::frozen_app_host_oracle_covers_every_baseline_byte_and_both_pins",
            "crates/workdeck-tui/src/lib.rs#tests::review_shortcuts_change_layout_and_navigation",
            "crates/workdeck-tui/src/lib.rs#tests::pinned_rapid_navigation_and_wheel_rendering_settles",
            "crates/workdeck-tui/src/lib.rs#tests::filtering_and_reloading_reconcile_horizontal_offset_without_scroll_input",
            "crates/workdeck-tui/src/lib.rs#tests::user_note_composer_edits_and_replies_with_stable_public_identity",
            "crates/workdeck-tui/src/lib.rs#tests::native_line_highlight_refresh_actions_update_live_epochs_and_notices",
            "crates/workdeck-tui/src/extension_pane_controller.rs#tests::frozen_hunk_pane_controller_oracle_is_main_only_and_complete",
            "crates/workdeck-tui/src/extension_commands.rs#tests::frozen_hunk_oracle_records_both_pinned_runs",
            "crates/workdeck-tui/src/extension_dialogs.rs#tests::frozen_hunk_dialog_oracle_records_both_pinned_baselines_and_all_tests",
            "crates/workdeck-tui/src/extension_navigation.rs#tests::frozen_hunk_extension_navigation_oracle_records_both_pinned_baselines",
            "crates/workdeck-tui/src/extension_panes.rs#tests::frozen_hunk_extension_pane_oracles_cover_both_pinned_trees",
        ],
    },
];

const SOURCE_MARKERS: &[&str] = &[
    "useRenderer",
    "useTerminalDimensions",
    "useEffect",
    "useLayoutEffect",
    "useMemo",
    "useRef",
    "useState",
    "experimentalFeatureEnabled",
    "resolveExperimentalDiffFiles",
    "isVcsReviewInput",
    "useTerminalReview",
    "useExtensionRuntimeBridge",
    "useExtensionRuntimeBindings",
    "useExtensionPaneController",
    "useExtensionDialogController",
    "useExtensionWorkspaceControls",
    "useHunkSessionBridge",
    "useAppKeyboardShortcuts",
    "useCurrentReviewRefreshController",
    "useThemeSelectorController",
    "useUserNoteComposer",
    "useViewPreferenceQuitController",
    "useFilePresentationController",
    "useFilePresentationRendering",
    "useLineHighlightsController",
    "useKeyboardModeController",
    "buildAppCommands",
    "buildAppMenus",
    "buildExtensionAppCommands",
    "resolveCommandKeys",
    "createExtensionPaneKeybindings",
    "resolveResponsiveLayout",
    "mergeLineHighlightMaps",
    "setMouseCapture",
    "openSelectedFileInEditor",
    "toggleSelectedHunkGap",
    "toggleAgentNotes",
    "showLineNumbers",
    "showHunkHeaders",
    "wrapLines",
    "layoutMode",
    "copyDecorations",
    "lineCursorAlignmentRequest",
    "paneLayout",
    "renderSidebar",
    "extensionDialog",
    "themeSelectorOpen",
    "pendingTrustRepoRoot",
    "reviewProducer",
    "hostClient",
    "onWorkspaceWriteCompleted",
    "runWorkspaceWrite",
    "workspaceFileWriter",
    "MenuBar",
    "ConfirmDialog",
    "ExtensionDialog",
    "StatusBar",
    "DiffPane",
    "ExtensionPaneHost",
    "PaneDivider",
    "HUNK_FILES_PANE_KEY",
];

const STABLE_ONLY_MARKERS: &[&str] = &[
    "ActiveAddNoteTarget",
    "ThemeSelectorState",
    "SELECTION_CHANGED_DEBOUNCE_MS",
    "withCurrentViewOptions",
    "WorkspaceRefreshRequest",
    "writeWorkspaceFile",
];

const ORACLE_FIXTURES: &[&str] = &[
    "port/hunk/oracles/app-host.json",
    "port/hunk/oracles/app-host-bootstrap-preferences.json",
    "port/hunk/oracles/app-host-extension-dialogs.json",
    "port/hunk/oracles/app-host-extension-navigation.json",
    "port/hunk/oracles/app-host-extension-sidebar.json",
    "port/hunk/oracles/app-host-extensions.json",
    "port/hunk/oracles/app-host-file-shortcuts.json",
    "port/hunk/oracles/app-host-file-view-modes.json",
    "port/hunk/oracles/app-host-file-views.json",
    "port/hunk/oracles/app-host-filter-selection.json",
    "port/hunk/oracles/app-host-hidden-menu.json",
    "port/hunk/oracles/app-host-horizontal-arrows.json",
    "port/hunk/oracles/app-host-key-routing.json",
    "port/hunk/oracles/app-host-keybindings.json",
    "port/hunk/oracles/app-host-keyboard-modes.json",
    "port/hunk/oracles/app-host-layout-anchor.json",
    "port/hunk/oracles/app-host-menu-reload.json",
    "port/hunk/oracles/app-host-menu-wrap.json",
    "port/hunk/oracles/app-host-page-selection.json",
    "port/hunk/oracles/app-host-pager-filter.json",
    "port/hunk/oracles/app-host-responsive.json",
    "port/hunk/oracles/app-host-scroll-regression.json",
    "port/hunk/oracles/app-host-selection.json",
    "port/hunk/oracles/app-host-session-filter.json",
    "port/hunk/oracles/app-host-sidebar-click.json",
    "port/hunk/oracles/app-host-sidebar-modes.json",
    "port/hunk/oracles/app-host-sidebar-resize.json",
    "port/hunk/oracles/app-host-sidebar-toggle.json",
    "port/hunk/oracles/app-host-sidebar-visibility.json",
    "port/hunk/oracles/app-host-startup-summary.json",
    "port/hunk/oracles/app-host-theme-agent-menus.json",
    "port/hunk/oracles/app-host-theme-events-partial.json",
    "port/hunk/oracles/app-host-theme-hover.json",
    "port/hunk/oracles/app-host-theme-jk.json",
    "port/hunk/oracles/app-host-theme-reopen.json",
    "port/hunk/oracles/app-host-theme-wheel.json",
    "port/hunk/oracles/app-host-transparent.json",
    "port/hunk/oracles/app-host-view-shortcuts.json",
    "port/hunk/oracles/app-host-viewport-notes.json",
    "port/hunk/oracles/app-host-watch.json",
    "port/hunk/oracles/app-host-wheel-anchor.json",
    "port/hunk/oracles/app-host-wheel-selection.json",
    "port/hunk/oracles/app-host-workspace.json",
    "port/hunk/oracles/app-host-wrap-anchor.json",
    "port/hunk/oracles/app-host-wrap-frames.json",
    "port/hunk/oracles/app-host-wrap-horizontal-reset.json",
    "port/hunk/oracles/app-host-wrap-toggle.json",
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
        .with_context(|| format!("App mapping lacks a Rust test anchor: {item}"))?;
    let source = if let Some(source) = sources.get(path) {
        source
    } else {
        let text = fs::read_to_string(repo.join(path))
            .with_context(|| format!("read App Rust evidence {path}"))?;
        let parsed =
            syn::parse_file(&text).with_context(|| format!("parse App Rust evidence {path}"))?;
        sources.entry(path.to_owned()).or_insert(parsed)
    };
    ensure!(
        crate::rust_items_have_test(&source.items, Some(anchor)),
        "App mapping references a missing executable Rust test: {item}"
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
        "pinned App {commit} changed size: {} != {bytes}",
        source.len()
    );
    ensure!(
        source.split(|byte| *byte == b'\n').count() == lines + 1,
        "pinned App {commit} changed line count"
    );
    ensure!(
        format!("{:x}", Sha256::digest(&source)) == sha,
        "pinned App {commit} changed SHA-256"
    );
    String::from_utf8(source).context("pinned App is not UTF-8")
}

fn verify_oracle(repo: &Path, path: &str) -> Result<()> {
    let bytes = fs::read(repo.join(path)).with_context(|| format!("read App oracle {path}"))?;
    let value: Value =
        serde_json::from_slice(&bytes).with_context(|| format!("parse App oracle {path}"))?;
    let encoded = value.to_string();
    ensure!(
        encoded.contains(BASELINE) && encoded.contains(STABLE),
        "App oracle {path} does not identify both pinned trees"
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
                    .with_context(|| format!("read App native surface {}", marker.path))?;
                contents.entry(marker.path).or_insert(text)
            };
            ensure!(
                source.contains(marker.marker),
                "App native surface {} is missing {:?} for {}",
                marker.path,
                marker.marker,
                contract.name
            );
        }
    }
    Ok(())
}

/// Verify both pinned App blobs and their complete Ratatui/AppHost projection.
pub(crate) fn verify(repo: &Path, baseline: &str) -> Result<()> {
    ensure!(
        baseline == BASELINE,
        "App verifier received unexpected baseline {baseline}"
    );
    let source = verify_source_blob(
        repo,
        BASELINE,
        BASELINE_BYTES,
        BASELINE_LINES,
        BASELINE_SHA256,
    )?;
    let stable = verify_source_blob(repo, STABLE, STABLE_BYTES, STABLE_LINES, STABLE_SHA256)?;

    ensure!(
        source_function_names(&source) == ["clamp", "App"],
        "pinned App baseline function surface changed"
    );
    ensure!(
        source_type_names(&source) == ["FocusArea"],
        "pinned App baseline type surface changed"
    );
    ensure!(
        source_interface_names(&source).is_empty(),
        "pinned App baseline unexpectedly gained an interface"
    );
    ensure!(
        source_const_names(&source)
            == [
                "FAST_CODE_HORIZONTAL_SCROLL_COLUMNS",
                "LazyAgentSkillDialog",
                "LazyHelpDialog",
                "LazyMenuDropdown",
                "LazyThemeSelectorDialog",
            ],
        "pinned App baseline constant surface changed"
    );

    ensure!(
        source_function_names(&stable) == ["clamp", "withCurrentViewOptions", "App"],
        "pinned stable App function surface changed"
    );
    ensure!(
        source_type_names(&stable)
            == [
                "FocusArea",
                "ActiveAddNoteTarget",
                "ThemeSelectorState",
                "WorkspaceFileWriter",
                "WorkspaceWriteRunner",
            ],
        "pinned stable App type surface changed"
    );
    ensure!(
        source_interface_names(&stable) == ["WorkspaceRefreshRequest"],
        "pinned stable App interface surface changed"
    );
    ensure!(
        source_const_names(&stable)
            == [
                "FAST_CODE_HORIZONTAL_SCROLL_COLUMNS",
                "SELECTION_CHANGED_DEBOUNCE_MS",
                "LazyAgentSkillDialog",
                "LazyHelpDialog",
                "LazyMenuDropdown",
                "LazyThemeSelectorDialog",
                "writeWorkspaceFile",
            ],
        "pinned stable App constant surface changed"
    );

    for marker in SOURCE_MARKERS {
        ensure!(
            source.contains(marker),
            "pinned App source is missing required contract marker {marker:?}"
        );
    }
    for marker in STABLE_ONLY_MARKERS {
        ensure!(
            stable.contains(marker),
            "pinned stable App source is missing stable-only marker {marker:?}"
        );
    }

    let contract_names = FUNCTION_CONTRACTS
        .iter()
        .map(|contract| contract.name)
        .collect::<Vec<_>>();
    let expected_names = ["clamp", "App", "withCurrentViewOptions"];
    let mut unique = BTreeSet::new();
    ensure!(
        contract_names.iter().all(|name| unique.insert(*name)),
        "App contract table repeats a source function"
    );
    ensure!(
        expected_names
            .iter()
            .all(|name| contract_names.contains(name)),
        "App contract table omits a pinned function"
    );
    let mut rust_sources = HashMap::new();
    for contract in FUNCTION_CONTRACTS {
        ensure!(
            !contract.native.is_empty() && !contract.tests.is_empty(),
            "App contract {} has no native surface or executable evidence",
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

    let migration = fs::read_to_string(repo.join("docs/app-shell-migration.md"))
        .context("read App migration documentation")?;
    for marker in [
        SOURCE_PATH,
        "57,514",
        "100,153",
        "non-overlapping",
        "Ratatui",
        "AppHostController",
        "extension",
        "workspace writes",
        "No TypeScript source mirror",
    ] {
        ensure!(
            migration.contains(marker),
            "App migration documentation is missing {marker:?}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_ratatui_app_shell_replaces_both_pinned_app_contracts() {
        let repo = crate::repo_root().unwrap();
        verify(&repo, BASELINE).unwrap();
    }
}
