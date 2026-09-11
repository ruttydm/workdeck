//! Native Rust target selection replacing Hunk's Bun `build-bin` host probe.
//!
//! Workdeck publishes Rust targets rather than runtime-specific Bun bundles.
//! x64 hosts receive an explicit target so release builds stay compatible with
//! the baseline CPU/libc contract; arm64 hosts compile for their native default.

use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
const STABLE: &str = "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd";
const SOURCE_PATH: &str = "scripts/build-bin.test.ts";
const SOURCE_BYTES: usize = 1_172;
const SOURCE_LINES: usize = 25;
const SOURCE_SHA256: &str = "0a198ec71b6a06c18a87ec49373222a4f37a356e1e053bc7afa979edf3bb19cb";

/// Return the explicit Rust target required for a published x64 host.
///
/// `None` means that the host's native Rust target is authoritative, which is
/// the same default the pinned script used for arm64 and unsupported hosts.
pub(crate) fn compile_target_for_host(
    platform: &str,
    arch: &str,
    musl: bool,
) -> Option<&'static str> {
    match (platform, arch) {
        ("darwin", "x64") => Some("x86_64-apple-darwin"),
        ("win32", "x64") => Some("x86_64-pc-windows-msvc"),
        ("linux", "x64") if musl => Some("x86_64-unknown-linux-musl"),
        ("linux", "x64") => Some("x86_64-unknown-linux-gnu"),
        _ => None,
    }
}

fn pinned_source(repo: &Path, commit: &str) -> Result<Vec<u8>> {
    let source = crate::git_stdout_bytes(repo, ["show", &format!("{commit}:{SOURCE_PATH}")])?;
    ensure!(
        source.len() == SOURCE_BYTES,
        "pinned {SOURCE_PATH} {commit} changed size: {} != {SOURCE_BYTES}",
        source.len()
    );
    ensure!(
        source.split(|byte| *byte == b'\n').count() == SOURCE_LINES + 1,
        "pinned {SOURCE_PATH} {commit} changed line count"
    );
    ensure!(
        format!("{:x}", Sha256::digest(&source)) == SOURCE_SHA256,
        "pinned {SOURCE_PATH} {commit} changed SHA-256"
    );
    Ok(source)
}

/// Verify the complete pinned build-target test projection and its release
/// matrix consumers.
pub(crate) fn verify(repo: &Path, baseline: &str) -> Result<()> {
    ensure!(
        baseline == BASELINE,
        "release-target verifier received unexpected baseline {baseline}"
    );
    let source = pinned_source(repo, BASELINE)?;
    let stable = pinned_source(repo, STABLE)?;
    ensure!(
        source == stable,
        "pinned build-bin test diverged between pins"
    );
    ensure!(
        compile_target_for_host("darwin", "x64", false) == Some("x86_64-apple-darwin")
            && compile_target_for_host("win32", "x64", false) == Some("x86_64-pc-windows-msvc")
            && compile_target_for_host("linux", "x64", false) == Some("x86_64-unknown-linux-gnu")
            && compile_target_for_host("linux", "x64", true) == Some("x86_64-unknown-linux-musl")
            && compile_target_for_host("linux", "arm64", false).is_none()
            && compile_target_for_host("freebsd", "x64", false).is_none(),
        "native release target matrix no longer matches the pinned host policy"
    );
    let source = std::str::from_utf8(&source)?;
    for marker in [
        "from \"bun:test\"",
        "compileTargetForHost",
        "compiles every x64 platform against Bun's baseline runtime",
        "keeps the host libc when compiling on a musl x64 host",
        "leaves arm64 hosts on Bun's own default runtime",
        "returns no target for platforms Hunk does not publish binaries for",
        "bun-darwin-x64-baseline",
        "bun-windows-x64-baseline",
        "bun-linux-x64-baseline",
        "bun-linux-x64-musl-baseline",
        "toBeNull()",
    ] {
        ensure!(
            source.contains(marker),
            "pinned build-bin test is missing marker {marker:?}"
        );
    }
    for (path, marker) in [
        (
            "xtask/src/release_targets.rs",
            "pub(crate) fn compile_target_for_host(",
        ),
        (
            "xtask/src/release_targets.rs",
            "native_rust_target_selection_matches_both_pinned_build_bin_tests",
        ),
        ("xtask/src/release_channel.rs", "aarch64-apple-darwin"),
        (".github/workflows/release.yml", "x86_64-unknown-linux-gnu"),
        (".github/workflows/release.yml", "x86_64-pc-windows-msvc"),
    ] {
        let native = fs::read_to_string(repo.join(path))
            .with_context(|| format!("read release-target native surface {path}"))?;
        ensure!(
            native.contains(marker),
            "release-target native surface {path} is missing {marker:?}"
        );
    }
    let docs = fs::read_to_string(repo.join("docs/release-targets-migration.md"))
        .context("read release-target migration documentation")?;
    for marker in [
        SOURCE_PATH,
        "1,172",
        SOURCE_SHA256,
        "x86_64-apple-darwin",
        "x86_64-unknown-linux-musl",
        "x86_64-pc-windows-msvc",
        "arm64",
        "native Rust target",
        "no Bun",
    ] {
        ensure!(
            docs.contains(marker),
            "release-target migration documentation is missing {marker:?}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_rust_target_selection_matches_both_pinned_build_bin_tests() {
        let repo = crate::repo_root().unwrap();
        verify(&repo, BASELINE).unwrap();
    }

    #[test]
    fn target_selection_preserves_x64_libc_and_native_arm64_defaults() {
        assert_eq!(
            compile_target_for_host("darwin", "x64", false),
            Some("x86_64-apple-darwin")
        );
        assert_eq!(
            compile_target_for_host("win32", "x64", false),
            Some("x86_64-pc-windows-msvc")
        );
        assert_eq!(
            compile_target_for_host("linux", "x64", false),
            Some("x86_64-unknown-linux-gnu")
        );
        assert_eq!(
            compile_target_for_host("linux", "x64", true),
            Some("x86_64-unknown-linux-musl")
        );
        for platform in ["darwin", "linux", "win32"] {
            assert_eq!(compile_target_for_host(platform, "arm64", false), None);
        }
        assert_eq!(compile_target_for_host("freebsd", "x64", false), None);
    }
}
