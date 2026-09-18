//! Exhaustive source accounting for Hunk's session-broker end-to-end tests.
//!
//! The source suite starts real terminal sessions, auto-starts an authenticated loopback daemon,
//! routes comments and highlights, checks multiple-session isolation, and handles a foreign
//! listener on the configured port.  Workdeck exercises those contracts with the native broker,
//! session CLI, and Ratatui PTY tests.  Only pinned Git blobs are inspected here; no TypeScript
//! source mirror or JavaScript runtime is part of the shipped product.

use anyhow::{Context, Result, ensure};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;

const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
const STABLE: &str = "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd";
const SOURCE_PATH: &str = "test/session/broker-e2e.test.ts";
const BASELINE_BYTES: usize = 23_792;
const BASELINE_LINES: usize = 790;
const BASELINE_SHA256: &str = "869e91e49edef872c157e7a192e64ea984ffeba5a71e9839507e7fe7788ad04a";
const STABLE_BYTES: usize = 23_235;
const STABLE_LINES: usize = 779;
const STABLE_SHA256: &str = "67555ddeb64ce4138faf0fd632a8e0ff93f332379035eb25d782747797b8671a";

struct NativeMarker {
    path: &'static str,
    marker: &'static str,
}

struct FunctionContract {
    name: &'static str,
    native: &'static [NativeMarker],
    tests: &'static [&'static str],
}

struct TestMapping {
    source: &'static str,
    native: &'static [&'static str],
}

const TEST_MAPPINGS: &[TestMapping] = &[
    TestMapping {
        source: "a live Hunk session auto-starts the daemon and renders CLI comments inline",
        native: &[
            "crates/workdeck-session/src/broker_client/tests.rs#native_client_authenticates_and_registers_with_native_daemon",
            "crates/workdeck-tui/src/app_host.rs#tests::queued_file_reload_preserves_live_comment_in_updated_terminal_frame",
            "crates/workdeck-cli/tests/terminal_pager.rs#saved_notes_support_clickable_threaded_edit_reply_and_delete",
        ],
    },
    TestMapping {
        source: "session CLI can inspect current focus and navigate hunks in a live session",
        native: &[
            "crates/workdeck-session/src/agent_commands.rs#tests::remaining_session_actions_dispatch_and_keep_text_output_stable",
            "crates/workdeck-session/src/agent_cli_format.rs#tests::command_result_formatters_describe_navigation_and_comment_side_effects",
            "crates/workdeck-tui/src/app_host.rs#tests::file_shortcuts_publish_selection_and_filter_focus_retains_selected_file",
        ],
    },
    TestMapping {
        source: "session CLI marks a character range and reveals its exact line in a live session",
        native: &[
            "crates/workdeck-session/src/agent_commands.rs#tests::highlight_actions_dispatch_and_keep_text_output_stable",
            "crates/workdeck-cli/tests/terminal_pager.rs#session_attention_highlight_reveals_and_paints_exact_range_then_clears_and_navigates",
            "crates/workdeck-tui/src/session_review_controller.rs#tests::agent_highlights_share_validation_limits_and_clear_counts",
        ],
    },
    TestMapping {
        source: "one daemon routes CLI comments to the correct Hunk session when multiple local sessions are open",
        native: &[
            "crates/workdeck-session/src/broker_client/tests.rs#native_client_authenticates_and_registers_with_native_daemon",
            "crates/workdeck-session/tests/native_broker_adapters.rs#serves_generic_daemon_api_and_websocket_path_through_bun_parity_surface",
            "crates/workdeck-session/src/broker_daemon.rs#tests::dispatches_one_raw_command_through_broker_api",
        ],
    },
    TestMapping {
        source: "a normal Hunk session still renders and exits cleanly when a non-Hunk listener owns the MCP port",
        native: &[
            "crates/workdeck-session/src/broker_client/tests.rs#conflicting_listener_warning_is_actionable_and_deduplicated",
            "crates/workdeck-session/tests/native_broker_adapters.rs#retains_bun_and_node_non_upgrade_socket_path_semantics",
            "crates/workdeck-cli/tests/terminal_lifecycle.rs#daemon_exits_cleanly_after_sigterm_instead_of_hot_looping",
        ],
    },
];

const BROKER_TESTS: &[&str] = &[
    "crates/workdeck-session/tests/native_broker_adapters.rs#serves_generic_daemon_api_and_websocket_path_through_bun_parity_surface",
    "crates/workdeck-session/tests/native_broker_adapters.rs#retains_bun_and_node_non_upgrade_socket_path_semantics",
    "crates/workdeck-session/tests/native_broker_adapters.rs#daemon_idle_shutdown_reuses_the_transport_stop_path",
    "crates/workdeck-session/src/broker_client/tests.rs#native_client_authenticates_and_registers_with_native_daemon",
    "crates/workdeck-session/src/broker_client/tests.rs#conflicting_listener_warning_is_actionable_and_deduplicated",
];

const SESSION_TESTS: &[&str] = &[
    "crates/workdeck-session/src/agent_commands.rs#tests::remaining_session_actions_dispatch_and_keep_text_output_stable",
    "crates/workdeck-session/src/agent_commands.rs#tests::highlight_actions_dispatch_and_keep_text_output_stable",
    "crates/workdeck-session/src/agent_cli_format.rs#tests::command_result_formatters_describe_navigation_and_comment_side_effects",
    "crates/workdeck-session/src/agent_cli_format.rs#tests::list_and_get_preserve_terminal_metadata_and_selected_hunk_summaries",
];

const TUI_TESTS: &[&str] = &[
    "crates/workdeck-tui/src/app_host.rs#tests::queued_file_reload_preserves_live_comment_in_updated_terminal_frame",
    "crates/workdeck-tui/src/app_host.rs#tests::file_shortcuts_publish_selection_and_filter_focus_retains_selected_file",
    "crates/workdeck-tui/src/session_review_controller.rs#tests::agent_highlights_share_validation_limits_and_clear_counts",
    "crates/workdeck-cli/tests/terminal_pager.rs#session_attention_highlight_reveals_and_paints_exact_range_then_clears_and_navigates",
];

const FUNCTION_CONTRACTS: &[FunctionContract] = &[
    FunctionContract {
        name: "supportsControllableScript",
        native: &[NativeMarker {
            path: "crates/workdeck-cli/tests/terminal_pager.rs",
            marker: "struct Session {",
        }],
        tests: TUI_TESTS,
    },
    FunctionContract {
        name: "cleanupTempDirs",
        native: &[NativeMarker {
            path: "crates/workdeck-cli/tests/terminal_pager.rs",
            marker: "impl Drop for Session",
        }],
        tests: TUI_TESTS,
    },
    FunctionContract {
        name: "shellQuote",
        native: &[NativeMarker {
            path: "crates/workdeck-session/src/agent_cli_format.rs",
            marker: "fn format_session_path",
        }],
        tests: SESSION_TESTS,
    },
    FunctionContract {
        name: "stripTerminalControl",
        native: &[NativeMarker {
            path: "crates/workdeck-session/src/agent_cli_format.rs",
            marker: "human_readable_session_paths_cannot_emit_terminal_controls",
        }],
        tests: &[
            "crates/workdeck-session/src/agent_cli_format.rs#tests::human_readable_session_paths_cannot_emit_terminal_controls",
        ],
    },
    FunctionContract {
        name: "createFixtureFiles",
        native: &[NativeMarker {
            path: "crates/workdeck-cli/tests/terminal_pager.rs",
            marker: "fn launch_in_config(",
        }],
        tests: TUI_TESTS,
    },
    FunctionContract {
        name: "reserveLoopbackPort",
        native: &[
            NativeMarker {
                path: "crates/workdeck-session/tests/native_broker_adapters.rs",
                marker: "fn start(daemon: TestDaemon)",
            },
            NativeMarker {
                path: "crates/workdeck-session/src/broker_client/tests.rs",
                marker: "fn native_client_authenticates_and_registers_with_native_daemon()",
            },
        ],
        tests: BROKER_TESTS,
    },
    FunctionContract {
        name: "spawnHunkSession",
        native: &[
            NativeMarker {
                path: "crates/workdeck-cli/tests/terminal_pager.rs",
                marker: "fn launch_in_config(",
            },
            NativeMarker {
                path: "crates/workdeck-session/src/broker_connection.rs",
                marker: "pub fn create_session_broker_connection",
            },
        ],
        tests: TUI_TESTS,
    },
    FunctionContract {
        name: "quitHunkSession",
        native: &[NativeMarker {
            path: "crates/workdeck-cli/tests/terminal_pager.rs",
            marker: "fn quit(&mut self)",
        }],
        tests: TUI_TESTS,
    },
    FunctionContract {
        name: "cleanupHunkSession",
        native: &[NativeMarker {
            path: "crates/workdeck-cli/tests/terminal_pager.rs",
            marker: "impl Drop for Session",
        }],
        tests: TUI_TESTS,
    },
    FunctionContract {
        name: "runSessionCli",
        native: &[NativeMarker {
            path: "crates/workdeck-session/src/agent_commands.rs",
            marker: "pub fn run_session_command(",
        }],
        tests: SESSION_TESTS,
    },
    FunctionContract {
        name: "waitUntil",
        native: &[
            NativeMarker {
                path: "crates/workdeck-session/tests/native_broker_adapters.rs",
                marker: "fn wait_until(label: &str",
            },
            NativeMarker {
                path: "crates/workdeck-session/src/broker_connection.rs",
                marker: "fn wait_until(condition: impl Fn() -> bool)",
            },
        ],
        tests: BROKER_TESTS,
    },
    FunctionContract {
        name: "readLaunchedDaemonPid",
        native: &[NativeMarker {
            path: "crates/workdeck-session/src/broker_launcher.rs",
            marker: "pub fn read_session_broker_launch_fingerprint(",
        }],
        tests: &[
            "crates/workdeck-session/src/broker_launcher.rs#tests::runtime_paths_are_branded_scoped_and_files_are_private",
        ],
    },
    FunctionContract {
        name: "waitForHealth",
        native: &[
            NativeMarker {
                path: "crates/workdeck-session/src/broker_launcher.rs",
                marker: "pub fn read_session_broker_health(",
            },
            NativeMarker {
                path: "crates/workdeck-session/src/broker_daemon.rs",
                marker: "pub fn get_health(&self)",
            },
        ],
        tests: &[
            "crates/workdeck-session/src/agent_commands.rs#tests::health_parser_accepts_minimal_and_bounded_rich_shapes_only",
            "crates/workdeck-session/tests/native_broker_adapters.rs#serves_generic_daemon_api_and_websocket_path_through_bun_parity_surface",
        ],
    },
    FunctionContract {
        name: "waitForTranscript",
        native: &[
            NativeMarker {
                path: "crates/workdeck-cli/tests/terminal_pager.rs",
                marker: "fn wait_for(",
            },
            NativeMarker {
                path: "crates/workdeck-cli/tests/terminal_lifecycle.rs",
                marker: "fn wait_for_frame(",
            },
        ],
        tests: TUI_TESTS,
    },
];

const SOURCE_MARKERS: &[&str] = &[
    "Bun.spawnSync",
    "Bun.spawn",
    "Bun.file",
    "createServer",
    "mkdtempSync",
    "testConfigHome",
    "XDG_CONFIG_HOME",
    "HUNK_MCP_PORT",
    "SessionListJson",
    "FixtureFiles",
    "spawnHunkSession",
    "runSessionCli",
    "waitForHealth",
    "waitForTranscript",
    "comment",
    "highlight",
    "navigate",
    "--focus",
    "--new-line",
    "--start",
    "--end",
    "non-Hunk listener",
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

fn source_type_names(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| {
            let line = line
                .strip_prefix("export type ")
                .or_else(|| line.strip_prefix("type "))?;
            let end = [line.find('='), line.find('<'), line.find('{')]
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

fn source_test_names(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| {
            let start = line.find("test(\"")? + "test(\"".len();
            let end = line[start..].find('"')? + start;
            Some(line[start..end].to_owned())
        })
        .collect()
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
        "pinned broker E2E {commit} changed size: {} != {bytes}",
        source.len()
    );
    ensure!(
        source.split(|byte| *byte == b'\n').count() == lines + 1,
        "pinned broker E2E {commit} changed line count"
    );
    ensure!(
        format!("{:x}", Sha256::digest(&source)) == sha,
        "pinned broker E2E {commit} changed SHA-256"
    );
    String::from_utf8(source).context("pinned broker E2E source is not UTF-8")
}

fn verify_rust_anchor(
    repo: &Path,
    item: &str,
    sources: &mut HashMap<String, syn::File>,
) -> Result<()> {
    let (path, anchor) = item
        .split_once('#')
        .with_context(|| format!("broker E2E mapping lacks a Rust test anchor: {item}"))?;
    let source = if let Some(source) = sources.get(path) {
        source
    } else {
        let text = fs::read_to_string(repo.join(path))
            .with_context(|| format!("read broker E2E Rust evidence {path}"))?;
        let parsed = syn::parse_file(&text)
            .with_context(|| format!("parse broker E2E Rust evidence {path}"))?;
        sources.entry(path.to_owned()).or_insert(parsed)
    };
    ensure!(
        crate::rust_items_have_test(&source.items, Some(anchor)),
        "broker E2E mapping references a missing executable Rust test: {item}"
    );
    Ok(())
}

fn verify_oracle(repo: &Path, path: &str) -> Result<()> {
    let bytes =
        fs::read(repo.join(path)).with_context(|| format!("read broker E2E oracle {path}"))?;
    let value: Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse broker E2E oracle {path}"))?;
    let encoded = value.to_string();
    ensure!(
        encoded.contains(BASELINE) && encoded.contains(STABLE),
        "broker E2E oracle {path} does not identify both pinned trees"
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
                    .with_context(|| format!("read broker E2E native surface {}", marker.path))?;
                contents.entry(marker.path).or_insert(text)
            };
            ensure!(
                source.contains(marker.marker),
                "broker E2E native surface {} is missing {:?} for {}",
                marker.path,
                marker.marker,
                contract.name
            );
        }
    }
    Ok(())
}

/// Verify both pinned broker E2E blobs and their native executable projection.
pub(crate) fn verify(repo: &Path, baseline: &str) -> Result<()> {
    ensure!(
        baseline == BASELINE,
        "broker E2E verifier received unexpected baseline {baseline}"
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
    let stable_functions = source_function_names(&stable);
    ensure!(
        baseline_functions
            == [
                "supportsControllableScript",
                "cleanupTempDirs",
                "shellQuote",
                "stripTerminalControl",
                "createFixtureFiles",
                "reserveLoopbackPort",
                "spawnHunkSession",
                "quitHunkSession",
                "cleanupHunkSession",
                "runSessionCli",
                "waitUntil",
                "readLaunchedDaemonPid",
                "waitForHealth",
                "waitForTranscript",
            ],
        "pinned broker E2E baseline function surface changed: {baseline_functions:?}"
    );
    ensure!(
        stable_functions
            == [
                "supportsControllableScript",
                "cleanupTempDirs",
                "shellQuote",
                "stripTerminalControl",
                "createFixtureFiles",
                "reserveLoopbackPort",
                "spawnHunkSession",
                "quitHunkSession",
                "cleanupHunkSession",
                "runSessionCli",
                "waitUntil",
                "waitForHealth",
                "waitForTranscript",
            ],
        "pinned broker E2E stable function surface changed: {stable_functions:?}"
    );
    let expected_constants = [
        "repoRoot",
        "sourceEntrypoint",
        "testConfigHome",
        "tempDirs",
        "ttyToolsAvailable",
    ];
    ensure!(
        source_const_names(&source) == expected_constants,
        "pinned broker E2E baseline constants changed: {:?}",
        source_const_names(&source)
    );
    ensure!(
        source_const_names(&stable) == expected_constants,
        "pinned broker E2E stable constants changed: {:?}",
        source_const_names(&stable)
    );
    for pinned in [&source, &stable] {
        ensure!(
            source_type_names(pinned) == ["HunkSessionProcess"],
            "pinned broker E2E type surface changed"
        );
        ensure!(
            source_interface_names(pinned) == ["HealthResponse", "SessionListJson", "FixtureFiles"],
            "pinned broker E2E interface surface changed"
        );
        for marker in SOURCE_MARKERS {
            ensure!(
                pinned.contains(marker),
                "pinned broker E2E source is missing required contract marker {marker:?}"
            );
        }
    }
    ensure!(
        source.contains("readLaunchedDaemonPid"),
        "pinned broker E2E baseline lost its daemon metadata helper"
    );
    ensure!(
        !stable.contains("readLaunchedDaemonPid"),
        "pinned broker E2E stable unexpectedly retained the removed daemon metadata helper"
    );
    let baseline_tests = source_test_names(&source);
    let stable_tests = source_test_names(&stable);
    let expected_tests = [
        "a live Hunk session auto-starts the daemon and renders CLI comments inline",
        "session CLI can inspect current focus and navigate hunks in a live session",
        "session CLI marks a character range and reveals its exact line in a live session",
        "one daemon routes CLI comments to the correct Hunk session when multiple local sessions are open",
        "a normal Hunk session still renders and exits cleanly when a non-Hunk listener owns the MCP port",
    ];
    ensure!(
        baseline_tests == expected_tests && stable_tests == expected_tests,
        "pinned broker E2E test surface changed: baseline={baseline_tests:?}, stable={stable_tests:?}"
    );
    let mapping_names = TEST_MAPPINGS
        .iter()
        .map(|mapping| mapping.source)
        .collect::<Vec<_>>();
    let mut unique = BTreeSet::new();
    ensure!(
        mapping_names.iter().all(|name| unique.insert(*name)),
        "broker E2E test mapping repeats a source test"
    );
    ensure!(
        expected_tests
            .iter()
            .all(|name| mapping_names.contains(name)),
        "broker E2E test mapping omits a source test"
    );
    let mut rust_sources = HashMap::new();
    for mapping in TEST_MAPPINGS {
        for test in mapping.native {
            verify_rust_anchor(repo, test, &mut rust_sources)?;
        }
    }
    let contract_names = FUNCTION_CONTRACTS
        .iter()
        .map(|contract| contract.name)
        .collect::<Vec<_>>();
    let mut unique = BTreeSet::new();
    ensure!(
        contract_names.iter().all(|name| unique.insert(*name)),
        "broker E2E contract table repeats a source function"
    );
    let expected_contract_names = baseline_functions
        .iter()
        .map(String::as_str)
        .chain(["readLaunchedDaemonPid"])
        .collect::<Vec<_>>();
    ensure!(
        expected_contract_names
            .iter()
            .all(|name| contract_names.contains(name)),
        "broker E2E contract table omits a pinned function"
    );
    for contract in FUNCTION_CONTRACTS {
        ensure!(
            !contract.native.is_empty() && !contract.tests.is_empty(),
            "broker E2E contract {} has no native surface or executable evidence",
            contract.name
        );
        for test in contract.tests {
            verify_rust_anchor(repo, test, &mut rust_sources)?;
        }
    }
    verify_native_surface(repo)?;
    for oracle in [
        "port/hunk/oracles/hunk-session-bridge-hook.json",
        "port/hunk/oracles/app-host-reload.json",
        "port/hunk/oracles/pty-session-attention.json",
        "port/hunk/oracles/extension-vcs-patch-result.json",
    ] {
        verify_oracle(repo, oracle)?;
    }
    let migration = fs::read_to_string(repo.join("docs/broker-e2e-migration.md"))
        .context("read broker E2E migration documentation")?;
    for marker in [
        SOURCE_PATH,
        "23,792",
        "23,235",
        "non-overlapping",
        "authenticated loopback",
        "multi-session",
        "port conflict",
        "No TypeScript source mirror",
    ] {
        ensure!(
            migration.contains(marker),
            "broker E2E migration documentation is missing {marker:?}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_rust_broker_e2e_replaces_both_pinned_suites() {
        let repo = crate::repo_root().unwrap();
        verify(&repo, BASELINE).unwrap();
    }
}
