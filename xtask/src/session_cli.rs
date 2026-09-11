//! Exhaustive source accounting for Hunk's session CLI integration tests.
//!
//! The pinned TypeScript suite drives a live terminal, an authenticated loopback daemon, reload
//! confinement, navigation, and both focused and non-focused comment mutations.  Workdeck keeps
//! those semantics in the native session runner, broker, and Ratatui host.  This verifier reads
//! only the two pinned Git blobs; it never mirrors or executes the TypeScript source.

use anyhow::{Context, Result, ensure};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;

const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
const STABLE: &str = "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd";
const SOURCE_PATH: &str = "test/session/cli.test.ts";
const BASELINE_BYTES: usize = 30_060;
const BASELINE_LINES: usize = 979;
const BASELINE_SHA256: &str = "b6fb57de48bdb94c6736808e8ddb90a1351d589c1ec1df624aac7b997256514c";
const STABLE_BYTES: usize = 28_511;
const STABLE_LINES: usize = 934;
const STABLE_SHA256: &str = "d9349c1643ea6d1c15e176dc4434d64d83b2eac8eb135d401c6bdf486f8ea84e";

struct NativeMarker {
    path: &'static str,
    marker: &'static str,
}

struct FunctionContract {
    name: &'static str,
    native: &'static [NativeMarker],
    tests: &'static [&'static str],
}

const SESSION_COMMAND_TESTS: &[&str] = &[
    "crates/workdeck-session/src/agent_commands.rs#tests::list_json_includes_structured_terminal_metadata_without_legacy_fields",
    "crates/workdeck-session/src/agent_commands.rs#tests::reload_returns_the_replacement_session_summary",
    "crates/workdeck-session/src/agent_commands.rs#tests::reload_forwards_structured_endpoints_and_separate_source_path",
    "crates/workdeck-session/src/agent_commands.rs#tests::comment_apply_forwards_batch_and_formats_applied_result",
    "crates/workdeck-session/src/agent_commands.rs#tests::remaining_session_actions_dispatch_and_keep_text_output_stable",
];

const BROKER_TESTS: &[&str] = &[
    "crates/workdeck-session/src/broker_daemon.rs#tests::serves_health_and_raw_list_get_requests_when_http_api_is_enabled",
    "crates/workdeck-session/src/broker_daemon.rs#tests::dispatches_one_raw_command_through_broker_api",
    "crates/workdeck-session/src/broker_daemon.rs#tests::rejects_unsupported_dispatch_controls_before_delivery",
    "crates/workdeck-session/src/broker_connection.rs#tests::registers_on_open_and_sends_snapshot_updates",
    "crates/workdeck-session/src/broker_connection.rs#tests::serializes_bridge_execution_in_arrival_order",
];

const TERMINAL_TESTS: &[&str] = &[
    "crates/workdeck-cli/tests/terminal_lifecycle.rs#daemon_exits_cleanly_after_sigterm_instead_of_hot_looping",
    "crates/workdeck-cli/tests/terminal_lifecycle.rs#exits_cleanly_when_host_closes_pty_master",
    "crates/workdeck-cli/tests/terminal_pager.rs#session_attention_highlight_reveals_and_paints_exact_range_then_clears_and_navigates",
];

const RELOAD_TESTS: &[&str] = &[
    "crates/workdeck-session/src/reload_bounds.rs#tests::option_like_ranges_endpoints_and_refs_are_rejected_but_pathspecs_are_exempt",
    "crates/workdeck-session/src/reload_bounds.rs#tests::source_paths_outside_root_and_parent_traversal_are_rejected",
    "crates/workdeck-session/src/reload_bounds.rs#tests::direct_files_outside_repo_are_unreloadable_but_inside_repo_use_the_repo_root",
    "crates/workdeck-tui/src/app_host.rs#tests::queued_file_reload_preserves_live_comment_in_updated_terminal_frame",
    "crates/workdeck-tui/src/app_host.rs#tests::queued_reload_outside_launch_root_is_rejected_before_loader_or_publication",
];

const COMMENT_TESTS: &[&str] = &[
    "crates/workdeck-tui/src/session_review_controller.rs#tests::comment_batches_preflight_every_target_and_commit_atomically",
    "crates/workdeck-tui/src/source_controller.rs#tests::alpha_comment_batch_preserves_order_and_reveals_first_hunk",
    "crates/workdeck-session/src/agent_cli_format.rs#tests::command_result_formatters_describe_navigation_and_comment_side_effects",
];

const FORMAT_TESTS: &[&str] = &[
    "crates/workdeck-session/src/agent_cli_format.rs#tests::list_and_get_preserve_terminal_metadata_and_selected_hunk_summaries",
    "crates/workdeck-session/src/agent_cli_format.rs#tests::command_result_formatters_describe_navigation_and_comment_side_effects",
    "crates/workdeck-session/src/agent_cli_format.rs#tests::note_and_highlight_formatters_preserve_scope_reveals_and_coordinates",
    "crates/workdeck-session/src/agent_cli_format.rs#tests::human_readable_session_paths_cannot_emit_terminal_controls",
];

const FUNCTION_CONTRACTS: &[FunctionContract] = &[
    FunctionContract {
        name: "supportsControllableScript",
        native: &[NativeMarker {
            path: "crates/workdeck-cli/tests/terminal_pager.rs",
            marker: "struct Session {",
        }],
        tests: TERMINAL_TESTS,
    },
    FunctionContract {
        name: "reserveLoopbackPort",
        native: &[NativeMarker {
            path: "crates/workdeck-session/src/agent_commands.rs",
            marker: "fn free_port() -> u16",
        }],
        tests: &[
            "crates/workdeck-session/src/agent_commands.rs#tests::real_availability_probe_returns_empty_list_when_no_daemon_listens",
        ],
    },
    FunctionContract {
        name: "cleanupTempDirs",
        native: &[NativeMarker {
            path: "crates/workdeck-cli/tests/terminal_pager.rs",
            marker: "impl Drop for Session",
        }],
        tests: TERMINAL_TESTS,
    },
    FunctionContract {
        name: "shellQuote",
        native: &[NativeMarker {
            path: "crates/workdeck-session/src/agent_cli_format.rs",
            marker: "fn format_session_path",
        }],
        tests: FORMAT_TESTS,
    },
    FunctionContract {
        name: "waitUntil",
        native: &[NativeMarker {
            path: "crates/workdeck-session/src/broker_connection.rs",
            marker: "fn wait_until(condition: impl Fn() -> bool)",
        }],
        tests: BROKER_TESTS,
    },
    FunctionContract {
        name: "createFixtureFiles",
        native: &[NativeMarker {
            path: "crates/workdeck-session/src/agent_commands.rs",
            marker: "SessionCommentApplyItemInput",
        }],
        tests: COMMENT_TESTS,
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
        tests: TERMINAL_TESTS,
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
        name: "requestHunkSessionQuit",
        native: &[NativeMarker {
            path: "crates/workdeck-cli/tests/terminal_pager.rs",
            marker: "fn quit(&mut self)",
        }],
        tests: TERMINAL_TESTS,
    },
    FunctionContract {
        name: "quitHunkSession",
        native: &[NativeMarker {
            path: "crates/workdeck-cli/tests/terminal_lifecycle.rs",
            marker: "impl Drop for ReviewChild",
        }],
        tests: TERMINAL_TESTS,
    },
    FunctionContract {
        name: "waitForRegisteredSessions",
        native: &[
            NativeMarker {
                path: "crates/workdeck-session/src/broker_daemon.rs",
                marker: "pub fn list_sessions(&self)",
            },
            NativeMarker {
                path: "crates/workdeck-session/src/broker_facade.rs",
                marker: "fn list_sessions(&self)",
            },
        ],
        tests: BROKER_TESTS,
    },
    FunctionContract {
        name: "readDaemonHealth",
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
            "crates/workdeck-session/src/broker_daemon.rs#tests::serves_health_and_raw_list_get_requests_when_http_api_is_enabled",
        ],
    },
    FunctionContract {
        name: "isProcessRunning",
        native: &[NativeMarker {
            path: "crates/workdeck-session/src/broker_launcher.rs",
            marker: "fn is_running_pid(pid: u64)",
        }],
        tests: &[
            "crates/workdeck-session/src/broker_launcher.rs#tests::detects_whether_some_process_is_listening_on_daemon_port",
        ],
    },
    FunctionContract {
        name: "signalProcess",
        native: &[NativeMarker {
            path: "crates/workdeck-cli/tests/terminal_lifecycle.rs",
            marker: "libc::SIGTERM",
        }],
        tests: &[
            "crates/workdeck-cli/tests/terminal_lifecycle.rs#daemon_exits_cleanly_after_sigterm_instead_of_hot_looping",
        ],
    },
    FunctionContract {
        name: "waitForDaemonExit",
        native: &[NativeMarker {
            path: "crates/workdeck-cli/tests/terminal_lifecycle.rs",
            marker: "wait_status_with_timeout",
        }],
        tests: TERMINAL_TESTS,
    },
    FunctionContract {
        name: "stopTestDaemon",
        native: &[
            NativeMarker {
                path: "crates/workdeck-session/src/broker_launcher.rs",
                marker: "pub fn ensure_session_broker_available(",
            },
            NativeMarker {
                path: "crates/workdeck-cli/tests/terminal_lifecycle.rs",
                marker: "daemon port remained open after SIGTERM",
            },
        ],
        tests: &[
            "crates/workdeck-cli/tests/terminal_lifecycle.rs#daemon_exits_cleanly_after_sigterm_instead_of_hot_looping",
            "crates/workdeck-session/src/broker_launcher.rs#tests::coordinates_concurrent_ensure_calls_so_only_one_launcher_runs",
        ],
    },
    FunctionContract {
        name: "cleanupHunkSession",
        native: &[
            NativeMarker {
                path: "crates/workdeck-cli/tests/terminal_pager.rs",
                marker: "fn launch_in_config(",
            },
            NativeMarker {
                path: "crates/workdeck-tui/src/app_host.rs",
                marker: "pub struct AppHostController",
            },
        ],
        tests: RELOAD_TESTS,
    },
    FunctionContract {
        name: "runSessionCli",
        native: &[
            NativeMarker {
                path: "crates/workdeck-session/src/agent_commands.rs",
                marker: "pub fn run_session_command(",
            },
            NativeMarker {
                path: "crates/workdeck-cli/tests/cli.rs",
                marker: "fn session_overviews_and_empty_list_are_headless_and_read_only()",
            },
        ],
        tests: SESSION_COMMAND_TESTS,
    },
];

const SOURCE_MARKERS: &[&str] = &[
    "Bun.spawnSync",
    "Bun.spawn",
    "Bun.sleep",
    "Bun.file",
    "createServer",
    "mkdtempSync",
    "testConfigHome",
    "testRuntimeDir",
    "XDG_CONFIG_HOME",
    "XDG_RUNTIME_DIR",
    "HUNK_MCP_PORT",
    "session-api",
    "outside the initial Hunk root",
    "option-like",
    "stripTerminalControl",
    "ownedDaemonPids",
    "waitForRegisteredSessions",
    "spawnHunkSession",
    "runSessionCli",
    "--json",
    "--focus",
    "--stdin",
    "comment",
    "navigate",
    "reload",
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
        "pinned session CLI {commit} changed size: {} != {bytes}",
        source.len()
    );
    ensure!(
        source.split(|byte| *byte == b'\n').count() == lines + 1,
        "pinned session CLI {commit} changed line count"
    );
    ensure!(
        format!("{:x}", Sha256::digest(&source)) == sha,
        "pinned session CLI {commit} changed SHA-256"
    );
    String::from_utf8(source).context("pinned session CLI is not UTF-8")
}

fn verify_rust_anchor(
    repo: &Path,
    item: &str,
    sources: &mut HashMap<String, syn::File>,
) -> Result<()> {
    let (path, anchor) = item
        .split_once('#')
        .with_context(|| format!("session CLI mapping lacks a Rust test anchor: {item}"))?;
    let source = if let Some(source) = sources.get(path) {
        source
    } else {
        let text = fs::read_to_string(repo.join(path))
            .with_context(|| format!("read session CLI Rust evidence {path}"))?;
        let parsed = syn::parse_file(&text)
            .with_context(|| format!("parse session CLI Rust evidence {path}"))?;
        sources.entry(path.to_owned()).or_insert(parsed)
    };
    ensure!(
        crate::rust_items_have_test(&source.items, Some(anchor)),
        "session CLI mapping references a missing executable Rust test: {item}"
    );
    Ok(())
}

fn verify_oracle(repo: &Path, path: &str) -> Result<()> {
    let bytes =
        fs::read(repo.join(path)).with_context(|| format!("read session CLI oracle {path}"))?;
    let value: Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse session CLI oracle {path}"))?;
    let encoded = value.to_string();
    ensure!(
        encoded.contains(BASELINE) && encoded.contains(STABLE),
        "session CLI oracle {path} does not identify both pinned trees"
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
                    .with_context(|| format!("read session CLI native surface {}", marker.path))?;
                contents.entry(marker.path).or_insert(text)
            };
            ensure!(
                source.contains(marker.marker),
                "session CLI native surface {} is missing {:?} for {}",
                marker.path,
                marker.marker,
                contract.name
            );
        }
    }
    Ok(())
}

fn verify_test_mapping(source_tests: &[String], names: &[&str]) -> Result<()> {
    let expected = names
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    for name in &expected {
        ensure!(
            source_tests.contains(name),
            "session CLI contract table omits pinned source test {name:?}"
        );
    }
    Ok(())
}

/// Verify both pinned session CLI integration-test blobs and their executable native projection.
pub(crate) fn verify(repo: &Path, baseline: &str) -> Result<()> {
    ensure!(
        baseline == BASELINE,
        "session CLI verifier received unexpected baseline {baseline}"
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
        "supportsControllableScript",
        "reserveLoopbackPort",
        "cleanupTempDirs",
        "shellQuote",
        "waitUntil",
        "createFixtureFiles",
        "spawnHunkSession",
        "stripTerminalControl",
        "requestHunkSessionQuit",
        "quitHunkSession",
        "waitForRegisteredSessions",
        "readDaemonHealth",
        "isProcessRunning",
        "signalProcess",
        "waitForDaemonExit",
        "stopTestDaemon",
        "cleanupHunkSession",
        "runSessionCli",
    ];
    ensure!(
        source_function_names(&source) == expected_functions,
        "pinned session CLI baseline function surface changed: {:?}",
        source_function_names(&source)
    );
    ensure!(
        source_function_names(&stable) == expected_functions,
        "pinned stable session CLI function surface changed: {:?}",
        source_function_names(&stable)
    );
    let expected_constants = [
        "repoRoot",
        "sourceEntrypoint",
        "testConfigHome",
        "testRuntimeDir",
        "tempDirs",
        "ttyToolsAvailable",
        "ownedDaemonPids",
        "sessionDescribe",
    ];
    ensure!(
        source_const_names(&source) == expected_constants,
        "pinned session CLI baseline constants changed: {:?}",
        source_const_names(&source)
    );
    ensure!(
        source_const_names(&stable) == expected_constants,
        "pinned stable session CLI constants changed: {:?}",
        source_const_names(&stable)
    );
    for pinned in [&source, &stable] {
        ensure!(
            source_type_names(pinned) == ["HunkSessionProcess"],
            "pinned session CLI type surface changed"
        );
        ensure!(
            source_interface_names(pinned) == ["SessionListJson"],
            "pinned session CLI interface surface changed"
        );
        for marker in SOURCE_MARKERS {
            ensure!(
                pinned.contains(marker),
                "pinned session CLI source is missing required contract marker {marker:?}"
            );
        }
    }

    let baseline_tests = source_test_names(&source);
    let stable_tests = source_test_names(&stable);
    let expected_baseline_tests = [
        "list/get/context expose live Hunk sessions through the daemon",
        "reload replaces what a live session is showing",
        "reload refuses to read files outside the live session root",
        "raw session API callers cannot present option-like VCS ranges",
        "navigate works, and comment add only focuses the session when --focus is passed",
        "comment apply adds a batch from stdin without moving focus by default",
        "comment apply with --focus jumps to the first applied comment",
    ];
    let expected_stable_tests = [
        "list/get/context expose live Hunk sessions through the daemon",
        "reload replaces what a live session is showing",
        "reload refuses to read files outside the live session root",
        "reload refuses option-like VCS ranges sent directly to the session API",
        "navigate works, and comment add only focuses the session when --focus is passed",
        "comment apply adds a batch from stdin without moving focus by default",
        "comment apply with --focus jumps to the first applied comment",
    ];
    verify_test_mapping(&baseline_tests, &expected_baseline_tests)?;
    verify_test_mapping(&stable_tests, &expected_stable_tests)?;
    ensure!(
        baseline_tests == expected_baseline_tests,
        "pinned session CLI baseline tests changed: {baseline_tests:?}"
    );
    ensure!(
        stable_tests == expected_stable_tests,
        "pinned stable session CLI tests changed: {stable_tests:?}"
    );

    let contract_names = FUNCTION_CONTRACTS
        .iter()
        .map(|contract| contract.name)
        .collect::<Vec<_>>();
    let mut unique = BTreeSet::new();
    ensure!(
        contract_names.iter().all(|name| unique.insert(*name)),
        "session CLI contract table repeats a source function"
    );
    ensure!(
        expected_functions
            .iter()
            .all(|name| contract_names.contains(name)),
        "session CLI contract table omits a pinned function"
    );
    let mut rust_sources = HashMap::new();
    for contract in FUNCTION_CONTRACTS {
        ensure!(
            !contract.native.is_empty() && !contract.tests.is_empty(),
            "session CLI contract {} has no native surface or executable evidence",
            contract.name
        );
        for test in contract.tests {
            verify_rust_anchor(repo, test, &mut rust_sources)?;
        }
    }
    verify_native_surface(repo)?;
    for oracle in [
        "port/hunk/oracles/session-bootstrap.json",
        "port/hunk/oracles/hunk-session-bridge-hook.json",
        "port/hunk/oracles/app-host-reload.json",
        "port/hunk/oracles/app-host-reload-root.json",
        "port/hunk/oracles/terminal-review-gap-reload-execution.json",
    ] {
        verify_oracle(repo, oracle)?;
    }

    let migration = fs::read_to_string(repo.join("docs/session-cli-migration.md"))
        .context("read session CLI migration documentation")?;
    for marker in [
        SOURCE_PATH,
        "30,060",
        "28,511",
        "non-overlapping",
        "authenticated loopback",
        "reload confinement",
        "--focus",
        "--stdin",
        "No TypeScript source mirror",
    ] {
        ensure!(
            migration.contains(marker),
            "session CLI migration documentation is missing {marker:?}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_rust_session_cli_replaces_both_pinned_integration_suites() {
        let repo = crate::repo_root().unwrap();
        verify(&repo, BASELINE).unwrap();
    }
}
