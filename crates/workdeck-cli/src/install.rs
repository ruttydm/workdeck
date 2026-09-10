//! Incremental MIT translation of Hunk install.sh. Preflight, private staging,
//! and a binary-replacement primitive; updater integration remains incomplete.

use anyhow::{Result, bail};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

mod assets;
pub mod attestation;
pub use assets::{install_authenticated_skills, install_skill_tree};
mod authenticated;
mod download;
mod fresh;
pub use fresh::{
    create_authenticated_installation, install_release, install_release_on_host,
    install_requested_on_host,
};
pub mod metadata;
mod release_identity;
pub mod shell_path;
pub use authenticated::{
    ReleaseIdentity, create_authenticated_archive, install_authenticated_archive,
};
pub use download::{DownloadedRelease, download_release};
pub use release_identity::{ResolvedRelease, resolve_release_identity};
mod staging;
mod transaction;
pub use staging::{prepare_authenticated_archive, prepare_verified_archive, stage};
pub use transaction::{create_binary, replace_binary_with_backup};

fn archive_entry_path(name: &str) -> Result<String> {
    let name = name.strip_suffix('/').unwrap_or(name);
    if name.is_empty() || name.contains(['\\', ':', '\0']) {
        bail!("Unsafe archive path: {name:?}");
    }
    for part in name.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.ends_with([' ', '.']) {
            bail!("Unsafe archive path: {name:?}");
        }
        let stem = part.split('.').next().unwrap_or(part).to_ascii_uppercase();
        if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        {
            bail!("Reserved archive path: {name:?}");
        }
    }
    Ok(name.to_owned())
}

fn verify_entry_bytes(reader: &mut impl std::io::Read, expected: u64) -> Result<()> {
    use std::io::Read;
    let limit = expected
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("Archive entry size overflow"))?;
    let copied = std::io::copy(&mut reader.take(limit), &mut std::io::sink())?;
    if copied != expected {
        bail!("Archive entry size mismatch");
    }
    Ok(())
}

#[cfg(test)]
fn inspect_archive(path: &Path) -> Result<(usize, u64)> {
    let (names, bytes) = inspect_archive_entries(path)?;
    Ok((names.len(), bytes))
}

fn verify_package_paths(names: &BTreeMap<String, bool>) -> Result<()> {
    let mut roots = std::collections::BTreeSet::new();
    for (name, directory) in names {
        if !directory && !name.contains('/') {
            bail!("Package file is outside its wrapper directory: {name}");
        }
        roots.insert(name.split('/').next().unwrap_or_default());
    }
    if roots.len() != 1 {
        bail!("Package must contain exactly one wrapper directory");
    }
    let root = roots.first().unwrap();
    let file = |name: &str| names.get(&format!("{root}/{name}")) == Some(&false);
    if usize::from(file("workdeck")) + usize::from(file("workdeck.exe")) != 1 {
        bail!("Package must contain exactly one Workdeck executable");
    }
    for required in [
        "LICENSE",
        "THIRD_PARTY_NOTICES",
        "licenses.json",
        "sbom.cdx.json",
        "provenance.json",
    ] {
        if !file(required) {
            bail!("Package is missing required regular file: {required}");
        }
    }
    Ok(())
}

fn inspect_archive_entries(path: &Path) -> Result<(BTreeMap<String, bool>, u64)> {
    validate_archive_input(&std::fs::metadata(path)?)?;
    inspect_archive_file(
        open_archive_input(path)?,
        path.extension().is_some_and(|extension| extension == "zip"),
    )
}

fn inspect_archive_file(file: std::fs::File, zip: bool) -> Result<(BTreeMap<String, bool>, u64)> {
    let mut names: BTreeMap<String, bool> = BTreeMap::new();
    let mut original_names = BTreeMap::new();
    let mut total = 0u64;
    let mut record = |name: &str, size: u64, directory: bool| -> Result<()> {
        let name = archive_entry_path(name)?;
        let key = name.to_lowercase();
        if names.contains_key(&key) {
            bail!("Duplicate archive path: {name}");
        }
        if directory && size != 0 {
            bail!("Archive directory has a payload: {name}");
        }
        for (offset, _) in key.match_indices('/') {
            if names.get(&key[..offset]) == Some(&false) {
                bail!("Archive path descends through a file: {name}");
            }
        }
        if !directory {
            let prefix = format!("{key}/");
            if names
                .range(prefix.clone()..)
                .next()
                .is_some_and(|(name, _)| name.starts_with(&prefix))
            {
                bail!("Archive file replaces a parent directory: {name}");
            }
        }
        names.insert(key, directory);
        original_names.insert(name, directory);
        total = total
            .checked_add(size)
            .ok_or_else(|| anyhow::anyhow!("Archive size overflow"))?;
        if names.len() > 100_000 || total > 2 * 1024 * 1024 * 1024 {
            bail!("Archive exceeds installation limits");
        }
        Ok(())
    };
    if zip {
        let mut archive = zip::ZipArchive::new(file)?;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            if entry
                .unix_mode()
                .is_some_and(|mode| !matches!(mode & 0o170000, 0 | 0o100000 | 0o040000))
            {
                bail!("Archive links and special files are not permitted");
            }
            record(entry.name(), entry.size(), entry.is_dir())?;
            let expected = entry.size();
            verify_entry_bytes(&mut entry, expected)?;
        }
    } else {
        use std::io::{BufRead, Read};
        let mut archive = tar::Archive::new(flate2::bufread::GzDecoder::new(
            std::io::BufReader::new(file),
        ));
        for entry in archive.entries()? {
            let mut entry = entry?;
            if !entry.header().entry_type().is_file() && !entry.header().entry_type().is_dir() {
                bail!("Archive links and special files are not permitted");
            }
            let bytes = entry.path_bytes();
            record(
                std::str::from_utf8(&bytes)?,
                entry.size(),
                entry.header().entry_type().is_dir(),
            )?;
            let expected = entry.size();
            verify_entry_bytes(&mut entry, expected)?;
        }
        // Tar iteration stops at its end marker, before gzip necessarily checks CRC/ISIZE.
        // Finish the decoder, permitting only bounded zero tar padding after that marker.
        let mut decoder = archive.into_inner();
        let mut padding = 0usize;
        let mut buffer = [0; 8192];
        loop {
            let count = decoder.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            padding += count;
            if padding > 1024 * 1024 || buffer[..count].iter().any(|byte| *byte != 0) {
                bail!("Invalid or excessive trailing tar padding");
            }
        }
        if !decoder.into_inner().fill_buf()?.is_empty() {
            bail!("Trailing data or concatenated gzip members are not permitted");
        }
    }
    Ok((original_names, total))
}

fn open_archive_input(path: &Path) -> Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // A FIFO substituted after path metadata inspection must not block open().
        options.custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32);
    }
    let file = options.open(path)?;
    validate_archive_input(&file.metadata()?)?;
    Ok(file)
}

fn validate_archive_input(metadata: &std::fs::Metadata) -> Result<()> {
    if !metadata.is_file() {
        bail!("Installation archive must be a regular file");
    }
    if metadata.len() > 2 * 1024 * 1024 * 1024 {
        bail!("Compressed archive exceeds 2 GiB installation limit");
    }
    Ok(())
}

pub fn inspect(mut args: impl Iterator<Item = String>) -> Result<()> {
    let path = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("install-inspect requires ARCHIVE"))?;
    let require_package = match args.next().as_deref() {
        None => false,
        Some("--package") => true,
        Some(_) => bail!("install-inspect accepts ARCHIVE [--package]"),
    };
    if args.next().is_some() {
        bail!("install-inspect accepts ARCHIVE [--package]");
    }
    let (names, bytes) = inspect_archive_entries(Path::new(&path))?;
    if require_package {
        verify_package_paths(&names)?;
    }
    let entries = names.len();
    println!(
        "{}",
        serde_json::to_string(
            &serde_json::json!({"entries": entries, "declaredBytes": bytes, "pathsChecked": true, "requiredPackagePathsChecked": require_package, "checksumVerified": false, "signatureVerified": false, "installed": false})
        )?
    );
    Ok(())
}

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

fn read_checksum_manifest(path: &Path) -> Result<String> {
    use std::io::Read;
    const MAX_BYTES: u64 = 1024 * 1024;
    let file = open_archive_input(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        bail!("Checksum manifest exceeds 1 MiB");
    }
    Ok(String::from_utf8(bytes)?)
}

fn hash_archive_bytes(reader: impl std::io::Read, expected: u64) -> Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    if expected > 2 * 1024 * 1024 * 1024 {
        bail!("Compressed archive exceeds 2 GiB installation limit");
    }
    let limit = expected
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("Archive size overflow"))?;
    let mut reader = reader.take(limit);
    let mut hash = Sha256::new();
    let mut count = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let bytes = reader.read(&mut buffer)?;
        if bytes == 0 {
            break;
        }
        count += bytes as u64;
        hash.update(&buffer[..bytes]);
    }
    if count != expected {
        bail!("Archive size changed during checksum verification");
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub fn verify(mut args: impl Iterator<Item = String>) -> Result<()> {
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
    let expected = expected_checksum(&read_checksum_manifest(Path::new(&checksums))?, name)?;
    let file = open_archive_input(archive)?;
    let actual = hash_archive_bytes(&file, file.metadata()?.len())?;
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

fn check_install_conflicts(
    root: &Path,
    entries: &[PathBuf],
    home: Option<&Path>,
    allow: bool,
) -> Result<()> {
    let executable = if cfg!(windows) {
        "workdeck.exe"
    } else {
        "workdeck"
    };
    let target = root.join("bin").join(executable);
    let inactive = home
        .map(|home| inactive_mise_candidates(home, executable))
        .transpose()?
        .unwrap_or_default();
    let observations = observe_candidates(
        &target,
        entries,
        executable,
        entries
            .iter()
            .map(|entry| entry.join(executable))
            .chain(inactive),
    );
    match conflict_decision(&observations, allow) {
        ConflictDecision::NoObservedExecutableConflicts | ConflictDecision::ExplicitlyAllowed => {
            Ok(())
        }
        ConflictDecision::UnresolvedAccess => bail!(
            "could not determine access to a competing Workdeck installation; no files were changed"
        ),
        ConflictDecision::RequiresForce => {
            let paths = observations
                .iter()
                .filter(|item| item.executable_access == Some(true))
                .map(|item| item.diagnostic_path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            bail!(
                "competing Workdeck installations: {paths}. Remove them or explicitly use --force to keep them; no existing installation will be overwritten"
            )
        }
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

fn current_platform() -> Result<(&'static str, &'static str)> {
    #[cfg(target_os = "macos")]
    let translated = {
        let mut value: libc::c_int = 0;
        let mut length = std::mem::size_of_val(&value);
        // SAFETY: the output pointer references a live c_int, length describes
        // its capacity, the name is NUL-terminated and no value is being written.
        let status = unsafe {
            libc::sysctlbyname(
                c"sysctl.proc_translated".as_ptr(),
                (&mut value as *mut libc::c_int).cast(),
                &mut length,
                std::ptr::null_mut(),
                0,
            )
        };
        status == 0 && length == std::mem::size_of_val(&value) && value == 1
    };
    #[cfg(not(target_os = "macos"))]
    let translated = false;
    platform(std::env::consts::OS, std::env::consts::ARCH, translated)
}

pub fn run(args: impl Iterator<Item = String>) -> Result<()> {
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
    let (os, arch) = current_platform()?;
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
    fn install_conflict_gate_requires_explicit_force_without_mutating_candidates() {
        let dir = tempfile::tempdir().unwrap();
        let executable = if cfg!(windows) {
            "workdeck.exe"
        } else {
            "workdeck"
        };
        let candidate = dir.path().join(executable);
        std::fs::write(&candidate, b"competing binary").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&candidate, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let target = dir.path().join("new-install");
        let entries = [dir.path().to_owned()];
        assert!(
            check_install_conflicts(&target, &entries, None, false)
                .unwrap_err()
                .to_string()
                .contains("--force")
        );
        check_install_conflicts(&target, &entries, None, true).unwrap();
        assert_eq!(std::fs::read(candidate).unwrap(), b"competing binary");
        assert!(!target.exists());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn platform_detection_matches_both_pinned_shell_oracles() {
        let fixtures: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../port/hunk/install-platform-oracle.json"
        ))
        .unwrap();
        assert_eq!(fixtures.len(), 20);
        for fixture in fixtures {
            let result = platform(
                fixture["os"].as_str().unwrap(),
                fixture["arch"].as_str().unwrap(),
                fixture["translated"].as_bool().unwrap(),
            );
            if fixture["exit_code"] == 0 {
                let (os, arch) = result.unwrap();
                assert_eq!(
                    format!("{os}\n{arch}\n"),
                    fixture["output"].as_str().unwrap()
                );
            } else {
                let error = result.unwrap_err().to_string();
                let category = if fixture["os"] == "FreeBSD" {
                    "Unsupported operating system"
                } else {
                    "Unsupported architecture"
                };
                assert!(fixture["output"].as_str().unwrap().contains(category));
                assert!(error.contains(category));
            }
        }
    }

    #[test]
    fn archive_hash_requires_exact_observed_length_and_bounded_reads() {
        use std::io::Cursor;
        assert_eq!(
            hash_archive_bytes(&b"abc"[..], 3).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(hash_archive_bytes(&b"ab"[..], 3).is_err());
        let mut growing = Cursor::new(b"abcdefghij");
        assert!(hash_archive_bytes(&mut growing, 3).is_err());
        assert_eq!(growing.position(), 4);
        assert!(hash_archive_bytes(std::io::repeat(0), 0).is_err());
        assert!(hash_archive_bytes(std::io::empty(), u64::MAX).is_err());
        assert_eq!(
            hash_archive_bytes(std::io::empty(), 0).unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn checksum_manifest_reads_are_bounded_and_require_utf8() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("checksums.txt");
        std::fs::write(&path, vec![b' '; 1024 * 1024]).unwrap();
        assert_eq!(read_checksum_manifest(&path).unwrap().len(), 1024 * 1024);
        std::fs::write(&path, vec![b' '; 1024 * 1024 + 1]).unwrap();
        assert!(
            read_checksum_manifest(&path)
                .unwrap_err()
                .to_string()
                .contains("1 MiB")
        );
        std::fs::write(&path, [0xff]).unwrap();
        assert!(read_checksum_manifest(&path).is_err());
        assert!(read_checksum_manifest(directory.path()).is_err());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    #[cfg(unix)]
    fn archive_open_rejects_fifo_without_waiting_for_a_writer() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("replaced.tar.gz");
        assert!(
            std::process::Command::new("mkfifo")
                .args(["-m", "600"])
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        // Exercise the open path directly, bypassing the earlier path metadata check.
        assert!(
            open_archive_input(&path)
                .unwrap_err()
                .to_string()
                .contains("regular file")
        );
        let regular = directory.path().join("regular.tar.gz");
        std::fs::write(&regular, b"fixture").unwrap();
        assert_eq!(
            open_archive_input(&regular)
                .unwrap()
                .metadata()
                .unwrap()
                .len(),
            7
        );
    }

    #[test]
    #[cfg(unix)] // set_len creates sparse fixtures here; avoid allocating GiBs on Windows.
    fn archive_input_rejects_oversized_sparse_files_and_directories() {
        let directory = tempfile::tempdir().unwrap();
        assert!(
            inspect_archive(directory.path())
                .unwrap_err()
                .to_string()
                .contains("regular file")
        );
        for name in ["oversized.zip", "oversized.tar.gz"] {
            let path = directory.path().join(name);
            let file = std::fs::File::create(&path).unwrap();
            file.set_len(2 * 1024 * 1024 * 1024 + 1).unwrap();
            assert!(
                inspect_archive(&path)
                    .unwrap_err()
                    .to_string()
                    .contains("2 GiB")
            );
            file.set_len(2 * 1024 * 1024 * 1024).unwrap();
            validate_archive_input(&file.metadata().unwrap()).unwrap();
        }
    }

    #[test]
    fn tar_inspection_checks_gzip_trailer_and_rejects_hidden_payloads() {
        use std::io::Write;
        let mut tar = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(3);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, "root/workdeck", &b"bin"[..])
            .unwrap();
        let raw = tar.into_inner().unwrap();
        let compress = |bytes: &[u8]| {
            let mut gzip =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            gzip.write_all(bytes).unwrap();
            gzip.finish().unwrap()
        };
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("fixture.tar.gz");
        let valid = compress(&raw);
        std::fs::write(&path, &valid).unwrap();
        assert_eq!(inspect_archive(&path).unwrap(), (1, 3));
        let mut bad_crc = valid.clone();
        let trailer = bad_crc.len() - 8;
        bad_crc[trailer] ^= 1;
        let mut trailing = valid.clone();
        trailing.extend_from_slice(b"hidden");
        let mut concatenated = valid.clone();
        concatenated.extend_from_slice(&compress(b"hidden"));
        let mut hidden = raw.clone();
        hidden.extend_from_slice(b"hidden");
        let mut excessive = raw.clone();
        excessive.resize(raw.len() + 1024 * 1024 + 1, 0);
        for bytes in [
            bad_crc,
            valid[..valid.len() - 4].to_vec(),
            trailing,
            concatenated,
            compress(&hidden),
            compress(&excessive),
        ] {
            std::fs::write(&path, bytes).unwrap();
            assert!(inspect_archive(&path).is_err());
        }
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn package_path_gate_requires_wrapper_executable_licenses_sbom_and_provenance() {
        let mut names: BTreeMap<String, bool> = [
            "workdeck",
            "LICENSE",
            "THIRD_PARTY_NOTICES",
            "licenses.json",
            "sbom.cdx.json",
            "provenance.json",
        ]
        .map(|name| (format!("root/{name}"), false))
        .into_iter()
        .collect();
        verify_package_paths(&names).unwrap();
        for missing in names.keys().cloned().collect::<Vec<_>>() {
            let mut incomplete = names.clone();
            incomplete.remove(&missing);
            assert!(verify_package_paths(&incomplete).is_err(), "{missing}");
        }
        names.insert("root/provenance.json".into(), true);
        assert!(verify_package_paths(&names).is_err());
        names.insert("root/provenance.json".into(), false);
        names.insert("root/workdeck.exe".into(), false);
        assert!(verify_package_paths(&names).is_err());
        names.remove("root/workdeck");
        verify_package_paths(&names).unwrap();
        names.insert("other/file".into(), false);
        assert!(verify_package_paths(&names).is_err());
    }

    #[test]
    fn archive_payload_reads_are_bounded_and_require_exact_declared_size() {
        use std::io::Cursor;
        assert!(verify_entry_bytes(&mut Cursor::new(b"abc"), 3).is_ok());
        assert!(verify_entry_bytes(&mut Cursor::new(b"ab"), 3).is_err());
        let mut oversized = Cursor::new(b"abcdefghij");
        assert!(verify_entry_bytes(&mut oversized, 3).is_err());
        assert_eq!(
            oversized.position(),
            4,
            "stop after one excess byte, not the whole payload"
        );
        assert!(verify_entry_bytes(&mut std::io::repeat(0), 0).is_err());
        assert!(verify_entry_bytes(&mut std::io::empty(), 0).is_ok());
        assert!(verify_entry_bytes(&mut std::io::empty(), u64::MAX).is_err());
    }

    #[test]
    fn archive_paths_reject_cross_platform_traversal_and_reserved_names() {
        assert_eq!(
            archive_entry_path("workdeck/skills/README.md").unwrap(),
            "workdeck/skills/README.md"
        );
        for name in [
            "/absolute",
            "../escape",
            "root/../escape",
            "root//file",
            "C:/file",
            "root\\file",
            "root/NUL.txt",
            "root/COM1",
            "root/trailing.",
            "root/trailing ",
        ] {
            assert!(archive_entry_path(name).is_err(), "{name}");
        }
    }

    #[test]
    fn zip_inspection_checks_payloads_and_case_collisions_without_extraction() {
        use std::io::Write;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("archive.zip");
        let write = |names: &[&str]| {
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            for name in names {
                zip.start_file(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                zip.write_all(b"bin").unwrap();
            }
            zip.finish().unwrap();
        };
        write(&["root/workdeck.exe"]);
        assert_eq!(inspect_archive(&path).unwrap(), (1, 3));
        assert!(!directory.path().join("root").exists());
        write(&["root/workdeck.exe", "root/WORKDECK.exe"]);
        assert!(inspect_archive(&path).is_err());
        write(&["../escape"]);
        assert!(inspect_archive(&path).is_err());
        for names in [
            ["root/file", "root/file/child"],
            ["root/file/child", "root/file"],
            ["root/FILE/child", "root/file"],
        ] {
            write(&names);
            assert!(inspect_archive(&path).is_err(), "{names:?}");
        }
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        zip.add_directory("root/", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.start_file(
            "root/workdeck.exe",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"bin").unwrap();
        zip.finish().unwrap();
        assert_eq!(inspect_archive(&path).unwrap(), (2, 3));
    }

    #[test]
    fn archive_inspection_reads_payloads_without_extracting_and_rejects_duplicates() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("archive.tar.gz");
        let write = |duplicate: bool| {
            let file = std::fs::File::create(&path).unwrap();
            let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
            let mut archive = tar::Builder::new(encoder);
            for _ in 0..if duplicate { 2 } else { 1 } {
                let mut header = tar::Header::new_gnu();
                header.set_size(3);
                header.set_mode(0o755);
                header.set_cksum();
                archive
                    .append_data(&mut header, "root/workdeck", &b"bin"[..])
                    .unwrap();
            }
            archive.into_inner().unwrap().finish().unwrap();
        };
        write(false);
        assert_eq!(inspect_archive(&path).unwrap(), (1, 3));
        assert!(!directory.path().join("root").exists());
        write(true);
        assert!(inspect_archive(&path).is_err());
    }

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
        let hash = hash_archive_bytes(
            std::fs::File::open(&archive).unwrap(),
            std::fs::metadata(&archive).unwrap().len(),
        )
        .unwrap();
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
