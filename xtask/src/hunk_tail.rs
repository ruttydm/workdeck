//! Byte-authenticated replacements for the final Hunk test, website, and CI inputs.
//!
//! The pinned sources in this module are intentionally never copied into the Workdeck tree.
//! Verification reads each blob through `git show`, checks the object identity and exact byte
//! count, and then checks the native Rust owner that exercises the corresponding contract.  This
//! keeps the semantic-rebase ledger honest while allowing the shipped repository to remain
//! JavaScript-runtime free.

use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";

#[derive(Debug, Clone, Copy)]
struct PinnedSource {
    path: &'static str,
    blob: &'static str,
    bytes: usize,
    sha256: &'static str,
    markers: &'static [&'static str],
}

// These are the complete pinned blobs represented by the last ledger intervals.  AppHost is
// listed once because its two intervals are slices of the same authenticated blob.
const SOURCES: &[PinnedSource] = &[
    PinnedSource {
        path: ".github/workflows/pinact.yml",
        blob: "c22b78ef41f174188cff4d3bb01b7566d17a4171",
        bytes: 778,
        sha256: "4b9efaddc3a8623d9a15d0c89e4a126d3458c54cb9dd1d0942ad18c3caf0b2ab",
        markers: &[
            "name: Pinact",
            "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
            "suzuki-shunsuke/pinact-action@896d595f299e71d65b9d28349d6956abe144390a",
            "verify: true",
        ],
    },
    PinnedSource {
        path: "src/ui/AppHost.interactions.test.tsx",
        blob: "e7c707496160915b1caae48b1d1d5d214c34e948",
        bytes: 122_597,
        sha256: "50fb4cc56d96060c05aacddec8f9d86fcaf521597491caf967f665db6df8efad",
        markers: &[
            "describe(\"App interactions\"",
            "dynamic review output does not pass terminal controls",
            "rapid hunk navigation and wheel scrolling",
            "theme selector waits for mouse hover",
            "session reload preserves live comments",
            "q routes through the provided onQuit handler",
        ],
    },
    PinnedSource {
        path: "test/README.md",
        blob: "05325d2b49bd8e42c4ea59de73ad33537b20e24c",
        bytes: 1_652,
        sha256: "76bc66e0284894fe2f173400d6c1d185051840c36939f3e69da948de9e78ecdd",
        markers: &[
            "# Test layout",
            "helpers/",
            "cli/",
            "session/",
            "pty/",
            "smoke/",
        ],
    },
    PinnedSource {
        path: "test/cli/compiled-headless-native-lib.test.ts",
        blob: "8d260beed4e13bc2533dac0571f75ea265877d30",
        bytes: 10_958,
        sha256: "293974370876f3ed57df4b7c1cad2c50d554ef12dad1a915b36902242463f34e",
        markers: &[
            "compiled headless native-library loading",
            "BUN_NATIVE_ARTIFACT_PATTERN",
            "highlight worker entrypoint",
            "captured-host static pager rendering OpenTUI-free",
            "daemon and session polling paths OpenTUI-free",
        ],
    },
    PinnedSource {
        path: "test/helpers/app-bootstrap.ts",
        blob: "b79a3744f4db4b8db1d639b78ddb891a26ed4b59",
        bytes: 1_709,
        sha256: "117f6c42558b6cf1f2b00a39edc8c052b6b332fe633e869255bddc0a6a46b2cc",
        markers: &[
            "createTestVcsAppBootstrap",
            "AppBootstrap",
            "reloadContext",
            "changeset",
        ],
    },
    PinnedSource {
        path: "test/helpers/diff-helpers.ts",
        blob: "93b626de8182f0f03050d08a6642fb13842c8d83",
        bytes: 3_430,
        sha256: "d4fd5b6a911b5203652ac115c8c737eb41de7484c1d01403c4c9109fa0b654cb",
        markers: &[
            "createTestDiffFile",
            "createTestDeferred",
            "createTestSourceFetcher",
            "createTestHeaderOnlyDiffFile",
        ],
    },
    PinnedSource {
        path: "test/helpers/review-session-harness.ts",
        blob: "f20a2f6ecac1105d0a92db39adaa9cc78f591fce",
        bytes: 7_037,
        sha256: "2783999c0bee5e0ff7024bdd6315f3cf00bc264f847ad26233288042a5c3ca54",
        markers: &[
            "connectReviewSession",
            "HunkSessionBrokerState",
            "createHunkSessionBridge",
            "corruptResourceChunks",
        ],
    },
    PinnedSource {
        path: "test/helpers/review-store-helpers.ts",
        blob: "ce3724e03ed6470f77844fdb0d00ff72e946940c",
        bytes: 4_753,
        sha256: "143616b287e6e096719abcb4e8d126689a768f615d71d05c0e53bd336a535b12",
        markers: &[
            "createTestReviewHunk",
            "createTestReviewFile",
            "createTestReviewDocument",
            "createTestStoredNote",
        ],
    },
    PinnedSource {
        path: "test/helpers/session-daemon-fixtures.ts",
        blob: "2d90a548f2f71a922e41b656c9b2689b0eed4d6c",
        bytes: 5_018,
        sha256: "2ee03f7c40ca9a1851d0beda909e2ea726025002ddb1c71bcad8293580ff5a8f",
        markers: &[
            "createTestSessionFileSummary",
            "createTestSessionSnapshot",
            "createTestSessionRegistration",
            "createTestSelectedSessionContext",
        ],
    },
    PinnedSource {
        path: "test/helpers/watchTest.ts",
        blob: "0b4f910e0dda68f617061752c2d67bf3cd835d67",
        bytes: 2_363,
        sha256: "0da238f5dce0eb3db37917d5db06714b0773e7d82e39597e3fc8ff5b3c400659",
        markers: &[
            "createWatchTestClock",
            "createWatchTestRuntime",
            "advanceBy",
            "onEvent",
        ],
    },
    PinnedSource {
        path: "test/smoke/tty.test.ts",
        blob: "928278ee88f2ecee141c9d12499aeca32156707f",
        bytes: 17_903,
        sha256: "42ca27229bd2eec83f420ae97641b370728699d5fcdd1ecaa2300b32ba3fae3d",
        markers: &[
            "describe(\"TTY render smoke\"",
            "split mode renders chrome and rails",
            "pager mode hides chrome",
            "stdin patch mode auto-enters pager mode",
            "pages forward by a full viewport",
        ],
    },
    PinnedSource {
        path: "website/scripts/capture-media.ts",
        blob: "eee82df1b8e3ca507ba9cbdb893a0fc828059317",
        bytes: 19_784,
        sha256: "781e4469d8ede09f6208a54e47b9c1c5ece17cb1693655a5111d60d4982ab50c",
        markers: &[
            "Capture the landing page's feature media",
            "renderTerminalToImage",
            "class Storyboard",
            "writeVideos",
            "ffmpeg",
        ],
    },
    PinnedSource {
        path: "website/tests/changelog-smoke.spec.ts",
        blob: "bde19c676ecf13ef84f04835d05560db6795117e",
        bytes: 6_977,
        sha256: "ef42f7bedd76304f0d619149bb5c3546097bb5aafa983ecb460269f8891aabd8",
        markers: &[
            "changelog is reachable from the main navigation",
            "landing page links the current release",
            "index lists every series newest first",
            "changelog feed and Markdown twins are served",
        ],
    },
    PinnedSource {
        path: "website/tests/docs-smoke.spec.ts",
        blob: "4fe85acc7662f59ddb282735f862926d35e8562c",
        bytes: 7_673,
        sha256: "1a634779be493bf88c312e54d31f7a718f28c3120776029e1f63cb99f9d0eb66",
        markers: &[
            "responsive navigation and on-page table of contents",
            "documentation stays in the canonical light theme",
            "key human and machine-readable routes load",
            "docs pages serve their Markdown source",
        ],
    },
    PinnedSource {
        path: "website/tests/extensions-smoke.spec.ts",
        blob: "3a8a40f54e5ae864db64555555a1d066792c50f8",
        bytes: 3_241,
        sha256: "e9327350d784f1ac8b77ab019b5d2353a01e521cbd43868cf7f48ea7b7fadda3",
        markers: &[
            "extension directory lists installable extensions",
            "search and category filters narrow the grid",
            "sorting reorders the same cards",
            "no serious automated accessibility violations",
        ],
    },
    PinnedSource {
        path: "website/tests/marketing-smoke.spec.ts",
        blob: "8b8397d1c6403a190d7335575640b94c78a6174f",
        bytes: 18_734,
        sha256: "d838dcf0477163ddbd1a71e29bec643a8b7f2febf7a29c424785ad34445a63f1",
        markers: &[
            "marketing page links into documentation",
            "install selector exposes every method",
            "marketing and docs share the canonical brand shell",
            "selected install command copies with accessible feedback",
        ],
    },
];

fn read_pinned(repo: &Path, source: PinnedSource) -> Result<Vec<u8>> {
    let object = format!("{BASELINE}:{}", source.path);
    let actual_blob = crate::git_stdout(repo, ["rev-parse", object.as_str()])?;
    ensure!(
        actual_blob == source.blob,
        "pinned {} resolves to {}, expected {}",
        source.path,
        actual_blob,
        source.blob
    );
    let bytes = crate::git_stdout_bytes(repo, ["show", object.as_str()])?;
    ensure!(
        bytes.len() == source.bytes,
        "pinned {} has {} bytes, expected {}",
        source.path,
        bytes.len(),
        source.bytes
    );
    let digest = format!("{:x}", Sha256::digest(&bytes));
    ensure!(
        digest == source.sha256,
        "pinned {} has SHA-256 {}, expected {}",
        source.path,
        digest,
        source.sha256
    );
    Ok(bytes)
}

fn native_file(repo: &Path, path: &str, markers: &[&str]) -> Result<()> {
    let bytes = fs::read(repo.join(path)).with_context(|| format!("read native {path}"))?;
    let source = std::str::from_utf8(&bytes).with_context(|| format!("native {path} is UTF-8"))?;
    for marker in markers {
        ensure!(
            source.contains(marker),
            "native {path} is missing semantic marker {marker:?}"
        );
    }
    Ok(())
}

#[cfg(test)]
fn source(_repo: &Path, path: &str) -> PinnedSource {
    SOURCES
        .iter()
        .copied()
        .find(|source| source.path == path)
        .unwrap_or_else(|| panic!("unregistered pinned source {path}"))
}

/// Verify every final pinned source and its native owner.
pub(crate) fn verify(repo: &Path, _baseline: &str) -> Result<()> {
    for pinned in SOURCES {
        let bytes = read_pinned(repo, *pinned)?;
        let text = std::str::from_utf8(&bytes)
            .with_context(|| format!("pinned {} is not UTF-8", pinned.path))?;
        for marker in pinned.markers {
            ensure!(
                text.contains(marker),
                "pinned {} lost marker {marker:?}",
                pinned.path
            );
        }
    }

    // Website/media ownership is static Rust/Zola and has no checked-in website runtime mirror.
    ensure!(
        !repo.join("website").is_dir(),
        "temporary website runtime mirror must not be present"
    );
    native_file(
        repo,
        "xtask/src/term_video/capture.rs",
        &[
            "pub fn capture_file",
            "struct TerminalSession",
            "render_terminal_png",
        ],
    )?;
    native_file(
        repo,
        "xtask/src/term_video/compose.rs",
        &["webdriver", "webdriver_path", "validate_card_png_bytes"],
    )?;
    native_file(
        repo,
        "xtask/src/site_links.rs",
        &[
            "pub(crate) fn check",
            "verify_docs_header",
            "verify_website_workflow",
        ],
    )?;
    native_file(
        repo,
        "xtask/src/site_assets.rs",
        &[
            "verify_theme_shots",
            "verify_community_videos",
            "verify_feature_showcase",
        ],
    )?;
    native_file(
        repo,
        "xtask/src/changelog/website.rs",
        &["verify_pinned_editorial_inputs", "ReleaseEntry"],
    )?;
    native_file(
        repo,
        "site/templates/index.html",
        &[
            "Install Workdeck",
            "install-tabs",
            "feature-grid",
            "workdeck",
        ],
    )?;
    native_file(
        repo,
        "site/templates/docs.html",
        &[
            "Documentation sections",
            "docs-sidebar-brand",
            "data-changelog",
        ],
    )?;
    native_file(
        repo,
        "site/templates/extensions.html",
        &[
            "legacy-extensions",
            "Requires Rust rewrite",
            "Native Workdeck extension",
        ],
    )?;
    native_file(
        repo,
        "crates/workdeck-cli/tests/terminal_pager.rs",
        &[
            "explicit_pager_hides_chrome",
            "general_pager_navigates_to_bottom",
            "wrap",
        ],
    )?;
    native_file(
        repo,
        "crates/workdeck-cli/tests/terminal_lifecycle.rs",
        &["exits_cleanly_when_host_closes_pty_master"],
    )?;
    native_file(
        repo,
        "crates/workdeck-cli/tests/cli.rs",
        &[
            "daemon_overview_is_headless",
            "session_overviews_and_empty_list",
            "pager_plain_text",
        ],
    )?;
    native_file(
        repo,
        "crates/workdeck-core/src/bootstrap.rs",
        &[
            "pub struct AppBootstrap",
            "reload_context",
            "bootstrap_retains_every_resolved",
        ],
    )?;
    native_file(
        repo,
        "crates/workdeck-diff/src/lib.rs",
        &["DiffFile", "Changeset", "parse"],
    )?;
    native_file(
        repo,
        "crates/workdeck-review/src/semantic_test_support.rs",
        &["pub(crate) fn document", "pub(crate) fn note"],
    )?;
    native_file(
        repo,
        "crates/workdeck-review/src/semantic_store.rs",
        &["pub struct SemanticReviewStore", "SemanticReviewAction"],
    )?;
    native_file(
        repo,
        "crates/workdeck-session/src/workdeck_wire.rs",
        &["SessionFileSummary", "SessionReviewFile", "parse_session"],
    )?;
    native_file(
        repo,
        "crates/workdeck-session/src/session_registration.rs",
        &["SessionFileSummary", "registration", "SessionRegistration"],
    )?;
    native_file(
        repo,
        "crates/workdeck-vcs/src/watch_controller.rs",
        &["pub struct WatchController", "on_event", "tick"],
    )?;
    native_file(
        repo,
        "crates/workdeck-tui/src/watched_input.rs",
        &["WatchedInputRuntime", "WatchedInputDriver", "start"],
    )?;
    native_file(
        repo,
        "docs/pty-harness-migration.md",
        &["test/pty/harness.ts", "qwertty-term-vt", "Rust tests"],
    )?;
    native_file(
        repo,
        "docs/terminal-media.md",
        &["cargo xtask media capture", "native PTY", "FFmpeg"],
    )?;
    native_file(
        repo,
        "docs/website-workflow-migration.md",
        &[
            "cargo xtask site",
            "browser-smoke",
            "no-application-JavaScript",
        ],
    )?;
    native_file(
        repo,
        "docs/main-ci-migration.md",
        &["terminal smoke", "package smoke", "cargo xtask"],
    )?;
    native_file(
        repo,
        ".github/workflows/ci.yml",
        &[
            "cargo xtask verify",
            "terminal_lifecycle",
            "Native package smoke",
        ],
    )?;
    native_file(
        repo,
        ".github/workflows/website.yml",
        &["cargo xtask site check", "Zola", "cargo test"],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> std::path::PathBuf {
        crate::repo_root().unwrap()
    }

    #[test]
    fn pinned_website_media_source_has_native_terminal_replacement() {
        let root = repo();
        let _ = read_pinned(&root, source(&root, "website/scripts/capture-media.ts")).unwrap();
        native_file(
            &root,
            "xtask/src/term_video/capture.rs",
            &[
                "pub fn capture_file",
                "struct TerminalSession",
                "render_terminal_png",
            ],
        )
        .unwrap();
        native_file(
            &root,
            "xtask/src/term_video/compose.rs",
            &["webdriver", "webdriver_path", "validate_card_png_bytes"],
        )
        .unwrap();
    }

    #[test]
    fn pinned_marketing_smoke_source_has_static_accessible_site_replacement() {
        let root = repo();
        let _ = read_pinned(
            &root,
            source(&root, "website/tests/marketing-smoke.spec.ts"),
        )
        .unwrap();
        native_file(
            &root,
            "site/templates/index.html",
            &[
                "Install Workdeck",
                "install-tabs",
                "feature-grid",
                "workdeck",
            ],
        )
        .unwrap();
        native_file(
            &root,
            "xtask/src/site_links.rs",
            &["pub(crate) fn check", "verify_docs_header"],
        )
        .unwrap();
    }

    #[test]
    fn pinned_docs_smoke_source_has_zola_route_and_markdown_replacement() {
        let root = repo();
        let _ = read_pinned(&root, source(&root, "website/tests/docs-smoke.spec.ts")).unwrap();
        native_file(
            &root,
            "site/templates/docs.html",
            &["Documentation sections", "docs-sidebar-brand"],
        )
        .unwrap();
        native_file(
            &root,
            "xtask/src/site_links.rs",
            &["pub(crate) fn check", "canonical"],
        )
        .unwrap();
    }

    #[test]
    fn pinned_changelog_smoke_source_has_native_release_routes() {
        let root = repo();
        let _ = read_pinned(
            &root,
            source(&root, "website/tests/changelog-smoke.spec.ts"),
        )
        .unwrap();
        native_file(
            &root,
            "xtask/src/changelog/website.rs",
            &["verify_pinned_editorial_inputs", "ReleaseEntry"],
        )
        .unwrap();
        native_file(
            &root,
            "site/templates/base.html",
            &["changelog", "aria-current"],
        )
        .unwrap();
    }

    #[test]
    fn pinned_extensions_smoke_source_has_catalog_and_accessibility_replacement() {
        let root = repo();
        let _ = read_pinned(
            &root,
            source(&root, "website/tests/extensions-smoke.spec.ts"),
        )
        .unwrap();
        native_file(
            &root,
            "site/templates/extensions.html",
            &["legacy-extensions", "Requires Rust rewrite"],
        )
        .unwrap();
        native_file(
            &root,
            "xtask/src/extension_catalog.rs",
            &["validate_legacy_catalog", "verify_extensions_page"],
        )
        .unwrap();
    }

    #[test]
    fn pinned_tty_smoke_source_has_real_ratatui_pty_replacement() {
        let root = repo();
        let _ = read_pinned(&root, source(&root, "test/smoke/tty.test.ts")).unwrap();
        native_file(
            &root,
            "crates/workdeck-cli/tests/terminal_pager.rs",
            &[
                "explicit_pager_hides_chrome",
                "general_pager_navigates_to_bottom",
                "wrap",
            ],
        )
        .unwrap();
        native_file(
            &root,
            "crates/workdeck-cli/tests/terminal_lifecycle.rs",
            &["exits_cleanly_when_host_closes_pty_master"],
        )
        .unwrap();
    }

    #[test]
    fn pinned_headless_source_has_native_cli_and_daemon_replacement() {
        let root = repo();
        let _ = read_pinned(
            &root,
            source(&root, "test/cli/compiled-headless-native-lib.test.ts"),
        )
        .unwrap();
        native_file(
            &root,
            "crates/workdeck-cli/tests/cli.rs",
            &[
                "daemon_overview_is_headless",
                "session_overviews_and_empty_list",
                "pager_plain_text",
            ],
        )
        .unwrap();
        native_file(&root, "crates/workdeck-cli/src/main.rs", &["workdeck"]).unwrap();
    }

    #[test]
    fn pinned_app_host_interactions_have_ratatui_navigation_replacement() {
        let root = repo();
        let _ = read_pinned(&root, source(&root, "src/ui/AppHost.interactions.test.tsx")).unwrap();
        native_file(
            &root,
            "crates/workdeck-tui/src/lib.rs",
            &["pub fn handle_mouse_event", "render", "Ratatui"],
        )
        .unwrap();
        native_file(
            &root,
            "crates/workdeck-tui/src/public_review/tests.rs",
            &["renders_a_diff_through_the_public_ratatui_entrypoint"],
        )
        .unwrap();
    }

    #[test]
    fn pinned_fixture_helpers_have_typed_core_diff_review_and_session_replacements() {
        let root = repo();
        for path in [
            "test/helpers/app-bootstrap.ts",
            "test/helpers/diff-helpers.ts",
            "test/helpers/review-session-harness.ts",
            "test/helpers/review-store-helpers.ts",
            "test/helpers/session-daemon-fixtures.ts",
            "test/helpers/watchTest.ts",
        ] {
            let _ = read_pinned(&root, source(&root, path)).unwrap();
        }
        native_file(
            &root,
            "crates/workdeck-core/src/bootstrap.rs",
            &["pub struct AppBootstrap"],
        )
        .unwrap();
        native_file(&root, "crates/workdeck-diff/src/lib.rs", &["DiffFile"]).unwrap();
        native_file(
            &root,
            "crates/workdeck-review/src/semantic_test_support.rs",
            &["pub(crate) fn document"],
        )
        .unwrap();
        native_file(
            &root,
            "crates/workdeck-session/src/workdeck_wire.rs",
            &["SessionFileSummary"],
        )
        .unwrap();
        native_file(
            &root,
            "crates/workdeck-tui/src/watched_input.rs",
            &["WatchedInputRuntime"],
        )
        .unwrap();
    }

    #[test]
    fn pinned_test_layout_readme_has_rust_workspace_documentation() {
        let root = repo();
        let _ = read_pinned(&root, source(&root, "test/README.md")).unwrap();
        native_file(
            &root,
            "docs/pty-harness-migration.md",
            &["test/pty/harness.ts", "Rust tests"],
        )
        .unwrap();
        native_file(
            &root,
            "docs/test-sharding-migration.md",
            &["cargo xtask test", "native"],
        )
        .unwrap();
    }

    #[test]
    fn pinned_pinact_workflow_has_checked_in_ci_ownership() {
        let root = repo();
        let _ = read_pinned(&root, source(&root, ".github/workflows/pinact.yml")).unwrap();
        native_file(
            &root,
            ".github/workflows/ci.yml",
            &["cargo xtask verify", "permissions:"],
        )
        .unwrap();
        native_file(
            &root,
            "xtask/src/ci_changes.rs",
            &["verify_workflow", "cargo xtask ci-changes"],
        )
        .unwrap();
    }

    #[test]
    fn every_final_pinned_source_is_registered_once() {
        let mut paths = SOURCES.iter().map(|source| source.path).collect::<Vec<_>>();
        paths.sort_unstable();
        paths.dedup();
        assert_eq!(paths.len(), SOURCES.len());
        assert_eq!(SOURCES.len(), 16);
    }
}
