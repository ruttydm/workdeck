//! Managed installation lifecycle for trusted native Workdeck extensions.

use anyhow::{Context, Result, bail};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use workdeck_core::INSTALLED_EXTENSIONS_DIR_NAME;
use workdeck_extension_api::{EXTENSION_ID_RULE, ExtensionManifest, is_valid_extension_stem};

const RECORDS_FILE_NAME: &str = "records.json";
static STAGING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtensionInstallSource {
    pub spec: String,
    pub clone_url: String,
    pub reference: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExtensionInstallRecord {
    pub source: String,
    pub clone_url: String,
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    pub commit: String,
    pub installed_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ExtensionInstallOutcome {
    pub name: String,
    pub directory: PathBuf,
    pub commit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ExtensionUpdateOutcome {
    #[serde(flatten)]
    pub install: ExtensionInstallOutcome,
    pub previous_commit: String,
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ExtensionInstallListEntry {
    pub name: String,
    pub record: ExtensionInstallRecord,
    pub directory: PathBuf,
    pub version: Option<String>,
    pub present: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ExtensionManager {
    installed_root: PathBuf,
}

impl ExtensionManager {
    pub(crate) fn from_config_root(config_root: &Path) -> Self {
        Self {
            installed_root: config_root
                .join("workdeck/extensions")
                .join(INSTALLED_EXTENSIONS_DIR_NAME),
        }
    }

    #[cfg(test)]
    fn new(installed_root: PathBuf) -> Self {
        Self { installed_root }
    }

    pub(crate) fn installed_root(&self) -> &Path {
        &self.installed_root
    }

    pub(crate) fn install(
        &self,
        source: &ExtensionInstallSource,
    ) -> Result<ExtensionInstallOutcome> {
        let records = self.read_records();
        let directory = self.installed_root.join(&source.name);
        if records.contains_key(&source.name) {
            bail!(
                "{:?} is already installed; run `workdeck extension update {}` or remove it first",
                source.name,
                source.name
            );
        }
        if directory.exists() {
            bail!(
                "{} already exists but is not a managed install; move it aside before installing {:?}",
                directory.display(),
                source.name
            );
        }

        let staged = self.stage_clone(source)?;
        let version = installed_version(&staged.directory)?;
        let promoted = promote_staged_clone(&staged.directory, &directory)?;
        let timestamp = now_timestamp();
        let record = ExtensionInstallRecord {
            source: source.spec.clone(),
            clone_url: source.clone_url.clone(),
            reference: source.reference.clone(),
            commit: staged.commit.clone(),
            installed_at: timestamp.clone(),
            updated_at: timestamp,
        };
        if let Err(error) = self.save_record(&source.name, record) {
            promoted.rollback(&directory);
            return Err(error);
        }
        promoted.finish()?;

        Ok(ExtensionInstallOutcome {
            name: source.name.clone(),
            directory,
            commit: staged.commit,
            version,
        })
    }

    pub(crate) fn update(&self, name: &str) -> Result<ExtensionUpdateOutcome> {
        validate_managed_name(name)?;
        let records = self.read_records();
        let record = records.get(name).with_context(|| {
            format!(
                "{name:?} is not a managed install; run `workdeck extension list` to see managed installs"
            )
        })?;
        let source = ExtensionInstallSource {
            spec: record.source.clone(),
            clone_url: record.clone_url.clone(),
            reference: record.reference.clone(),
            name: name.to_owned(),
        };
        let staged = self.stage_clone(&source)?;
        let directory = self.installed_root.join(name);
        let version = installed_version(&staged.directory)?;

        if staged.commit == record.commit && directory.is_dir() {
            remove_dir_if_present(&staged.directory)?;
            return Ok(ExtensionUpdateOutcome {
                install: ExtensionInstallOutcome {
                    name: name.to_owned(),
                    directory,
                    commit: staged.commit,
                    version,
                },
                previous_commit: record.commit.clone(),
                changed: false,
            });
        }

        let promoted = promote_staged_clone(&staged.directory, &directory)?;
        let mut updated = record.clone();
        updated.commit.clone_from(&staged.commit);
        updated.updated_at = now_timestamp();
        if let Err(error) = self.save_record(name, updated) {
            promoted.rollback(&directory);
            return Err(error);
        }
        promoted.finish()?;

        Ok(ExtensionUpdateOutcome {
            install: ExtensionInstallOutcome {
                name: name.to_owned(),
                directory,
                commit: staged.commit,
                version,
            },
            previous_commit: record.commit.clone(),
            changed: true,
        })
    }

    pub(crate) fn update_all(&self) -> Result<Vec<ExtensionUpdateOutcome>> {
        self.list()
            .into_iter()
            .map(|entry| self.update(&entry.name))
            .collect()
    }

    pub(crate) fn remove(&self, name: &str) -> Result<ExtensionInstallRecord> {
        validate_managed_name(name)?;
        let records = self.read_records();
        let record = records.get(name).cloned().with_context(|| {
            format!(
                "{name:?} is not a managed install; run `workdeck extension list` to see managed installs"
            )
        })?;
        let directory = self.installed_root.join(name);
        let aside = unique_sibling(&self.installed_root, ".removing", name);
        let moved = if directory.exists() {
            fs::rename(&directory, &aside)
                .with_context(|| format!("move managed extension {} aside", directory.display()))?;
            true
        } else {
            false
        };

        let mut remaining = records;
        remaining.remove(name);
        if let Err(error) = self.write_records(&remaining) {
            if moved {
                let _ = fs::rename(&aside, &directory);
            }
            return Err(error);
        }
        if moved {
            remove_dir_if_present(&aside)?;
        }
        Ok(record)
    }

    pub(crate) fn list(&self) -> Vec<ExtensionInstallListEntry> {
        self.read_records()
            .into_iter()
            .map(|(name, record)| {
                let directory = self.installed_root.join(&name);
                let present = directory.is_dir();
                let version = present
                    .then(|| installed_version(&directory).ok().flatten())
                    .flatten();
                ExtensionInstallListEntry {
                    name,
                    record,
                    directory,
                    version,
                    present,
                }
            })
            .collect()
    }

    fn stage_clone(&self, source: &ExtensionInstallSource) -> Result<StagedClone> {
        fs::create_dir_all(&self.installed_root).with_context(|| {
            format!(
                "create managed extension root {}",
                self.installed_root.display()
            )
        })?;
        let staging = unique_sibling(&self.installed_root, ".staging", &source.name);
        remove_dir_if_present(&staging)?;
        let result = clone_source(source, &staging).and_then(|commit| {
            validate_extension_directory(&staging)?;
            Ok(StagedClone {
                directory: staging.clone(),
                commit,
            })
        });
        if result.is_err() {
            let _ = remove_dir_if_present(&staging);
        }
        result
    }

    fn records_path(&self) -> PathBuf {
        self.installed_root.join(RECORDS_FILE_NAME)
    }

    fn read_records(&self) -> BTreeMap<String, ExtensionInstallRecord> {
        let stored = workdeck_store::read_app_state_record(self.records_path());
        let Some(Value::Object(installs)) = stored.get("installs") else {
            return BTreeMap::new();
        };
        installs
            .iter()
            .filter_map(|(name, value)| {
                if !is_valid_extension_stem(name) {
                    return None;
                }
                serde_json::from_value(value.clone())
                    .ok()
                    .map(|record| (name.clone(), record))
            })
            .collect()
    }

    fn save_record(&self, name: &str, record: ExtensionInstallRecord) -> Result<()> {
        let mut records = self.read_records();
        records.insert(name.to_owned(), record);
        self.write_records(&records)
    }

    fn write_records(&self, records: &BTreeMap<String, ExtensionInstallRecord>) -> Result<()> {
        let mut root = Map::new();
        root.insert("installs".into(), serde_json::to_value(records)?);
        workdeck_store::write_app_state_record(self.records_path(), &root)
            .context("write managed extension install records")
    }
}

struct StagedClone {
    directory: PathBuf,
    commit: String,
}

struct PromotedClone {
    previous: PathBuf,
    had_previous: bool,
}

impl PromotedClone {
    fn rollback(&self, directory: &Path) {
        let _ = remove_dir_if_present(directory);
        if self.had_previous {
            let _ = fs::rename(&self.previous, directory);
        }
    }

    fn finish(self) -> Result<()> {
        if self.had_previous {
            remove_dir_if_present(&self.previous)?;
        }
        Ok(())
    }
}

pub(crate) fn parse_extension_install_source(
    spec: &str,
    cwd: &Path,
) -> Result<ExtensionInstallSource> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        bail!("install source must not be empty");
    }
    let explicit_git = trimmed.starts_with("git:") && !trimmed.starts_with("git://");
    let raw_location = if explicit_git { &trimmed[4..] } else { trimmed };
    let (location, reference) = split_ref_suffix(raw_location)?;
    if location.is_empty() {
        bail!("install source {spec:?} names no repository");
    }

    let clone_url = if has_explicit_transport(location) {
        location.to_owned()
    } else if is_local_path(location) {
        absolute_local_path(location, cwd)?
            .to_string_lossy()
            .into_owned()
    } else if !explicit_git && is_github_shorthand(location) {
        format!("https://github.com/{location}")
    } else if location.contains('/') {
        format!("https://{location}")
    } else {
        bail!(
            "install source {spec:?} is not a repository; use owner/repo, git:host/path, a Git URL, or a local path"
        );
    };
    let name = derive_repository_name(location);
    if name.is_empty() {
        bail!("install source {spec:?} names no repository");
    }
    if !is_valid_extension_stem(&name) {
        bail!(
            "repository name {name:?} cannot identify a managed extension: {EXTENSION_ID_RULE}; rename the repository or install it manually"
        );
    }

    Ok(ExtensionInstallSource {
        spec: trimmed.to_owned(),
        clone_url,
        reference,
        name,
    })
}

fn split_ref_suffix(location: &str) -> Result<(&str, Option<String>)> {
    let last_separator = location.rfind(['/', '\\']).unwrap_or(0);
    let Some(at) = location
        .rfind('@')
        .filter(|at| *at > 0 && *at >= last_separator)
    else {
        return Ok((location, None));
    };
    let reference = &location[at + 1..];
    if reference.is_empty() {
        bail!("install source {location:?} has an empty ref after '@'");
    }
    Ok((&location[..at], Some(reference.to_owned())))
}

fn has_explicit_transport(location: &str) -> bool {
    ["http://", "https://", "ssh://", "git://", "file://"]
        .iter()
        .any(|prefix| location.starts_with(prefix))
        || location.split_once('@').is_some_and(|(user, host_path)| {
            !user.contains('/')
                && host_path
                    .split_once(':')
                    .is_some_and(|(host, path)| !host.contains('/') && !path.is_empty())
        })
}

fn is_local_path(location: &str) -> bool {
    Path::new(location).is_absolute()
        || ["./", "../", ".\\", "..\\", "~"]
            .iter()
            .any(|prefix| location.starts_with(prefix))
}

fn absolute_local_path(location: &str, cwd: &Path) -> Result<PathBuf> {
    let expanded = if location == "~" || location.starts_with("~/") || location.starts_with("~\\") {
        let home = env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .or_else(|| env::var_os("USERPROFILE").filter(|value| !value.is_empty()))
            .context("cannot expand '~' because HOME/USERPROFILE is unset")?;
        let suffix = location
            .strip_prefix("~/")
            .or_else(|| location.strip_prefix("~\\"))
            .unwrap_or("");
        PathBuf::from(home).join(suffix)
    } else {
        PathBuf::from(location)
    };
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        cwd.join(expanded)
    };
    Ok(normalize_path(&absolute))
}

fn normalize_path(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn is_github_shorthand(location: &str) -> bool {
    let mut segments = location.split('/');
    let (Some(owner), Some(repo), None) = (segments.next(), segments.next(), segments.next())
    else {
        return false;
    };
    !owner.is_empty()
        && !repo.is_empty()
        && owner
            .bytes()
            .chain(repo.bytes())
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

fn derive_repository_name(location: &str) -> String {
    let trimmed = location.trim_end_matches(['/', '\\']);
    let last = trimmed.rsplit(['/', '\\', ':']).next().unwrap_or("");
    last.strip_suffix(".git").unwrap_or(last).to_owned()
}

fn validate_managed_name(name: &str) -> Result<()> {
    if is_valid_extension_stem(name) {
        Ok(())
    } else {
        bail!("invalid managed extension name {name:?}: {EXTENSION_ID_RULE}")
    }
}

fn clone_source(source: &ExtensionInstallSource, destination: &Path) -> Result<String> {
    if let Some(reference) = &source.reference {
        let shallow = run_git(
            None,
            [
                OsStr::new("clone"),
                OsStr::new("--quiet"),
                OsStr::new("--depth"),
                OsStr::new("1"),
                OsStr::new("--branch"),
                OsStr::new(reference),
                OsStr::new("--"),
                OsStr::new(&source.clone_url),
                destination.as_os_str(),
            ],
        );
        if shallow.is_err() {
            remove_dir_if_present(destination)?;
            run_git(
                None,
                [
                    OsStr::new("clone"),
                    OsStr::new("--quiet"),
                    OsStr::new("--"),
                    OsStr::new(&source.clone_url),
                    destination.as_os_str(),
                ],
            )?;
            run_git(
                Some(destination),
                [
                    OsStr::new("checkout"),
                    OsStr::new("--quiet"),
                    OsStr::new(reference),
                ],
            )?;
        }
    } else {
        run_git(
            None,
            [
                OsStr::new("clone"),
                OsStr::new("--quiet"),
                OsStr::new("--depth"),
                OsStr::new("1"),
                OsStr::new("--"),
                OsStr::new(&source.clone_url),
                destination.as_os_str(),
            ],
        )?;
    }
    let output = run_git(
        Some(destination),
        [OsStr::new("rev-parse"), OsStr::new("HEAD")],
    )?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn run_git<'a>(cwd: Option<&Path>, args: impl IntoIterator<Item = &'a OsStr>) -> Result<Output> {
    let mut command = Command::new("git");
    command.args(args);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command
        .output()
        .context("could not run git; installing extensions requires git on PATH")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.lines().rev().find(|line| !line.trim().is_empty());
        bail!(
            "git command failed{}",
            detail.map_or_else(String::new, |line| format!(": {}", line.trim()))
        );
    }
    Ok(output)
}

fn validate_extension_directory(directory: &Path) -> Result<Vec<PathBuf>> {
    let manifests = extension_manifests_in(directory);
    if manifests.is_empty() {
        bail!(
            "{} does not contain a native Workdeck extension; expected workdeck-extension.toml at the repository root or one level below",
            directory.display()
        );
    }
    let mut ids = BTreeSet::new();
    for path in &manifests {
        let manifest = ExtensionManifest::load(path)
            .with_context(|| format!("validate installed manifest {}", path.display()))?;
        if !ids.insert(manifest.id.clone()) {
            bail!(
                "managed repository declares duplicate extension id {:?}",
                manifest.id
            );
        }
        let executable = path
            .parent()
            .expect("manifest has a parent")
            .join(&manifest.executable);
        if !executable.is_file() {
            bail!(
                "extension {:?} executable is missing: {}",
                manifest.id,
                executable.display()
            );
        }
    }
    Ok(manifests)
}

fn extension_manifests_in(directory: &Path) -> Vec<PathBuf> {
    let mut manifests = BTreeSet::new();
    let direct = directory.join("workdeck-extension.toml");
    if direct.is_file() {
        manifests.insert(direct);
    }
    if let Ok(entries) = fs::read_dir(directory) {
        for entry in entries.flatten() {
            let manifest = entry.path().join("workdeck-extension.toml");
            if manifest.is_file() {
                manifests.insert(manifest);
            }
        }
    }
    manifests.into_iter().collect()
}

fn installed_version(directory: &Path) -> Result<Option<String>> {
    let manifests = validate_extension_directory(directory)?;
    if manifests.len() != 1 {
        return Ok(None);
    }
    Ok(Some(ExtensionManifest::load(&manifests[0])?.version))
}

fn promote_staged_clone(staging: &Path, directory: &Path) -> Result<PromotedClone> {
    let root = directory.parent().expect("managed install has a parent");
    let name = directory
        .file_name()
        .and_then(OsStr::to_str)
        .expect("validated managed name");
    let previous = unique_sibling(root, ".previous", name);
    remove_dir_if_present(&previous)?;
    let had_previous = directory.exists();
    if had_previous {
        fs::rename(directory, &previous)
            .with_context(|| format!("move previous extension {} aside", directory.display()))?;
    }
    if let Err(error) = fs::rename(staging, directory) {
        if had_previous {
            let _ = fs::rename(&previous, directory);
        }
        let _ = remove_dir_if_present(staging);
        return Err(error)
            .with_context(|| format!("install extension into {}", directory.display()));
    }
    Ok(PromotedClone {
        previous,
        had_previous,
    })
}

fn unique_sibling(root: &Path, prefix: &str, name: &str) -> PathBuf {
    let sequence = STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    root.join(format!("{prefix}-{name}-{}-{sequence}", std::process::id()))
}

fn remove_dir_if_present(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path).with_context(|| format!("remove directory {}", path.display()))?;
    }
    Ok(())
}

fn now_timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn git(cwd: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn fixture(root: &Path, name: &str, version: &str) -> PathBuf {
        let repo = root.join(name);
        fs::create_dir_all(repo.join("bin")).unwrap();
        git(&repo, &["init", "--quiet"]);
        git(&repo, &["config", "user.email", "test@example.com"]);
        git(&repo, &["config", "user.name", "Workdeck Test"]);
        fs::write(
            repo.join("workdeck-extension.toml"),
            format!(
                "id = '{name}'\nname = '{name}'\nversion = '{version}'\napi_version = 1\nexecutable = 'bin/{name}'\ncapabilities = []\n"
            ),
        )
        .unwrap();
        fs::write(repo.join("bin").join(name), "native fixture\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "--quiet", "-m", "initial"]);
        repo
    }

    fn manager(root: &Path) -> ExtensionManager {
        ExtensionManager::new(root.join("config/workdeck/extensions/installed"))
    }

    #[test]
    fn parses_every_supported_source_shape_and_ref_rule() {
        let cwd = Path::new("/tmp/workdeck-source-test");
        assert_eq!(
            parse_extension_install_source("acme/workdeck-word-diff", cwd).unwrap(),
            ExtensionInstallSource {
                spec: "acme/workdeck-word-diff".into(),
                clone_url: "https://github.com/acme/workdeck-word-diff".into(),
                reference: None,
                name: "workdeck-word-diff".into(),
            }
        );
        let tagged = parse_extension_install_source("acme/workdeck-word-diff@v1.2.0", cwd).unwrap();
        assert_eq!(tagged.reference.as_deref(), Some("v1.2.0"));
        assert_eq!(
            tagged.clone_url,
            "https://github.com/acme/workdeck-word-diff"
        );
        let hosted = parse_extension_install_source("git:codeberg.org/acme/ext@main", cwd).unwrap();
        assert_eq!(hosted.clone_url, "https://codeberg.org/acme/ext");
        assert_eq!(hosted.reference.as_deref(), Some("main"));
        let scp = parse_extension_install_source("git@github.com:acme/native-ext@v2", cwd).unwrap();
        assert_eq!(scp.clone_url, "git@github.com:acme/native-ext");
        assert_eq!(scp.reference.as_deref(), Some("v2"));
        assert_eq!(
            parse_extension_install_source("https://example.test/acme/native-ext.git", cwd)
                .unwrap()
                .clone_url,
            "https://example.test/acme/native-ext.git"
        );
    }

    #[test]
    fn pinned_hunk_source_parse_oracle_runs_under_workdeck_naming() {
        let oracle: Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/extension-management.json"
        ))
        .unwrap();
        assert_eq!(
            oracle["baselines"][0]["commit"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(
            oracle["baselines"][1]["commit"],
            "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"
        );
        let cwd = Path::new("/tmp/workdeck-extension-oracle");
        for run in oracle["source_parse_runs"].as_array().unwrap() {
            let spec = run["spec"].as_str().unwrap();
            let parsed = parse_extension_install_source(spec, cwd);
            if run["ok"].as_bool().unwrap() {
                let parsed = parsed.unwrap_or_else(|error| panic!("{spec}: {error:#}"));
                let value = &run["value"];
                assert_eq!(parsed.spec, value["spec"].as_str().unwrap(), "{spec}");
                assert_eq!(parsed.name, value["name"].as_str().unwrap(), "{spec}");
                assert_eq!(parsed.reference.as_deref(), value["ref"].as_str(), "{spec}");
                let expected_clone = value["cloneUrl"]
                    .as_str()
                    .unwrap()
                    .replace("$ORACLE_ROOT", cwd.to_string_lossy().as_ref());
                assert_eq!(parsed.clone_url, expected_clone, "{spec}");
            } else {
                assert!(parsed.is_err(), "{spec} unexpectedly parsed");
            }
        }
    }

    #[test]
    fn normalizes_local_sources_and_rejects_ambiguous_specs() {
        let root = TempDir::new().unwrap();
        let relative =
            parse_extension_install_source("./fixtures/native-ext", root.path()).unwrap();
        assert!(
            relative
                .clone_url
                .starts_with(root.path().to_string_lossy().as_ref())
        );
        assert_eq!(relative.name, "native-ext");
        if let Some(home) = env::var_os("HOME").filter(|home| !home.is_empty()) {
            let tilde = parse_extension_install_source("~/dev/native-ext@v1", root.path()).unwrap();
            assert_eq!(
                tilde.clone_url,
                PathBuf::from(home).join("dev/native-ext").to_string_lossy()
            );
            assert_eq!(tilde.reference.as_deref(), Some("v1"));
        }
        assert!(parse_extension_install_source("", root.path()).is_err());
        assert!(parse_extension_install_source("not-a-repo", root.path()).is_err());
        assert!(parse_extension_install_source("acme/native-ext@", root.path()).is_err());
        assert!(parse_extension_install_source("acme/my.weird.repo", root.path()).is_err());
    }

    #[test]
    fn installs_records_lists_updates_and_removes_a_local_repository() {
        let root = TempDir::new().unwrap();
        let repo = fixture(root.path(), "managed-ext", "1.0.0");
        let manager = manager(root.path());
        let source = parse_extension_install_source(repo.to_str().unwrap(), root.path()).unwrap();
        let installed = manager.install(&source).unwrap();
        assert_eq!(installed.version.as_deref(), Some("1.0.0"));
        assert!(installed.directory.join("bin/managed-ext").is_file());
        assert_eq!(manager.list().len(), 1);
        let records: Value = serde_json::from_str(
            &fs::read_to_string(manager.records_path()).expect("managed records"),
        )
        .unwrap();
        assert!(records["installs"]["managed-ext"].get("ref").is_none());
        assert!(
            records["installs"]["managed-ext"]
                .get("reference")
                .is_none()
        );
        assert!(manager.install(&source).is_err());

        fs::write(
            repo.join("workdeck-extension.toml"),
            "id = 'managed-ext'\nname = 'managed-ext'\nversion = '1.1.0'\napi_version = 1\nexecutable = 'bin/managed-ext'\ncapabilities = []\n",
        )
        .unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "--quiet", "-m", "update"]);
        let updated = manager.update("managed-ext").unwrap();
        assert!(updated.changed);
        assert_ne!(updated.previous_commit, updated.install.commit);
        assert_eq!(updated.install.version.as_deref(), Some("1.1.0"));
        assert!(!manager.update("managed-ext").unwrap().changed);

        manager.remove("managed-ext").unwrap();
        assert!(manager.list().is_empty());
        assert!(!installed.directory.exists());
        assert!(manager.remove("managed-ext").is_err());
    }

    #[test]
    fn honors_pinned_refs_and_rehydrates_a_missing_install() {
        let root = TempDir::new().unwrap();
        let repo = fixture(root.path(), "pinned-ext", "1.0.0");
        git(&repo, &["tag", "v1"]);
        let manager = manager(root.path());
        let source =
            parse_extension_install_source(&format!("{}@v1", repo.display()), root.path()).unwrap();
        let first = manager.install(&source).unwrap();
        let records: Value = serde_json::from_str(
            &fs::read_to_string(manager.records_path()).expect("managed records"),
        )
        .unwrap();
        assert_eq!(records["installs"]["pinned-ext"]["ref"], "v1");
        assert!(records["installs"]["pinned-ext"].get("reference").is_none());
        fs::write(repo.join("later"), "later\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "--quiet", "-m", "later"]);
        assert!(!manager.update("pinned-ext").unwrap().changed);
        fs::remove_dir_all(&first.directory).unwrap();
        let restored = manager.update("pinned-ext").unwrap();
        assert!(restored.changed);
        assert_eq!(restored.previous_commit, restored.install.commit);
        assert!(restored.install.directory.is_dir());
    }

    #[test]
    fn failed_install_or_update_never_replaces_the_live_directory() {
        let root = TempDir::new().unwrap();
        let empty = root.path().join("not-an-extension");
        fs::create_dir_all(&empty).unwrap();
        git(&empty, &["init", "--quiet"]);
        git(&empty, &["config", "user.email", "test@example.com"]);
        git(&empty, &["config", "user.name", "Workdeck Test"]);
        fs::write(empty.join("README.md"), "no extension\n").unwrap();
        git(&empty, &["add", "."]);
        git(&empty, &["commit", "--quiet", "-m", "initial"]);
        let manager = manager(root.path());
        let source = parse_extension_install_source(empty.to_str().unwrap(), root.path()).unwrap();
        assert!(manager.install(&source).is_err());
        assert!(manager.list().is_empty());
        assert!(!manager.installed_root().join("not-an-extension").exists());

        let repo = fixture(root.path(), "safe-ext", "1.0.0");
        let source = parse_extension_install_source(repo.to_str().unwrap(), root.path()).unwrap();
        let live = manager.install(&source).unwrap();
        let original = fs::read(live.directory.join("workdeck-extension.toml")).unwrap();
        fs::write(repo.join("workdeck-extension.toml"), "invalid = [").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "--quiet", "-m", "invalid"]);
        assert!(manager.update("safe-ext").is_err());
        assert_eq!(
            fs::read(live.directory.join("workdeck-extension.toml")).unwrap(),
            original
        );
    }

    #[test]
    fn never_overwrites_unmanaged_directories_and_ignores_damaged_records() {
        let root = TempDir::new().unwrap();
        let repo = fixture(root.path(), "collision", "1.0.0");
        let manager = manager(root.path());
        fs::create_dir_all(manager.installed_root().join("collision")).unwrap();
        let source = parse_extension_install_source(repo.to_str().unwrap(), root.path()).unwrap();
        assert!(manager.install(&source).is_err());

        fs::write(manager.records_path(), "{ damaged").unwrap();
        assert!(manager.list().is_empty());
    }

    #[test]
    fn accepts_one_level_collection_and_refuses_duplicate_ids_or_missing_binaries() {
        let root = TempDir::new().unwrap();
        let collection = root.path().join("native-pack");
        fs::create_dir_all(&collection).unwrap();
        git(&collection, &["init", "--quiet"]);
        git(&collection, &["config", "user.email", "test@example.com"]);
        git(&collection, &["config", "user.name", "Workdeck Test"]);
        for folder in ["one", "two"] {
            fs::create_dir_all(collection.join(folder)).unwrap();
            fs::write(
                collection.join(folder).join("workdeck-extension.toml"),
                format!("id = '{folder}'\nname = '{folder}'\nversion = '1.0.0'\napi_version = 1\nexecutable = '{folder}'\ncapabilities = []\n"),
            )
            .unwrap();
            fs::write(collection.join(folder).join(folder), "binary\n").unwrap();
        }
        git(&collection, &["add", "."]);
        git(&collection, &["commit", "--quiet", "-m", "collection"]);
        let manager = manager(root.path());
        let source =
            parse_extension_install_source(collection.to_str().unwrap(), root.path()).unwrap();
        let installed = manager.install(&source).unwrap();
        assert_eq!(installed.version, None);

        let missing = root.path().join("missing-binary");
        fs::create_dir_all(&missing).unwrap();
        git(&missing, &["init", "--quiet"]);
        git(&missing, &["config", "user.email", "test@example.com"]);
        git(&missing, &["config", "user.name", "Workdeck Test"]);
        fs::write(
            missing.join("workdeck-extension.toml"),
            "id = 'missing'\nname = 'missing'\nversion = '1.0.0'\napi_version = 1\nexecutable = 'absent'\ncapabilities = []\n",
        )
        .unwrap();
        git(&missing, &["add", "."]);
        git(&missing, &["commit", "--quiet", "-m", "missing"]);
        let source =
            parse_extension_install_source(missing.to_str().unwrap(), root.path()).unwrap();
        assert!(manager.install(&source).is_err());
    }
}
