//! Incremental MIT translation of Hunk install.sh. Preflight only; no installation writes.

use anyhow::{Result, bail};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Resolve existing parent directories and at most eight executable symlink hops, matching
/// the source installer even when the final binary has not been installed yet.
fn canonical_executable_path(path: &Path) -> std::io::Result<PathBuf> {
    let mut path = path.to_owned();
    for depth in 0..=8 {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let physical = std::fs::canonicalize(parent)?;
        let name = path
            .file_name()
            .ok_or_else(|| std::io::Error::other("executable path has no file name"))?;
        let candidate = physical.join(name);
        if depth < 8
            && let Ok(target) = std::fs::read_link(&candidate)
        {
            path = if target.is_absolute() {
                target
            } else {
                physical.join(target)
            };
            continue;
        }
        return Ok(candidate);
    }
    unreachable!()
}

fn executable_identity(path: &Path) -> PathBuf {
    canonical_executable_path(path).unwrap_or_else(|_| path.to_owned())
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Shadowing {
    NotOnPath,
    ShadowsTarget,
    ShadowedByTarget,
}

fn shadowing(candidate: &Path, target: &Path, entries: &[PathBuf], executable: &str) -> Shadowing {
    let candidate = executable_identity(candidate);
    let target = executable_identity(target);
    let identities: Vec<_> = entries
        .iter()
        .map(|entry| {
            let directory = if entry.as_os_str().is_empty() {
                Path::new(".")
            } else {
                entry
            };
            executable_identity(&directory.join(executable))
        })
        .collect();
    match (
        identities.iter().position(|path| path == &candidate),
        identities.iter().position(|path| path == &target),
    ) {
        (None, _) => Shadowing::NotOnPath,
        (Some(_), None) => Shadowing::ShadowsTarget,
        (Some(candidate), Some(target)) if candidate < target => Shadowing::ShadowsTarget,
        _ => Shadowing::ShadowedByTarget,
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Options {
    version: String,
    no_modify_path: bool,
    allow_conflicts: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PathFileObservation {
    path: PathBuf,
    identity: PathBuf,
    aliases: Vec<PathBuf>,
    shadowing: Shadowing,
    manager_hint: Option<&'static str>,
    diagnostic_path: PathBuf,
}

fn manager_hint(path: &Path) -> Option<&'static str> {
    let path = path.to_string_lossy().replace('\\', "/");
    let segments: Vec<_> = path.split('/').filter(|part| !part.is_empty()).collect();
    if !matches!(segments.last().copied(), Some("workdeck" | "workdeck.exe")) {
        return None;
    }
    let adjacent = |first, second| segments.windows(2).any(|parts| parts == [first, second]);
    if adjacent(".cargo", "bin") {
        return Some("Cargo");
    }
    if path.starts_with("/nix/store/")
        || adjacent(".nix-profile", "bin")
        || adjacent("profiles", "per-user")
    {
        return Some("Nix");
    }
    if segments.contains(&"Cellar")
        || path.starts_with("/opt/homebrew/")
        || path.starts_with("/home/linuxbrew/.linuxbrew/")
    {
        return Some("Homebrew");
    }
    if path == "/usr/local/bin/workdeck" {
        return Some("Homebrew or another package manager");
    }
    if adjacent("mise", "installs") {
        return Some("mise");
    }
    if adjacent(".workdeck", "bin") {
        return Some("Workdeck standalone installer");
    }
    None
}

fn observe_path_files(
    target: &Path,
    entries: &[PathBuf],
    executable: &str,
) -> Vec<PathFileObservation> {
    let target_identity = executable_identity(target);
    let mut observations: Vec<PathFileObservation> = Vec::new();
    let mut positions = BTreeMap::new();
    for entry in entries {
        let directory = if entry.as_os_str().is_empty() {
            Path::new(".")
        } else {
            entry
        };
        let path = directory.join(executable);
        if !path.is_file() {
            continue;
        }
        let identity = executable_identity(&path);
        if identity == target_identity {
            continue;
        }
        if let Some(index) = positions.get(&identity).copied() {
            let observation: &mut PathFileObservation = &mut observations[index];
            if !observation.aliases.contains(&path) {
                observation.aliases.push(path);
            }
            continue;
        }
        positions.insert(identity.clone(), observations.len());
        observations.push(PathFileObservation {
            shadowing: shadowing(&path, target, entries, executable),
            aliases: vec![path.clone()],
            diagnostic_path: path.clone(),
            manager_hint: None,
            path,
            identity,
        });
    }
    // Preserve discovery order for PATH behavior, but prefer the first recognized alias for
    // ownership diagnostics, as the source installer does. Hints are not verified provenance.
    for observation in &mut observations {
        if let Some((path, owner)) = observation
            .aliases
            .iter()
            .find_map(|path| manager_hint(path).map(|owner| (path, owner)))
        {
            observation.diagnostic_path = path.clone();
            observation.manager_hint = Some(owner);
        }
    }
    observations
}

fn options(
    args: impl Iterator<Item = String>,
    env: &BTreeMap<String, String>,
) -> Result<Option<Options>> {
    let mut options = Options {
        version: env.get("WORKDECK_VERSION").cloned().unwrap_or_default(),
        no_modify_path: env
            .get("WORKDECK_NO_MODIFY_PATH")
            .is_some_and(|value| value == "1"),
        allow_conflicts: env
            .get("WORKDECK_ALLOW_CONFLICTING_INSTALLS")
            .is_some_and(|value| value == "1"),
    };
    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "--no-modify-path" => options.no_modify_path = true,
            "-f" | "--force" => options.allow_conflicts = true,
            arg if arg.starts_with('-') => {
                bail!("Unknown option: {arg} (run with --help to see the supported options)")
            }
            _ => options.version = arg,
        }
    }
    options.version = options
        .version
        .strip_prefix('v')
        .unwrap_or(&options.version)
        .to_owned();
    Ok(Some(options))
}

fn platform(os: &str, arch: &str, translated: bool) -> Result<(&'static str, &'static str)> {
    let os = match os {
        "Darwin" | "macos" => "darwin",
        "Linux" | "linux" => "linux",
        "Windows" | "windows" => "windows",
        _ => bail!("Unsupported operating system: {os}. No native Workdeck archive is available."),
    };
    let arch = match arch {
        "x86_64" | "amd64" if os == "darwin" && translated => "arm64",
        "x86_64" | "amd64" => "x64",
        "arm64" | "aarch64" if os != "windows" => "arm64",
        _ => bail!("Unsupported architecture: {arch}. No native Workdeck archive is available."),
    };
    Ok((os, arch))
}

pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let env = [
        "WORKDECK_VERSION",
        "WORKDECK_NO_MODIFY_PATH",
        "WORKDECK_ALLOW_CONFLICTING_INSTALLS",
    ]
    .into_iter()
    .filter_map(|key| std::env::var(key).ok().map(|value| (key.to_owned(), value)))
    .collect();
    let Some(options) = options(args, &env)? else {
        println!(
            "Read-only Workdeck installer preflight\nUsage: cargo xtask install-plan [version] [--no-modify-path] [-f|--force]\nNo downloads, shell-profile edits, or installation writes are performed."
        );
        return Ok(());
    };
    let translated = cfg!(target_os = "macos")
        && std::process::Command::new("sysctl")
            .args(["-n", "sysctl.proc_translated"])
            .output()
            .ok()
            .is_some_and(|output| {
                output.status.success()
                    && output.stdout.strip_suffix(b"\n").unwrap_or(&output.stdout) == b"1"
            });
    let (os, arch) = platform(std::env::consts::OS, std::env::consts::ARCH, translated)?;
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("No home directory is available for installer preflight"))?;
    let bin = std::env::var_os("WORKDECK_INSTALL_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(home).join(".workdeck/bin"));
    let executable = if os == "windows" {
        "workdeck.exe"
    } else {
        "workdeck"
    };
    let target = bin.join(executable);
    let target_identity = executable_identity(&target);
    let entries: Vec<_> =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
    // These are file observations, not completed executable/manager conflict classification.
    let existing_path_files = observe_path_files(&target, &entries, executable);
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
        "options": options, "os": os, "arch": arch, "executionAvailable": false,
        "targetBinary": target, "targetIdentity": target_identity, "existingPathFiles": existing_path_files,
            "remaining": ["release resolution", "competing installs", "verified archive extraction", "atomic installation", "shell profile updates"]
        }))?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manager_hints_are_layout_inferences_not_generic_substring_matches() {
        for (path, owner) in [
            ("/users/test/.cargo/bin/workdeck", "Cargo"),
            ("C:\\Users\\test\\.cargo\\bin\\workdeck.exe", "Cargo"),
            ("/opt/homebrew/bin/workdeck", "Homebrew"),
            (
                "/usr/local/bin/workdeck",
                "Homebrew or another package manager",
            ),
            ("/nix/store/hash-workdeck/bin/workdeck", "Nix"),
            ("/users/test/.nix-profile/bin/workdeck", "Nix"),
            (
                "/users/test/.local/share/mise/installs/workdeck/1/bin/workdeck",
                "mise",
            ),
        ] {
            assert_eq!(manager_hint(Path::new(path)), Some(owner), "{path}");
        }
        for path in [
            "/tmp/cargo/bin/workdeck",
            "/tmp/myCellar/workdeck",
            "/tmp/.cargo/bin/not-workdeck",
            "/tmp/workdeck",
        ] {
            assert_eq!(manager_hint(Path::new(path)), None, "{path}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn manager_shaped_alias_is_preferred_without_changing_first_path_or_identity() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let opaque = root.join("opaque");
        let managed = root.join(".cargo/bin");
        std::fs::create_dir(&opaque).unwrap();
        std::fs::create_dir_all(&managed).unwrap();
        std::fs::write(opaque.join("workdeck"), b"not executed").unwrap();
        std::os::unix::fs::symlink(opaque.join("workdeck"), managed.join("workdeck")).unwrap();
        let observations = observe_path_files(
            &root.join("destination/workdeck"),
            &[opaque.clone(), managed.clone()],
            "workdeck",
        );
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].path, opaque.join("workdeck"));
        assert_eq!(observations[0].identity, opaque.join("workdeck"));
        assert_eq!(observations[0].diagnostic_path, managed.join("workdeck"));
        assert_eq!(observations[0].manager_hint, Some("Cargo"));
    }

    #[cfg(unix)]
    #[test]
    fn path_observation_deduplicates_identity_but_retains_aliases_and_order() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let owner = root.join("owner");
        let alias = root.join("alias");
        let destination = root.join("destination");
        for path in [&owner, &alias, &destination] {
            std::fs::create_dir(path).unwrap();
        }
        std::fs::write(owner.join("workdeck"), b"not executed").unwrap();
        std::os::unix::fs::symlink(owner.join("workdeck"), alias.join("workdeck")).unwrap();
        let target = destination.join("workdeck");
        std::fs::write(&target, b"target unchanged").unwrap();
        let entries = vec![
            alias.clone(),
            destination.clone(),
            owner.clone(),
            alias.clone(),
        ];
        let observations = observe_path_files(&target, &entries, "workdeck");
        assert_eq!(observations.len(), 1);
        let observation = &observations[0];
        assert_eq!(observation.path, alias.join("workdeck"));
        assert_eq!(observation.identity, owner.join("workdeck"));
        assert_eq!(
            observation.aliases,
            [alias.join("workdeck"), owner.join("workdeck")]
        );
        assert_eq!(observation.shadowing, Shadowing::ShadowsTarget);
        assert_eq!(std::fs::read(&target).unwrap(), b"target unchanged");
        assert_eq!(
            observe_path_files(&owner.join("workdeck"), &entries[..1], "workdeck").len(),
            0
        );
    }

    #[test]
    fn empty_path_entry_has_current_directory_identity_without_changing_process_cwd() {
        let cwd = std::env::current_dir().unwrap();
        let candidate = cwd.join("workdeck");
        assert_eq!(
            shadowing(
                &candidate,
                &cwd.join("other/workdeck"),
                &[PathBuf::new()],
                "workdeck"
            ),
            Shadowing::ShadowsTarget
        );
    }

    #[test]
    fn identity_handles_missing_binary_and_path_order_without_writes() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first");
        let second = directory.path().join("second");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        let target = first.join("workdeck");
        let candidate = second.join("workdeck");
        assert_eq!(
            canonical_executable_path(&target).unwrap(),
            first.canonicalize().unwrap().join("workdeck")
        );
        assert_eq!(
            shadowing(
                &candidate,
                &target,
                &[second.clone(), first.clone()],
                "workdeck"
            ),
            Shadowing::ShadowsTarget
        );
        assert_eq!(
            shadowing(
                &candidate,
                &target,
                &[first.clone(), second.clone()],
                "workdeck"
            ),
            Shadowing::ShadowedByTarget
        );
        assert_eq!(
            shadowing(&candidate, &target, &[first], "workdeck"),
            Shadowing::NotOnPath
        );
        assert_eq!(
            shadowing(&candidate, &target, &[second], "workdeck"),
            Shadowing::ShadowsTarget
        );
        assert!(!target.exists() && !candidate.exists());
    }

    #[cfg(unix)]
    #[test]
    fn executable_symlinks_resolve_relative_absolute_and_stop_at_eight_hops() {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        symlink("missing-workdeck", root.join("relative")).unwrap();
        symlink(root.join("relative"), root.join("absolute")).unwrap();
        assert_eq!(
            canonical_executable_path(&root.join("absolute")).unwrap(),
            root.join("missing-workdeck")
        );
        for index in 0..10 {
            symlink(
                format!("link{}", index + 1),
                root.join(format!("link{index}")),
            )
            .unwrap();
        }
        assert_eq!(
            canonical_executable_path(&root.join("link0")).unwrap(),
            root.join("link8")
        );
        symlink("loop", root.join("loop")).unwrap();
        assert_eq!(
            canonical_executable_path(&root.join("loop")).unwrap(),
            root.join("loop")
        );
    }

    #[test]
    fn installer_options_preserve_source_order_environment_and_single_prefix_removal() {
        let env = BTreeMap::from([
            ("WORKDECK_VERSION".into(), "v1.0".into()),
            ("WORKDECK_NO_MODIFY_PATH".into(), "true".into()),
        ]);
        let parsed = options(
            ["v2.0", "--force", "vv3.0", "--no-modify-path"]
                .map(str::to_owned)
                .into_iter(),
            &env,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            parsed,
            Options {
                version: "v3.0".into(),
                no_modify_path: true,
                allow_conflicts: true
            }
        );
        let defaults = options(std::iter::empty(), &env).unwrap().unwrap();
        assert_eq!(defaults.version, "1.0");
        assert!(!defaults.no_modify_path && !defaults.allow_conflicts);
        assert!(
            options(["--help", "--bad"].map(str::to_owned).into_iter(), &env)
                .unwrap()
                .is_none()
        );
        assert!(options(["--bad", "--help"].map(str::to_owned).into_iter(), &env).is_err());
        assert!(options(["--".into()].into_iter(), &env).is_err());
    }

    #[test]
    fn archive_platform_corrects_rosetta_without_changing_linux_or_windows() {
        for arch in ["amd64", "x86_64"] {
            assert_eq!(platform("Darwin", arch, true).unwrap(), ("darwin", "arm64"));
            assert_eq!(platform("Darwin", arch, false).unwrap(), ("darwin", "x64"));
            assert_eq!(platform("Linux", arch, true).unwrap(), ("linux", "x64"));
            assert_eq!(platform("Windows", arch, true).unwrap(), ("windows", "x64"));
        }
        for arch in ["arm64", "aarch64"] {
            assert_eq!(platform("Linux", arch, false).unwrap(), ("linux", "arm64"));
            assert!(platform("Windows", arch, false).is_err());
        }
        assert!(platform("FreeBSD", "x86_64", false).is_err());
        assert!(platform("Linux", "i686", false).is_err());
    }
}
