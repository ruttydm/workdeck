//! Subprocess host for trusted native Workdeck extensions.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;
use thiserror::Error;
use workdeck_core::{Changeset, ReviewSnapshot};
use workdeck_extension_api::{
    API_VERSION, DEFAULT_REQUEST_TIMEOUT_MS, ExtensionManifest, ExtensionPaneView,
    HandshakeRequest, HandshakeResponse, JsonRpcRequest, JsonRpcResponse, MAX_MESSAGE_BYTES,
    ManifestError, PaneRenderRequest, PaneRenderResponse, Registration, TransformRequest,
    TransformResponse, validate_view,
};

#[derive(Debug, Error)]
pub enum HostError {
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error("extension executable does not exist: {0}")]
    MissingExecutable(PathBuf),
    #[error("failed to start extension {id}: {source}")]
    Spawn { id: String, source: std::io::Error },
    #[error("extension {0} did not expose stdin/stdout pipes")]
    MissingPipe(String),
    #[error("extension {id} I/O failed: {source}")]
    Io { id: String, source: std::io::Error },
    #[error("extension {0} timed out")]
    Timeout(String),
    #[error("extension {0} closed its protocol stream")]
    Closed(String),
    #[error("extension {id} returned an oversized protocol message ({bytes} bytes)")]
    Oversized { id: String, bytes: usize },
    #[error("extension {id} returned invalid JSON: {source}")]
    InvalidJson {
        id: String,
        source: serde_json::Error,
    },
    #[error("extension {id} response id {actual} did not match request {expected}")]
    ResponseId {
        id: String,
        expected: u64,
        actual: u64,
    },
    #[error("extension {id} failed request: {message}")]
    Remote { id: String, message: String },
    #[error("extension {id} handshake was invalid: {message}")]
    Handshake { id: String, message: String },
    #[error("extension {id} returned an invalid {kind}: {message}")]
    InvalidPayload {
        id: String,
        kind: &'static str,
        message: String,
    },
    #[error("repository extension {0} has no current trust grant")]
    Untrusted(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExtensionEventBusPhase {
    Loading = 0,
    Ready = 1,
    Closing = 2,
    Closed = 3,
}

#[derive(Debug)]
pub struct ExtensionRuntimeRegistry {
    phase: AtomicU8,
}

impl ExtensionRuntimeRegistry {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            phase: AtomicU8::new(ExtensionEventBusPhase::Loading as u8),
        }
    }

    #[must_use]
    pub fn phase(&self) -> ExtensionEventBusPhase {
        match self.phase.load(Ordering::Acquire) {
            0 => ExtensionEventBusPhase::Loading,
            1 => ExtensionEventBusPhase::Ready,
            2 => ExtensionEventBusPhase::Closing,
            _ => ExtensionEventBusPhase::Closed,
        }
    }

    pub fn set_phase(&self, phase: ExtensionEventBusPhase) {
        self.phase.store(phase as u8, Ordering::Release);
    }
}

impl Default for ExtensionRuntimeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

type RegistryProbe = Arc<dyn Fn() -> Option<Arc<ExtensionRuntimeRegistry>> + Send + Sync>;
type LifetimeProbe = Arc<dyn Fn() -> bool + Send + Sync>;

pub struct ExtensionCapabilityLeaseInputs {
    pub owning_registry: Option<Arc<ExtensionRuntimeRegistry>>,
    pub get_active_registry: RegistryProbe,
    pub is_app_alive: LifetimeProbe,
    pub is_review_current: Option<LifetimeProbe>,
}

/// Immutable host-owned lease for capabilities retained by extension callbacks.
pub struct ExtensionCapabilityLease {
    owning_registry: Option<Arc<ExtensionRuntimeRegistry>>,
    get_active_registry: RegistryProbe,
    is_app_alive: LifetimeProbe,
    is_review_current: Option<LifetimeProbe>,
}

impl ExtensionCapabilityLease {
    #[must_use]
    pub fn is_live(&self) -> bool {
        let Some(owning_registry) = self.owning_registry.as_ref() else {
            return false;
        };
        (self.is_app_alive)()
            && owning_registry.phase() != ExtensionEventBusPhase::Closed
            && (self.get_active_registry)()
                .as_ref()
                .is_some_and(|active| Arc::ptr_eq(active, owning_registry))
            && self
                .is_review_current
                .as_ref()
                .is_none_or(|is_current| is_current())
    }
}

#[must_use]
pub fn create_extension_capability_lease(
    inputs: ExtensionCapabilityLeaseInputs,
) -> ExtensionCapabilityLease {
    ExtensionCapabilityLease {
        owning_registry: inputs.owning_registry,
        get_active_registry: inputs.get_active_registry,
        is_app_alive: inputs.is_app_alive,
        is_review_current: inputs.is_review_current,
    }
}

#[derive(Debug)]
pub struct LoadedExtension {
    pub manifest: ExtensionManifest,
    pub handshake: HandshakeResponse,
    child: Child,
    stdin: ChildStdin,
    responses: mpsc::Receiver<Result<String, std::io::Error>>,
    next_id: u64,
    registry: Arc<ExtensionRuntimeRegistry>,
}

impl LoadedExtension {
    pub fn spawn(manifest_path: &Path, host_version: &str) -> Result<Self, HostError> {
        let manifest = ExtensionManifest::load(manifest_path)?;
        let directory = manifest_path.parent().unwrap_or_else(|| Path::new("."));
        let executable = directory.join(&manifest.executable);
        if !executable.is_file() {
            return Err(HostError::MissingExecutable(executable));
        }
        let mut child = Command::new(&executable)
            .current_dir(directory)
            .env("WORKDECK_EXTENSION_API_VERSION", API_VERSION.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|source| HostError::Spawn {
                id: manifest.id.clone(),
                source,
            })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| HostError::MissingPipe(manifest.id.clone()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| HostError::MissingPipe(manifest.id.clone()))?;
        let (sender, responses) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        if sender.send(Ok(line)).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        break;
                    }
                }
            }
        });
        let mut loaded = Self {
            manifest,
            handshake: HandshakeResponse {
                extension_api_version: 0,
                extension_version: String::new(),
                registrations: Vec::new(),
            },
            child,
            stdin,
            responses,
            next_id: 1,
            registry: Arc::new(ExtensionRuntimeRegistry::new()),
        };
        let result = loaded.request(
            "workdeck/handshake",
            HandshakeRequest {
                host_api_version: API_VERSION,
                host_version: host_version.to_owned(),
                extension_id: loaded.manifest.id.clone(),
                granted_capabilities: loaded.manifest.capabilities.clone(),
            },
            Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS),
        )?;
        let handshake: HandshakeResponse =
            serde_json::from_value(result).map_err(|error| HostError::Handshake {
                id: loaded.manifest.id.clone(),
                message: error.to_string(),
            })?;
        if handshake.extension_api_version != API_VERSION {
            return Err(HostError::Handshake {
                id: loaded.manifest.id.clone(),
                message: format!(
                    "extension API {}, expected {}",
                    handshake.extension_api_version, API_VERSION
                ),
            });
        }
        validate_registrations(&loaded.manifest, &handshake)?;
        loaded.handshake = handshake;
        loaded.registry.set_phase(ExtensionEventBusPhase::Ready);
        Ok(loaded)
    }

    #[must_use]
    pub fn registry(&self) -> Arc<ExtensionRuntimeRegistry> {
        Arc::clone(&self.registry)
    }

    pub fn request(
        &mut self,
        method: &str,
        params: impl Serialize,
        timeout: Duration,
    ) -> Result<Value, HostError> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let request =
            JsonRpcRequest::new(id, method, params).map_err(|source| HostError::InvalidJson {
                id: self.manifest.id.clone(),
                source,
            })?;
        let mut encoded =
            serde_json::to_vec(&request).map_err(|source| HostError::InvalidJson {
                id: self.manifest.id.clone(),
                source,
            })?;
        if encoded.len() > MAX_MESSAGE_BYTES {
            return Err(HostError::Oversized {
                id: self.manifest.id.clone(),
                bytes: encoded.len(),
            });
        }
        encoded.push(b'\n');
        self.stdin
            .write_all(&encoded)
            .map_err(|source| HostError::Io {
                id: self.manifest.id.clone(),
                source,
            })?;
        self.stdin.flush().map_err(|source| HostError::Io {
            id: self.manifest.id.clone(),
            source,
        })?;

        let line = self
            .responses
            .recv_timeout(timeout)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => HostError::Timeout(self.manifest.id.clone()),
                mpsc::RecvTimeoutError::Disconnected => HostError::Closed(self.manifest.id.clone()),
            })?
            .map_err(|source| HostError::Io {
                id: self.manifest.id.clone(),
                source,
            })?;
        if line.len() > MAX_MESSAGE_BYTES {
            return Err(HostError::Oversized {
                id: self.manifest.id.clone(),
                bytes: line.len(),
            });
        }
        let response: JsonRpcResponse =
            serde_json::from_str(&line).map_err(|source| HostError::InvalidJson {
                id: self.manifest.id.clone(),
                source,
            })?;
        if response.id != id {
            return Err(HostError::ResponseId {
                id: self.manifest.id.clone(),
                expected: id,
                actual: response.id,
            });
        }
        if let Some(error) = response.error {
            return Err(HostError::Remote {
                id: self.manifest.id.clone(),
                message: error.message,
            });
        }
        Ok(response.result.unwrap_or(Value::Null))
    }

    pub fn apply_changeset_transforms(
        &mut self,
        mut changeset: Changeset,
    ) -> Result<Changeset, HostError> {
        let transforms = self
            .handshake
            .registrations
            .iter()
            .filter_map(|registration| match registration {
                Registration::ChangesetTransform { id } => Some(id.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        for transform_id in transforms {
            let value = self.request(
                "workdeck/changeset/transform",
                TransformRequest {
                    transform_id,
                    changeset,
                },
                Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS),
            )?;
            let response: TransformResponse =
                serde_json::from_value(value).map_err(|error| HostError::InvalidPayload {
                    id: self.manifest.id.clone(),
                    kind: "changeset transform",
                    message: error.to_string(),
                })?;
            changeset = response.changeset;
        }
        Ok(changeset)
    }

    pub fn render_panes(
        &mut self,
        snapshot: &ReviewSnapshot,
    ) -> Result<Vec<ExtensionPaneView>, HostError> {
        let panes = self
            .handshake
            .registrations
            .iter()
            .filter_map(|registration| match registration {
                Registration::Pane(pane) => Some(pane.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        panes
            .into_iter()
            .map(|pane| {
                let value = self.request(
                    "workdeck/pane/render",
                    PaneRenderRequest {
                        pane_id: pane.id.clone(),
                        snapshot: snapshot.clone(),
                    },
                    Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS),
                )?;
                let response: PaneRenderResponse =
                    serde_json::from_value(value).map_err(|error| HostError::InvalidPayload {
                        id: self.manifest.id.clone(),
                        kind: "pane",
                        message: error.to_string(),
                    })?;
                validate_view(&response.content).map_err(|message| HostError::InvalidPayload {
                    id: self.manifest.id.clone(),
                    kind: "pane",
                    message,
                })?;
                Ok(ExtensionPaneView {
                    extension_id: self.manifest.id.clone(),
                    pane,
                    content: response.content,
                })
            })
            .collect()
    }
}

fn validate_registrations(
    manifest: &ExtensionManifest,
    handshake: &HandshakeResponse,
) -> Result<(), HostError> {
    let mut registration_keys = BTreeSet::new();
    for registration in &handshake.registrations {
        let key = registration.key();
        if !registration_keys.insert(key.clone()) {
            return Err(HostError::Handshake {
                id: manifest.id.clone(),
                message: format!("duplicate registration {key}"),
            });
        }
        let required = registration.required_capability();
        if !manifest.capabilities.contains(&required) {
            return Err(HostError::Handshake {
                id: manifest.id.clone(),
                message: format!("registration {key} requires undeclared capability {required:?}"),
            });
        }
    }
    Ok(())
}

impl Drop for LoadedExtension {
    fn drop(&mut self) {
        self.registry.set_phase(ExtensionEventBusPhase::Closing);
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.registry.set_phase(ExtensionEventBusPhase::Closed);
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustStore {
    #[serde(default)]
    pub repositories: BTreeMap<String, TrustDecision>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrustDecision {
    Trusted,
    Denied,
    Legacy,
}

impl TrustStore {
    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|source| toml::from_str(&source).ok())
            .unwrap_or_default()
    }

    pub fn decision(&self, repo: &Path) -> Option<TrustDecision> {
        let canonical = canonical_path(repo);
        self.repositories.get(&canonical).copied()
    }

    pub fn grant(&mut self, repo: &Path, decision: TrustDecision) {
        self.repositories.insert(canonical_path(repo), decision);
    }

    pub fn save(&self, path: &Path) -> Result<(), std::io::Error> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let encoded = toml::to_string_pretty(self).expect("trust store is TOML serializable");
        let temporary = path.with_extension("toml.tmp");
        fs::write(&temporary, encoded)?;
        fs::rename(temporary, path)
    }
}

pub fn discover_manifests(
    global_directory: Option<&Path>,
    repo_root: Option<&Path>,
    trust: &TrustStore,
    explicit: &[PathBuf],
) -> Result<Vec<PathBuf>, HostError> {
    let mut manifests = BTreeSet::new();
    if let Some(global) = global_directory {
        scan_manifests(global, &mut manifests);
    }
    if let Some(repo) = repo_root {
        let directory = repo.join(".agents/workdeck/extensions");
        if directory.exists() {
            if trust.decision(repo) != Some(TrustDecision::Trusted) {
                return Err(HostError::Untrusted(directory));
            }
            scan_manifests(&directory, &mut manifests);
        }
    }
    for path in explicit {
        manifests.insert(if path.is_dir() {
            path.join("workdeck-extension.toml")
        } else {
            path.clone()
        });
    }
    Ok(manifests
        .into_iter()
        .filter(|path| path.is_file())
        .collect())
}

fn scan_manifests(directory: &Path, manifests: &mut BTreeSet<PathBuf>) {
    let direct = directory.join("workdeck-extension.toml");
    if direct.is_file() {
        manifests.insert(direct);
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let manifest = entry.path().join("workdeck-extension.toml");
        if manifest.is_file() {
            manifests.insert(manifest);
        }
    }
}

fn canonical_path(path: &Path) -> String {
    fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_owned())
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicBool;
    use tempfile::TempDir;

    #[test]
    fn repo_discovery_is_trust_gated() {
        let repo = TempDir::new().unwrap();
        let extension = repo.path().join(".agents/workdeck/extensions/demo");
        fs::create_dir_all(&extension).unwrap();
        fs::write(extension.join("workdeck-extension.toml"), "id = 'demo'").unwrap();
        let trust = TrustStore::default();
        assert!(matches!(
            discover_manifests(None, Some(repo.path()), &trust, &[]),
            Err(HostError::Untrusted(_))
        ));
        let mut trust = trust;
        trust.grant(repo.path(), TrustDecision::Trusted);
        assert_eq!(
            discover_manifests(None, Some(repo.path()), &trust, &[])
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn trust_store_round_trips_atomically() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("trust.toml");
        let mut trust = TrustStore::default();
        trust.grant(directory.path(), TrustDecision::Legacy);
        trust.save(&path).unwrap();
        assert_eq!(
            TrustStore::load(&path).decision(directory.path()),
            Some(TrustDecision::Legacy)
        );
    }

    #[test]
    fn handshake_rejects_duplicate_and_undeclared_registrations() {
        let command = Registration::Command(workdeck_extension_api::CommandRegistration {
            id: "review.accept".into(),
            title: "Accept".into(),
            description: None,
            default_keys: Vec::new(),
        });
        let mut manifest = ExtensionManifest {
            id: "demo".into(),
            name: "Demo".into(),
            version: "1.0.0".into(),
            api_version: API_VERSION,
            executable: "demo".into(),
            capabilities: vec![workdeck_extension_api::Capability::Commands],
            description: None,
        };
        let duplicate = HandshakeResponse {
            extension_api_version: API_VERSION,
            extension_version: "1.0.0".into(),
            registrations: vec![command.clone(), command.clone()],
        };
        assert!(
            validate_registrations(&manifest, &duplicate)
                .unwrap_err()
                .to_string()
                .contains("duplicate registration")
        );

        manifest.capabilities.clear();
        let undeclared = HandshakeResponse {
            extension_api_version: API_VERSION,
            extension_version: "1.0.0".into(),
            registrations: vec![command],
        };
        assert!(
            validate_registrations(&manifest, &undeclared)
                .unwrap_err()
                .to_string()
                .contains("undeclared capability")
        );
    }

    #[test]
    fn capability_lease_follows_app_runtime_and_review_generation_ownership() {
        let owning = Arc::new(ExtensionRuntimeRegistry::new());
        owning.set_phase(ExtensionEventBusPhase::Ready);
        let active = Arc::new(Mutex::new(Some(Arc::clone(&owning))));
        let app_alive = Arc::new(AtomicBool::new(true));
        let review_current = Arc::new(AtomicBool::new(true));
        let lease = create_extension_capability_lease(ExtensionCapabilityLeaseInputs {
            owning_registry: Some(Arc::clone(&owning)),
            get_active_registry: {
                let active = Arc::clone(&active);
                Arc::new(move || active.lock().unwrap().clone())
            },
            is_app_alive: {
                let app_alive = Arc::clone(&app_alive);
                Arc::new(move || app_alive.load(Ordering::Acquire))
            },
            is_review_current: Some({
                let review_current = Arc::clone(&review_current);
                Arc::new(move || review_current.load(Ordering::Acquire))
            }),
        });

        assert!(lease.is_live());
        review_current.store(false, Ordering::Release);
        assert!(!lease.is_live());
        review_current.store(true, Ordering::Release);
        *active.lock().unwrap() = Some(Arc::new(ExtensionRuntimeRegistry::new()));
        assert!(!lease.is_live());
        *active.lock().unwrap() = Some(Arc::clone(&owning));
        owning.set_phase(ExtensionEventBusPhase::Closed);
        assert!(!lease.is_live());
        owning.set_phase(ExtensionEventBusPhase::Ready);
        app_alive.store(false, Ordering::Release);
        assert!(!lease.is_live());
    }

    #[test]
    fn capability_lease_can_hold_runtime_authority_without_review_generation() {
        let registry = Arc::new(ExtensionRuntimeRegistry::new());
        registry.set_phase(ExtensionEventBusPhase::Ready);
        let lease = create_extension_capability_lease(ExtensionCapabilityLeaseInputs {
            owning_registry: Some(Arc::clone(&registry)),
            get_active_registry: Arc::new(move || Some(Arc::clone(&registry))),
            is_app_alive: Arc::new(|| true),
            is_review_current: None,
        });
        assert!(lease.is_live());
    }
}
