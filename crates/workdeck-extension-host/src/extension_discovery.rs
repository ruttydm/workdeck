//! Deterministic discovery for native Workdeck extension manifests.

use crate::{HostError, TrustDecision, TrustStore};
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use workdeck_core::INSTALLED_EXTENSIONS_DIR_NAME;

const MANIFEST_NAME: &str = "workdeck-extension.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ManifestOrigin {
    Bundled,
    Explicit,
    UserConfig,
    Global,
    Repository,
}

impl ManifestOrigin {
    /// Public Hunk-compatible provenance label for diagnostics and registrations.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Bundled => "bundled",
            Self::Explicit => "flag",
            Self::UserConfig => "config",
            Self::Global => "global",
            Self::Repository => "repo",
        }
    }
}

/// Identity of one loaded native extension and the source that won its namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionMetadata {
    pub id: String,
    pub source_path: PathBuf,
    pub origin: ManifestOrigin,
}

/// Derive the stable legacy id used when inventorying a Hunk entry file.
///
/// `foo.ts` and `foo/index.ts` both resolve to `foo`; native execution still requires an
/// explicit manifest id and never executes the legacy source.
#[must_use]
pub fn derive_extension_id(entry_path: &Path) -> String {
    let stem = entry_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if stem != "index" {
        return stem.to_owned();
    }
    let parent = entry_path.parent();
    if parent.is_some_and(|parent| parent.as_os_str().is_empty() || parent == Path::new(".")) {
        return ".".into();
    }
    parent
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(stem)
        .to_owned()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestCandidate {
    pub path: PathBuf,
    pub origin: ManifestOrigin,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManifestDiscovery {
    pub candidates: Vec<ManifestCandidate>,
    pub manifests: Vec<PathBuf>,
    pub pending_trust_repo_root: Option<PathBuf>,
}

impl ManifestDiscovery {
    #[must_use]
    pub fn origin(&self, path: &Path) -> Option<ManifestOrigin> {
        self.candidates
            .iter()
            .find(|candidate| candidate.path == path)
            .map(|candidate| candidate.origin)
    }
}

pub fn discover_manifests(
    global_directory: Option<&Path>,
    repo_root: Option<&Path>,
    trust: &TrustStore,
    explicit: &[PathBuf],
) -> Result<Vec<PathBuf>, HostError> {
    Ok(discover_manifests_with_status(global_directory, repo_root, trust, explicit)?.manifests)
}

pub fn discover_manifests_with_status(
    global_directory: Option<&Path>,
    repo_root: Option<&Path>,
    trust: &TrustStore,
    explicit: &[PathBuf],
) -> Result<ManifestDiscovery, HostError> {
    discover_manifests_with_config(
        global_directory,
        repo_root,
        trust,
        explicit,
        &[],
        &[],
        &env::current_dir().unwrap_or_default(),
    )
}

/// Discover manifests in Hunk-compatible precedence order after adapting entry files to native
/// manifest directories. Explicit and user-config paths are trusted user intent. Repository
/// directory and repository-config paths share one trust decision even when a configured path
/// points outside the checkout.
#[allow(clippy::too_many_arguments)]
pub fn discover_manifests_with_config(
    global_directory: Option<&Path>,
    repo_root: Option<&Path>,
    trust: &TrustStore,
    explicit: &[PathBuf],
    user_config_paths: &[PathBuf],
    repo_config_paths: &[PathBuf],
    cwd: &Path,
) -> Result<ManifestDiscovery, HostError> {
    let mut discovery = ManifestDiscovery::default();
    let mut seen = BTreeSet::new();

    let explicit = expand_paths(explicit, cwd, true);
    append_candidates(
        explicit,
        ManifestOrigin::Explicit,
        &mut discovery,
        &mut seen,
    );

    let user_config = expand_paths(user_config_paths, cwd, true);
    append_candidates(
        user_config,
        ManifestOrigin::UserConfig,
        &mut discovery,
        &mut seen,
    );

    if let Some(global) = global_directory {
        let mut global_manifests = scan_manifest_directory(global);
        global_manifests.extend(scan_installed_root(
            &global.join(INSTALLED_EXTENSIONS_DIR_NAME),
        ));
        append_candidates(
            global_manifests,
            ManifestOrigin::Global,
            &mut discovery,
            &mut seen,
        );
    }

    if let Some(repo) = repo_root {
        let repository_directory = repo.join(".agents/workdeck/extensions");
        let has_repository_sources = repository_directory.exists() || !repo_config_paths.is_empty();
        match trust.decision(repo) {
            Some(TrustDecision::Trusted) => {
                let mut repository = scan_manifest_directory(&repository_directory);
                repository.extend(expand_paths(repo_config_paths, repo, true));
                append_candidates(
                    repository,
                    ManifestOrigin::Repository,
                    &mut discovery,
                    &mut seen,
                );
            }
            Some(TrustDecision::Denied) => {}
            Some(TrustDecision::Legacy) | None if has_repository_sources => {
                discovery.pending_trust_repo_root = Some(repo.to_owned());
            }
            Some(TrustDecision::Legacy) | None => {}
        }
    }

    discovery.manifests = discovery
        .candidates
        .iter()
        .map(|candidate| candidate.path.clone())
        .collect();
    Ok(discovery)
}

fn append_candidates(
    mut paths: Vec<PathBuf>,
    origin: ManifestOrigin,
    discovery: &mut ManifestDiscovery,
    seen: &mut BTreeSet<PathBuf>,
) {
    paths.sort();
    for path in paths {
        let identity = fs::canonicalize(&path).unwrap_or_else(|_| normalize_path(&path));
        if seen.insert(identity) {
            discovery
                .candidates
                .push(ManifestCandidate { path, origin });
        }
    }
}

fn expand_paths(paths: &[PathBuf], cwd: &Path, keep_missing: bool) -> Vec<PathBuf> {
    paths
        .iter()
        .flat_map(|path| {
            let path = resolve_authored_path(path, cwd);
            resolve_manifest_container(&path, keep_missing)
        })
        .collect()
}

fn resolve_authored_path(path: &Path, cwd: &Path) -> PathBuf {
    let expanded = expand_home_path(path);
    if expanded.is_absolute() {
        normalize_path(&expanded)
    } else {
        normalize_path(&cwd.join(expanded))
    }
}

/// Expand only a bare `~`, `~/`, or `~\` prefix; `~someone` stays literal.
#[must_use]
pub fn expand_home_path(path: &Path) -> PathBuf {
    let value = path.as_os_str().to_string_lossy();
    if (value == "~" || value.starts_with("~/") || value.starts_with("~\\"))
        && let Some(home) = env::var_os("HOME")
            .filter(|home| !home.is_empty())
            .or_else(|| env::var_os("USERPROFILE").filter(|home| !home.is_empty()))
    {
        let suffix = value
            .strip_prefix("~/")
            .or_else(|| value.strip_prefix("~\\"))
            .unwrap_or("");
        return PathBuf::from(home).join(suffix);
    }
    path.to_owned()
}

fn resolve_manifest_container(path: &Path, keep_missing: bool) -> Vec<PathBuf> {
    if path.is_dir() {
        let direct = path.join(MANIFEST_NAME);
        if direct.is_file() {
            return vec![direct];
        }
        return scan_manifest_directory(path);
    }
    if path.is_file() || path.file_name().is_some_and(|name| name == MANIFEST_NAME) {
        return vec![path.to_owned()];
    }
    if keep_missing {
        vec![path.join(MANIFEST_NAME)]
    } else {
        Vec::new()
    }
}

fn scan_manifest_directory(directory: &Path) -> Vec<PathBuf> {
    let mut manifests = BTreeSet::new();
    let direct = directory.join(MANIFEST_NAME);
    if direct.is_file() {
        manifests.insert(direct);
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let manifest = path.join(MANIFEST_NAME);
        if manifest.is_file() {
            manifests.insert(manifest);
        }
    }
    manifests.into_iter().collect()
}

fn scan_installed_root(root: &Path) -> Vec<PathBuf> {
    let mut manifests = Vec::new();
    let Ok(repositories) = fs::read_dir(root) else {
        return manifests;
    };
    let mut repositories = repositories.flatten().collect::<Vec<_>>();
    repositories.sort_by_key(|entry| entry.file_name());
    for repository in repositories {
        if !repository.path().is_dir() || repository.file_name().to_string_lossy().starts_with('.')
        {
            continue;
        }
        manifests.extend(resolve_manifest_container(&repository.path(), false));
    }
    manifests
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
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn derives_stable_legacy_ids_for_single_files_and_index_folders() {
        assert_eq!(derive_extension_id(Path::new("foo.ts")), "foo");
        assert_eq!(
            derive_extension_id(Path::new("nested/foo.test.ts")),
            "foo.test"
        );
        assert_eq!(derive_extension_id(Path::new("foo/index.ts")), "foo");
        assert_eq!(derive_extension_id(Path::new("index.ts")), ".");
        assert_eq!(derive_extension_id(Path::new("/index.ts")), "index");
    }

    #[test]
    fn origin_labels_cover_the_complete_public_provenance_contract() {
        assert_eq!(ManifestOrigin::Bundled.label(), "bundled");
        assert_eq!(ManifestOrigin::Explicit.label(), "flag");
        assert_eq!(ManifestOrigin::UserConfig.label(), "config");
        assert_eq!(ManifestOrigin::Global.label(), "global");
        assert_eq!(ManifestOrigin::Repository.label(), "repo");
    }

    fn manifest(path: &Path, id: &str) -> PathBuf {
        fs::create_dir_all(path).unwrap();
        let manifest = path.join(MANIFEST_NAME);
        fs::write(
            &manifest,
            format!("id = '{id}'\nname = '{id}'\nversion = '1.0.0'\napi_version = 1\nexecutable = '{id}'\n"),
        )
        .unwrap();
        manifest
    }

    #[test]
    fn scans_only_direct_manifest_folders_and_orders_them() {
        let root = TempDir::new().unwrap();
        let root_manifest = manifest(root.path(), "root");
        let alpha = manifest(&root.path().join("alpha"), "alpha");
        let beta = manifest(&root.path().join("beta"), "beta");
        manifest(&root.path().join("nested/deeper"), "ignored");
        fs::write(root.path().join("notes.md"), "not an extension").unwrap();
        assert_eq!(
            scan_manifest_directory(root.path()),
            [alpha, beta, root_manifest]
        );
    }

    #[test]
    fn explicit_folder_is_one_extension_and_container_expands_one_level() {
        let root = TempDir::new().unwrap();
        let direct = manifest(&root.path().join("direct"), "direct");
        fs::write(root.path().join("direct/helper"), "helper").unwrap();
        let one = manifest(&root.path().join("pack/one"), "one");
        let two = manifest(&root.path().join("pack/two"), "two");
        assert_eq!(
            expand_paths(&[root.path().join("direct")], root.path(), true),
            [direct]
        );
        assert_eq!(
            expand_paths(&[root.path().join("pack")], root.path(), true),
            [one, two]
        );
    }

    #[test]
    fn explicit_manifest_file_is_kept_without_directory_reinterpretation() {
        let root = TempDir::new().unwrap();
        let direct = manifest(&root.path().join("direct"), "direct");
        assert_eq!(
            expand_paths(std::slice::from_ref(&direct), root.path(), true),
            [direct]
        );
    }

    #[test]
    fn native_manifest_is_the_only_entrypoint_authority() {
        let root = TempDir::new().unwrap();
        let extension = root.path().join("review");
        fs::create_dir_all(extension.join("src")).unwrap();
        fs::write(extension.join("index.ts"), "export default () => {};\n").unwrap();
        fs::write(extension.join("index.tsx"), "export default () => {};\n").unwrap();
        fs::write(extension.join("src/helper.js"), "module.exports = {};\n").unwrap();
        fs::write(
            extension.join("package.json"),
            r#"{"hunk":{"extensions":["index.ts"]}}"#,
        )
        .unwrap();
        let manifest_path = extension.join(MANIFEST_NAME);
        fs::write(
            &manifest_path,
            "id = 'review'\nname = 'Review'\nversion = '1.0.0'\napi_version = 1\nexecutable = 'bin/review.native'\n",
        )
        .unwrap();

        let discovered = expand_paths(std::slice::from_ref(&extension), root.path(), true);
        assert_eq!(discovered.as_slice(), std::slice::from_ref(&manifest_path));
        let loaded = workdeck_extension_api::ExtensionManifest::load(&manifest_path).unwrap();
        assert_eq!(loaded.executable, PathBuf::from("bin/review.native"));
    }

    #[test]
    fn javascript_sources_and_package_manifests_are_never_discovered() {
        let root = TempDir::new().unwrap();
        fs::write(
            root.path().join("standalone.ts"),
            "export default () => {};\n",
        )
        .unwrap();
        let folder = root.path().join("legacy");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("index.js"), "module.exports = {};\n").unwrap();
        fs::write(
            folder.join("package.json"),
            r#"{"hunk":{"extensions":["./index.js"]}}"#,
        )
        .unwrap();

        assert!(scan_manifest_directory(root.path()).is_empty());
        assert!(resolve_manifest_container(&folder, false).is_empty());
    }

    #[test]
    fn one_level_native_collection_replaces_multi_entry_source_manifests() {
        let root = TempDir::new().unwrap();
        let beta = manifest(&root.path().join("collection/beta"), "beta");
        let alpha = manifest(&root.path().join("collection/alpha"), "alpha");
        manifest(&root.path().join("collection/nested/ignored"), "ignored");

        assert_eq!(
            expand_paths(&[root.path().join("collection")], root.path(), true),
            [alpha, beta]
        );
    }

    #[test]
    fn keeps_missing_explicit_manifest_for_an_attributed_load_issue() {
        let root = TempDir::new().unwrap();
        assert_eq!(
            expand_paths(&[PathBuf::from("absent")], root.path(), true),
            [root.path().join("absent/workdeck-extension.toml")]
        );
        assert_eq!(
            expand_paths(
                &[PathBuf::from("absent/workdeck-extension.toml")],
                root.path(),
                true
            ),
            [root.path().join("absent/workdeck-extension.toml")]
        );
    }

    #[test]
    fn keeps_a_manifest_with_a_missing_executable_for_an_attributed_host_issue() {
        let root = TempDir::new().unwrap();
        let missing = manifest(&root.path().join("missing"), "missing");
        let discovered = discover_manifests(
            None,
            None,
            &TrustStore::default(),
            std::slice::from_ref(&missing),
        )
        .unwrap();
        assert_eq!(discovered.as_slice(), std::slice::from_ref(&missing));
        assert!(matches!(
            crate::LoadedExtension::spawn(&missing, "test"),
            Err(HostError::MissingExecutable(path)) if path.ends_with("missing/missing")
        ));
    }

    #[test]
    fn orders_groups_and_deduplicates_by_resolved_path() {
        let root = TempDir::new().unwrap();
        let repo = root.path().join("repo");
        let global = root.path().join("global");
        let explicit = manifest(&root.path().join("explicit"), "explicit");
        let configured = manifest(&root.path().join("configured"), "configured");
        let global_manifest = manifest(&global.join("global"), "global");
        let repository = manifest(&repo.join(".agents/workdeck/extensions/repo"), "repo");
        let mut trust = TrustStore::default();
        trust.grant(&repo, TrustDecision::Trusted);
        let discovery = discover_manifests_with_config(
            Some(&global),
            Some(&repo),
            &trust,
            &[explicit.parent().unwrap().to_owned(), repository.clone()],
            &[configured.parent().unwrap().to_owned()],
            &[],
            root.path(),
        )
        .unwrap();
        assert_eq!(
            discovery.manifests,
            [explicit, repository, configured, global_manifest]
        );
        assert_eq!(
            discovery
                .candidates
                .iter()
                .map(|candidate| candidate.origin)
                .collect::<Vec<_>>(),
            [
                ManifestOrigin::Explicit,
                ManifestOrigin::Explicit,
                ManifestOrigin::UserConfig,
                ManifestOrigin::Global,
            ]
        );
    }

    #[test]
    fn sorts_paths_within_each_precedence_group() {
        let root = TempDir::new().unwrap();
        let alpha = manifest(&root.path().join("alpha"), "alpha");
        let beta = manifest(&root.path().join("beta"), "beta");
        let discovery = discover_manifests_with_config(
            None,
            None,
            &TrustStore::default(),
            &[
                beta.parent().unwrap().to_owned(),
                alpha.parent().unwrap().to_owned(),
            ],
            &[],
            &[],
            root.path(),
        )
        .unwrap();
        assert_eq!(discovery.manifests, [alpha, beta]);
    }

    #[test]
    fn repository_config_paths_share_repository_trust_even_outside_repo() {
        let root = TempDir::new().unwrap();
        let repo = root.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        let external = manifest(&root.path().join("external"), "external");
        let paths = [external.parent().unwrap().to_owned()];
        let pending = discover_manifests_with_config(
            None,
            Some(&repo),
            &TrustStore::default(),
            &[],
            &[],
            &paths,
            &repo,
        )
        .unwrap();
        assert!(pending.manifests.is_empty());
        assert_eq!(
            pending.pending_trust_repo_root.as_deref(),
            Some(repo.as_path())
        );

        let mut trust = TrustStore::default();
        trust.grant(&repo, TrustDecision::Trusted);
        let trusted =
            discover_manifests_with_config(None, Some(&repo), &trust, &[], &[], &paths, &repo)
                .unwrap();
        assert_eq!(trusted.manifests, [external]);
        assert_eq!(trusted.candidates[0].origin, ManifestOrigin::Repository);
    }

    #[test]
    fn denied_repository_sources_are_not_loaded_or_prompted_again() {
        let root = TempDir::new().unwrap();
        let repo = root.path().join("repo");
        manifest(&repo.join(".agents/workdeck/extensions/repo"), "repo");
        let external = manifest(&root.path().join("external"), "external");
        let mut trust = TrustStore::default();
        trust.grant(&repo, TrustDecision::Denied);

        let discovery = discover_manifests_with_config(
            None,
            Some(&repo),
            &trust,
            &[],
            &[],
            &[external.parent().unwrap().to_owned()],
            &repo,
        )
        .unwrap();
        assert!(discovery.manifests.is_empty());
        assert!(discovery.pending_trust_repo_root.is_none());
    }

    #[test]
    fn user_config_is_immediate_intent_and_wins_repo_deduplication() {
        let root = TempDir::new().unwrap();
        let repo = root.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        let shared = manifest(&root.path().join("shared"), "shared");
        let path = shared.parent().unwrap().to_owned();
        let mut trust = TrustStore::default();
        trust.grant(&repo, TrustDecision::Trusted);
        let discovery = discover_manifests_with_config(
            None,
            Some(&repo),
            &trust,
            &[],
            std::slice::from_ref(&path),
            std::slice::from_ref(&path),
            &repo,
        )
        .unwrap();
        assert_eq!(discovery.manifests, [shared]);
        assert_eq!(discovery.candidates[0].origin, ManifestOrigin::UserConfig);
    }

    #[test]
    fn manifest_api_version_is_strict_and_the_candidate_keeps_its_source_path() {
        let root = TempDir::new().unwrap();
        let directory = root.path().join("future");
        let future = manifest(&directory, "future");
        fs::write(
            &future,
            "id = 'future'\nname = 'Future'\nversion = '1.0.0'\napi_version = 9\nexecutable = 'future'\n",
        )
        .unwrap();
        let discovery =
            discover_manifests(None, None, &TrustStore::default(), &[directory]).unwrap();
        assert_eq!(discovery.as_slice(), std::slice::from_ref(&future));
        assert!(matches!(
            workdeck_extension_api::ExtensionManifest::load(&future),
            Err(workdeck_extension_api::ManifestError::ApiVersion {
                found: 9,
                expected: workdeck_extension_api::API_VERSION
            })
        ));
    }

    #[test]
    fn malformed_native_api_version_is_an_attributed_manifest_error() {
        let root = TempDir::new().unwrap();
        let directory = root.path().join("malformed");
        let malformed = manifest(&directory, "malformed");
        fs::write(
            &malformed,
            "id = 'malformed'\nname = 'Malformed'\nversion = '1.0.0'\napi_version = 'four'\nexecutable = 'malformed'\n",
        )
        .unwrap();
        let discovery =
            discover_manifests(None, None, &TrustStore::default(), &[directory]).unwrap();
        assert_eq!(discovery.as_slice(), std::slice::from_ref(&malformed));
        assert!(matches!(
            workdeck_extension_api::ExtensionManifest::load(&malformed),
            Err(workdeck_extension_api::ManifestError::Parse { path, .. }) if path == malformed
        ));
    }

    #[test]
    fn managed_root_loads_direct_and_collection_repositories_but_skips_workspaces() {
        let root = TempDir::new().unwrap();
        let direct = manifest(&root.path().join("real"), "real");
        let a = manifest(&root.path().join("collection/a"), "a");
        let b = manifest(&root.path().join("collection/b"), "b");
        manifest(&root.path().join(".staging-real-1"), "staging");
        manifest(&root.path().join(".previous-real-1"), "previous");
        assert_eq!(scan_installed_root(root.path()), [a, b, direct]);
    }

    #[test]
    fn authored_paths_expand_home_and_normalize_dot_segments() {
        if let Some(home) = env::var_os("HOME").filter(|home| !home.is_empty()) {
            assert_eq!(
                resolve_authored_path(Path::new("~/dev/../extension"), Path::new("/elsewhere")),
                PathBuf::from(home).join("extension")
            );
        }
        assert_eq!(
            resolve_authored_path(Path::new("./tools/../extension"), Path::new("/repo")),
            PathBuf::from("/repo/extension")
        );
        assert_eq!(
            resolve_authored_path(Path::new("~someone/extension"), Path::new("/repo")),
            PathBuf::from("/repo/~someone/extension")
        );
    }

    #[test]
    fn bare_and_windows_written_home_prefixes_expand_without_expanding_named_users() {
        let Some(home) = env::var_os("HOME")
            .filter(|home| !home.is_empty())
            .or_else(|| env::var_os("USERPROFILE").filter(|home| !home.is_empty()))
        else {
            return;
        };
        assert_eq!(expand_home_path(Path::new("~")), PathBuf::from(&home));
        assert_eq!(
            expand_home_path(Path::new("~\\dev\\extension")),
            PathBuf::from(&home).join("dev\\extension")
        );
        assert_eq!(
            expand_home_path(Path::new("~someone/extension")),
            PathBuf::from("~someone/extension")
        );
    }

    #[test]
    fn legacy_wrapper_keeps_explicit_global_repo_order_and_trust_gate() {
        let root = TempDir::new().unwrap();
        let explicit = manifest(&root.path().join("explicit"), "explicit");
        let global = root.path().join("global");
        let global_manifest = manifest(&global.join("global"), "global");
        let repo = root.path().join("repo");
        let repository = manifest(&repo.join(".agents/workdeck/extensions/repo"), "repo");
        let pending = discover_manifests_with_status(
            Some(&global),
            Some(&repo),
            &TrustStore::default(),
            &[explicit.parent().unwrap().to_owned()],
        )
        .unwrap();
        assert_eq!(
            pending.manifests,
            [explicit.clone(), global_manifest.clone()]
        );
        assert_eq!(
            pending.pending_trust_repo_root.as_deref(),
            Some(repo.as_path())
        );
        let mut trust = TrustStore::default();
        trust.grant(&repo, TrustDecision::Trusted);
        assert_eq!(
            discover_manifests(
                Some(&global),
                Some(&repo),
                &trust,
                &[explicit.parent().unwrap().to_owned()]
            )
            .unwrap(),
            [explicit, global_manifest, repository]
        );
    }

    #[test]
    fn expansion_preserves_non_utf8_paths_without_panicking() {
        #[cfg(unix)]
        {
            use std::ffi::OsString;
            use std::os::unix::ffi::OsStringExt;
            let path = PathBuf::from(OsString::from_vec(vec![b'x', 0xff]));
            assert_eq!(expand_home_path(&path), path);
        }
    }

    #[test]
    fn frozen_hunk_discovery_oracle_records_both_pinned_baselines_and_every_source_test() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../port/hunk/oracles/extension-discovery.json");
        let oracle: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        let baselines = oracle["baselines"].as_array().unwrap();
        assert_eq!(baselines.len(), 2);
        assert_eq!(
            baselines
                .iter()
                .map(|baseline| baseline["commit"].as_str().unwrap())
                .collect::<Vec<_>>(),
            [
                "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2",
                "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"
            ]
        );
        assert!(baselines.iter().all(|baseline| {
            baseline["passed"] == 28
                && baseline["failed"] == 0
                && baseline["source_blob"] == "7806ec6ab4e2e92071d070ac22ac4109314be81a"
                && baseline["test_blob"] == "479ef52c85c5dede5277c456577bf7f9d2c8e10e"
        }));
        let mappings = oracle["test_mapping"].as_array().unwrap();
        assert_eq!(mappings.len(), 28);
        assert!(mappings.iter().all(|mapping| {
            mapping["source_test"]
                .as_str()
                .is_some_and(|name| !name.is_empty())
                && mapping["rust_tests"]
                    .as_array()
                    .is_some_and(|tests| !tests.is_empty())
        }));
    }
}
