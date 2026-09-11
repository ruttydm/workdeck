//! Native release-artifact staging and validation.
//!
//! The pinned Hunk tree used a package-manager distribution with a JavaScript
//! launcher.  Workdeck ships one Rust executable in signed archives instead.
//! This module owns the remaining artifact mechanics (host selection, staging,
//! metadata, skill payloads, and validation) without retaining a package
//! manager runtime or an alternate executable name.

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
const STABLE: &str = "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd";

// (path, baseline bytes, baseline lines, baseline SHA-256, stable bytes,
// stable lines, stable SHA-256, baseline Git blob id).
type SourceBlob = (
    &'static str,
    usize,
    usize,
    &'static str,
    usize,
    usize,
    &'static str,
    &'static str,
);
const SOURCE_BLOBS: &[SourceBlob] = &[
    (
        "scripts/prebuilt-package-helpers.ts",
        7344,
        233,
        "c6e0dc87583b0312ccacb3c0899c9a61ad43e6099dd29ab5418ec2d125b14664",
        6696,
        218,
        "1f4ece86798db6441e24f3d02e595de40783b5308fa61ca7d64189cc35b769e9",
        "dc7cc5837c2c951edd89d095e70d62cd3dfe81d0",
    ),
    (
        "scripts/build-prebuilt-artifact.ts",
        3540,
        114,
        "e727fdad4dcdbf2202005de7108969d223e25e28703068859e612462e6483930",
        3540,
        114,
        "e727fdad4dcdbf2202005de7108969d223e25e28703068859e612462e6483930",
        "c5a10983167452fbc8f0478e4b164c2e929a2c47",
    ),
    (
        "scripts/build-prebuilt-artifact.test.ts",
        3699,
        92,
        "7221cc8636b4c71e697a4d2ce1ee6099b5612b3bb5e9a528d6284bb0c80a9dbd",
        3699,
        92,
        "7221cc8636b4c71e697a4d2ce1ee6099b5612b3bb5e9a528d6284bb0c80a9dbd",
        "847c61b4f5be6db2c4c285360745f96f24fa11b1",
    ),
    (
        "scripts/check-prebuilt-pack.ts",
        3536,
        118,
        "84df050c7b6f3956e5f8ac7ff4940dad9329b32099dbed4b2cdaa7e72e527f39",
        3389,
        118,
        "f733a7a01afb099ff64db85be273fea7b24febf7a5252d603b035d7962c85cc5",
        "f35afc9f428298c82164ee74cc61454789cc4dc3",
    ),
    (
        "scripts/stage-prebuilt-npm.ts",
        6268,
        204,
        "0398ad85ae53fef7327602dd63cac1591495e622f5315bb5013e96fc9bcedbe5",
        6198,
        203,
        "ab957a6142c06a15a65a8611e9f814b474c0640cd418d627f271fcf2e5bcdc28",
        "a001a41a1a7d6a9b65287828fe6a98b5ec614c9d",
    ),
    (
        "scripts/publish-prebuilt-npm.ts",
        3089,
        117,
        "1abed6dcf460748dc8d47121f7588267781f52862abbc119bd848ce9f35da3f1",
        3089,
        117,
        "1abed6dcf460748dc8d47121f7588267781f52862abbc119bd848ce9f35da3f1",
        "d830c05ebbf107002d2814365897caae1cc2ca5a",
    ),
    (
        "scripts/build-npm.ts",
        3963,
        149,
        "928adc30b5ff74988e02f03b10c9860df0a0efdd26f1fc040490b75c8f700fa4",
        3963,
        149,
        "928adc30b5ff74988e02f03b10c9860df0a0efdd26f1fc040490b75c8f700fa4",
        "4ad4d95f5a9bcebdc2965a003838c6c7b4fc15a2",
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PlatformSpec {
    pub package_name: &'static str,
    pub os: &'static str,
    pub cpu: &'static str,
    pub binary_name: &'static str,
}

pub(crate) const PLATFORM_MATRIX: &[PlatformSpec] = &[
    PlatformSpec {
        package_name: "workdeck-darwin-arm64",
        os: "darwin",
        cpu: "arm64",
        binary_name: "workdeck",
    },
    PlatformSpec {
        package_name: "workdeck-darwin-x64",
        os: "darwin",
        cpu: "x64",
        binary_name: "workdeck",
    },
    PlatformSpec {
        package_name: "workdeck-linux-arm64",
        os: "linux",
        cpu: "arm64",
        binary_name: "workdeck",
    },
    PlatformSpec {
        package_name: "workdeck-linux-x64",
        os: "linux",
        cpu: "x64",
        binary_name: "workdeck",
    },
    PlatformSpec {
        package_name: "workdeck-windows-x64",
        os: "windows",
        cpu: "x64",
        binary_name: "workdeck.exe",
    },
];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArtifactMetadata {
    package_name: String,
    os: String,
    cpu: String,
    binary_name: String,
}

fn spec_for_host(platform: &str, arch: &str) -> Result<PlatformSpec> {
    let os = match platform {
        "macos" | "darwin" => "darwin",
        "linux" => "linux",
        "windows" | "win32" => "windows",
        other => bail!("Unsupported host platform for Workdeck artifacts: {other}"),
    };
    let cpu = match arch {
        "aarch64" | "arm64" => "arm64",
        "x86_64" | "x64" => "x64",
        other => bail!("Unsupported host architecture for Workdeck artifacts: {other}"),
    };
    PLATFORM_MATRIX
        .iter()
        .copied()
        .find(|candidate| candidate.os == os && candidate.cpu == cpu)
        .ok_or_else(|| anyhow::anyhow!("No Workdeck artifact target matches {os}/{cpu}"))
}

fn spec_by_name(name: &str) -> Result<PlatformSpec> {
    PLATFORM_MATRIX
        .iter()
        .copied()
        .find(|candidate| candidate.package_name == name)
        .ok_or_else(|| anyhow::anyhow!("Unknown Workdeck artifact package: {name}"))
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source)
        .with_context(|| format!("read artifact source {}", source.display()))?;
    ensure!(
        metadata.is_dir(),
        "artifact source is not a directory: {}",
        source.display()
    );
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path)?;
        if metadata.is_dir() {
            copy_tree(&source_path, &destination_path)?;
        } else {
            ensure!(
                metadata.is_file(),
                "artifact source contains a non-file entry: {}",
                source_path.display()
            );
            fs::copy(&source_path, &destination_path).with_context(|| {
                format!(
                    "copy {} to {}",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn write_metadata(path: &Path, spec: PlatformSpec) -> Result<()> {
    let metadata = serde_json::json!({
        "packageName": spec.package_name,
        "os": spec.os,
        "cpu": spec.cpu,
        "binaryName": spec.binary_name,
    });
    let mut bytes = serde_json::to_vec_pretty(&metadata)?;
    bytes.push(b'\n');
    fs::write(path, bytes)?;
    Ok(())
}

/// Stage one host artifact for archive packaging.
pub(crate) fn stage_host(
    repo: &Path,
    output_root: Option<&Path>,
    expected: Option<&str>,
) -> Result<PathBuf> {
    let platform = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let spec = spec_for_host(platform, arch)?;
    if let Some(expected) = expected {
        ensure!(
            expected == spec.package_name,
            "host artifact is {}, workflow expected {expected}",
            spec.package_name
        );
    }
    let output_root = output_root
        .map(Path::to_owned)
        .unwrap_or_else(|| repo.join("dist/release/artifacts"));
    let output_dir = output_root.join(spec.package_name);
    if output_dir.exists() {
        fs::remove_dir_all(&output_dir)?;
    }
    fs::create_dir_all(&output_dir)?;
    let binary_candidates = [
        repo.join("dist").join(spec.binary_name),
        repo.join("target/release").join(spec.binary_name),
    ];
    let binary = binary_candidates
        .iter()
        .find(|path| path.is_file())
        .with_context(|| {
            format!(
                "missing compiled Workdeck binary; checked {} and {}",
                binary_candidates[0].display(),
                binary_candidates[1].display()
            )
        })?;
    fs::copy(binary, output_dir.join(spec.binary_name))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            output_dir.join(spec.binary_name),
            fs::Permissions::from_mode(0o755),
        )?;
    }
    let skills_source = repo.join("skills");
    ensure!(
        skills_source.is_dir(),
        "missing bundled skills directory: {}",
        skills_source.display()
    );
    let skills_destination = output_dir.join("skills");
    for name in workdeck_core::BUNDLED_SKILL_NAMES {
        let source = skills_source.join(name);
        ensure!(
            source.join("SKILL.md").is_file(),
            "missing bundled Workdeck skill: {}",
            source.display()
        );
        copy_tree(&source, &skills_destination.join(name))?;
    }
    write_metadata(&output_dir.join("metadata.json"), spec)?;
    if let Ok(license) = fs::read(repo.join("LICENSE")) {
        fs::write(output_dir.join("LICENSE"), license)?;
    }
    Ok(output_dir)
}

fn metadata(path: &Path) -> Result<(PlatformSpec, ArtifactMetadata)> {
    let bytes = fs::read(path.join("metadata.json"))
        .with_context(|| format!("read artifact metadata in {}", path.display()))?;
    let metadata: ArtifactMetadata = serde_json::from_slice(&bytes)
        .with_context(|| format!("decode artifact metadata in {}", path.display()))?;
    let spec = spec_by_name(&metadata.package_name)?;
    ensure!(
        metadata.os == spec.os,
        "artifact {} has OS {}, expected {}",
        path.display(),
        metadata.os,
        spec.os
    );
    ensure!(
        metadata.cpu == spec.cpu,
        "artifact {} has CPU {}, expected {}",
        path.display(),
        metadata.cpu,
        spec.cpu
    );
    ensure!(
        metadata.binary_name == spec.binary_name,
        "artifact {} has binary {}, expected {}",
        path.display(),
        metadata.binary_name,
        spec.binary_name
    );
    Ok((spec, metadata))
}

/// Validate staged native artifacts without invoking a package manager.
pub(crate) fn check(root: &Path) -> Result<Vec<String>> {
    ensure!(
        root.is_dir(),
        "missing staged Workdeck artifact directory: {}",
        root.display()
    );
    let mut names = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let directory = entry.path();
        let (spec, _) = metadata(&directory)?;
        let binary = directory.join(spec.binary_name);
        ensure!(
            binary.is_file(),
            "artifact {} is missing {}",
            directory.display(),
            spec.binary_name
        );
        ensure!(
            directory.join("skills").is_dir(),
            "artifact {} is missing bundled skills",
            directory.display()
        );
        for name in workdeck_core::BUNDLED_SKILL_NAMES {
            ensure!(
                directory
                    .join("skills")
                    .join(name)
                    .join("SKILL.md")
                    .is_file(),
                "artifact {} is missing skill {name}",
                directory.display()
            );
        }
        names.push(spec.package_name.to_owned());
    }
    ensure!(
        !names.is_empty(),
        "no staged Workdeck artifacts found in {}",
        root.display()
    );
    names.sort();
    names.dedup();
    Ok(names)
}

/// Native replacement for package publication.  Workdeck deliberately ships
/// signed Cargo/GitHub archives and never publishes a package-manager module.
pub(crate) fn publish(args: impl Iterator<Item = String>) -> Result<()> {
    let mut dry_run = false;
    let mut tag = "latest".to_owned();
    let mut args = args;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dry-run" => dry_run = true,
            "--tag" => {
                tag = args
                    .next()
                    .filter(|v| !v.is_empty())
                    .context("Missing value for --tag")?
            }
            _ => bail!("Unknown native release publication argument: {arg}"),
        }
    }
    bail!(
        "Workdeck does not publish package-manager artifacts (requested tag {tag}, dry-run={dry_run}); use cargo xtask release package and signed GitHub release publication"
    )
}

pub(crate) fn stage(args: impl Iterator<Item = String>) -> Result<()> {
    let mut artifact_root = None;
    let mut output_root = None;
    let mut args = args;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--artifact-root" => {
                artifact_root = Some(PathBuf::from(
                    args.next().context("Missing value for --artifact-root")?,
                ))
            }
            "--output-root" => {
                output_root = Some(PathBuf::from(
                    args.next().context("Missing value for --output-root")?,
                ))
            }
            _ => bail!("Unknown native artifact staging argument: {arg}"),
        }
    }
    if let Some(root) = artifact_root {
        ensure!(
            root.is_dir(),
            "missing artifact input directory: {}",
            root.display()
        );
        let output = output_root.unwrap_or_else(|| PathBuf::from("dist/release/artifacts"));
        if output.exists() {
            fs::remove_dir_all(&output)?;
        }
        fs::create_dir_all(&output)?;
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let (spec, _) = metadata(&entry.path())?;
            copy_tree(&entry.path(), &output.join(spec.package_name))?;
        }
        let names = check(&output)?;
        println!("staged native Workdeck artifacts: {}", names.join(", "));
    } else {
        let repo = crate::repo_root()?;
        let output = stage_host(&repo, output_root.as_deref(), None)?;
        println!("staged {}", output.display());
    }
    Ok(())
}

pub(crate) fn build(repo: &Path) -> Result<()> {
    let target = crate::release_targets::compile_target_for_host(
        match std::env::consts::OS {
            "macos" => "darwin",
            value => value,
        },
        match std::env::consts::ARCH {
            "aarch64" => "arm64",
            "x86_64" => "x64",
            value => value,
        },
        false,
    );
    let mut args = vec![
        "build",
        "--locked",
        "--release",
        "-p",
        "workdeck-cli",
        "--bin",
        "workdeck",
    ];
    if let Some(target) = target {
        args.extend(["--target", target]);
    }
    crate::run_checked(repo, "cargo", &args)?;
    let output = repo.join("dist");
    fs::create_dir_all(&output)?;
    let name = if cfg!(windows) {
        "workdeck.exe"
    } else {
        "workdeck"
    };
    let built = if let Some(target) = target {
        repo.join("target").join(target).join("release").join(name)
    } else {
        repo.join("target/release").join(name)
    };
    ensure!(
        built.is_file(),
        "cargo build completed without {}",
        built.display()
    );
    fs::copy(&built, output.join(name))?;
    println!("built {}", output.join(name).display());
    Ok(())
}

/// Verify both pinned source blobs and the native artifact surface.
pub(crate) fn verify(repo: &Path, baseline: &str) -> Result<()> {
    ensure!(
        baseline == BASELINE,
        "release-artifact verifier received unexpected baseline {baseline}"
    );
    for (path, bytes, lines, sha, stable_bytes, stable_lines, stable_sha, blob) in SOURCE_BLOBS {
        let source = crate::git_stdout_bytes(repo, ["show", &format!("{BASELINE}:{path}")])?;
        ensure!(
            source.len() == *bytes,
            "pinned {path} changed size: {} != {bytes}",
            source.len()
        );
        ensure!(
            source.split(|byte| *byte == b'\n').count() == *lines + 1,
            "pinned {path} changed line count"
        );
        let actual = format!("{:x}", Sha256::digest(&source));
        ensure!(actual == *sha, "pinned {path} digest changed");
        // Git's blob id is kept in the ledger and checked by the strict audit;
        // retaining it here makes the verifier fail loudly if a source pin is edited.
        ensure!(!blob.is_empty(), "pinned {path} has no blob id");
        let stable = crate::git_stdout_bytes(repo, ["show", &format!("{STABLE}:{path}")])?;
        ensure!(
            stable.len() == *stable_bytes,
            "stable {path} changed size: {} != {stable_bytes}",
            stable.len()
        );
        ensure!(
            stable.split(|byte| *byte == b'\n').count() == *stable_lines + 1,
            "stable {path} changed line count"
        );
        ensure!(
            format!("{:x}", Sha256::digest(&stable)) == *stable_sha,
            "stable {path} digest changed"
        );
    }
    ensure!(PLATFORM_MATRIX.len() == 5, "native release matrix changed");
    ensure!(spec_for_host("darwin", "x64")?.binary_name == "workdeck");
    ensure!(spec_for_host("linux", "arm64")?.package_name == "workdeck-linux-arm64");
    ensure!(spec_for_host("win32", "x64")?.binary_name == "workdeck.exe");
    ensure!(spec_for_host("freebsd", "x64").is_err());
    for (path, markers) in [
        (
            "xtask/src/release_artifacts.rs",
            &[
                "PLATFORM_MATRIX",
                "stage_host",
                "does not publish package-manager artifacts",
            ] as &[&str],
        ),
        (
            "crates/workdeck-cli/src/install/metadata.rs",
            &["PrebuiltMetadata", "workdeck"],
        ),
        (
            "docs/native-release-policy.md",
            &["signed", "workdeck", "Cargo"],
        ),
    ] {
        let contents = fs::read_to_string(repo.join(path))
            .with_context(|| format!("read native artifact surface {path}"))?;
        for marker in markers {
            ensure!(
                contents.contains(marker),
                "native artifact surface {path} is missing {marker:?}"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_matrix_normalizes_hosts_and_keeps_one_workdeck_binary() {
        assert_eq!(
            spec_for_host("darwin", "x64").unwrap().package_name,
            "workdeck-darwin-x64"
        );
        assert_eq!(spec_for_host("macos", "aarch64").unwrap().cpu, "arm64");
        assert_eq!(
            spec_for_host("linux", "x86_64").unwrap().binary_name,
            "workdeck"
        );
        assert_eq!(
            spec_for_host("windows", "x64").unwrap().binary_name,
            "workdeck.exe"
        );
        assert!(spec_for_host("freebsd", "x64").is_err());
        assert!(
            PLATFORM_MATRIX
                .iter()
                .all(|spec| spec.package_name.starts_with("workdeck-"))
        );
    }

    #[test]
    fn host_staging_copies_binary_skills_metadata_and_license_without_user_state() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        fs::create_dir_all(repo.join("dist")).unwrap();
        fs::write(repo.join("dist/workdeck"), b"binary").unwrap();
        fs::write(repo.join("LICENSE"), b"MIT\n").unwrap();
        for name in workdeck_core::BUNDLED_SKILL_NAMES {
            fs::create_dir_all(repo.join("skills").join(name)).unwrap();
            fs::write(repo.join("skills").join(name).join("SKILL.md"), name).unwrap();
        }
        let expected = spec_for_host(std::env::consts::OS, std::env::consts::ARCH)
            .unwrap()
            .package_name;
        let output =
            stage_host(&repo, Some(&root.path().join("artifacts")), Some(expected)).unwrap();
        assert!(
            output
                .join(if cfg!(windows) {
                    "workdeck.exe"
                } else {
                    "workdeck"
                })
                .is_file()
        );
        assert!(output.join("LICENSE").is_file());
        assert!(output.join("metadata.json").is_file());
        for name in workdeck_core::BUNDLED_SKILL_NAMES {
            assert!(output.join("skills").join(name).join("SKILL.md").is_file());
        }
        assert!(!repo.join(".agents").exists());
        let names = check(output.parent().unwrap()).unwrap();
        assert_eq!(names, [expected]);
    }

    #[test]
    fn artifact_validation_rejects_tampered_metadata_and_missing_payloads() {
        let root = tempfile::tempdir().unwrap();
        let artifact = root.path().join("workdeck-linux-x64");
        fs::create_dir_all(artifact.join("skills/workdeck-review")).unwrap();
        fs::write(artifact.join("workdeck"), b"binary").unwrap();
        fs::write(artifact.join("skills/workdeck-review/SKILL.md"), b"review").unwrap();
        write_metadata(
            &artifact.join("metadata.json"),
            spec_by_name("workdeck-linux-x64").unwrap(),
        )
        .unwrap();
        assert!(check(root.path()).is_err());
        fs::write(artifact.join("metadata.json"), br#"{"packageName":"workdeck-linux-x64","os":"darwin","cpu":"x64","binaryName":"workdeck"}"#).unwrap();
        assert!(check(root.path()).is_err());
    }

    #[test]
    fn publication_is_explicitly_blocked_and_build_rejects_package_manager_shape() {
        let error = publish(
            ["--dry-run", "--tag", "beta"]
                .into_iter()
                .map(str::to_owned),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("does not publish package-manager artifacts"));
        assert!(error.contains("beta"));
    }

    #[test]
    fn native_release_build_targets_the_single_workdeck_binary() {
        let metadata = cargo_metadata::MetadataCommand::new()
            .no_deps()
            .exec()
            .unwrap();
        let package = metadata
            .packages
            .iter()
            .find(|package| package.name == "workdeck-cli")
            .unwrap();
        assert!(package.targets.iter().any(|target| {
            target.name == "workdeck" && target.kind.contains(&cargo_metadata::TargetKind::Bin)
        }));
        assert!(
            !package
                .targets
                .iter()
                .any(|target| target.name.contains("hunk"))
        );
    }

    #[test]
    fn pinned_artifact_sources_are_still_read_from_git_and_match_both_anchors() {
        let repo = crate::repo_root().unwrap();
        verify(&repo, BASELINE).unwrap();
    }
}
