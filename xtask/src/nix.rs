//! Cargo-only Nix packaging contract and external evaluation gate.

use anyhow::{Context, Result, bail};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

pub(crate) fn check(repo: &Path) -> Result<()> {
    validate_sources(repo)?;
    super::run_checked(
        repo,
        "nix",
        &[
            "flake",
            "check",
            "--no-write-lock-file",
            "--print-build-logs",
        ],
    )?;
    super::run_checked(
        repo,
        "nix",
        &["run", "--no-write-lock-file", ".#workdeck", "--", "--help"],
    )?;
    println!("Nix flake evaluation, package build, and installed-style help smoke passed.");
    Ok(())
}

fn validate_sources(repo: &Path) -> Result<()> {
    let expression_paths = [
        "flake.nix",
        "nix/package.nix",
        "nix/devShell.nix",
        "nix/home-manager.nix",
    ];
    let expressions = expression_paths
        .iter()
        .map(|path| {
            fs::read_to_string(repo.join(path)).with_context(|| format!("read Nix source {path}"))
        })
        .collect::<Result<Vec<_>>>()?;
    for (path, source) in expression_paths.iter().zip(&expressions) {
        let parsed = rnix::Root::parse(source);
        if !parsed.errors().is_empty() {
            bail!("{path} has Nix syntax errors: {:?}", parsed.errors());
        }
    }
    let joined = expressions.join("\n").to_ascii_lowercase();
    for forbidden in ["bun", "nodejs", "npm", "opentui", "wasm", "hunk"] {
        if joined.contains(forbidden) {
            bail!("Nix runtime/build expressions retain forbidden token {forbidden:?}");
        }
    }

    let flake = &expressions[0];
    for required in [
        "packages = systemOutput \"packages\"",
        "apps = systemOutput \"apps\"",
        "checks = systemOutput \"checks\"",
        "devShells = systemOutput \"devShells\"",
        "homeManagerModules",
        "programs.workdeck.package",
    ] {
        if !flake.contains(required) {
            bail!("flake.nix is missing required output contract {required:?}");
        }
    }

    let package = &expressions[1];
    for required in [
        "cargoLock.lockFile = ../Cargo.lock",
        "\"workdeck-cli\"",
        "\"workdeck\"",
        "LICENSE THIRD_PARTY_NOTICES",
        "mainProgram = \"workdeck\"",
    ] {
        if !package.contains(required) {
            bail!("nix/package.nix is missing required Cargo package contract {required:?}");
        }
    }

    let home_manager = &expressions[3];
    for required in [
        "programs.workdeck",
        "workdeck/config.toml",
        "workdeck pager",
        ".claude/skills/workdeck-review",
    ] {
        if !home_manager.contains(required) {
            bail!("Home Manager module is missing required contract {required:?}");
        }
    }

    let envrc = fs::read_to_string(repo.join(".envrc")).context("read .envrc")?;
    if envrc != "use flake\n" {
        bail!(".envrc must contain exactly `use flake`");
    }

    let lock: serde_json::Value =
        serde_json::from_slice(&fs::read(repo.join("flake.lock")).context("read flake.lock")?)
            .context("parse flake.lock")?;
    let nodes = lock
        .get("nodes")
        .and_then(serde_json::Value::as_object)
        .context("flake.lock has no nodes object")?;
    let names = nodes.keys().cloned().collect::<BTreeSet<_>>();
    if names != BTreeSet::from(["nixpkgs".into(), "root".into(), "systems".into()]) {
        bail!("flake.lock contains unexpected inputs: {names:?}");
    }
    if lock.get("version").and_then(serde_json::Value::as_u64) != Some(7) {
        bail!("flake.lock must use schema version 7");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_nix_surface_is_cargo_only_and_complete() {
        let repo = super::super::repo_root().unwrap();
        validate_sources(&repo).unwrap();
    }
}
