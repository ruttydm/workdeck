//! Staged, failure-isolated loading for native extension processes.
//!
//! Hunk settled every candidate namespace before importing the first module, published a
//! provisional registry while asynchronous factories were pending, and treated retirement as a
//! terminal state. Native Workdeck keeps those semantics across a process boundary: manifests are
//! accepted as one deterministic pass, the prepared load is observable before any executable is
//! started, and a concurrent retirement prevents a late handshake from acquiring authority.

use crate::{
    EXTENSION_SHUTDOWN_TIMEOUT, ExtensionEventBusPhase, ExtensionRuntimeRegistry, HostError,
    LoadedExtension, ManifestCandidate, ManifestOrigin, PrevalidatedExtensionSpawn,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use workdeck_extension_api::{ExtensionManifest, ExtensionNotificationHub, ManifestError};

/// One candidate whose manifest passed namespace and API validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedExtensionCandidate {
    pub manifest_path: PathBuf,
    pub origin: ManifestOrigin,
    pub manifest: ExtensionManifest,
}

/// A contained failure attributed to the candidate that could not load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionLoadIssue {
    pub extension_id: Option<String>,
    pub path: PathBuf,
    pub origin: ManifestOrigin,
    pub message: String,
}

impl fmt::Display for ExtensionLoadIssue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.extension_id {
            Some(id) => write!(
                formatter,
                "native extension {id:?} at {} was skipped: {}",
                self.path.display(),
                self.message
            ),
            None => write!(
                formatter,
                "native extension {} was skipped: {}",
                self.path.display(),
                self.message
            ),
        }
    }
}

/// Immutable inputs and claimed namespaces represented by one completed pass.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExtensionLoadState {
    /// Working directory whose discovery and configuration produced this pass.
    pub cwd: PathBuf,
    pub candidates: Vec<ManifestCandidate>,
    pub extension_configs: BTreeMap<String, Value>,
    /// Every compatible namespace accepted before process startup, including a process that later
    /// failed. Retaining failed claims makes first-wins ordering stable across incremental passes.
    pub claimed_by: BTreeMap<String, PathBuf>,
}

/// Shared terminal state published with a prepared load.
#[derive(Debug, Clone)]
pub struct ExtensionLoadControl {
    registry: Arc<ExtensionRuntimeRegistry>,
}

impl ExtensionLoadControl {
    pub(crate) fn new() -> Self {
        Self {
            registry: Arc::new(ExtensionRuntimeRegistry::new()),
        }
    }

    #[must_use]
    pub fn phase(&self) -> ExtensionEventBusPhase {
        self.registry.phase()
    }

    /// Revoke the load pass before a pending executable can publish a late handshake.
    #[must_use]
    pub fn begin_retirement(&self) -> bool {
        self.registry.begin_closing()
    }

    pub(crate) fn begin_loading(&self) -> bool {
        self.registry.begin_loading()
    }

    fn finish_loading(&self) -> bool {
        self.registry.finish_loading()
    }

    fn finish_retirement(&self) {
        self.registry.set_phase(ExtensionEventBusPhase::Closed);
    }
}

/// Complete native counterpart of Hunk's `ExtensionLoadResult`.
#[derive(Debug)]
pub struct ExtensionLoadResult {
    pub extensions: Vec<LoadedExtension>,
    pub issues: Vec<ExtensionLoadIssue>,
    pub notifications: ExtensionNotificationHub,
    pub logs: crate::ExtensionLogHub,
    pub pending_trust_repo_root: Option<PathBuf>,
    pub load_state: ExtensionLoadState,
    pub control: ExtensionLoadControl,
}

impl ExtensionLoadResult {
    /// Publish a deferred load to the composition root exactly once.
    #[must_use]
    pub fn bind_event_bus(&self) -> bool {
        self.control.finish_loading()
    }

    /// Revoke every runtime first, then share one shutdown deadline across the entire pass.
    pub fn retire(&mut self) {
        let _ = self.control.begin_retirement();
        for extension in &mut self.extensions {
            let _ = extension.begin_retirement();
        }
        let deadline = Instant::now() + EXTENSION_SHUTDOWN_TIMEOUT;
        for extension in &mut self.extensions {
            extension.finish_retirement(deadline);
        }
        self.control.finish_retirement();
    }
}

/// Provisional ownership and validation result produced before any child process starts.
#[derive(Debug)]
pub struct PreparedExtensionLoad {
    pub accepted: Vec<AcceptedExtensionCandidate>,
    pub result: ExtensionLoadResult,
    host_version: String,
}

impl PreparedExtensionLoad {
    #[must_use]
    pub fn control(&self) -> ExtensionLoadControl {
        self.result.control.clone()
    }
}

/// Inputs for a fresh or incremental native extension load.
pub struct LoadExtensionsOptions<'a> {
    pub candidates: &'a [ManifestCandidate],
    /// Session working directory exposed to every extension factory.
    pub cwd: &'a std::path::Path,
    /// Complete order represented after this pass; defaults to `candidates`.
    pub all_candidates: Option<&'a [ManifestCandidate]>,
    /// Completed prefix extended by `candidates`.
    pub previous_load: Option<ExtensionLoadResult>,
    pub host_version: &'a str,
    pub extension_configs: &'a BTreeMap<String, Value>,
    /// Supplying the existing hub preserves the mounted TUI sink during reload.
    pub notifications: Option<ExtensionNotificationHub>,
    /// Trust remains a discovery concern; this carries its pending prompt into the result.
    pub pending_trust_repo_root: Option<PathBuf>,
}

/// Validate and claim a full candidate pass before any executable can run.
#[must_use]
pub fn prepare_extension_load(options: LoadExtensionsOptions<'_>) -> PreparedExtensionLoad {
    let mut result = match options.previous_load {
        Some(mut previous) => {
            if let Some(notifications) = options.notifications {
                previous.notifications = notifications;
            }
            previous
        }
        None => ExtensionLoadResult {
            extensions: Vec::new(),
            issues: Vec::new(),
            notifications: options.notifications.unwrap_or_default(),
            logs: crate::ExtensionLogHub::default(),
            pending_trust_repo_root: None,
            load_state: ExtensionLoadState::default(),
            control: ExtensionLoadControl::new(),
        },
    };

    result.load_state.candidates = options
        .all_candidates
        .unwrap_or(options.candidates)
        .to_vec();
    result.load_state.cwd = options.cwd.to_owned();
    result.load_state.extension_configs = options.extension_configs.clone();
    result.pending_trust_repo_root = options.pending_trust_repo_root;

    let mut accepted = Vec::new();
    for candidate in options.candidates {
        let manifest = match read_candidate_manifest(&candidate.path) {
            Ok(manifest) => manifest,
            Err(error) => {
                result.issues.push(ExtensionLoadIssue {
                    extension_id: None,
                    path: candidate.path.clone(),
                    origin: candidate.origin,
                    message: error.to_string(),
                });
                continue;
            }
        };
        if let Some(owner) = result.load_state.claimed_by.get(&manifest.id) {
            result.issues.push(ExtensionLoadIssue {
                extension_id: Some(manifest.id.clone()),
                path: candidate.path.clone(),
                origin: candidate.origin,
                message: format!(
                    "another extension already loaded as {:?} ({})",
                    manifest.id,
                    owner.display()
                ),
            });
            continue;
        }
        if let Err(error) = manifest
            .validate_api_compatibility()
            .and_then(|()| manifest.validate_executable())
        {
            result.issues.push(ExtensionLoadIssue {
                extension_id: Some(manifest.id),
                path: candidate.path.clone(),
                origin: candidate.origin,
                message: error.to_string(),
            });
            continue;
        }
        result
            .load_state
            .claimed_by
            .insert(manifest.id.clone(), candidate.path.clone());
        accepted.push(AcceptedExtensionCandidate {
            manifest_path: candidate.path.clone(),
            origin: candidate.origin,
            manifest,
        });
    }

    let _ = result.control.begin_loading();
    PreparedExtensionLoad {
        accepted,
        result,
        host_version: options.host_version.to_owned(),
    }
}

/// Start every accepted process in order, containing all manifest, spawn, and handshake failures.
#[must_use]
pub fn execute_extension_load(mut prepared: PreparedExtensionLoad) -> ExtensionLoadResult {
    let cwd = prepared.result.load_state.cwd.clone();
    for candidate in prepared.accepted {
        if prepared.result.control.phase() != ExtensionEventBusPhase::Loading {
            break;
        }
        let config = prepared
            .result
            .load_state
            .extension_configs
            .get(&candidate.manifest.id)
            .cloned()
            .unwrap_or_else(|| Value::Object(Default::default()));
        match LoadedExtension::spawn_prevalidated(PrevalidatedExtensionSpawn {
            manifest_path: &candidate.manifest_path,
            expected_manifest: &candidate.manifest,
            origin: candidate.origin,
            host_version: &prepared.host_version,
            cwd: &cwd,
            notifications: prepared.result.notifications.clone(),
            config,
            logs: prepared.result.logs.clone(),
        }) {
            Ok(mut extension) => {
                if prepared.result.control.phase() == ExtensionEventBusPhase::Loading {
                    prepared.result.extensions.push(extension);
                } else {
                    extension.retire();
                    break;
                }
            }
            Err(error) => prepared.result.issues.push(ExtensionLoadIssue {
                extension_id: Some(candidate.manifest.id),
                path: candidate.manifest_path,
                origin: candidate.origin,
                message: describe_host_error(&error),
            }),
        }
    }
    let _ = prepared.result.control.finish_loading();
    prepared.result
}

/// Prepare and execute one native extension load pass.
#[must_use]
pub fn load_extensions(options: LoadExtensionsOptions<'_>) -> ExtensionLoadResult {
    execute_extension_load(prepare_extension_load(options))
}

fn describe_host_error(error: &HostError) -> String {
    error.to_string()
}

fn read_candidate_manifest(path: &std::path::Path) -> Result<ExtensionManifest, ManifestError> {
    let source = std::fs::read_to_string(path).map_err(|source| ManifestError::Read {
        path: path.to_owned(),
        source,
    })?;
    let manifest: ExtensionManifest =
        toml::from_str(&source).map_err(|source| ManifestError::Parse {
            path: path.to_owned(),
            source,
        })?;
    manifest.validate_identity()?;
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;
    use workdeck_extension_api::{API_VERSION, ExtensionNotifyType};

    fn candidate(
        root: &TempDir,
        folder: &str,
        id: &str,
        api_version: u32,
        origin: ManifestOrigin,
    ) -> ManifestCandidate {
        let directory = root.path().join(folder);
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("workdeck-extension.toml");
        fs::write(
            &path,
            format!(
                "id = {id:?}\nname = {id:?}\nversion = \"1.0.0\"\napi_version = {api_version}\nexecutable = \"missing\"\n"
            ),
        )
        .unwrap();
        ManifestCandidate { path, origin }
    }

    fn options<'a>(
        candidates: &'a [ManifestCandidate],
        configs: &'a BTreeMap<String, Value>,
    ) -> LoadExtensionsOptions<'a> {
        LoadExtensionsOptions {
            candidates,
            cwd: std::path::Path::new("."),
            all_candidates: None,
            previous_load: None,
            host_version: "test",
            extension_configs: configs,
            notifications: None,
            pending_trust_repo_root: None,
        }
    }

    #[test]
    fn publishes_all_provisional_claims_before_starting_a_process() {
        let root = TempDir::new().unwrap();
        let candidates = [candidate(
            &root,
            "review",
            "review",
            API_VERSION,
            ManifestOrigin::Explicit,
        )];
        let prepared = prepare_extension_load(options(&candidates, &BTreeMap::new()));
        assert_eq!(
            prepared.result.control.phase(),
            ExtensionEventBusPhase::Loading
        );
        assert!(prepared.result.extensions.is_empty());
        assert_eq!(prepared.accepted[0].manifest.id, "review");
        assert_eq!(
            prepared.result.load_state.claimed_by.get("review"),
            Some(&candidates[0].path)
        );
    }

    #[test]
    fn retirement_is_terminal_before_a_prepared_load_resumes() {
        let root = TempDir::new().unwrap();
        let candidates = [candidate(
            &root,
            "late",
            "late",
            API_VERSION,
            ManifestOrigin::Global,
        )];
        let prepared = prepare_extension_load(options(&candidates, &BTreeMap::new()));
        let control = prepared.control();
        assert!(control.begin_retirement());
        let mut result = execute_extension_load(prepared);
        assert!(result.extensions.is_empty());
        assert_eq!(result.control.phase(), ExtensionEventBusPhase::Closing);
        result.retire();
        assert_eq!(result.control.phase(), ExtensionEventBusPhase::Closed);
    }

    #[test]
    fn manifest_change_after_provisional_claim_is_rejected_before_process_start() {
        let root = TempDir::new().unwrap();
        let candidates = [candidate(
            &root,
            "racing",
            "original",
            API_VERSION,
            ManifestOrigin::Explicit,
        )];
        let prepared = prepare_extension_load(options(&candidates, &BTreeMap::new()));
        fs::write(candidates[0].path.parent().unwrap().join("missing"), "").unwrap();
        fs::write(
            &candidates[0].path,
            "id = \"replacement\"\nname = \"Replacement\"\nversion = \"1.0.0\"\napi_version = 1\nexecutable = \"missing\"\n",
        )
        .unwrap();

        let result = execute_extension_load(prepared);
        assert!(result.extensions.is_empty());
        assert_eq!(result.issues.len(), 1);
        assert!(
            result.issues[0]
                .message
                .contains("changed after provisional")
        );
        assert_eq!(
            result.load_state.claimed_by.get("original"),
            Some(&candidates[0].path)
        );
        assert!(!result.load_state.claimed_by.contains_key("replacement"));
    }

    #[test]
    fn candidate_ids_are_settled_first_wins_before_any_process_starts() {
        let root = TempDir::new().unwrap();
        let candidates = [
            candidate(
                &root,
                "first",
                "notes",
                API_VERSION,
                ManifestOrigin::UserConfig,
            ),
            candidate(
                &root,
                "second",
                "notes",
                API_VERSION,
                ManifestOrigin::Global,
            ),
        ];
        let prepared = prepare_extension_load(options(&candidates, &BTreeMap::new()));
        assert_eq!(prepared.accepted.len(), 1);
        assert_eq!(prepared.accepted[0].manifest_path, candidates[0].path);
        assert_eq!(prepared.result.issues.len(), 1);
        assert_eq!(prepared.result.issues[0].origin, ManifestOrigin::Global);
        assert!(prepared.result.issues[0].message.contains("already loaded"));
    }

    #[test]
    fn duplicate_refusal_precedes_api_refusal_for_an_already_claimed_namespace() {
        let root = TempDir::new().unwrap();
        let candidates = [
            candidate(
                &root,
                "current",
                "tool",
                API_VERSION,
                ManifestOrigin::Explicit,
            ),
            candidate(
                &root,
                "future",
                "tool",
                API_VERSION + 1,
                ManifestOrigin::Global,
            ),
        ];
        let prepared = prepare_extension_load(options(&candidates, &BTreeMap::new()));
        assert_eq!(prepared.accepted.len(), 1);
        assert_eq!(prepared.result.issues.len(), 1);
        assert!(prepared.result.issues[0].message.contains("already loaded"));
        assert!(!prepared.result.issues[0].message.contains("incompatible"));
    }

    #[test]
    fn incompatible_candidate_does_not_claim_the_later_compatible_namespace() {
        let root = TempDir::new().unwrap();
        let candidates = [
            candidate(
                &root,
                "future",
                "tool",
                API_VERSION + 1,
                ManifestOrigin::Global,
            ),
            candidate(
                &root,
                "current",
                "tool",
                API_VERSION,
                ManifestOrigin::Explicit,
            ),
        ];
        let prepared = prepare_extension_load(options(&candidates, &BTreeMap::new()));
        assert_eq!(prepared.accepted.len(), 1);
        assert_eq!(prepared.accepted[0].manifest_path, candidates[1].path);
        assert_eq!(prepared.result.issues.len(), 1);
        assert!(prepared.result.issues[0].message.contains("incompatible"));
    }

    #[test]
    fn failed_process_claim_survives_an_incremental_suffix() {
        let root = TempDir::new().unwrap();
        let first = [candidate(
            &root,
            "first",
            "notes",
            API_VERSION,
            ManifestOrigin::Explicit,
        )];
        let previous = load_extensions(options(&first, &BTreeMap::new()));
        assert!(previous.extensions.is_empty());
        assert_eq!(previous.issues.len(), 1);
        let suffix = [candidate(
            &root,
            "suffix",
            "notes",
            API_VERSION,
            ManifestOrigin::Global,
        )];
        let full = [first[0].clone(), suffix[0].clone()];
        let prepared = prepare_extension_load(LoadExtensionsOptions {
            candidates: &suffix,
            cwd: root.path(),
            all_candidates: Some(&full),
            previous_load: Some(previous),
            host_version: "test",
            extension_configs: &BTreeMap::new(),
            notifications: None,
            pending_trust_repo_root: None,
        });
        assert!(prepared.accepted.is_empty());
        assert_eq!(prepared.result.issues.len(), 2);
        assert_eq!(prepared.result.load_state.candidates, full);
    }

    #[test]
    fn load_state_owns_configuration_and_reuses_the_requested_notification_hub() {
        let root = TempDir::new().unwrap();
        let candidates = [candidate(
            &root,
            "configured",
            "configured",
            API_VERSION,
            ManifestOrigin::Explicit,
        )];
        let mut configs = BTreeMap::from([(
            "configured".into(),
            serde_json::json!({"severity": "blocking"}),
        )]);
        let notifications = ExtensionNotificationHub::new();
        let prepared = prepare_extension_load(LoadExtensionsOptions {
            candidates: &candidates,
            cwd: root.path(),
            all_candidates: None,
            previous_load: None,
            host_version: "test",
            extension_configs: &configs,
            notifications: Some(notifications.clone()),
            pending_trust_repo_root: None,
        });
        configs.clear();
        assert_eq!(
            prepared.result.load_state.extension_configs["configured"]["severity"],
            "blocking"
        );
        notifications.notify("before mount", ExtensionNotifyType::Warning);
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        let _subscription = prepared
            .result
            .notifications
            .subscribe(move |notice| sink.lock().unwrap().push(notice.message));
        assert_eq!(*seen.lock().unwrap(), ["before mount"]);
    }

    #[test]
    fn malformed_reserved_and_unparseable_ids_are_isolated_before_execution() {
        let root = TempDir::new().unwrap();
        let malformed = root.path().join("malformed/workdeck-extension.toml");
        fs::create_dir_all(malformed.parent().unwrap()).unwrap();
        fs::write(&malformed, "this is not toml {{{").unwrap();
        let candidates = [
            ManifestCandidate {
                path: malformed,
                origin: ManifestOrigin::Explicit,
            },
            candidate(
                &root,
                "reserved",
                "workdeck",
                API_VERSION,
                ManifestOrigin::Global,
            ),
            candidate(
                &root,
                "leading",
                "-leading",
                API_VERSION,
                ManifestOrigin::Global,
            ),
            candidate(
                &root,
                "healthy",
                "healthy",
                API_VERSION,
                ManifestOrigin::Explicit,
            ),
        ];
        let prepared = prepare_extension_load(options(&candidates, &BTreeMap::new()));
        assert_eq!(prepared.accepted.len(), 1);
        assert_eq!(prepared.accepted[0].manifest.id, "healthy");
        assert_eq!(prepared.result.issues.len(), 3);
        assert!(
            prepared
                .result
                .issues
                .iter()
                .any(|issue| issue.message.contains("reserved"))
        );
        assert!(
            prepared
                .result
                .issues
                .iter()
                .any(|issue| issue.message.contains("must start"))
        );
    }

    #[test]
    fn pending_repository_trust_and_full_candidate_order_survive_preparation() {
        let root = TempDir::new().unwrap();
        let candidates = [candidate(
            &root,
            "explicit",
            "explicit",
            API_VERSION,
            ManifestOrigin::Explicit,
        )];
        let pending = root.path().join("repo");
        let prepared = prepare_extension_load(LoadExtensionsOptions {
            candidates: &candidates,
            cwd: root.path(),
            all_candidates: Some(&candidates),
            previous_load: None,
            host_version: "test",
            extension_configs: &BTreeMap::new(),
            notifications: None,
            pending_trust_repo_root: Some(pending.clone()),
        });
        assert_eq!(prepared.result.pending_trust_repo_root, Some(pending));
        assert_eq!(prepared.result.load_state.candidates, candidates);
    }

    #[test]
    fn frozen_hunk_host_oracle_records_both_pins_and_every_source_test() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../port/hunk/oracles/extension-host.json");
        let oracle: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
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
            baseline["tests"] == 22
                && baseline["passed"] == 22
                && baseline["failed"] == 0
                && baseline["expect_calls"] == 80
                && baseline["source_blob"] == "b2197b5bf873eb298bbcf35ed472812302592b20"
        }));
        assert_eq!(
            baselines[0]["test_blob"],
            "456bcf230c90c1b9ed87b25c40d619ceaf09859f"
        );
        assert_eq!(
            baselines[1]["test_blob"],
            "9eb55bce9507d140d667732e0ed40fa33fbe8392"
        );
        let mappings = oracle["test_mapping"].as_array().unwrap();
        assert_eq!(mappings.len(), 22);
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
