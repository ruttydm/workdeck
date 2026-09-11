//! Exhaustive source accounting for Hunk's public install script and its contract tests.
//!
//! The installer is the boundary that runs before Workdeck exists.  Its behavior is therefore
//! represented by the native Rust release downloader, authenticated archive staging, conflict
//! diagnostics, platform selection, and PATH transaction.  This verifier reads both pinned
//! blobs through `git show`; it never keeps or executes a TypeScript source mirror.

use anyhow::{Context, Result, ensure};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;

const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
const STABLE: &str = "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd";
const INSTALL_PATH: &str = "install.sh";
const TEST_PATH: &str = "scripts/install-sh.test.ts";
const BIN_PATH: &str = "scripts/install-bin.ts";
const INSTALL_BASELINE_BYTES: usize = 20_132;
const INSTALL_BASELINE_LINES: usize = 554;
const INSTALL_BASELINE_SHA256: &str =
    "566ac5bce9f7c04ffc356aadc3e7aca14ddd5c2929d95bfe3b3bb98d6a08b4dc";
const INSTALL_STABLE_BYTES: usize = 12_810;
const INSTALL_STABLE_LINES: usize = 372;
const INSTALL_STABLE_SHA256: &str =
    "f15239daaeea4179caee3d96b564cd200ce332fa5377486ca00a90e60e8283ec";
const TEST_BASELINE_BYTES: usize = 13_638;
const TEST_BASELINE_LINES: usize = 361;
const TEST_BASELINE_SHA256: &str =
    "fd0ff678cf220285c22a9ca7bd5908e8422fe927b03a960b70c002928785513e";
const TEST_STABLE_BYTES: usize = 6_536;
const TEST_STABLE_LINES: usize = 157;
const TEST_STABLE_SHA256: &str = "f89381df3304faff020ef1b489d32d7e52fe437dafad9bcbe7a9b87aed08bbd3";
const BIN_BYTES: usize = 2_011;
const BIN_LINES: usize = 60;
const BIN_SHA256: &str = "0ed22e1a045724112cf02c8c10ff88e0ab71030fe57b7196f5961b3ff25c0c89";
const STAGE_PATH: &str = "scripts/stage-install-script.ts";
const STAGE_BYTES: usize = 1_101;
const STAGE_LINES: usize = 28;
const STAGE_SHA256: &str = "059742cafd9d6c8bd6e17f64ef673e6849c148056176cd411954a5a134c1d081";

struct NativeMarker {
    path: &'static str,
    marker: &'static str,
}

#[derive(Copy, Clone)]
struct SourceTestMapping {
    name: &'static str,
    native: &'static [&'static str],
}

const NATIVE_INSTALL_SCRIPT_TEST: &str =
    "xtask/src/install_script.rs#tests::native_rust_install_script_replaces_both_pinned_suites";

const BASELINE_TESTS: &[SourceTestMapping] = &[
    SourceTestMapping {
        name: "parses as a POSIX shell program",
        native: &[NATIVE_INSTALL_SCRIPT_TEST],
    },
    SourceTestMapping {
        name: "names the release assets the publish workflow uploads",
        native: &[
            "crates/workdeck-cli/src/install/download.rs#tests::downloads_use_packaged_names_and_cleanup_on_success_or_failure",
        ],
    },
    SourceTestMapping {
        name: "installs beside the bundled skills so skill resolution still finds them",
        native: &[
            "crates/workdeck-cli/tests/cli.rs#relocated_binary_resolves_packaged_skills_without_user_state",
        ],
    },
    SourceTestMapping {
        name: "defers every statement to a main call on the last line",
        native: &[NATIVE_INSTALL_SCRIPT_TEST],
    },
    SourceTestMapping {
        name: "points unsupported platforms at the npm package",
        native: &[
            "crates/workdeck-cli/src/install.rs#tests::platform_detection_matches_both_pinned_shell_oracles",
        ],
    },
    SourceTestMapping {
        name: "refuses every visible and inactive-nvm competing install with exact remediation",
        native: &[
            "crates/workdeck-cli/src/install.rs#tests::inactive_nvm_installations_are_discovered_before_mise_without_execution",
            "crates/workdeck-cli/src/install.rs#tests::install_conflict_gate_requires_explicit_force_without_mutating_candidates",
        ],
    },
    SourceTestMapping {
        name: "names when the managed target shadows a competing install",
        native: &[
            "crates/workdeck-cli/src/install.rs#tests::conflict_details_report_inferred_owner_and_all_path_order_states",
        ],
    },
    SourceTestMapping {
        name: "uses canonical PATH identities for shadowing through a managed-directory alias",
        native: &[
            "crates/workdeck-cli/src/install.rs#tests::manager_shaped_alias_is_preferred_without_changing_first_path_or_identity",
            "crates/workdeck-cli/src/install.rs#tests::identity_handles_missing_binary_and_path_order_without_writes",
        ],
    },
    SourceTestMapping {
        name: "reports one conflict when PATH contains directory aliases to the same foreign install",
        native: &[
            "crates/workdeck-cli/src/install.rs#tests::path_observation_deduplicates_identity_but_retains_aliases_and_order",
        ],
    },
    SourceTestMapping {
        name: "uses a canonical manager path when an unrecognized alias is discovered first",
        native: &[
            "crates/workdeck-cli/src/install.rs#tests::manager_shaped_alias_is_preferred_without_changing_first_path_or_identity",
        ],
    },
    SourceTestMapping {
        name: "does not treat a PATH symlink to the managed binary as another install",
        native: &[
            "crates/workdeck-cli/src/install.rs#tests::identity_handles_missing_binary_and_path_order_without_writes",
        ],
    },
    SourceTestMapping {
        name: "allows the scripted force environment variable",
        native: &[
            "crates/workdeck-cli/src/install.rs#tests::install_conflict_gate_requires_explicit_force_without_mutating_candidates",
        ],
    },
    SourceTestMapping {
        name: "allows an explicit force flag and preserves the already-current fast path",
        native: &[
            "crates/workdeck-cli/src/install.rs#tests::install_conflict_gate_requires_explicit_force_without_mutating_candidates",
        ],
    },
    SourceTestMapping {
        name: "resolves every published macOS and Linux platform pair",
        native: &[
            "crates/workdeck-cli/src/install.rs#tests::platform_detection_matches_both_pinned_shell_oracles",
        ],
    },
    SourceTestMapping {
        name: "corrects a Rosetta-translated shell to the native arm64 build",
        native: &[
            "crates/workdeck-cli/src/install.rs#tests::platform_detection_matches_both_pinned_shell_oracles",
        ],
    },
    SourceTestMapping {
        name: "rejects Windows-style uname output",
        native: &[
            "crates/workdeck-cli/src/install.rs#tests::platform_detection_matches_both_pinned_shell_oracles",
        ],
    },
    SourceTestMapping {
        name: "rejects unsupported architectures",
        native: &[
            "crates/workdeck-cli/src/install.rs#tests::platform_detection_matches_both_pinned_shell_oracles",
        ],
    },
];

const STABLE_TESTS: &[SourceTestMapping] = &[
    BASELINE_TESTS[0],
    BASELINE_TESTS[1],
    BASELINE_TESTS[2],
    BASELINE_TESTS[3],
    BASELINE_TESTS[4],
    BASELINE_TESTS[13],
    BASELINE_TESTS[14],
    BASELINE_TESTS[15],
    BASELINE_TESTS[16],
];

const NATIVE_SURFACE: &[NativeMarker] = &[
    NativeMarker {
        path: "crates/workdeck-cli/src/install.rs",
        marker: "pub fn run(",
    },
    NativeMarker {
        path: "crates/workdeck-cli/src/install.rs",
        marker: "fn platform(",
    },
    NativeMarker {
        path: "crates/workdeck-cli/src/install.rs",
        marker: "fn current_platform(",
    },
    NativeMarker {
        path: "crates/workdeck-cli/src/install.rs",
        marker: "fn canonical_executable_path(",
    },
    NativeMarker {
        path: "crates/workdeck-cli/src/install.rs",
        marker: "fn shadowing(",
    },
    NativeMarker {
        path: "crates/workdeck-cli/src/install.rs",
        marker: "fn check_install_conflicts(",
    },
    NativeMarker {
        path: "crates/workdeck-cli/src/install.rs",
        marker: "fn conflict_remediation(",
    },
    NativeMarker {
        path: "crates/workdeck-cli/src/install.rs",
        marker: "fn manager_hint(",
    },
    NativeMarker {
        path: "crates/workdeck-cli/src/install.rs",
        marker: "fn options(",
    },
    NativeMarker {
        path: "crates/workdeck-cli/src/install.rs",
        marker: "fn archive_entry_path(",
    },
    NativeMarker {
        path: "crates/workdeck-cli/src/install.rs",
        marker: "fn verify_package_paths(",
    },
    NativeMarker {
        path: "crates/workdeck-cli/src/install/download.rs",
        marker: "pub fn download_release(",
    },
    NativeMarker {
        path: "crates/workdeck-cli/src/install/fresh.rs",
        marker: "pub fn install_requested_on_host(",
    },
    NativeMarker {
        path: "crates/workdeck-cli/src/install/fresh.rs",
        marker: "pub fn install_release_on_host(",
    },
    NativeMarker {
        path: "crates/workdeck-cli/src/install/shell_path.rs",
        marker: "pub fn plan(",
    },
    NativeMarker {
        path: "crates/workdeck-cli/src/install/shell_path.rs",
        marker: "pub fn apply(",
    },
    NativeMarker {
        path: "crates/workdeck-cli/src/install/staging.rs",
        marker: "pub fn prepare_authenticated_archive(",
    },
    NativeMarker {
        path: "crates/workdeck-cli/src/install/transaction.rs",
        marker: "pub fn replace_binary_with_backup(",
    },
    NativeMarker {
        path: "crates/workdeck-cli/src/install/metadata.rs",
        marker: "pub fn install(",
    },
    NativeMarker {
        path: "crates/workdeck-cli/tests/cli.rs",
        marker: "fn first_install_command_is_headless_and_rejects_invalid_versions_without_state()",
    },
];

const INSTALL_MARKERS: &[&str] = &[
    "set -eu",
    "REPO=\"modem-dev/hunk\"",
    "RELEASES_API",
    "DOWNLOAD_BASE",
    "detect_os()",
    "detect_arch()",
    "download()",
    "fetch()",
    "installed_version()",
    "SHA256SUMS",
    "sha256sum",
    "shasum -a 256",
    "tar -xzf",
    "--strip-components=1",
    "skills.new",
    "skills.old",
    "trap cleanup EXIT",
    "HUNK_VERSION",
    "HUNK_INSTALL_DIR",
    "HUNK_NO_MODIFY_PATH",
    "GITHUB_PATH",
    "main() {",
    "main \"$@\"",
    "npm install -g hunkdiff",
];

const BASELINE_INSTALL_MARKERS: &[&str] = &[
    "HUNK_ALLOW_CONFLICTING_INSTALLS",
    "canonical_executable_path()",
    "add_hunk_candidate()",
    "preferred_manager_path()",
    "shadowing_direction()",
    "competing_install_channel()",
    "competing_install_remediation()",
    "check_competing_installs()",
    "--force",
    "rerun this installer with --force",
];

const TEST_MARKERS: &[&str] = &[
    "from \"bun:test\"",
    "INSTALL_SCRIPT_PATH",
    "PLATFORM_PACKAGE_MATRIX",
    "Bun.spawnSync",
    "detectPlatform",
    "CURL_INSTALLABLE_SPECS",
    "\"-n\"",
    "SHA256SUMS",
    "bundled skills",
    "main call on the last line",
];

fn source_blob(
    repo: &Path,
    commit: &str,
    path: &str,
    bytes: usize,
    lines: usize,
    sha: &str,
) -> Result<String> {
    let source = crate::git_stdout_bytes(repo, ["show", &format!("{commit}:{path}")])?;
    ensure!(
        source.len() == bytes,
        "pinned {path} {commit} changed size: {} != {bytes}",
        source.len()
    );
    ensure!(
        source.split(|byte| *byte == b'\n').count() == lines + 1,
        "pinned {path} {commit} changed line count"
    );
    ensure!(
        format!("{:x}", Sha256::digest(&source)) == sha,
        "pinned {path} {commit} changed SHA-256"
    );
    String::from_utf8(source).with_context(|| format!("pinned {path} source is not UTF-8"))
}

fn source_function_names(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let end = line.find("() {")?;
            let name = line[..end].trim();
            (!name.is_empty()
                && name
                    .chars()
                    .all(|char| char.is_ascii_alphanumeric() || char == '_'))
            .then(|| name.to_owned())
        })
        .collect()
}

fn source_test_names(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut pending = false;
    for line in source.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("test.skipIf") {
            if let Some(start) = trimmed.find(")(\"") {
                let body = &trimmed[start + 3..];
                if let Some(end) = body.find('"') {
                    names.push(body[..end].to_owned());
                }
                pending = false;
                continue;
            }
            pending = true;
            // The predicate itself normally contains a quoted `"win32"`; the test name is on
            // the following line and must not be mistaken for that platform guard.
            continue;
        }
        if trimmed.starts_with("test(\"") {
            pending = true;
        }
        if pending && let Some(start) = line.find('"') {
            let body = &line[start + 1..];
            if let Some(end) = body.find('"') {
                names.push(body[..end].to_owned());
                pending = false;
            }
        }
    }
    names
}

fn verify_native_anchor(
    repo: &Path,
    item: &str,
    sources: &mut HashMap<String, syn::File>,
) -> Result<()> {
    let (path, anchor) = item
        .split_once('#')
        .with_context(|| format!("install script evidence lacks a Rust anchor: {item}"))?;
    let source = if let Some(source) = sources.get(path) {
        source
    } else {
        let text = fs::read_to_string(repo.join(path))
            .with_context(|| format!("read install script evidence {path}"))?;
        let parsed = syn::parse_file(&text)
            .with_context(|| format!("parse install script evidence {path}"))?;
        sources.entry(path.to_owned()).or_insert(parsed)
    };
    ensure!(
        crate::rust_items_have_test(&source.items, Some(anchor)),
        "install script evidence references missing executable Rust test: {item}"
    );
    Ok(())
}

fn verify_native_surface(repo: &Path) -> Result<()> {
    let mut contents = BTreeMap::new();
    for marker in NATIVE_SURFACE {
        let source = if let Some(source) = contents.get(marker.path) {
            source
        } else {
            let text = fs::read_to_string(repo.join(marker.path))
                .with_context(|| format!("read native installer surface {}", marker.path))?;
            contents.entry(marker.path).or_insert(text)
        };
        ensure!(
            source.contains(marker.marker),
            "native installer surface {} is missing {:?}",
            marker.path,
            marker.marker
        );
    }
    Ok(())
}

fn verify_oracle(repo: &Path) -> Result<()> {
    let path = repo.join("port/hunk/install-platform-oracle.json");
    let bytes = fs::read(&path).context("read install platform oracle")?;
    let value: Value = serde_json::from_slice(&bytes).context("parse install platform oracle")?;
    let array = value
        .as_array()
        .context("install platform oracle is not an array")?;
    ensure!(
        array.len() == 20,
        "install platform oracle has {} cases, expected 20",
        array.len()
    );
    let encoded = value.to_string();
    ensure!(
        encoded.contains("hunk-port/main-2c00f435") && encoded.contains("hunk-port/stable-v0.20.1"),
        "install platform oracle does not identify both pinned trees"
    );
    for case in array {
        for field in [
            "pin",
            "os",
            "arch",
            "translated",
            "exit_code",
            "stdout",
            "stderr",
            "output",
        ] {
            ensure!(
                case.get(field).is_some(),
                "install platform oracle case lacks {field}"
            );
        }
    }
    Ok(())
}

fn verify_bin_helper(repo: &Path) -> Result<()> {
    let source = source_blob(repo, BASELINE, BIN_PATH, BIN_BYTES, BIN_LINES, BIN_SHA256)?;
    let stable = source_blob(repo, STABLE, BIN_PATH, BIN_BYTES, BIN_LINES, BIN_SHA256)?;
    ensure!(
        source == stable,
        "pinned install-bin helper diverged between pins"
    );
    for marker in [
        "#!/usr/bin/env bun",
        "Bun.spawnSync",
        "scripts/build-bin.ts",
        "binaryName",
        "legacyBinaryName",
        "copyFileSync",
        "chmodSync",
        "rmSync",
        "LOCALAPPDATA",
        "Warning: ${installDir} is not on PATH",
    ] {
        ensure!(
            source.contains(marker),
            "pinned install-bin helper is missing marker {marker:?}"
        );
    }
    for (path, marker) in [
        (
            "crates/workdeck-cli/src/install/fresh.rs",
            "pub fn install_requested_on_host(",
        ),
        (
            "crates/workdeck-cli/src/install/transaction.rs",
            "pub fn create_binary(",
        ),
        (
            "crates/workdeck-cli/src/install/shell_path.rs",
            "pub fn plan(",
        ),
        (
            "crates/workdeck-cli/tests/cli.rs",
            "fn first_install_command_is_headless_and_rejects_invalid_versions_without_state()",
        ),
    ] {
        let native = fs::read_to_string(repo.join(path))
            .with_context(|| format!("read install-bin native surface {path}"))?;
        ensure!(
            native.contains(marker),
            "install-bin native surface {path} is missing {marker:?}"
        );
    }
    Ok(())
}

fn verify_stage_helper(repo: &Path) -> Result<()> {
    let source = source_blob(
        repo,
        BASELINE,
        STAGE_PATH,
        STAGE_BYTES,
        STAGE_LINES,
        STAGE_SHA256,
    )?;
    let stable = source_blob(
        repo,
        STABLE,
        STAGE_PATH,
        STAGE_BYTES,
        STAGE_LINES,
        STAGE_SHA256,
    )?;
    ensure!(
        source == stable,
        "pinned installer staging helper diverged between pins"
    );
    for marker in [
        "copyFileSync",
        "existsSync",
        "const REPO_ROOT",
        "const SOURCE",
        "const DIST_DIR",
        "join(REPO_ROOT, \"website\", \"dist\")",
        "install.sh",
        "process.exit(1)",
        "Runs after `astro build`",
    ] {
        ensure!(
            source.contains(marker),
            "pinned installer staging helper is missing marker {marker:?}"
        );
    }
    for (path, marker) in [
        (
            "xtask/src/site_markdown.rs",
            "pub(crate) fn stage_install_script(",
        ),
        ("xtask/src/main.rs", "site_markdown::stage_install_script("),
        (
            "xtask/src/site_preview.rs",
            "stage_install_script(root, &output)",
        ),
        (
            "xtask/src/site_markdown.rs",
            "stages_installer_idempotently_and_rejects_conflicting_or_symlinked_outputs",
        ),
    ] {
        let native = fs::read_to_string(repo.join(path))
            .with_context(|| format!("read installer staging native surface {path}"))?;
        ensure!(
            native.contains(marker),
            "installer staging native surface {path} is missing {marker:?}"
        );
    }
    Ok(())
}

fn verify_test_mapping(repo: &Path, source: &str, expected: &[SourceTestMapping]) -> Result<()> {
    let actual = source_test_names(source);
    let expected_names: Vec<_> = expected.iter().map(|mapping| mapping.name).collect();
    ensure!(
        actual == expected_names,
        "pinned install test surface changed: {actual:?}"
    );
    let mut unique = BTreeSet::new();
    ensure!(
        expected_names.iter().all(|name| unique.insert(*name)),
        "install test mapping repeats a source test"
    );
    let mut rust_sources = HashMap::new();
    for mapping in expected {
        ensure!(
            !mapping.native.is_empty(),
            "install test {} has no native evidence",
            mapping.name
        );
        for item in mapping.native {
            verify_native_anchor(repo, item, &mut rust_sources)?;
        }
    }
    Ok(())
}

/// Verify the complete pinned installer and test-suite projection.
pub(crate) fn verify(repo: &Path, baseline: &str) -> Result<()> {
    ensure!(
        baseline == BASELINE,
        "install script verifier received unexpected baseline {baseline}"
    );
    let install = source_blob(
        repo,
        BASELINE,
        INSTALL_PATH,
        INSTALL_BASELINE_BYTES,
        INSTALL_BASELINE_LINES,
        INSTALL_BASELINE_SHA256,
    )?;
    let stable_install = source_blob(
        repo,
        STABLE,
        INSTALL_PATH,
        INSTALL_STABLE_BYTES,
        INSTALL_STABLE_LINES,
        INSTALL_STABLE_SHA256,
    )?;
    let tests = source_blob(
        repo,
        BASELINE,
        TEST_PATH,
        TEST_BASELINE_BYTES,
        TEST_BASELINE_LINES,
        TEST_BASELINE_SHA256,
    )?;
    let stable_tests = source_blob(
        repo,
        STABLE,
        TEST_PATH,
        TEST_STABLE_BYTES,
        TEST_STABLE_LINES,
        TEST_STABLE_SHA256,
    )?;

    let expected_baseline_functions = [
        "info",
        "warn",
        "fail",
        "usage",
        "detect_os",
        "detect_arch",
        "download",
        "fetch",
        "installed_version",
        "canonical_executable_path",
        "add_hunk_candidate",
        "preferred_manager_path",
        "shadowing_direction",
        "competing_install_channel",
        "competing_install_remediation",
        "check_competing_installs",
        "add_path_line",
        "squote",
        "first_existing",
        "main",
        "cleanup",
    ];
    let expected_stable_functions = [
        "info",
        "warn",
        "fail",
        "usage",
        "detect_os",
        "detect_arch",
        "download",
        "fetch",
        "installed_version",
        "add_path_line",
        "squote",
        "first_existing",
        "main",
        "cleanup",
    ];
    ensure!(
        source_function_names(&install) == expected_baseline_functions,
        "pinned installer baseline function surface changed"
    );
    ensure!(
        source_function_names(&stable_install) == expected_stable_functions,
        "pinned installer stable function surface changed"
    );
    for source in [&install, &stable_install] {
        for marker in INSTALL_MARKERS {
            ensure!(
                source.contains(marker),
                "pinned installer source is missing required marker {marker:?}"
            );
        }
    }
    for marker in BASELINE_INSTALL_MARKERS {
        ensure!(
            install.contains(marker),
            "pinned installer baseline is missing required marker {marker:?}"
        );
        ensure!(
            !stable_install.contains(marker),
            "pinned installer stable unexpectedly retains marker {marker:?}"
        );
    }
    for source in [&tests, &stable_tests] {
        for marker in TEST_MARKERS {
            ensure!(
                source.contains(marker),
                "pinned installer test source is missing required marker {marker:?}"
            );
        }
    }
    verify_test_mapping(repo, &tests, BASELINE_TESTS)?;
    verify_test_mapping(repo, &stable_tests, STABLE_TESTS)?;
    verify_bin_helper(repo)?;
    verify_stage_helper(repo)?;
    verify_native_surface(repo)?;
    verify_oracle(repo)?;

    let docs = fs::read_to_string(repo.join("docs/install-script-migration.md"))
        .context("read install script migration documentation")?;
    for marker in [
        INSTALL_PATH,
        TEST_PATH,
        "20,132",
        "12,810",
        "13,638",
        "6,536",
        INSTALL_BASELINE_SHA256,
        INSTALL_STABLE_SHA256,
        TEST_BASELINE_SHA256,
        TEST_STABLE_SHA256,
        "non-overlapping",
        "platform oracle",
        "conflict",
        "TypeScript source",
        BIN_PATH,
        "install-bin",
        STAGE_PATH,
        "1,101",
        STAGE_SHA256,
        "static/install.sh",
        "collision",
        "symlink",
    ] {
        ensure!(
            docs.contains(marker),
            "install script migration documentation is missing {marker:?}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_rust_install_script_replaces_both_pinned_suites() {
        let repo = crate::repo_root().unwrap();
        super::verify(&repo, super::BASELINE).unwrap();
    }
}
