//! Cross-platform native process helpers.
//!
//! Hunk's scripts carried a JavaScript-specific `npm` shim and a case-insensitive Windows PATH
//! workaround. Workdeck has no package-manager runtime, but its Rust tooling still needs the
//! same deterministic environment boundary when it launches Cargo or Git subprocesses.

use anyhow::{Context, Result, ensure};
use sha2::Digest;
use std::ffi::{OsStr, OsString};
use std::process::Command;

const SOURCE_PATH: &str = "scripts/script-helpers.ts";
const SOURCE_BYTES: usize = 1_386;
const SOURCE_LINES: usize = 37;
const SOURCE_SHA256: &str = "e5997dcba8a94558dede41e70be51503e8faa16020587b20b9c2f33695010424";
const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
const STABLE: &str = "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd";

/// Return environment entries with every case variant of PATH removed and one canonical PATH
/// inserted. Windows treats `Path` and `PATH` as the same variable; retaining both lets the
/// operating system choose an unexpected value. On Unix the comparison is harmless and keeps
/// the launch contract deterministic across hosts.
pub(crate) fn env_with_path<I>(base: I, path: &OsStr) -> Vec<(OsString, OsString)>
where
    I: IntoIterator<Item = (OsString, OsString)>,
{
    let mut environment = base
        .into_iter()
        .filter(|(key, _)| !key.eq_ignore_ascii_case(OsStr::new("PATH")))
        .collect::<Vec<_>>();
    environment.push((OsString::from("PATH"), path.to_owned()));
    environment
}

/// Build a child command with the current environment and an explicit PATH. Callers can then
/// append arguments and command-specific variables without inheriting duplicate Windows PATH
/// keys.
pub(crate) fn command_with_path(program: impl AsRef<OsStr>, path: &OsStr) -> Command {
    let mut command = Command::new(program);
    command.env_clear();
    for (key, value) in env_with_path(std::env::vars_os(), path) {
        command.env(key, value);
    }
    command
}

/// Verify the complete source helper through Git without retaining or running its Bun module.
pub(crate) fn verify(repo: &std::path::Path, baseline: &str) -> Result<()> {
    if baseline != BASELINE {
        return Ok(());
    }
    for pin in [BASELINE, STABLE] {
        let source = crate::git_stdout_bytes(repo, ["show", &format!("{pin}:{SOURCE_PATH}")])?;
        ensure!(
            source.len() == SOURCE_BYTES,
            "pinned {SOURCE_PATH} {pin} changed size: {} != {SOURCE_BYTES}",
            source.len()
        );
        ensure!(
            source.split(|byte| *byte == b'\n').count() == SOURCE_LINES + 1,
            "pinned {SOURCE_PATH} {pin} changed line count"
        );
        ensure!(
            format!("{:x}", sha2::Sha256::digest(&source)) == SOURCE_SHA256,
            "pinned {SOURCE_PATH} {pin} changed SHA-256"
        );
    }
    let docs = std::fs::read_to_string(repo.join("docs/process-environment-migration.md"))
        .context("read process environment migration documentation")?;
    for marker in [
        SOURCE_PATH,
        "case-insensitive",
        "PATH",
        "Cargo",
        "package-manager runtime",
    ] {
        ensure!(
            docs.contains(marker),
            "process environment migration is missing {marker:?}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::Digest;

    #[test]
    fn path_environment_removes_case_variants_and_preserves_other_entries() {
        let entries = vec![
            (OsString::from("Path"), OsString::from("old")),
            (OsString::from("PATH"), OsString::from("older")),
            (OsString::from("HOME"), OsString::from("/tmp")),
        ];
        let clean = env_with_path(entries, OsStr::new("/native/bin"));
        assert_eq!(
            clean,
            vec![
                (OsString::from("HOME"), OsString::from("/tmp")),
                (OsString::from("PATH"), OsString::from("/native/bin")),
            ]
        );
    }

    #[test]
    fn command_with_path_is_a_real_native_process_boundary() {
        let command = command_with_path("echo", OsStr::new("/native/bin"));
        let path_entries = command
            .get_envs()
            .filter(|(key, _)| key.eq_ignore_ascii_case(OsStr::new("PATH")))
            .collect::<Vec<_>>();
        assert_eq!(path_entries.len(), 1);
        assert_eq!(path_entries[0].1, Some(OsStr::new("/native/bin")));
        assert!(
            command
                .get_envs()
                .any(|(key, value)| { key == OsStr::new("HOME") && value.is_some() })
        );
    }

    #[test]
    fn source_capture_matches_both_pinned_anchors() {
        let repo = crate::repo_root().unwrap();
        verify(&repo, BASELINE).unwrap();
        let bytes =
            crate::git_stdout_bytes(&repo, ["show", &format!("{BASELINE}:{SOURCE_PATH}")]).unwrap();
        assert_eq!(format!("{:x}", sha2::Sha256::digest(bytes)), SOURCE_SHA256);
    }
}
