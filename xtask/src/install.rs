//! Incremental MIT translation of Hunk install.sh. Preflight only; no installation writes.

use anyhow::{Result, bail};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn expected_checksum(contents: &str, archive_name: &str) -> Result<String> {
    let mut selected = None;
    for line in contents.lines() {
        let mut fields = line.split_ascii_whitespace();
        let Some(hash) = fields.next() else {
            continue;
        };
        let Some(name) = fields.next() else {
            continue;
        };
        if name.strip_prefix('*').unwrap_or(name) != archive_name {
            continue;
        }
        if fields.next().is_some()
            || hash.len() != 64
            || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            bail!("Malformed checksum entry for {archive_name}");
        }
        if selected.replace(hash.to_ascii_lowercase()).is_some() {
            bail!("Duplicate checksum entry for {archive_name}");
        }
    }
    selected.ok_or_else(|| {
        anyhow::anyhow!(
            "Checksum file has no entry for {archive_name}; refusing an unverified archive"
        )
    })
}

pub(super) fn verify(mut args: impl Iterator<Item = String>) -> Result<()> {
    let archive = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("install-verify requires ARCHIVE CHECKSUM_FILE"))?;
    let checksums = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("install-verify requires ARCHIVE CHECKSUM_FILE"))?;
    if args.next().is_some() {
        bail!("install-verify accepts exactly ARCHIVE CHECKSUM_FILE");
    }
    let archive = Path::new(&archive);
    let name = archive
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("Archive name is not valid UTF-8"))?;
    let expected = expected_checksum(&std::fs::read_to_string(checksums)?, name)?;
    let actual = super::sha256_file(archive)?;
    if actual != expected {
        bail!("Checksum verification failed for {name}; refusing a corrupted or tampered archive");
    }
    println!(
        "{}",
        serde_json::to_string(
            &serde_json::json!({"archive": archive, "sha256": actual, "checksumVerified": true, "signatureVerified": false, "installed": false})
        )?
    );
    Ok(())
}

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
    executable_access: Option<bool>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ConflictDecision {
    NoObservedExecutableConflicts,
    RequiresForce,
    ExplicitlyAllowed,
    UnresolvedAccess,
}

fn conflict_decision(
    observations: &[PathFileObservation],
    allow_conflicts: bool,
) -> ConflictDecision {
    if observations
        .iter()
        .any(|item| item.executable_access.is_none())
    {
        return ConflictDecision::UnresolvedAccess;
    }
    if !observations
        .iter()
        .any(|item| item.executable_access == Some(true))
    {
        return ConflictDecision::NoObservedExecutableConflicts;
    }
    if allow_conflicts {
        ConflictDecision::ExplicitlyAllowed
    } else {
        ConflictDecision::RequiresForce
    }
}

fn executable_access(path: &Path) -> Option<bool> {
    #[cfg(unix)]
    {
        use rustix::fs::{Access, AtFlags, CWD, accessat};
        match std::fs::metadata(path) {
            Ok(metadata) if !metadata.is_file() => return Some(false),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(false),
            Err(_) => return None,
            _ => {}
        }
        classify_execute_access(accessat(CWD, path, Access::EXEC_OK, AtFlags::EACCESS))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        // File extension alone is not proof of native executable access on Windows.
        None
    }
}

#[cfg(unix)]
fn classify_execute_access(result: Result<(), rustix::io::Errno>) -> Option<bool> {
    use rustix::io::Errno;
    match result {
        Ok(()) => Some(true),
        Err(Errno::ACCESS | Errno::PERM | Errno::NOENT | Errno::NOTDIR) => Some(false),
        Err(_) => None,
    }
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

#[cfg(test)]
fn observe_path_files(
    target: &Path,
    entries: &[PathBuf],
    executable: &str,
) -> Vec<PathFileObservation> {
    observe_candidates(
        target,
        entries,
        executable,
        entries.iter().map(|entry| entry.join(executable)),
    )
}

fn inactive_mise_candidates(home: &Path, executable: &str) -> std::io::Result<Vec<PathBuf>> {
    let root = home.join(".local/share/mise/installs/workdeck");
    let listing = match std::fs::read_dir(&root) {
        Ok(listing) => listing,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut versions = listing
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    versions.sort();
    // Source glob order: all version-root binaries, followed by all version/bin binaries.
    Ok(versions
        .iter()
        .map(|version| version.join(executable))
        .chain(
            versions
                .iter()
                .map(|version| version.join("bin").join(executable)),
        )
        .collect())
}

fn observe_candidates(
    target: &Path,
    entries: &[PathBuf],
    executable: &str,
    candidates: impl IntoIterator<Item = PathBuf>,
) -> Vec<PathFileObservation> {
    let target_identity = executable_identity(target);
    let mut observations: Vec<PathFileObservation> = Vec::new();
    let mut positions = BTreeMap::new();
    for path in candidates {
        match std::fs::metadata(&path) {
            Ok(metadata) if !metadata.is_file() => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            // A metadata error is not proof that no competing executable exists.
            _ => {}
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
            executable_access: executable_access(&path),
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
        .unwrap_or_else(|| PathBuf::from(&home).join(".workdeck/bin"));
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
    let inactive = inactive_mise_candidates(Path::new(&home), executable)?;
    let existing_path_files = observe_candidates(
        &target,
        &entries,
        executable,
        entries
            .iter()
            .map(|entry| entry.join(executable))
            .chain(inactive),
    );
    let conflict_decision = conflict_decision(&existing_path_files, options.allow_conflicts);
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
        "options": options, "os": os, "arch": arch, "executionAvailable": false,
        "targetBinary": target, "targetIdentity": target_identity, "existingInstallFiles": existing_path_files,
        "observedConflictDecision": conflict_decision,
            "remaining": ["release resolution", "competing installs", "verified archive extraction", "atomic installation", "shell profile updates"]
        }))?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_selection_requires_exact_unique_valid_archive_entry() {
        let hash = "ab".repeat(32);
        assert_eq!(
            expected_checksum(&format!("{hash}  workdeck.tar.gz\n"), "workdeck.tar.gz").unwrap(),
            hash
        );
        assert_eq!(
            expected_checksum(
                &format!("{} *workdeck.tar.gz\r\n", hash.to_uppercase()),
                "workdeck.tar.gz"
            )
            .unwrap(),
            hash
        );
        for text in [
            format!("{hash} workdeck.tar.gz.extra"),
            "bad workdeck.tar.gz".into(),
            format!("{hash} workdeck.tar.gz extra"),
            format!("{hash} workdeck.tar.gz\n{hash} workdeck.tar.gz"),
        ] {
            assert!(expected_checksum(&text, "workdeck.tar.gz").is_err());
        }
    }

    #[test]
    fn native_archive_checksum_verification_rejects_corruption_without_installing() {
        let directory = tempfile::tempdir().unwrap();
        let archive = directory.path().join("workdeck.tar.gz");
        let checksums = directory.path().join("SHA256SUMS");
        std::fs::write(&archive, b"fixture archive bytes").unwrap();
        let hash = super::super::sha256_file(&archive).unwrap();
        std::fs::write(&checksums, format!("{hash}  workdeck.tar.gz\n")).unwrap();
        let args = || {
            [
                archive.to_string_lossy().into_owned(),
                checksums.to_string_lossy().into_owned(),
            ]
            .into_iter()
        };
        verify(args()).unwrap();
        std::fs::write(&archive, b"corrupt archive bytes").unwrap();
        assert!(verify(args()).is_err());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 2);
        assert!(verify(std::iter::empty()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn access_probe_errors_remain_unknown_unless_the_os_confirmed_denial_or_absence() {
        use rustix::io::Errno;
        assert_eq!(classify_execute_access(Ok(())), Some(true));
        for error in [Errno::ACCESS, Errno::PERM, Errno::NOENT, Errno::NOTDIR] {
            assert_eq!(classify_execute_access(Err(error)), Some(false));
        }
        for error in [Errno::NOSYS, Errno::IO, Errno::INTR] {
            assert_eq!(classify_execute_access(Err(error)), None);
        }
    }

    #[cfg(unix)]
    #[test]
    fn inaccessible_metadata_is_retained_as_unresolved_not_dropped_from_discovery() {
        let directory = tempfile::tempdir().unwrap();
        let candidate = directory.path().join("workdeck");
        std::os::unix::fs::symlink("workdeck", &candidate).unwrap();
        let observations = observe_path_files(
            &directory.path().join("target/workdeck"),
            &[directory.path().to_owned()],
            "workdeck",
        );
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].executable_access, None);
        assert_eq!(
            conflict_decision(&observations, false),
            ConflictDecision::UnresolvedAccess
        );
        assert_eq!(
            conflict_decision(&observations, true),
            ConflictDecision::UnresolvedAccess
        );
    }

    #[test]
    fn conflict_decisions_preserve_force_and_do_not_waive_unknown_access() {
        let observation = |access| PathFileObservation {
            path: "other/workdeck".into(),
            identity: "other/workdeck".into(),
            aliases: vec!["other/workdeck".into()],
            shadowing: Shadowing::NotOnPath,
            manager_hint: None,
            diagnostic_path: "other/workdeck".into(),
            executable_access: access,
        };
        assert_eq!(
            conflict_decision(&[], false),
            ConflictDecision::NoObservedExecutableConflicts
        );
        assert_eq!(
            conflict_decision(&[observation(Some(false))], false),
            ConflictDecision::NoObservedExecutableConflicts
        );
        assert_eq!(
            conflict_decision(&[observation(Some(true))], false),
            ConflictDecision::RequiresForce
        );
        assert_eq!(
            conflict_decision(&[observation(Some(true))], true),
            ConflictDecision::ExplicitlyAllowed
        );
        for allow in [false, true] {
            assert_eq!(
                conflict_decision(&[observation(None)], allow),
                ConflictDecision::UnresolvedAccess
            );
            assert_eq!(
                conflict_decision(&[observation(Some(true)), observation(None)], allow),
                ConflictDecision::UnresolvedAccess
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn executable_access_uses_os_permissions_without_running_file_contents() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join("workdeck");
        std::fs::write(&binary, b"not an executable format; must never run").unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(executable_access(&binary), Some(false));
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(executable_access(&binary), Some(true));
        let alias = directory.path().join("alias");
        symlink(&binary, &alias).unwrap();
        assert_eq!(executable_access(&alias), Some(true));
        assert_eq!(executable_access(directory.path()), Some(false));
        assert_eq!(
            executable_access(&directory.path().join("missing")),
            Some(false)
        );
    }

    #[test]
    fn inactive_mise_scan_is_bounded_ordered_and_marks_absent_path_candidates() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path();
        assert!(
            inactive_mise_candidates(home, "workdeck")
                .unwrap()
                .is_empty()
        );
        assert_eq!(std::fs::read_dir(home).unwrap().count(), 0);
        let root = home.join(".local/share/mise/installs/workdeck");
        for version in ["2", "1"] {
            let bin = root.join(version).join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            std::fs::write(root.join(version).join("workdeck"), b"not executed").unwrap();
            std::fs::write(bin.join("workdeck"), b"not executed").unwrap();
        }
        let candidates = inactive_mise_candidates(home, "workdeck").unwrap();
        assert_eq!(
            candidates,
            [
                root.join("1/workdeck"),
                root.join("2/workdeck"),
                root.join("1/bin/workdeck"),
                root.join("2/bin/workdeck")
            ]
        );
        let observations =
            observe_candidates(&home.join("target/workdeck"), &[], "workdeck", candidates);
        assert_eq!(observations.len(), 4);
        assert!(observations.iter().all(
            |item| item.shadowing == Shadowing::NotOnPath && item.manager_hint == Some("mise")
        ));
    }

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
