//! Native prebuilt-install smoke test.
//!
//! This replaces the package-manager smoke path with the release artifact and authenticated
//! install primitives that Workdeck actually ships. It stages one host artifact, installs it in
//! an isolated directory, executes the real binary, and verifies bundled skills without touching
//! the user's home or repository state.

use anyhow::{Context, Result, bail, ensure};
use serde_json::json;
use sha2::Digest;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};

const SOURCE_PATH: &str = "scripts/smoke-prebuilt-install.ts";
const SOURCE_BYTES: usize = 8_882;
const SOURCE_LINES: usize = 248;
const SOURCE_SHA256: &str = "38e86b4f1930bc6be3affa31849676dfe6cdd658f0c7d2d437b84f6e847faa0a";
const STABLE_BYTES: usize = 7_997;
const STABLE_LINES: usize = 222;
const STABLE_SHA256: &str = "273de8710fd848c7309f60d74458577165bda8d2a722476ec3165ed417788fde";
const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
const STABLE: &str = "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd";

#[derive(Debug, Clone, Copy)]
struct HostArtifact {
    package_name: &'static str,
    os: &'static str,
    cpu: &'static str,
    binary_name: &'static str,
}

fn host_artifact() -> Result<HostArtifact> {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        "windows" => "windows",
        other => bail!("unsupported host OS for native install smoke: {other}"),
    };
    let cpu = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => bail!("unsupported host architecture for native install smoke: {other}"),
    };
    let package_name = match (os, cpu) {
        ("darwin", "arm64") => "workdeck-darwin-arm64",
        ("darwin", "x64") => "workdeck-darwin-x64",
        ("linux", "arm64") => "workdeck-linux-arm64",
        ("linux", "x64") => "workdeck-linux-x64",
        ("windows", "x64") => "workdeck-windows-x64",
        _ => bail!("unsupported native release target {os}-{cpu}"),
    };
    Ok(HostArtifact {
        package_name,
        os,
        cpu,
        binary_name: if cfg!(windows) {
            "workdeck.exe"
        } else {
            "workdeck"
        },
    })
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    ensure!(
        source.is_dir(),
        "artifact source is not a directory: {}",
        source.display()
    );
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&source_path, &destination_path)?;
        } else {
            ensure!(
                entry.file_type()?.is_file(),
                "artifact source contains a non-file entry"
            );
            fs::copy(source_path, destination_path)?;
        }
    }
    Ok(())
}

fn binary_candidates(repo: &Path, artifact: HostArtifact) -> [PathBuf; 2] {
    [
        repo.join("target/debug").join(artifact.binary_name),
        repo.join("target/release").join(artifact.binary_name),
    ]
}

fn stage_fixture(repo: &Path, root: &Path, artifact: HostArtifact) -> Result<PathBuf> {
    let artifact_root = root.join("artifacts");
    let artifact_dir = artifact_root.join(artifact.package_name);
    fs::create_dir_all(&artifact_dir)?;
    let binary = binary_candidates(repo, artifact)
        .into_iter()
        .find(|path| path.is_file())
        .with_context(|| "build workdeck before running the install smoke test")?;
    fs::copy(binary, artifact_dir.join(artifact.binary_name))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            artifact_dir.join(artifact.binary_name),
            fs::Permissions::from_mode(0o755),
        )?;
    }
    let skills = artifact_dir.join("skills");
    for name in workdeck_core::BUNDLED_SKILL_NAMES {
        copy_tree(&repo.join("skills").join(name), &skills.join(name))?;
    }
    fs::copy(repo.join("LICENSE"), artifact_dir.join("LICENSE"))?;
    fs::copy(
        repo.join("THIRD_PARTY_NOTICES"),
        artifact_dir.join("THIRD_PARTY_NOTICES"),
    )?;
    let licenses = repo.join("dist/licenses.json");
    if licenses.is_file() {
        fs::copy(licenses, artifact_dir.join("licenses.json"))?;
    } else {
        fs::write(artifact_dir.join("licenses.json"), b"{\"components\":[]}")?;
    }
    fs::write(
        artifact_dir.join("sbom.cdx.json"),
        serde_json::to_vec_pretty(&json!({
            "bomFormat": "CycloneDX",
            "specVersion": "1.5",
            "components": [{"name": "workdeck", "type": "application"}]
        }))?,
    )?;
    fs::write(
        artifact_dir.join("provenance.json"),
        serde_json::to_vec_pretty(&json!({
            "builder": "cargo xtask install-smoke",
            "artifact": artifact.package_name
        }))?,
    )?;
    fs::write(
        artifact_dir.join("metadata.json"),
        serde_json::to_vec_pretty(&json!({
            "packageName": artifact.package_name,
            "os": artifact.os,
            "cpu": artifact.cpu,
            "binaryName": artifact.binary_name
        }))?,
    )?;
    Ok(artifact_dir)
}

fn run_binary(binary: &Path, args: &[&str], home: &Path, path: &OsStr) -> Result<Output> {
    let mut command = crate::process::command_with_path(binary, path);
    command
        .args(args)
        .current_dir(home)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = command.output()?;
    ensure!(
        output.status.success(),
        "{} {:?} failed: {}",
        binary.display(),
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(output)
}

fn run_smoke(repo: &Path) -> Result<serde_json::Value> {
    let temporary = tempfile::Builder::new()
        .prefix("workdeck-prebuilt-install-smoke-")
        .tempdir()?;
    let home = temporary.path().join("home");
    let install = temporary.path().join("install");
    fs::create_dir_all(&home)?;
    fs::create_dir_all(&install)?;
    let artifact = host_artifact()?;
    let artifact_dir = stage_fixture(repo, temporary.path(), artifact)?;
    let artifact_names = crate::release_artifacts::check(&temporary.path().join("artifacts"))?;
    ensure!(
        artifact_names == vec![artifact.package_name.to_owned()],
        "native artifact catalog mismatch"
    );
    let installed = install.join(artifact.binary_name);
    fs::copy(artifact_dir.join(artifact.binary_name), &installed)?;
    copy_tree(&artifact_dir.join("skills"), &install.join("skills"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&installed, fs::Permissions::from_mode(0o755))?;
    }
    let path = OsString::from(install.as_os_str());
    let help = run_binary(&installed, &["--help"], &home, &path)?;
    let help_text = String::from_utf8(help.stdout)?;
    ensure!(
        help_text.contains("Usage: workdeck"),
        "installed help lost Workdeck naming"
    );
    let version = run_binary(&installed, &["--version"], &home, &path)?;
    let version_text = String::from_utf8(version.stdout)?;
    ensure!(
        version_text
            == format!(
                "workdeck {}\n",
                workdeck_cli::version::resolve_cli_version()
            ),
        "installed version output differs from Cargo package version"
    );
    let mut skill_paths = Vec::new();
    for name in ["workdeck-review", "workdeck-extensions"] {
        let output = run_binary(&installed, &["skill", "path", name], &home, &path)?;
        let path_text = String::from_utf8(output.stdout)?.trim().to_owned();
        let skill_path = PathBuf::from(&path_text);
        ensure!(
            skill_path.is_file(),
            "installed skill path is missing: {path_text}"
        );
        ensure!(
            skill_path.starts_with(&install),
            "skill path escaped install root: {path_text}"
        );
        skill_paths.push(path_text);
    }
    let forbidden = ["bun", "node", "npm", "@oven", "@pierre", "hunk"];
    for entry in walk_paths(&install)? {
        let text = entry.to_string_lossy().to_ascii_lowercase();
        ensure!(
            !forbidden.iter().any(|token| text.contains(token)),
            "native install contains forbidden runtime path: {}",
            entry.display()
        );
    }
    ensure!(
        !home.join(".agents").exists(),
        "smoke test created repository state"
    );
    ensure!(
        !home.join("config/workdeck").exists(),
        "skill lookup wrote user state"
    );
    Ok(json!({
        "artifact": artifact.package_name,
        "binary": installed,
        "help": true,
        "version": version_text.trim(),
        "skills": skill_paths,
        "packageManager": false,
        "runtimeDependencies": false
    }))
}

fn walk_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        paths.push(path.clone());
        if entry.file_type()?.is_dir() {
            paths.extend(walk_paths(&path)?);
        }
    }
    Ok(paths)
}

pub(crate) fn run(repo: &Path, mut args: impl Iterator<Item = String>) -> Result<()> {
    ensure!(args.next().is_none(), "install-smoke accepts no arguments");
    println!("{}", serde_json::to_string_pretty(&run_smoke(repo)?)?);
    Ok(())
}

pub(crate) fn verify(repo: &Path, baseline: &str) -> Result<()> {
    if baseline != BASELINE {
        return Ok(());
    }
    for (pin, bytes_expected, lines_expected, sha_expected) in [
        (BASELINE, SOURCE_BYTES, SOURCE_LINES, SOURCE_SHA256),
        (STABLE, STABLE_BYTES, STABLE_LINES, STABLE_SHA256),
    ] {
        let source = crate::git_stdout_bytes(repo, ["show", &format!("{pin}:{SOURCE_PATH}")])?;
        ensure!(
            source.len() == bytes_expected,
            "pinned {SOURCE_PATH} {pin} changed size"
        );
        ensure!(
            source.split(|byte| *byte == b'\n').count() == lines_expected + 1,
            "pinned {SOURCE_PATH} {pin} changed line count"
        );
        ensure!(
            format!("{:x}", sha2::Sha256::digest(&source)) == sha_expected,
            "pinned {SOURCE_PATH} {pin} changed SHA-256"
        );
    }
    for (path, marker) in [
        ("xtask/src/install_smoke.rs", "run_smoke"),
        ("xtask/src/release_artifacts.rs", "check(root"),
        ("crates/workdeck-cli/src/version.rs", "resolve_cli_version"),
        ("docs/install-smoke-migration.md", "cargo xtask"),
    ] {
        let contents = fs::read_to_string(repo.join(path))
            .with_context(|| format!("read install smoke native surface {path}"))?;
        ensure!(
            contents.contains(marker),
            "install smoke native surface {path} is missing {marker:?}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_target_is_one_of_the_shipped_native_artifacts() {
        let artifact = host_artifact().unwrap();
        assert!(artifact.package_name.starts_with("workdeck-"));
        assert!(
            artifact.package_name.ends_with("-arm64") || artifact.package_name.ends_with("-x64")
        );
        assert_eq!(
            artifact.binary_name,
            if cfg!(windows) {
                "workdeck.exe"
            } else {
                "workdeck"
            }
        );
    }

    #[test]
    fn pinned_source_capture_matches_both_anchors() {
        verify(&crate::repo_root().unwrap(), BASELINE).unwrap();
    }

    #[test]
    fn native_artifact_smoke_executes_binary_and_skills() {
        let repo = crate::repo_root().unwrap();
        let result = run_smoke(&repo).unwrap();
        assert_eq!(result["packageManager"], false);
        assert_eq!(result["runtimeDependencies"], false);
        assert_eq!(result["skills"].as_array().unwrap().len(), 2);
    }
}
