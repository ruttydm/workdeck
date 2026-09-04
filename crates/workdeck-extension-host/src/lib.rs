//! Subprocess host for trusted native Workdeck extensions.

mod extension_application;
mod extension_discovery;
mod extension_document_reader;
mod extension_loading;
mod extension_registration;
mod extension_selection;
mod extension_trust;
mod file_view_host;
mod file_view_mode;
mod file_view_state;
mod file_views;
mod keyboard_mode;
mod keyboard_mode_controller;
mod line_highlights;
mod native_vcs;
mod runtime_boundary;
mod startup;
mod synchronous_callbacks;

pub use extension_application::*;
pub use extension_discovery::*;
pub use extension_document_reader::*;
pub use extension_loading::*;
pub use extension_registration::*;
pub use extension_selection::*;
pub use extension_trust::*;
pub use file_view_host::*;
pub use file_view_mode::*;
pub use file_view_state::*;
pub use file_views::*;
pub use keyboard_mode::*;
pub use keyboard_mode_controller::*;
pub use line_highlights::*;
pub use native_vcs::*;
pub use runtime_boundary::*;
pub use startup::*;
pub use synchronous_callbacks::*;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError, mpsc};
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;
use workdeck_core::{Changeset, DiffFile, ReviewSnapshot};
use workdeck_diff::{SanitizeOptions, sanitize_terminal_text};
use workdeck_extension_api::{
    API_VERSION, CliCommandExecution, CliCommandInvocation, CliCommandResult,
    CliOutputNotification, CliOutputStream, CliStdinChunk, CliStdinReadRequest, CommandExecution,
    CommandInvocation, ConfirmDialogSubmission, DEFAULT_HANDSHAKE_TIMEOUT_MS,
    DEFAULT_REQUEST_TIMEOUT_MS, ExtensionCommandAvailability, ExtensionDiffFile,
    ExtensionEventContext, ExtensionFileSide, ExtensionHostAction, ExtensionKeyEvent,
    ExtensionManifest, ExtensionNotificationHub, ExtensionNotifyType, ExtensionPaneView,
    ExtensionVcsAdapterRegistration, ExtensionVcsDetectRequest, ExtensionVcsFileSourceInvocation,
    ExtensionVcsFileSourceRequest, ExtensionVcsFileSourceResult, ExtensionVcsOperationKind,
    ExtensionVcsOperationRequest, ExtensionVcsPatchResult, ExtensionVcsReviewInput,
    ExtensionVcsWatchPlan, ExtensionWorkspaceSnapshot, ExtensionWorkspaceWriteCompletion,
    FileViewLayoutRequest, FileViewMatchRequest, FileViewModeKeyRequest,
    FileViewModeLifecycleRequest, HandshakeRequest, HandshakeResponse, InputDialogSubmission,
    JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, KeyboardModeExecution,
    KeyboardModeKeyRequest, KeyboardModeLifecycleRequest, LineHighlightRequest,
    MAX_CLI_STDIN_CHUNK_BYTES, MAX_MESSAGE_BYTES, ManifestError, PaneActionInvocation,
    PaneAvailabilityRequest, PaneAvailabilityResponse, PaneRenderRequest, PaneRenderResponse,
    Registration, ReviewEvent, SelectDialogSubmission, TransformRequest, TransformResponse,
    ValidatedFileViewLayout, validate_view,
};

#[derive(Debug, Error)]
pub enum HostError {
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error("extension executable does not exist: {0}")]
    MissingExecutable(PathBuf),
    #[error("extension manifest changed after provisional validation: {0}")]
    ManifestChanged(PathBuf),
    #[error("failed to start extension {id}: {source}")]
    Spawn { id: String, source: std::io::Error },
    #[error("extension {0} did not expose stdin/stdout pipes")]
    MissingPipe(String),
    #[error("extension {id} I/O failed: {source}")]
    Io { id: String, source: std::io::Error },
    #[error("extension {0} timed out")]
    Timeout(String),
    #[error("extension {0} request was cancelled")]
    Cancelled(String),
    #[error("extension {0} closed its protocol stream")]
    Closed(String),
    #[error("extension {0} is already handling a command")]
    Busy(String),
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

/// One line written by an extension to its reserved stderr log stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionLogEntry {
    pub extension_id: String,
    pub message: String,
}

/// Shared ordered log collection for every process in one load result.
#[derive(Debug, Clone, Default)]
pub struct ExtensionLogHub {
    entries: Arc<Mutex<Vec<ExtensionLogEntry>>>,
}

impl ExtensionLogHub {
    fn record(&self, extension_id: &str, message: String) {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(ExtensionLogEntry {
                extension_id: extension_id.to_owned(),
                message,
            });
    }

    #[must_use]
    pub fn snapshot(&self) -> Vec<ExtensionLogEntry> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

/// Maximum time a retiring native runtime may delay application teardown.
pub const EXTENSION_SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(250);

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

    /// Begin or resume a staged load without reviving a retiring registry.
    #[must_use]
    pub fn begin_loading(&self) -> bool {
        loop {
            let current = self.phase.load(Ordering::Acquire);
            if current >= ExtensionEventBusPhase::Closing as u8 {
                return false;
            }
            if current == ExtensionEventBusPhase::Loading as u8 {
                return true;
            }
            if self
                .phase
                .compare_exchange(
                    current,
                    ExtensionEventBusPhase::Loading as u8,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                return true;
            }
        }
    }

    /// Seal a successful staged load while preserving concurrent retirement.
    #[must_use]
    pub fn finish_loading(&self) -> bool {
        self.phase
            .compare_exchange(
                ExtensionEventBusPhase::Loading as u8,
                ExtensionEventBusPhase::Ready as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    /// Revoke ordinary runtime authority exactly once before shutdown begins.
    #[must_use]
    pub fn begin_closing(&self) -> bool {
        loop {
            let current = self.phase.load(Ordering::Acquire);
            if current >= ExtensionEventBusPhase::Closing as u8 {
                return false;
            }
            if self
                .phase
                .compare_exchange(
                    current,
                    ExtensionEventBusPhase::Closing as u8,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                return true;
            }
        }
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
            && owning_registry.phase() == ExtensionEventBusPhase::Ready
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
struct InstalledEventContextProvider {
    cwd: PathBuf,
}

/// Identity-bearing slot for the event context installed by a committed review app.
///
/// Installation returns a guard whose cleanup only removes that exact provider.
/// A retired app or runtime therefore cannot detach a successor installed in
/// the same slot.
#[derive(Debug, Clone, Default)]
pub struct ExtensionEventContextProviderSlot {
    active: Arc<Mutex<Option<Arc<InstalledEventContextProvider>>>>,
}

impl ExtensionEventContextProviderSlot {
    #[must_use]
    pub fn install(&self, cwd: PathBuf) -> ExtensionEventContextProviderInstallation {
        let provider = Arc::new(InstalledEventContextProvider { cwd });
        *self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Arc::clone(&provider));
        ExtensionEventContextProviderInstallation {
            slot: self.clone(),
            provider,
        }
    }

    #[must_use]
    pub fn context(&self, open_panes: Vec<String>) -> Option<ExtensionEventContext> {
        self.active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .map(|provider| ExtensionEventContext::new(provider.cwd.clone(), open_panes))
    }

    #[must_use]
    pub fn has_provider(&self) -> bool {
        self.active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_some()
    }
}

#[derive(Debug)]
pub struct ExtensionEventContextProviderInstallation {
    slot: ExtensionEventContextProviderSlot,
    provider: Arc<InstalledEventContextProvider>,
}

impl Drop for ExtensionEventContextProviderInstallation {
    fn drop(&mut self) {
        let mut active = self
            .slot
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if active
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &self.provider))
        {
            *active = None;
        }
    }
}

/// Immutable invalidation counters shared by pull-based extension surfaces.
///
/// Keys encode either one whole registered scope or one item inside that scope.
/// The common case remains allocation-light: an empty state owns no entries,
/// and reconciliation preserves the existing allocation when nothing changed.
#[derive(Debug, Clone, Default)]
pub struct ScopedEpochState(Arc<BTreeMap<String, u64>>);

impl ScopedEpochState {
    #[must_use]
    pub fn from_encoded_entries(entries: impl IntoIterator<Item = (String, u64)>) -> Self {
        Self(Arc::new(entries.into_iter().collect()))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

fn scoped_epoch_key(scope_key: &str, item_id: Option<&str>) -> String {
    match item_id {
        Some(item_id) => serde_json::to_string(&[scope_key, item_id]),
        None => serde_json::to_string(&[scope_key]),
    }
    .expect("string tuples are JSON serializable")
}

fn parse_scoped_epoch_key(key: &str) -> Option<(String, Option<String>)> {
    let parsed = serde_json::from_str::<Vec<Value>>(key).ok()?;
    if parsed.len() != 1 && parsed.len() != 2 {
        return None;
    }
    let mut parts = parsed.into_iter();
    let scope_key = parts.next()?.as_str()?.to_owned();
    let item_id = match parts.next() {
        Some(part) => Some(part.as_str()?.to_owned()),
        None => None,
    };
    Some((scope_key, item_id))
}

/// Return the invalidation epoch retained for one `(scope, item)` preparation.
/// Scope-wide and item-specific counters are summed so neither can mask the
/// other, regardless of the order in which invalidations arrive.
#[must_use]
pub fn scoped_epoch(epochs: &ScopedEpochState, scope_key: &str, item_id: &str) -> u64 {
    epochs
        .0
        .get(&scoped_epoch_key(scope_key, None))
        .copied()
        .unwrap_or_default()
        .saturating_add(
            epochs
                .0
                .get(&scoped_epoch_key(scope_key, Some(item_id)))
                .copied()
                .unwrap_or_default(),
        )
}

/// Invalidate every prepared artifact for a scope, or only one item in it.
#[must_use]
pub fn bump_scoped_epoch(
    current: &ScopedEpochState,
    scope_key: &str,
    item_id: Option<&str>,
) -> ScopedEpochState {
    let key = scoped_epoch_key(scope_key, item_id);
    let mut next = current.0.as_ref().clone();
    let epoch = next.get(&key).copied().unwrap_or_default();
    next.insert(key, epoch.saturating_add(1));
    ScopedEpochState(Arc::new(next))
}

/// Drop epochs orphaned by a reload while retaining state identity when every
/// encoded scope and optional item is still present.
#[must_use]
pub fn reconcile_scoped_epochs(
    current: &ScopedEpochState,
    item_ids: &[String],
    scope_keys: &BTreeSet<String>,
) -> ScopedEpochState {
    if current.is_empty() {
        return current.clone();
    }
    let valid_item_ids = item_ids.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let mut next = BTreeMap::new();
    for (key, epoch) in current.0.iter() {
        let Some((scope_key, item_id)) = parse_scoped_epoch_key(key) else {
            continue;
        };
        if scope_keys.contains(&scope_key)
            && item_id
                .as_deref()
                .is_none_or(|item_id| valid_item_ids.contains(item_id))
        {
            next.insert(key.clone(), *epoch);
        }
    }
    if next.len() == current.len() {
        current.clone()
    } else {
        ScopedEpochState(Arc::new(next))
    }
}

#[derive(Debug, Clone)]
pub struct LoadedExtension {
    pub manifest: ExtensionManifest,
    pub manifest_path: PathBuf,
    pub origin: ManifestOrigin,
    pub handshake: HandshakeResponse,
    connection: Arc<Mutex<ExtensionConnection>>,
    registry: Arc<ExtensionRuntimeRegistry>,
    notifications: ExtensionNotificationHub,
    logs: ExtensionLogHub,
}

pub(crate) struct PrevalidatedExtensionSpawn<'a> {
    pub manifest_path: &'a Path,
    pub expected_manifest: &'a ExtensionManifest,
    pub origin: ManifestOrigin,
    pub host_version: &'a str,
    pub cwd: &'a Path,
    pub notifications: ExtensionNotificationHub,
    pub config: Value,
    pub logs: ExtensionLogHub,
}

struct ExtensionSpawnContext {
    host_version: String,
    cwd: PathBuf,
    notifications: ExtensionNotificationHub,
    config: Value,
    logs: ExtensionLogHub,
    origin: ManifestOrigin,
}

#[derive(Debug)]
struct ExtensionConnection {
    child: Child,
    stdin: ChildStdin,
    responses: mpsc::Receiver<Result<String, std::io::Error>>,
    next_id: u64,
    pending_request: Option<PendingExecutionRequest>,
    registry: Arc<ExtensionRuntimeRegistry>,
}

impl Drop for ExtensionConnection {
    fn drop(&mut self) {
        if self.registry.phase() != ExtensionEventBusPhase::Closed {
            let _ = self.registry.begin_closing();
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.pending_request = None;
            self.registry.set_phase(ExtensionEventBusPhase::Closed);
        }
    }
}

/// Validate the identity invariants that Serde's typed transform response cannot express.
///
/// All renderer-critical object/array/number fields are required by the Rust model itself. The
/// invocation-local file id remains semantic: empty or duplicate values would corrupt selection,
/// note targeting, and keyed extension state just as duplicate `DiffFile.id` values do in Hunk.
pub fn validate_transformed_changeset(changeset: &Changeset) -> Result<(), String> {
    let mut claimed_ids = BTreeSet::new();
    for (index, file) in changeset.files.iter().enumerate() {
        if file.runtime_id.is_empty() {
            return Err(format!("files[{index}].id is not a non-empty string"));
        }
        if !claimed_ids.insert(file.runtime_id.as_str()) {
            return Err(format!("duplicate file id {:?}", file.runtime_id));
        }
    }
    Ok(())
}

fn decode_transform_response(value: Value) -> Result<Changeset, String> {
    let response: TransformResponse =
        serde_json::from_value(value).map_err(|error| error.to_string())?;
    validate_transformed_changeset(&response.changeset)?;
    Ok(response.changeset)
}

fn settle_transform_attempt(
    extension_id: &str,
    notifications: &ExtensionNotificationHub,
    previous: Changeset,
    attempt: Result<Value, String>,
) -> Changeset {
    let value = match attempt {
        Ok(value) => value,
        Err(error) => {
            notifications.notify(
                format!("Extension {extension_id} failed transforming the changeset • {error}"),
                ExtensionNotifyType::Warning,
            );
            return previous;
        }
    };
    match decode_transform_response(value) {
        Ok(changeset) => changeset,
        Err(error) => {
            notifications.notify(
                format!(
                    "Extension {extension_id} returned an invalid changeset ({error}) • keeping the previous one"
                ),
                ExtensionNotifyType::Warning,
            );
            previous
        }
    }
}

fn changeset_transform_ids(handshake: &HandshakeResponse) -> Vec<String> {
    handshake
        .registrations
        .iter()
        .filter_map(|registration| match registration {
            Registration::ChangesetTransform { id } => Some(id.clone()),
            _ => None,
        })
        .collect()
}

#[derive(Debug, Clone, Copy)]
struct PendingExecutionRequest {
    id: u64,
    deadline: Instant,
    kind: PendingExecutionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingExecutionKind {
    Command,
    Event,
}

impl LoadedExtension {
    /// Publish a host-attributed warning through this runtime's shared notification hub.
    pub fn notify_warning(&self, message: impl Into<String>) {
        self.notifications
            .notify(message, ExtensionNotifyType::Warning);
    }

    pub fn spawn(manifest_path: &Path, host_version: &str) -> Result<Self, HostError> {
        Self::spawn_with_configuration(
            manifest_path,
            host_version,
            Value::Object(Default::default()),
        )
    }

    pub fn spawn_with_configuration(
        manifest_path: &Path,
        host_version: &str,
        config: Value,
    ) -> Result<Self, HostError> {
        Self::spawn_with_notifications_and_configuration(
            manifest_path,
            host_version,
            ExtensionNotificationHub::new(),
            config,
        )
    }

    pub fn spawn_with_notifications(
        manifest_path: &Path,
        host_version: &str,
        notifications: ExtensionNotificationHub,
    ) -> Result<Self, HostError> {
        Self::spawn_with_notifications_and_configuration(
            manifest_path,
            host_version,
            notifications,
            Value::Object(Default::default()),
        )
    }

    pub fn spawn_with_notifications_and_configuration(
        manifest_path: &Path,
        host_version: &str,
        notifications: ExtensionNotificationHub,
        config: Value,
    ) -> Result<Self, HostError> {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::spawn_with_expected_manifest(
            manifest_path,
            ExtensionSpawnContext {
                host_version: host_version.to_owned(),
                cwd,
                notifications,
                config,
                logs: ExtensionLogHub::default(),
                origin: ManifestOrigin::Explicit,
            },
            None,
        )
    }

    pub(crate) fn spawn_prevalidated(
        options: PrevalidatedExtensionSpawn<'_>,
    ) -> Result<Self, HostError> {
        Self::spawn_with_expected_manifest(
            options.manifest_path,
            ExtensionSpawnContext {
                host_version: options.host_version.to_owned(),
                cwd: options.cwd.to_owned(),
                notifications: options.notifications,
                config: options.config,
                logs: options.logs,
                origin: options.origin,
            },
            Some(options.expected_manifest),
        )
    }

    fn spawn_with_expected_manifest(
        manifest_path: &Path,
        context: ExtensionSpawnContext,
        expected_manifest: Option<&ExtensionManifest>,
    ) -> Result<Self, HostError> {
        let ExtensionSpawnContext {
            host_version,
            cwd,
            notifications,
            config,
            logs,
            origin,
        } = context;
        let entrypoint = resolve_native_extension_entrypoint(manifest_path)?;
        if expected_manifest.is_some_and(|expected| expected != &entrypoint.manifest) {
            return Err(HostError::ManifestChanged(entrypoint.manifest_path));
        }
        let NativeExtensionEntrypoint {
            manifest,
            manifest_path: resolved_manifest_path,
            directory,
            executable,
        } = entrypoint;
        let mut child = Command::new(&executable)
            .current_dir(&directory)
            .env("WORKDECK_EXTENSION_API_VERSION", API_VERSION.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
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
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| HostError::MissingPipe(manifest.id.clone()))?;
        let log_extension_id = manifest.id.clone();
        let extension_logs = logs.clone();
        thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            loop {
                let mut bytes = Vec::new();
                match reader.read_until(b'\n', &mut bytes) {
                    Ok(0) => break,
                    Ok(_) => {
                        if bytes.last() == Some(&b'\n') {
                            bytes.pop();
                        }
                        if bytes.last() == Some(&b'\r') {
                            bytes.pop();
                        }
                        extension_logs.record(
                            &log_extension_id,
                            String::from_utf8_lossy(&bytes).into_owned(),
                        );
                    }
                    Err(_) => break,
                }
            }
        });
        let (sender, responses) = mpsc::channel();
        let output_notifications = notifications.clone();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        if let Some(notification) = parse_extension_notification(&line) {
                            output_notifications
                                .notify(notification.message, notification.notification_type);
                            continue;
                        }
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
        let registry = Arc::new(ExtensionRuntimeRegistry::new());
        let mut loaded = Self {
            manifest,
            manifest_path: resolved_manifest_path,
            origin,
            handshake: HandshakeResponse {
                extension_api_version: 0,
                extension_version: String::new(),
                registrations: Vec::new(),
            },
            connection: Arc::new(Mutex::new(ExtensionConnection {
                child,
                stdin,
                responses,
                next_id: 1,
                pending_request: None,
                registry: Arc::clone(&registry),
            })),
            registry,
            notifications,
            logs,
        };
        let result = loaded.request(
            "workdeck/handshake",
            HandshakeRequest {
                host_api_version: API_VERSION,
                host_version,
                extension_id: loaded.manifest.id.clone(),
                cwd,
                granted_capabilities: loaded.manifest.capabilities.clone(),
                config: granted_extension_config(&loaded.manifest, config),
            },
            Duration::from_millis(DEFAULT_HANDSHAKE_TIMEOUT_MS),
        )?;
        let mut handshake: HandshakeResponse =
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
        normalize_and_validate_registrations(&loaded.manifest, &mut handshake).map_err(
            |message| HostError::Handshake {
                id: loaded.manifest.id.clone(),
                message,
            },
        )?;
        loaded.handshake = handshake;
        loaded.registry.set_phase(ExtensionEventBusPhase::Ready);
        Ok(loaded)
    }

    #[must_use]
    pub fn registry(&self) -> Arc<ExtensionRuntimeRegistry> {
        Arc::clone(&self.registry)
    }

    #[must_use]
    pub fn notifications(&self) -> ExtensionNotificationHub {
        self.notifications.clone()
    }

    #[must_use]
    pub fn logs(&self) -> Vec<ExtensionLogEntry> {
        self.logs.snapshot()
    }

    #[must_use]
    pub fn metadata(&self) -> ExtensionMetadata {
        ExtensionMetadata {
            id: self.manifest.id.clone(),
            source_path: self.manifest_path.clone(),
            origin: self.origin,
        }
    }

    fn try_connection(&self) -> Result<MutexGuard<'_, ExtensionConnection>, HostError> {
        match self.connection.try_lock() {
            Ok(connection) => Ok(connection),
            Err(TryLockError::WouldBlock) => Err(HostError::Busy(self.manifest.id.clone())),
            Err(TryLockError::Poisoned(error)) => Ok(error.into_inner()),
        }
    }

    /// Revoke all retained authority and start best-effort native shutdown exactly once.
    ///
    /// This half is deliberately nonblocking so a collection of extensions can all receive the
    /// retirement signal before sharing one global deadline.
    #[must_use]
    pub fn begin_retirement(&mut self) -> bool {
        if !self.registry.begin_closing() {
            return false;
        }
        if self.subscribes_to_event("shutdown") {
            let _ = self.send_notification("workdeck/shutdown", serde_json::json!({}));
        }
        true
    }

    /// Finish a previously started retirement no later than `deadline`.
    pub fn finish_retirement(&mut self, deadline: Instant) {
        if self.registry.phase() == ExtensionEventBusPhase::Closed {
            return;
        }
        let Ok(mut connection) = self.try_connection() else {
            self.registry.set_phase(ExtensionEventBusPhase::Closed);
            return;
        };
        if self.subscribes_to_event("shutdown") {
            while Instant::now() < deadline {
                if connection.child.try_wait().ok().flatten().is_some() {
                    connection.pending_request = None;
                    self.registry.set_phase(ExtensionEventBusPhase::Closed);
                    return;
                }
                thread::sleep(Duration::from_millis(2));
            }
        }
        let _ = connection.child.kill();
        let _ = connection.child.wait();
        connection.pending_request = None;
        self.registry.set_phase(ExtensionEventBusPhase::Closed);
    }

    /// Revoke and retire one runtime within Hunk's pinned 250 ms shutdown bound.
    pub fn retire(&mut self) {
        let _ = self.begin_retirement();
        self.finish_retirement(Instant::now() + EXTENSION_SHUTDOWN_TIMEOUT);
    }

    pub fn request(
        &mut self,
        method: &str,
        params: impl Serialize,
        timeout: Duration,
    ) -> Result<Value, HostError> {
        let mut connection = self.try_connection()?;
        if connection.pending_request.is_some() {
            return Err(HostError::Busy(self.manifest.id.clone()));
        }
        let id = self.send_request_on(&mut connection, method, params)?;
        let line = self.receive_protocol_line_on(&connection, id, Instant::now() + timeout)?;
        self.decode_response(id, &line)
    }

    fn request_cancellable(
        &mut self,
        method: &str,
        params: impl Serialize,
        timeout: Duration,
        cancelled: &AtomicBool,
    ) -> Result<Value, HostError> {
        let mut connection = self.try_connection()?;
        if connection.pending_request.is_some() {
            return Err(HostError::Busy(self.manifest.id.clone()));
        }
        let id = self.send_request_on(&mut connection, method, params)?;
        let deadline = Instant::now() + timeout;
        let line = loop {
            if cancelled.load(Ordering::Acquire) {
                let _ = self.send_notification_on(
                    &mut connection,
                    "$/cancelRequest",
                    serde_json::json!({ "id": id }),
                );
                return Err(HostError::Cancelled(self.manifest.id.clone()));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                let _ = self.send_notification_on(
                    &mut connection,
                    "$/cancelRequest",
                    serde_json::json!({ "id": id }),
                );
                return Err(HostError::Timeout(self.manifest.id.clone()));
            }
            match connection
                .responses
                .recv_timeout(remaining.min(Duration::from_millis(25)))
            {
                Ok(Ok(line)) => {
                    if parse_cli_output_notification(&line).is_some()
                        || parse_cli_stdin_read_notification(&line).is_some()
                    {
                        continue;
                    }
                    if json_rpc_response_id(&line).is_some_and(|response_id| response_id < id) {
                        continue;
                    }
                    break line;
                }
                Ok(Err(source)) => {
                    return Err(HostError::Io {
                        id: self.manifest.id.clone(),
                        source,
                    });
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(HostError::Closed(self.manifest.id.clone()));
                }
            }
        };
        self.decode_response(id, &line)
    }

    fn vcs_adapter_registration(
        &self,
        adapter_id: &str,
    ) -> Result<ExtensionVcsAdapterRegistration, HostError> {
        self.handshake
            .registrations
            .iter()
            .find_map(|registration| match registration {
                Registration::VcsAdapter(adapter) if adapter.id == adapter_id => {
                    Some(adapter.clone())
                }
                _ => None,
            })
            .ok_or_else(|| HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "VCS adapter",
                message: format!("adapter {adapter_id:?} is not registered"),
            })
    }

    fn require_vcs_operation(
        &self,
        adapter_id: &str,
        operation: ExtensionVcsOperationKind,
    ) -> Result<workdeck_extension_api::ExtensionVcsOperationRegistration, HostError> {
        self.vcs_adapter_registration(adapter_id)?
            .operations
            .get(&operation)
            .copied()
            .ok_or_else(|| HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "VCS operation",
                message: format!("adapter {adapter_id:?} does not register {operation:?}"),
            })
    }

    /// Ask one registered native adapter whether it owns `cwd`.
    ///
    /// Detection stays permissive at this layer so the adapter-local normalizer can reproduce
    /// Hunk's miss and mismatched-id behavior exactly.
    pub fn detect_vcs_adapter(
        &mut self,
        adapter_id: &str,
        cwd: PathBuf,
        timeout: Duration,
    ) -> Result<Value, HostError> {
        self.vcs_adapter_registration(adapter_id)?;
        self.request(
            "workdeck/vcs/detect",
            ExtensionVcsDetectRequest {
                adapter_id: adapter_id.to_owned(),
                cwd,
            },
            timeout,
        )
    }

    /// Execute one declared VCS review operation through the native process.
    pub fn load_vcs_operation(
        &mut self,
        adapter_id: &str,
        operation: ExtensionVcsOperationKind,
        input: ExtensionVcsReviewInput,
        cwd: PathBuf,
        timeout: Duration,
    ) -> Result<ExtensionVcsPatchResult, HostError> {
        self.require_vcs_operation(adapter_id, operation)?;
        if !vcs_input_matches_operation(&input, operation) {
            return Err(HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "VCS operation",
                message: format!("input kind does not match {operation:?}"),
            });
        }
        if matches!(
            &input,
            ExtensionVcsReviewInput::Vcs {
                range: Some(_),
                range_endpoints: Some(_),
                ..
            }
        ) {
            return Err(HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "VCS operation",
                message: "range and rangeEndpoints are mutually exclusive".into(),
            });
        }
        let value = self.request(
            "workdeck/vcs/load",
            ExtensionVcsOperationRequest {
                adapter_id: adapter_id.to_owned(),
                operation,
                input,
                context: workdeck_extension_api::ExtensionVcsLoadContext { cwd },
            },
            timeout,
        )?;
        serde_json::from_value(value).map_err(|error| HostError::InvalidPayload {
            id: self.manifest.id.clone(),
            kind: "VCS patch result",
            message: error.to_string(),
        })
    }

    /// Read one exact file side from the operation-owned source snapshot.
    pub fn read_vcs_file_source(
        &mut self,
        adapter_id: &str,
        load_token: &str,
        request: ExtensionVcsFileSourceRequest,
        timeout: Duration,
    ) -> Result<ExtensionVcsFileSourceResult, HostError> {
        self.vcs_adapter_registration(adapter_id)?;
        if load_token.is_empty() {
            return Err(HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "VCS source reader",
                message: "load token must be non-empty".into(),
            });
        }
        let value = self.request(
            "workdeck/vcs/source/read",
            ExtensionVcsFileSourceInvocation {
                adapter_id: adapter_id.to_owned(),
                load_token: load_token.to_owned(),
                request,
            },
            timeout,
        )?;
        serde_json::from_value(value).map_err(|error| HostError::InvalidPayload {
            id: self.manifest.id.clone(),
            kind: "VCS source reader",
            message: error.to_string(),
        })
    }

    pub fn vcs_watch_signature(
        &mut self,
        request: ExtensionVcsOperationRequest,
        timeout: Duration,
    ) -> Result<String, HostError> {
        let callbacks = self.require_vcs_operation(&request.adapter_id, request.operation)?;
        if !callbacks.watch_signature {
            return Err(HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "VCS watch signature",
                message: "operation does not register watchSignature".into(),
            });
        }
        if !vcs_input_matches_operation(&request.input, request.operation) {
            return Err(HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "VCS watch signature",
                message: "input kind does not match operation".into(),
            });
        }
        let value = self.request("workdeck/vcs/watch-signature", request, timeout)?;
        serde_json::from_value(value).map_err(|error| HostError::InvalidPayload {
            id: self.manifest.id.clone(),
            kind: "VCS watch signature",
            message: error.to_string(),
        })
    }

    pub fn vcs_watch_plan(
        &mut self,
        request: ExtensionVcsOperationRequest,
        timeout: Duration,
    ) -> Result<ExtensionVcsWatchPlan, HostError> {
        let callbacks = self.require_vcs_operation(&request.adapter_id, request.operation)?;
        if !callbacks.watch_plan {
            return Err(HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "VCS watch plan",
                message: "operation does not register watchPlan".into(),
            });
        }
        if !vcs_input_matches_operation(&request.input, request.operation) {
            return Err(HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "VCS watch plan",
                message: "input kind does not match operation".into(),
            });
        }
        let value = self.request("workdeck/vcs/watch-plan", request, timeout)?;
        serde_json::from_value(value).map_err(|error| HostError::InvalidPayload {
            id: self.manifest.id.clone(),
            kind: "VCS watch plan",
            message: error.to_string(),
        })
    }

    fn send_request_on(
        &self,
        connection: &mut ExtensionConnection,
        method: &str,
        params: impl Serialize,
    ) -> Result<u64, HostError> {
        let id = connection.next_id;
        connection.next_id = connection.next_id.saturating_add(1);
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
        connection
            .stdin
            .write_all(&encoded)
            .map_err(|source| HostError::Io {
                id: self.manifest.id.clone(),
                source,
            })?;
        connection.stdin.flush().map_err(|source| HostError::Io {
            id: self.manifest.id.clone(),
            source,
        })?;
        Ok(id)
    }

    fn send_notification_on(
        &self,
        connection: &mut ExtensionConnection,
        method: &str,
        params: impl Serialize,
    ) -> Result<(), HostError> {
        let notification =
            JsonRpcNotification::new(method, params).map_err(|source| HostError::InvalidJson {
                id: self.manifest.id.clone(),
                source,
            })?;
        let mut encoded =
            serde_json::to_vec(&notification).map_err(|source| HostError::InvalidJson {
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
        connection
            .stdin
            .write_all(&encoded)
            .map_err(|source| HostError::Io {
                id: self.manifest.id.clone(),
                source,
            })?;
        connection.stdin.flush().map_err(|source| HostError::Io {
            id: self.manifest.id.clone(),
            source,
        })
    }

    fn send_notification(&mut self, method: &str, params: impl Serialize) -> Result<(), HostError> {
        let mut connection = self.try_connection()?;
        self.send_notification_on(&mut connection, method, params)
    }

    fn receive_protocol_line_on(
        &self,
        connection: &ExtensionConnection,
        expected_id: u64,
        deadline: Instant,
    ) -> Result<String, HostError> {
        loop {
            let timeout = deadline.saturating_duration_since(Instant::now());
            let line = connection
                .responses
                .recv_timeout(timeout)
                .map_err(|error| match error {
                    mpsc::RecvTimeoutError::Timeout => HostError::Timeout(self.manifest.id.clone()),
                    mpsc::RecvTimeoutError::Disconnected => {
                        HostError::Closed(self.manifest.id.clone())
                    }
                })?
                .map_err(|source| HostError::Io {
                    id: self.manifest.id.clone(),
                    source,
                })?;
            // A native process cannot retain a valid output or stdin capability after its CLI
            // response. Drop a late notification here so it can never impersonate the response
            // to a later request on the same process.
            if parse_cli_output_notification(&line).is_some()
                || parse_cli_stdin_read_notification(&line).is_some()
            {
                continue;
            }
            if json_rpc_response_id(&line).is_some_and(|id| id < expected_id) {
                continue;
            }
            return Ok(line);
        }
    }

    fn decode_response(&self, id: u64, line: &str) -> Result<Value, HostError> {
        if line.len() > MAX_MESSAGE_BYTES {
            return Err(HostError::Oversized {
                id: self.manifest.id.clone(),
                bytes: line.len(),
            });
        }
        let response: JsonRpcResponse =
            serde_json::from_str(line).map_err(|source| HostError::InvalidJson {
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
            let mut message = sanitize_terminal_text(
                &error.message,
                SanitizeOptions {
                    preserve_newlines: false,
                    preserve_tabs: false,
                    preserve_ansi_style: false,
                },
            );
            if let Some(suggestions) = error
                .data
                .as_ref()
                .and_then(|data| data.get("suggestions"))
                .and_then(Value::as_array)
            {
                for suggestion in suggestions.iter().filter_map(Value::as_str) {
                    message.push('\n');
                    message.push_str(&sanitize_terminal_text(
                        suggestion,
                        SanitizeOptions {
                            preserve_newlines: false,
                            preserve_tabs: false,
                            preserve_ansi_style: false,
                        },
                    ));
                }
            }
            return Err(HostError::Remote {
                id: self.manifest.id.clone(),
                message,
            });
        }
        Ok(response.result.unwrap_or(Value::Null))
    }

    /// Invoke a registered extension CLI command while retaining ownership of terminal output.
    pub fn invoke_cli_command(
        &mut self,
        command_name: &str,
        args: Vec<String>,
        cwd: &Path,
        timeout: Duration,
        stdout: &mut dyn Write,
        stderr: &mut dyn Write,
    ) -> Result<CliCommandExecution, HostError> {
        self.invoke_cli_command_cancellable_with_input(
            command_name,
            args,
            cwd,
            timeout,
            &AtomicBool::new(false),
            &mut std::io::empty(),
            stdout,
            stderr,
        )
    }

    /// Invoke a CLI command and forward cooperative cancellation over JSON-RPC.
    #[allow(clippy::too_many_arguments)]
    pub fn invoke_cli_command_cancellable(
        &mut self,
        command_name: &str,
        args: Vec<String>,
        cwd: &Path,
        timeout: Duration,
        cancelled: &AtomicBool,
        stdout: &mut dyn Write,
        stderr: &mut dyn Write,
    ) -> Result<CliCommandExecution, HostError> {
        self.invoke_cli_command_cancellable_with_input(
            command_name,
            args,
            cwd,
            timeout,
            cancelled,
            &mut std::io::empty(),
            stdout,
            stderr,
        )
    }

    /// Invoke a CLI command with a lazily leased host-stdin source.
    #[allow(clippy::too_many_arguments)]
    pub fn invoke_cli_command_with_input(
        &mut self,
        command_name: &str,
        args: Vec<String>,
        cwd: &Path,
        timeout: Duration,
        stdin: &mut dyn Read,
        stdout: &mut dyn Write,
        stderr: &mut dyn Write,
    ) -> Result<CliCommandExecution, HostError> {
        self.invoke_cli_command_cancellable_with_input(
            command_name,
            args,
            cwd,
            timeout,
            &AtomicBool::new(false),
            stdin,
            stdout,
            stderr,
        )
    }

    /// Invoke a CLI command with lazy stdin and cooperative cancellation.
    #[allow(clippy::too_many_arguments)]
    pub fn invoke_cli_command_cancellable_with_input(
        &mut self,
        command_name: &str,
        args: Vec<String>,
        cwd: &Path,
        timeout: Duration,
        cancelled: &AtomicBool,
        stdin: &mut dyn Read,
        stdout: &mut dyn Write,
        stderr: &mut dyn Write,
    ) -> Result<CliCommandExecution, HostError> {
        if !self.handshake.registrations.iter().any(|registration| {
            matches!(registration, Registration::CliCommand(command) if command.name == command_name)
        }) {
            return Err(HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "CLI command",
                message: format!("command {command_name:?} is not registered"),
            });
        }

        let mut connection = self.try_connection()?;
        if connection.pending_request.is_some() {
            return Err(HostError::Busy(self.manifest.id.clone()));
        }
        let id = self.send_request_on(
            &mut connection,
            "workdeck/cli/invoke",
            CliCommandInvocation {
                command_name: command_name.to_owned(),
                args,
                cwd: cwd.to_owned(),
            },
        )?;
        let deadline = Instant::now() + timeout;
        let mut stdout_bytes = 0_usize;
        let mut cancellation_sent = false;
        let mut stdin_read_started = false;
        let mut stdin_consumed = false;
        let mut stdin_done = false;
        let mut seen_stdin_reads = BTreeSet::new();
        let mut deferred_io_error = None;

        let value = loop {
            if cancelled.load(Ordering::Acquire) && !cancellation_sent {
                self.send_notification_on(
                    &mut connection,
                    "$/cancelRequest",
                    serde_json::json!({ "id": id }),
                )?;
                cancellation_sent = true;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(HostError::Timeout(self.manifest.id.clone()));
            }
            let wait = remaining.min(Duration::from_millis(25));
            let line = match connection.responses.recv_timeout(wait) {
                Ok(Ok(line)) => line,
                Ok(Err(source)) => {
                    return Err(HostError::Io {
                        id: self.manifest.id.clone(),
                        source,
                    });
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(HostError::Closed(self.manifest.id.clone()));
                }
            };
            if line.len() > MAX_MESSAGE_BYTES {
                return Err(HostError::Oversized {
                    id: self.manifest.id.clone(),
                    bytes: line.len(),
                });
            }
            if let Some(output) = parse_cli_output_notification(&line) {
                if output.request_id != id {
                    // Output belonging to a settled request is a revoked late write. It must not
                    // reach the terminal or poison the response stream for this invocation.
                    continue;
                }
                match output.stream {
                    CliOutputStream::Stdout => {
                        stdout_bytes = stdout_bytes.saturating_add(output.bytes.len());
                        if let Err(source) = stdout
                            .write_all(&output.bytes)
                            .and_then(|()| stdout.flush())
                            && deferred_io_error.is_none()
                        {
                            deferred_io_error = Some(source);
                        }
                    }
                    CliOutputStream::Stderr => {
                        if let Err(source) = stderr
                            .write_all(&output.bytes)
                            .and_then(|()| stderr.flush())
                            && deferred_io_error.is_none()
                        {
                            deferred_io_error = Some(source);
                        }
                    }
                }
                continue;
            }
            if let Some(read) = parse_cli_stdin_read_notification(&line) {
                if read.request_id != id {
                    // A read requested after its handler settled owns no host-stdin lease.
                    continue;
                }
                if read.max_bytes == 0 || read.max_bytes > MAX_CLI_STDIN_CHUNK_BYTES {
                    return Err(HostError::InvalidPayload {
                        id: self.manifest.id.clone(),
                        kind: "CLI stdin",
                        message: format!(
                            "stdin max_bytes must be from 1 through {MAX_CLI_STDIN_CHUNK_BYTES}"
                        ),
                    });
                }
                if !seen_stdin_reads.insert(read.read_id) {
                    return Err(HostError::InvalidPayload {
                        id: self.manifest.id.clone(),
                        kind: "CLI stdin",
                        message: format!("stdin read id {} was reused", read.read_id),
                    });
                }
                stdin_read_started = true;
                let mut bytes = vec![0; read.max_bytes];
                let (count, error) = if stdin_done {
                    (0, None)
                } else {
                    match stdin.read(&mut bytes) {
                        Ok(count) => (count, None),
                        Err(source) => {
                            let message = source.to_string();
                            if deferred_io_error.is_none() {
                                deferred_io_error = Some(source);
                            }
                            (0, Some(message))
                        }
                    }
                };
                bytes.truncate(count);
                stdin_consumed |= count > 0;
                stdin_done |= count == 0;
                self.send_notification_on(
                    &mut connection,
                    "workdeck/cli/stdin/chunk",
                    CliStdinChunk {
                        request_id: id,
                        read_id: read.read_id,
                        bytes,
                        done: stdin_done,
                        error,
                    },
                )?;
                continue;
            }
            if json_rpc_response_id(&line).is_some_and(|response_id| response_id < id) {
                continue;
            }
            break self.decode_response(id, &line)?;
        };

        if let Some(source) = deferred_io_error {
            return Err(HostError::Io {
                id: self.manifest.id.clone(),
                source,
            });
        }

        let mut execution: CliCommandExecution =
            serde_json::from_value(value).map_err(|error| HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "CLI command",
                message: error.to_string(),
            })?;
        // The host owns stdin and does not trust subprocess-reported lease metadata.
        execution.stdin_read_started = stdin_read_started;
        execution.stdin_consumed = stdin_consumed;
        validate_cli_execution(&execution, stdout_bytes).map_err(|message| {
            HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "CLI command",
                message,
            }
        })?;
        Ok(execution)
    }

    pub fn apply_changeset_transforms(&mut self, mut changeset: Changeset) -> Changeset {
        let transforms = changeset_transform_ids(&self.handshake);
        for transform_id in transforms {
            let attempt = self
                .request(
                    "workdeck/changeset/transform",
                    TransformRequest {
                        transform_id,
                        changeset: changeset.clone(),
                    },
                    Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS),
                )
                .map_err(|error| error.to_string());
            changeset = settle_transform_attempt(
                &self.manifest.id,
                &self.notifications,
                changeset,
                attempt,
            );
        }
        changeset
    }

    /// Ask one registered native file view whether it accepts this immutable file snapshot.
    pub fn file_view_matches(
        &mut self,
        view_id: &str,
        file: ExtensionDiffFile,
    ) -> Result<bool, HostError> {
        self.require_file_view(view_id)?;
        let value = self.request(
            "workdeck/file-view/matches",
            FileViewMatchRequest {
                view_id: view_id.to_owned(),
                file,
            },
            Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS),
        )?;
        serde_json::from_value(value).map_err(|error| HostError::InvalidPayload {
            id: self.manifest.id.clone(),
            kind: "file view match",
            message: error.to_string(),
        })
    }

    /// Calculate and validate one immutable native file-view presentation.
    pub fn layout_file_view(
        &mut self,
        view_id: &str,
        input: FileViewInput,
    ) -> Result<Option<ValidatedFileViewLayout>, HostError> {
        self.require_file_view(view_id)?;
        if input.cancellation.is_cancelled() {
            return Ok(None);
        }
        let mut documents = BTreeMap::new();
        for side in [ExtensionFileSide::Old, ExtensionFileSide::New] {
            let document = input
                .documents
                .read_document(side)
                .wait(&input.cancellation)
                .map_err(|_| HostError::InvalidPayload {
                    id: self.manifest.id.clone(),
                    kind: "file view layout",
                    message: "layout request was aborted".into(),
                })?;
            documents.insert(side, document);
        }
        if input.cancellation.is_cancelled() {
            return Ok(None);
        }
        let hunk_count = input.file.hunks.len();
        let value = self.request(
            "workdeck/file-view/layout",
            FileViewLayoutRequest {
                view_id: view_id.to_owned(),
                file: input.file.as_ref().clone(),
                width: input.width,
                changes: input.changes.as_ref().to_vec(),
                documents: documents.clone(),
                aborted: false,
            },
            Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS),
        )?;
        if value.is_null() {
            return Ok(None);
        }
        let validated =
            validate_file_view_layout(&value, hunk_count, input.width).map_err(|message| {
                HostError::InvalidPayload {
                    id: self.manifest.id.clone(),
                    kind: "file view layout",
                    message,
                }
            })?;
        if let Some(issue) = validate_file_view_source_ranges(&validated.layout, &documents) {
            return Err(HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "file view layout",
                message: issue.detail,
            });
        }
        Ok(Some(validated))
    }

    /// Calculate one registered native highlighter's marks for an immutable file snapshot.
    pub fn highlight_file(
        &mut self,
        highlighter_id: &str,
        file: &DiffFile,
    ) -> Result<Value, HostError> {
        self.highlight_file_cancellable(highlighter_id, file, &AtomicBool::new(false))
    }

    /// Calculate marks while allowing a review reload to revoke the request promptly.
    pub fn highlight_file_cancellable(
        &mut self,
        highlighter_id: &str,
        file: &DiffFile,
        cancelled: &AtomicBool,
    ) -> Result<Value, HostError> {
        self.require_line_highlighter(highlighter_id)?;
        let documents = [
            (
                ExtensionFileSide::Old,
                file.sources
                    .old
                    .as_ref()
                    .map(|source| source.content.clone()),
            ),
            (
                ExtensionFileSide::New,
                file.sources
                    .new
                    .as_ref()
                    .map(|source| source.content.clone()),
            ),
        ]
        .into_iter()
        .collect();
        self.request_cancellable(
            "workdeck/line-highlighter/highlight",
            LineHighlightRequest {
                highlighter_id: highlighter_id.to_owned(),
                file: project_extension_diff_file(file),
                documents,
                aborted: false,
            },
            LINE_HIGHLIGHT_TIMEOUT,
            cancelled,
        )
    }

    /// Notify an attached file-view mode that it acquired or released the keyboard.
    pub fn file_view_mode_lifecycle(
        &mut self,
        method: &str,
        request: FileViewModeLifecycleRequest,
    ) -> Result<CommandExecution, HostError> {
        self.require_interactive_file_view(&request.view_id)?;
        let value = self.request(
            method,
            request,
            Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS),
        )?;
        let execution: CommandExecution =
            serde_json::from_value(value).map_err(|error| HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "file view mode lifecycle",
                message: error.to_string(),
            })?;
        self.validate_host_actions(&execution.actions, "file view mode lifecycle")?;
        Ok(execution)
    }

    /// Route one key through an attached file-view mode.
    pub fn route_file_view_mode_key(
        &mut self,
        request: FileViewModeKeyRequest,
    ) -> Result<KeyboardModeExecution, HostError> {
        self.require_interactive_file_view(&request.view_id)?;
        let value = self.request(
            "workdeck/file-view-mode/key",
            request,
            Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS),
        )?;
        let execution: KeyboardModeExecution =
            serde_json::from_value(value).map_err(|error| HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "file view mode key",
                message: error.to_string(),
            })?;
        self.validate_host_actions(&execution.actions, "file view mode key")?;
        Ok(execution)
    }

    /// Return the result of a host-owned, consented workspace operation.
    pub fn complete_workspace_write(
        &mut self,
        completion: ExtensionWorkspaceWriteCompletion,
    ) -> Result<CommandExecution, HostError> {
        let value = self.request(
            "workdeck/workspace/write-complete",
            completion,
            Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS),
        )?;
        let execution: CommandExecution =
            serde_json::from_value(value).map_err(|error| HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "workspace write completion",
                message: error.to_string(),
            })?;
        self.validate_host_actions(&execution.actions, "workspace write completion")?;
        Ok(execution)
    }

    /// Invoke one registered pane's synchronous availability callback.
    pub fn pane_available(&mut self, request: PaneAvailabilityRequest) -> Result<bool, HostError> {
        let pane = self
            .handshake
            .registrations
            .iter()
            .find_map(|registration| match registration {
                Registration::Pane(pane) if pane.id == request.pane_id => Some(pane),
                _ => None,
            })
            .ok_or_else(|| HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "pane availability",
                message: format!("pane {:?} is not registered", request.pane_id),
            })?;
        if request.placement != pane.placement {
            return Err(HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "pane availability",
                message: format!(
                    "pane {:?} requested placement {:?}, registered as {:?}",
                    pane.id, request.placement, pane.placement
                ),
            });
        }
        if !pane.available {
            return Ok(true);
        }
        if !pane.current_line && request.current_line.is_some() {
            return Err(HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "pane availability",
                message: format!("pane {:?} did not opt into current-line context", pane.id),
            });
        }
        let value = self.request(
            "workdeck/pane/available",
            request,
            Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS),
        )?;
        let response: PaneAvailabilityResponse =
            serde_json::from_value(value).map_err(|error| HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "pane availability",
                message: error.to_string(),
            })?;
        Ok(response.available)
    }

    /// Render one registered pane inside the exact rectangle allocated by the host.
    pub fn render_pane(
        &mut self,
        request: PaneRenderRequest,
    ) -> Result<ExtensionPaneView, HostError> {
        let pane = self
            .handshake
            .registrations
            .iter()
            .find_map(|registration| match registration {
                Registration::Pane(pane) if pane.id == request.pane_id => Some(pane.clone()),
                _ => None,
            })
            .ok_or_else(|| HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "pane",
                message: format!("pane {:?} is not registered", request.pane_id),
            })?;
        if request.placement != pane.placement {
            return Err(HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "pane",
                message: format!(
                    "pane {:?} requested placement {:?}, registered as {:?}",
                    pane.id, request.placement, pane.placement
                ),
            });
        }
        if request.width == 0 || request.height == 0 {
            return Err(HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "pane",
                message: "pane render rectangles must be non-empty".into(),
            });
        }
        let value = self.request(
            "workdeck/pane/render",
            request,
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
    }

    /// Invoke one extension-owned action embedded in a declarative pane tree.
    pub fn invoke_pane_action(
        &mut self,
        invocation: PaneActionInvocation,
    ) -> Result<CommandExecution, HostError> {
        if !self.handshake.registrations.iter().any(|registration| {
            matches!(registration, Registration::Pane(pane) if pane.id == invocation.pane_id)
        }) {
            return Err(HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "pane action",
                message: format!("pane {:?} is not registered", invocation.pane_id),
            });
        }
        if invocation.action_id.trim().is_empty() || invocation.action_id.len() > 1_024 {
            return Err(HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "pane action",
                message: "pane action ids must be 1..=1024 bytes".into(),
            });
        }
        let value = self.request(
            "workdeck/pane/action",
            invocation,
            Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS),
        )?;
        let execution: CommandExecution =
            serde_json::from_value(value).map_err(|error| HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "pane action",
                message: error.to_string(),
            })?;
        self.validate_host_actions(&execution.actions, "pane action")?;
        Ok(execution)
    }

    #[must_use]
    pub fn subscribes_to_event(&self, name: &str) -> bool {
        self.handshake.registrations.iter().any(|registration| {
            matches!(
                registration,
                Registration::EventSubscription { names }
                    | Registration::CustomEventSubscription { names }
                    if names.iter().any(|candidate| candidate == name)
            )
        })
    }

    /// Deliver one host lifecycle or namespaced extension event to a declared subscriber.
    pub fn deliver_event(&mut self, event: ReviewEvent) -> Result<CommandExecution, HostError> {
        if self.registry.phase() != ExtensionEventBusPhase::Ready && event.name != "shutdown" {
            return Ok(CommandExecution::default());
        }
        if !self.subscribes_to_event(&event.name) {
            return Err(HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "event",
                message: format!("event {:?} is not subscribed", event.name),
            });
        }
        let value = self.request(
            "workdeck/event",
            event,
            Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS),
        )?;
        let execution: CommandExecution =
            serde_json::from_value(value).map_err(|error| HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "event",
                message: error.to_string(),
            })?;
        self.validate_host_actions(&execution.actions, "event")?;
        Ok(execution)
    }

    /// Start one lifecycle/custom event without waiting on extension code.
    ///
    /// A native process owns one ordered request stream, so event delivery is serialized with
    /// commands for that extension while remaining independent of every other extension and the
    /// Ratatui event loop.
    pub fn begin_event(&mut self, event: ReviewEvent) -> Result<(), HostError> {
        if self.registry.phase() != ExtensionEventBusPhase::Ready {
            return Ok(());
        }
        let mut connection = self.try_connection()?;
        if connection.pending_request.is_some() {
            return Err(HostError::Busy(self.manifest.id.clone()));
        }
        if !self.subscribes_to_event(&event.name) {
            return Err(HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "event",
                message: format!("event {:?} is not subscribed", event.name),
            });
        }
        let id = self.send_request_on(&mut connection, "workdeck/event", event)?;
        connection.pending_request = Some(PendingExecutionRequest {
            id,
            deadline: Instant::now() + Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS),
            kind: PendingExecutionKind::Event,
        });
        Ok(())
    }

    /// Invoke one registered in-review command and validate all requested host mutations.
    pub fn invoke_command(
        &mut self,
        command_id: &str,
        snapshot: ReviewSnapshot,
        open_panes: Vec<String>,
    ) -> Result<CommandExecution, HostError> {
        self.invoke_command_with_context(command_id, snapshot, open_panes, None)
    }

    pub fn invoke_command_with_context(
        &mut self,
        command_id: &str,
        snapshot: ReviewSnapshot,
        open_panes: Vec<String>,
        active_keyboard_mode: Option<String>,
    ) -> Result<CommandExecution, HostError> {
        self.invoke_command_with_review_context(
            command_id,
            snapshot,
            open_panes,
            active_keyboard_mode,
            std::env::current_dir().unwrap_or_default(),
            None,
        )
    }

    pub fn invoke_command_with_review_context(
        &mut self,
        command_id: &str,
        snapshot: ReviewSnapshot,
        open_panes: Vec<String>,
        active_keyboard_mode: Option<String>,
        cwd: PathBuf,
        review: Option<workdeck_extension_api::ExtensionReviewSnapshot>,
    ) -> Result<CommandExecution, HostError> {
        self.invoke_command_with_workspace_context(
            command_id,
            snapshot,
            open_panes,
            active_keyboard_mode,
            cwd,
            review,
            ExtensionCommandAvailability::default(),
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn invoke_command_with_workspace_context(
        &mut self,
        command_id: &str,
        snapshot: ReviewSnapshot,
        open_panes: Vec<String>,
        active_keyboard_mode: Option<String>,
        cwd: PathBuf,
        review: Option<workdeck_extension_api::ExtensionReviewSnapshot>,
        commands: ExtensionCommandAvailability,
        workspace: Option<ExtensionWorkspaceSnapshot>,
    ) -> Result<CommandExecution, HostError> {
        if !self.handshake.registrations.iter().any(|registration| {
            matches!(registration, Registration::Command(command) if command.id == command_id)
        }) {
            return Err(HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "command",
                message: format!("command {command_id:?} is not registered"),
            });
        }
        let selection = build_extension_review_selection_from_snapshot(&snapshot);
        let value = self.request(
            "workdeck/command/invoke",
            CommandInvocation {
                command_id: command_id.to_owned(),
                snapshot,
                selection,
                cwd,
                review,
                open_panes,
                active_keyboard_mode,
                workspace,
                commands,
            },
            Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS),
        )?;
        let execution: CommandExecution =
            serde_json::from_value(value).map_err(|error| HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "command",
                message: error.to_string(),
            })?;
        self.validate_host_actions(&execution.actions, "command")?;
        Ok(execution)
    }

    /// Start one command without waiting on the extension process.
    ///
    /// Ratatui polls the result from its event loop, preserving Hunk's rule that selection is
    /// frozen at invocation while the rest of the review can continue changing during an await.
    #[allow(clippy::too_many_arguments)]
    pub fn begin_command_with_workspace_context(
        &mut self,
        command_id: &str,
        snapshot: ReviewSnapshot,
        open_panes: Vec<String>,
        active_keyboard_mode: Option<String>,
        cwd: PathBuf,
        review: Option<workdeck_extension_api::ExtensionReviewSnapshot>,
        commands: ExtensionCommandAvailability,
        workspace: Option<ExtensionWorkspaceSnapshot>,
    ) -> Result<(), HostError> {
        if self.registry.phase() != ExtensionEventBusPhase::Ready {
            return Err(HostError::Closed(self.manifest.id.clone()));
        }
        let mut connection = self.try_connection()?;
        if connection.pending_request.is_some() {
            return Err(HostError::Busy(self.manifest.id.clone()));
        }
        if !self.handshake.registrations.iter().any(|registration| {
            matches!(registration, Registration::Command(command) if command.id == command_id)
        }) {
            return Err(HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "command",
                message: format!("command {command_id:?} is not registered"),
            });
        }
        let selection = build_extension_review_selection_from_snapshot(&snapshot);
        let id = self.send_request_on(
            &mut connection,
            "workdeck/command/invoke",
            CommandInvocation {
                command_id: command_id.to_owned(),
                snapshot,
                selection,
                cwd,
                review,
                open_panes,
                active_keyboard_mode,
                workspace,
                commands,
            },
        )?;
        connection.pending_request = Some(PendingExecutionRequest {
            id,
            deadline: Instant::now() + Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS),
            kind: PendingExecutionKind::Command,
        });
        Ok(())
    }

    #[must_use]
    pub fn command_pending(&self) -> bool {
        self.connection.try_lock().is_ok_and(|connection| {
            matches!(
                connection.pending_request,
                Some(PendingExecutionRequest {
                    kind: PendingExecutionKind::Command,
                    ..
                })
            )
        })
    }

    #[must_use]
    pub fn event_pending(&self) -> bool {
        self.connection.try_lock().is_ok_and(|connection| {
            matches!(
                connection.pending_request,
                Some(PendingExecutionRequest {
                    kind: PendingExecutionKind::Event,
                    ..
                })
            )
        })
    }

    #[must_use]
    pub fn request_pending(&self) -> bool {
        self.connection
            .try_lock()
            .map_or(true, |connection| connection.pending_request.is_some())
    }

    /// Poll the in-flight command once without blocking the host event loop.
    pub fn poll_command(&mut self) -> Option<Result<CommandExecution, HostError>> {
        self.poll_execution(PendingExecutionKind::Command, "command")
    }

    /// Poll one fire-and-forget event handler without blocking the host event loop.
    pub fn poll_event(&mut self) -> Option<Result<CommandExecution, HostError>> {
        self.poll_execution(PendingExecutionKind::Event, "event")
    }

    fn poll_execution(
        &mut self,
        expected: PendingExecutionKind,
        payload_kind: &'static str,
    ) -> Option<Result<CommandExecution, HostError>> {
        let mut connection = match self.try_connection() {
            Ok(connection) => connection,
            Err(error) => return Some(Err(error)),
        };
        let pending = connection.pending_request?;
        if pending.kind != expected {
            return None;
        }
        let line = loop {
            match connection.responses.try_recv() {
                Ok(Ok(line)) => {
                    if parse_cli_output_notification(&line).is_some()
                        || parse_cli_stdin_read_notification(&line).is_some()
                    {
                        continue;
                    }
                    if json_rpc_response_id(&line)
                        .is_some_and(|response_id| response_id < pending.id)
                    {
                        continue;
                    }
                    break line;
                }
                Ok(Err(source)) => {
                    connection.pending_request = None;
                    return Some(Err(HostError::Io {
                        id: self.manifest.id.clone(),
                        source,
                    }));
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    connection.pending_request = None;
                    return Some(Err(HostError::Closed(self.manifest.id.clone())));
                }
                Err(mpsc::TryRecvError::Empty) if Instant::now() >= pending.deadline => {
                    let _ = self.send_notification_on(
                        &mut connection,
                        "$/cancelRequest",
                        serde_json::json!({ "id": pending.id }),
                    );
                    connection.pending_request = None;
                    return Some(Err(HostError::Timeout(self.manifest.id.clone())));
                }
                Err(mpsc::TryRecvError::Empty) => return None,
            }
        };
        connection.pending_request = None;
        let result = self.decode_response(pending.id, &line).and_then(|value| {
            let execution: CommandExecution =
                serde_json::from_value(value).map_err(|error| HostError::InvalidPayload {
                    id: self.manifest.id.clone(),
                    kind: payload_kind,
                    message: error.to_string(),
                })?;
            self.validate_host_actions(&execution.actions, payload_kind)?;
            Ok(execution)
        });
        Some(result)
    }

    pub fn enter_keyboard_mode(
        &mut self,
        mode_id: &str,
        snapshot: ReviewSnapshot,
    ) -> Result<CommandExecution, HostError> {
        self.enter_keyboard_mode_with_commands(
            mode_id,
            snapshot,
            ExtensionCommandAvailability::default(),
        )
    }

    pub fn enter_keyboard_mode_with_commands(
        &mut self,
        mode_id: &str,
        snapshot: ReviewSnapshot,
        commands: ExtensionCommandAvailability,
    ) -> Result<CommandExecution, HostError> {
        self.keyboard_mode_lifecycle("workdeck/keyboard-mode/enter", mode_id, snapshot, commands)
    }

    pub fn exit_keyboard_mode(
        &mut self,
        mode_id: &str,
        snapshot: ReviewSnapshot,
    ) -> Result<CommandExecution, HostError> {
        self.exit_keyboard_mode_with_commands(
            mode_id,
            snapshot,
            ExtensionCommandAvailability::default(),
        )
    }

    pub fn exit_keyboard_mode_with_commands(
        &mut self,
        mode_id: &str,
        snapshot: ReviewSnapshot,
        commands: ExtensionCommandAvailability,
    ) -> Result<CommandExecution, HostError> {
        self.keyboard_mode_lifecycle("workdeck/keyboard-mode/exit", mode_id, snapshot, commands)
    }

    fn keyboard_mode_lifecycle(
        &mut self,
        method: &str,
        mode_id: &str,
        snapshot: ReviewSnapshot,
        commands: ExtensionCommandAvailability,
    ) -> Result<CommandExecution, HostError> {
        self.require_keyboard_mode(mode_id)?;
        let value = self.request(
            method,
            KeyboardModeLifecycleRequest {
                mode_id: mode_id.to_owned(),
                snapshot,
                commands,
            },
            Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS),
        )?;
        let execution: CommandExecution =
            serde_json::from_value(value).map_err(|error| HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "keyboard mode lifecycle",
                message: error.to_string(),
            })?;
        self.validate_host_actions(&execution.actions, "keyboard mode lifecycle")?;
        Ok(execution)
    }

    pub fn route_keyboard_mode_key(
        &mut self,
        mode_id: &str,
        key: ExtensionKeyEvent,
        snapshot: ReviewSnapshot,
    ) -> Result<KeyboardModeExecution, HostError> {
        self.route_keyboard_mode_key_with_commands(
            mode_id,
            key,
            snapshot,
            ExtensionCommandAvailability::default(),
        )
    }

    pub fn route_keyboard_mode_key_with_commands(
        &mut self,
        mode_id: &str,
        key: ExtensionKeyEvent,
        snapshot: ReviewSnapshot,
        commands: ExtensionCommandAvailability,
    ) -> Result<KeyboardModeExecution, HostError> {
        self.require_keyboard_mode(mode_id)?;
        let value = self.request(
            "workdeck/keyboard-mode/key",
            KeyboardModeKeyRequest {
                mode_id: mode_id.to_owned(),
                key,
                snapshot,
                commands,
            },
            Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS),
        )?;
        let execution: KeyboardModeExecution =
            serde_json::from_value(value).map_err(|error| HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "keyboard mode key",
                message: error.to_string(),
            })?;
        self.validate_host_actions(&execution.actions, "keyboard mode key")?;
        Ok(execution)
    }

    pub fn submit_input_dialog(
        &mut self,
        action_id: &str,
        value: Option<String>,
        snapshot: ReviewSnapshot,
        active_keyboard_mode: Option<String>,
    ) -> Result<CommandExecution, HostError> {
        self.submit_input_dialog_with_context(
            action_id,
            value,
            snapshot,
            active_keyboard_mode,
            std::env::current_dir().unwrap_or_default(),
            None,
            ExtensionCommandAvailability::default(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn submit_input_dialog_with_context(
        &mut self,
        action_id: &str,
        value: Option<String>,
        snapshot: ReviewSnapshot,
        active_keyboard_mode: Option<String>,
        cwd: PathBuf,
        review: Option<workdeck_extension_api::ExtensionReviewSnapshot>,
        commands: ExtensionCommandAvailability,
    ) -> Result<CommandExecution, HostError> {
        let value = self.request(
            "workdeck/dialog/input",
            InputDialogSubmission {
                action_id: action_id.to_owned(),
                value,
                snapshot,
                cwd,
                review,
                active_keyboard_mode,
                commands,
            },
            Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS),
        )?;
        let execution: CommandExecution =
            serde_json::from_value(value).map_err(|error| HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "input dialog",
                message: error.to_string(),
            })?;
        self.validate_host_actions(&execution.actions, "input dialog")?;
        Ok(execution)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn submit_select_dialog_with_context(
        &mut self,
        action_id: &str,
        value: Option<String>,
        snapshot: ReviewSnapshot,
        active_keyboard_mode: Option<String>,
        cwd: PathBuf,
        review: Option<workdeck_extension_api::ExtensionReviewSnapshot>,
        commands: ExtensionCommandAvailability,
    ) -> Result<CommandExecution, HostError> {
        let value = self.request(
            "workdeck/dialog/select",
            SelectDialogSubmission {
                action_id: action_id.to_owned(),
                value,
                snapshot,
                cwd,
                review,
                active_keyboard_mode,
                commands,
            },
            Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS),
        )?;
        let execution: CommandExecution =
            serde_json::from_value(value).map_err(|error| HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "select dialog",
                message: error.to_string(),
            })?;
        self.validate_host_actions(&execution.actions, "select dialog")?;
        Ok(execution)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn submit_confirm_dialog_with_context(
        &mut self,
        action_id: &str,
        confirmed: bool,
        snapshot: ReviewSnapshot,
        active_keyboard_mode: Option<String>,
        cwd: PathBuf,
        review: Option<workdeck_extension_api::ExtensionReviewSnapshot>,
        commands: ExtensionCommandAvailability,
    ) -> Result<CommandExecution, HostError> {
        let value = self.request(
            "workdeck/dialog/confirm",
            ConfirmDialogSubmission {
                action_id: action_id.to_owned(),
                confirmed,
                snapshot,
                cwd,
                review,
                active_keyboard_mode,
                commands,
            },
            Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS),
        )?;
        let execution: CommandExecution =
            serde_json::from_value(value).map_err(|error| HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "confirm dialog",
                message: error.to_string(),
            })?;
        self.validate_host_actions(&execution.actions, "confirm dialog")?;
        Ok(execution)
    }

    fn require_keyboard_mode(&self, mode_id: &str) -> Result<(), HostError> {
        if self.handshake.registrations.iter().any(|registration| {
            matches!(registration, Registration::KeyboardMode(mode) if mode.id == mode_id)
        }) {
            Ok(())
        } else {
            Err(HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "keyboard mode",
                message: format!("mode {mode_id:?} is not registered"),
            })
        }
    }

    fn require_file_view(&self, view_id: &str) -> Result<(), HostError> {
        if self.handshake.registrations.iter().any(|registration| {
            matches!(registration, Registration::FileView { id, .. } if id == view_id)
        }) {
            Ok(())
        } else {
            Err(HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "file view",
                message: format!("file view {view_id:?} is not registered"),
            })
        }
    }

    fn require_line_highlighter(&self, highlighter_id: &str) -> Result<(), HostError> {
        if self.handshake.registrations.iter().any(|registration| {
            matches!(registration, Registration::LineHighlighter { id } if id == highlighter_id)
        }) {
            Ok(())
        } else {
            Err(HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "line highlighter",
                message: format!("line highlighter {highlighter_id:?} is not registered"),
            })
        }
    }

    fn require_interactive_file_view(&self, view_id: &str) -> Result<(), HostError> {
        if self.handshake.registrations.iter().any(|registration| {
            matches!(registration, Registration::FileView { id, interactive_mode: true, .. } if id == view_id)
        }) {
            Ok(())
        } else {
            Err(HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "file view mode",
                message: format!("interactive file view {view_id:?} is not registered"),
            })
        }
    }

    fn validate_host_actions(
        &self,
        actions: &[ExtensionHostAction],
        kind: &'static str,
    ) -> Result<(), HostError> {
        if actions.len() > 64 {
            return Err(HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind,
                message: "response exceeds 64 host actions".into(),
            });
        }
        let owns = |id: &str, registration_kind: &str| {
            let local_id = id
                .strip_prefix(&format!("{}:", self.manifest.id))
                .unwrap_or(id);
            if id.contains(':') && local_id == id {
                return false;
            }
            self.handshake
                .registrations
                .iter()
                .any(|registration| match registration {
                    Registration::Pane(pane) if registration_kind == "pane" => pane.id == local_id,
                    Registration::KeyboardMode(mode) if registration_kind == "keyboard mode" => {
                        mode.id == local_id
                    }
                    Registration::FileView { id, .. } if registration_kind == "file view" => {
                        id == local_id
                    }
                    _ => false,
                })
        };
        for action in actions {
            let valid = match action {
                ExtensionHostAction::OpenPane { id }
                | ExtensionHostAction::ClosePane { id }
                | ExtensionHostAction::RefreshPane { id } => {
                    self.manifest
                        .capabilities
                        .contains(&workdeck_extension_api::Capability::Panes)
                        && owns(id, "pane")
                }
                ExtensionHostAction::EnterKeyboardMode { id } => {
                    self.manifest
                        .capabilities
                        .contains(&workdeck_extension_api::Capability::KeyboardModes)
                        && kind != "keyboard mode lifecycle"
                        && owns(id, "keyboard mode")
                }
                ExtensionHostAction::ExitKeyboardMode => {
                    self.manifest
                        .capabilities
                        .contains(&workdeck_extension_api::Capability::KeyboardModes)
                        && kind != "keyboard mode lifecycle"
                }
                ExtensionHostAction::ExecuteReviewCommand { id, count } => {
                    self.manifest
                        .capabilities
                        .contains(&workdeck_extension_api::Capability::ReviewNavigation)
                        && is_public_review_command(id)
                        && count.is_none_or(|count| (1..=10_000).contains(&count))
                }
                ExtensionHostAction::TryReviewCommand {
                    id,
                    count,
                    unavailable_message,
                } => {
                    self.manifest
                        .capabilities
                        .contains(&workdeck_extension_api::Capability::ReviewNavigation)
                        && self
                            .manifest
                            .capabilities
                            .contains(&workdeck_extension_api::Capability::Notifications)
                        && is_public_review_command(id)
                        && count.is_none_or(|count| (1..=10_000).contains(&count))
                        && !unavailable_message.trim().is_empty()
                }
                ExtensionHostAction::SelectReviewFile { file_id } => {
                    self.manifest
                        .capabilities
                        .contains(&workdeck_extension_api::Capability::ReviewNavigation)
                        && !file_id.trim().is_empty()
                }
                ExtensionHostAction::SelectReviewHunk {
                    file_id,
                    hunk_index,
                } => {
                    self.manifest
                        .capabilities
                        .contains(&workdeck_extension_api::Capability::ReviewNavigation)
                        && !file_id.trim().is_empty()
                        && *hunk_index <= 1_000_000
                }
                ExtensionHostAction::RevealReviewLine { file_id, line, .. } => {
                    self.manifest
                        .capabilities
                        .contains(&workdeck_extension_api::Capability::ReviewNavigation)
                        && !file_id.trim().is_empty()
                        && *line > 0
                }
                ExtensionHostAction::ToggleFileView { id } => {
                    self.manifest
                        .capabilities
                        .contains(&workdeck_extension_api::Capability::FileViews)
                        && owns(id, "file view")
                }
                ExtensionHostAction::EnterFileViewMode { id } => {
                    self.manifest
                        .capabilities
                        .contains(&workdeck_extension_api::Capability::FileViews)
                        && kind != "file view mode lifecycle"
                        && owns(id, "file view")
                }
                ExtensionHostAction::ExitFileViewMode => {
                    self.manifest
                        .capabilities
                        .contains(&workdeck_extension_api::Capability::FileViews)
                        && kind != "file view mode lifecycle"
                }
                ExtensionHostAction::RefreshFileView { id, file_id } => {
                    self.manifest
                        .capabilities
                        .contains(&workdeck_extension_api::Capability::FileViews)
                        && owns(id, "file view")
                        && file_id
                            .as_deref()
                            .is_none_or(|file_id| !file_id.trim().is_empty())
                }
                ExtensionHostAction::RefreshLineHighlights { .. } => self
                    .manifest
                    .capabilities
                    .contains(&workdeck_extension_api::Capability::LineHighlighters),
                ExtensionHostAction::RequestWorkspaceWrite {
                    request_id,
                    file_id,
                    ..
                } => {
                    self.manifest
                        .capabilities
                        .contains(&workdeck_extension_api::Capability::WorkspaceWrite)
                        && !request_id.trim().is_empty()
                        && !file_id.trim().is_empty()
                        && matches!(kind, "command" | "file view mode key")
                }
                ExtensionHostAction::OpenInputDialog { id, title, .. } => {
                    self.manifest
                        .capabilities
                        .contains(&workdeck_extension_api::Capability::Dialogs)
                        && !id.trim().is_empty()
                        && !title.trim().is_empty()
                }
                ExtensionHostAction::OpenSelectDialog { id, title, options } => {
                    self.manifest
                        .capabilities
                        .contains(&workdeck_extension_api::Capability::Dialogs)
                        && !id.trim().is_empty()
                        && !title.trim().is_empty()
                        && !options.is_empty()
                        && options.len() <= 1_000
                        && options.iter().all(|option| option.len() <= 4 * 1_024)
                }
                ExtensionHostAction::OpenConfirmDialog {
                    id,
                    title,
                    body,
                    confirm_label,
                    cancel_label,
                } => {
                    self.manifest
                        .capabilities
                        .contains(&workdeck_extension_api::Capability::Dialogs)
                        && !id.trim().is_empty()
                        && !title.trim().is_empty()
                        && body.len() <= 64 * 1_024
                        && confirm_label.len() <= 4 * 1_024
                        && cancel_label
                            .as_ref()
                            .is_none_or(|label| label.len() <= 4 * 1_024)
                }
                ExtensionHostAction::EmitEvent { name, .. } => {
                    self.manifest
                        .capabilities
                        .contains(&workdeck_extension_api::Capability::Events)
                        && valid_custom_event_name(name)
                }
                ExtensionHostAction::Notify { .. } => self
                    .manifest
                    .capabilities
                    .contains(&workdeck_extension_api::Capability::Notifications),
            };
            if !valid {
                return Err(HostError::InvalidPayload {
                    id: self.manifest.id.clone(),
                    kind,
                    message: format!("invalid or undeclared host action {action:?}"),
                });
            }
        }
        Ok(())
    }
}

fn granted_extension_config(manifest: &ExtensionManifest, config: Value) -> Value {
    if manifest
        .capabilities
        .contains(&workdeck_extension_api::Capability::Configuration)
    {
        config
    } else {
        Value::Object(Default::default())
    }
}

fn is_public_review_command(id: &str) -> bool {
    let canonical = match id {
        "workdeck.view.cursor-line-row" => "workdeck.view.cursorLineRow",
        "workdeck.review.step-down" => "workdeck.review.stepDown",
        "workdeck.review.step-up" => "workdeck.review.stepUp",
        "workdeck.review.previous-hunk" => "workdeck.review.previousHunk",
        "workdeck.review.next-hunk" => "workdeck.review.nextHunk",
        "workdeck.review.align-current-line-top" => "workdeck.review.alignCurrentLineTop",
        "workdeck.review.align-current-line-center" => "workdeck.review.alignCurrentLineCenter",
        "workdeck.review.align-current-line-bottom" => "workdeck.review.alignCurrentLineBottom",
        "workdeck.review.half-page-down" => "workdeck.review.halfPageDown",
        "workdeck.review.half-page-up" => "workdeck.review.halfPageUp",
        "workdeck.review.jump-to-top" => "workdeck.review.jumpToTop",
        "workdeck.review.jump-to-bottom" => "workdeck.review.jumpToBottom",
        _ => id,
    };
    workdeck_review::app_command_catalog_entry(canonical)
        .is_some_and(|command| command.public_to_extensions)
}

fn valid_custom_event_name(name: &str) -> bool {
    !name.trim().is_empty()
}

#[derive(Debug, Deserialize)]
struct ExtensionNotifyParams {
    message: String,
    #[serde(default, rename = "type")]
    notification_type: Option<ExtensionNotifyType>,
}

#[derive(Debug)]
struct ParsedExtensionNotification {
    message: String,
    notification_type: ExtensionNotifyType,
}

fn json_rpc_response_id(line: &str) -> Option<u64> {
    let value = serde_json::from_str::<Value>(line).ok()?;
    let object = value.as_object()?;
    if object.get("jsonrpc")?.as_str()? != "2.0" || object.contains_key("method") {
        return None;
    }
    object.get("id")?.as_u64()
}

fn parse_extension_notification(line: &str) -> Option<ParsedExtensionNotification> {
    let value = serde_json::from_str::<Value>(line).ok()?;
    let object = value.as_object()?;
    if object.get("jsonrpc")?.as_str()? != "2.0"
        || object.get("method")?.as_str()? != "workdeck/notify"
        || object.contains_key("id")
    {
        return None;
    }
    let params =
        serde_json::from_value::<ExtensionNotifyParams>(object.get("params")?.clone()).ok()?;
    Some(ParsedExtensionNotification {
        message: params.message,
        notification_type: params
            .notification_type
            .unwrap_or(ExtensionNotifyType::Info),
    })
}

fn parse_cli_output_notification(line: &str) -> Option<CliOutputNotification> {
    let value = serde_json::from_str::<Value>(line).ok()?;
    let object = value.as_object()?;
    if object.get("jsonrpc")?.as_str()? != "2.0"
        || object.get("method")?.as_str()? != "workdeck/cli/output"
        || object.contains_key("id")
    {
        return None;
    }
    serde_json::from_value(object.get("params")?.clone()).ok()
}

fn parse_cli_stdin_read_notification(line: &str) -> Option<CliStdinReadRequest> {
    let value = serde_json::from_str::<Value>(line).ok()?;
    let object = value.as_object()?;
    if object.get("jsonrpc")?.as_str()? != "2.0"
        || object.get("method")?.as_str()? != "workdeck/cli/stdin/read"
        || object.contains_key("id")
    {
        return None;
    }
    serde_json::from_value(object.get("params")?.clone()).ok()
}

fn validate_cli_execution(
    execution: &CliCommandExecution,
    stdout_bytes: usize,
) -> Result<(), String> {
    if execution.stdin_consumed && !execution.stdin_read_started {
        return Err("stdin cannot be consumed before a read starts".into());
    }
    let CliCommandResult::Delegate { argv } = &execution.result else {
        return Ok(());
    };
    if argv.is_empty() {
        return Err("delegate argv must be a non-empty array of strings".into());
    }
    if argv.iter().any(|token| token.contains('\0')) {
        return Err("delegate argv must contain only strings without NUL characters".into());
    }
    if argv.iter().any(|token| {
        token == "--extension"
            || token.starts_with("--extension=")
            || token == "--extensions"
            || token == "--no-extensions"
    }) {
        return Err("delegate argv cannot change extension bootstrap flags".into());
    }
    if stdout_bytes > 0 {
        return Err("extension wrote to stdout before delegating to Workdeck".into());
    }
    if execution.stdin_read_started {
        return Err("extension read stdin before delegating to a built-in Workdeck command".into());
    }
    Ok(())
}

fn vcs_input_matches_operation(
    input: &ExtensionVcsReviewInput,
    operation: ExtensionVcsOperationKind,
) -> bool {
    matches!(
        (input, operation),
        (
            ExtensionVcsReviewInput::Vcs { .. },
            ExtensionVcsOperationKind::WorkingTreeDiff
        ) | (
            ExtensionVcsReviewInput::Show { .. },
            ExtensionVcsOperationKind::RevisionShow
        ) | (
            ExtensionVcsReviewInput::StashShow { .. },
            ExtensionVcsOperationKind::StashShow
        )
    )
}

#[cfg(test)]
fn validate_registrations(
    manifest: &ExtensionManifest,
    handshake: &HandshakeResponse,
) -> Result<(), HostError> {
    let mut handshake = handshake.clone();
    normalize_and_validate_registrations(manifest, &mut handshake).map_err(|message| {
        HostError::Handshake {
            id: manifest.id.clone(),
            message,
        }
    })
}

impl Drop for LoadedExtension {
    fn drop(&mut self) {
        if Arc::strong_count(&self.connection) == 1 {
            self.retire();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicBool;
    use tempfile::TempDir;

    #[test]
    fn trust_store_round_trips_atomically() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("legacy-trust.toml");
        let mut trust = TrustStore::default();
        trust.grant(directory.path(), TrustDecision::Legacy);
        trust.save(&path).unwrap();
        assert_eq!(
            TrustStore::load_legacy_toml(&path).decision(directory.path()),
            Some(TrustDecision::Legacy)
        );
    }

    #[test]
    fn handshake_keeps_duplicate_declarations_but_rejects_undeclared_capabilities() {
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
        assert!(validate_registrations(&manifest, &duplicate).is_ok());

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
    fn native_vcs_registration_defaults_to_no_operations_and_rejects_legacy_markers() {
        let manifest = ExtensionManifest {
            id: "fossil-tools".into(),
            name: "Fossil tools".into(),
            version: "1.0.0".into(),
            api_version: API_VERSION,
            executable: PathBuf::from("fossil-tools"),
            capabilities: vec![workdeck_extension_api::Capability::VcsAdapters],
            description: None,
        };
        let handshake: HandshakeResponse = serde_json::from_value(serde_json::json!({
            "extension_api_version": API_VERSION,
            "extension_version": "1.0.0",
            "registrations": [{
                "kind": "vcs-adapter",
                "id": "fossil",
                "name": "Fossil"
            }]
        }))
        .unwrap();
        validate_registrations(&manifest, &handshake).unwrap();

        let Registration::VcsAdapter(adapter) = &handshake.registrations[0] else {
            panic!("expected VCS adapter");
        };
        assert!(adapter.operations.is_empty());

        let legacy_markers = serde_json::from_value::<HandshakeResponse>(serde_json::json!({
            "extension_api_version": API_VERSION,
            "extension_version": "1.0.0",
            "registrations": [{
                "kind": "vcs-adapter",
                "id": "fossil",
                "name": "Fossil",
                "markers": [".fslckout"]
            }]
        }))
        .unwrap_err();
        assert!(legacy_markers.to_string().contains("markers"));

        for operations in [
            serde_json::json!("not-an-object"),
            serde_json::json!([]),
            serde_json::json!({ "working-tree-diff": { "load": "not-a-boolean" } }),
        ] {
            let malformed = serde_json::from_value::<HandshakeResponse>(serde_json::json!({
                "extension_api_version": API_VERSION,
                "extension_version": "1.0.0",
                "registrations": [{
                    "kind": "vcs-adapter",
                    "id": "fossil",
                    "name": "Fossil",
                    "operations": operations
                }]
            }));
            assert!(malformed.is_err());
        }
    }

    #[test]
    fn handshake_exposes_configuration_only_when_manifest_requested_it() {
        let mut manifest = ExtensionManifest {
            id: "demo".into(),
            name: "Demo".into(),
            version: "1.0.0".into(),
            api_version: API_VERSION,
            executable: "demo".into(),
            capabilities: Vec::new(),
            description: None,
        };
        let config = serde_json::json!({ "command": "untrusted value", "threshold": 3 });
        assert_eq!(
            granted_extension_config(&manifest, config.clone()),
            serde_json::json!({})
        );
        manifest
            .capabilities
            .push(workdeck_extension_api::Capability::Configuration);
        assert_eq!(granted_extension_config(&manifest, config.clone()), config);
    }

    #[test]
    fn handshake_validates_pane_geometry_and_command_keys() {
        use workdeck_extension_api::{
            Capability, CommandRegistration, ExtensionPaneSize, PanePlacement, PaneRegistration,
        };

        let manifest = ExtensionManifest {
            id: "demo".into(),
            name: "Demo".into(),
            version: "1.0.0".into(),
            api_version: API_VERSION,
            executable: "demo".into(),
            capabilities: vec![Capability::Commands, Capability::Panes],
            description: None,
        };
        let valid_pane = PaneRegistration {
            id: "side".into(),
            title: "Side".into(),
            placement: PanePlacement::Right,
            default_open: false,
            preferred_size: None,
            width: Some(ExtensionPaneSize {
                preferred: 28,
                min: Some(18),
                max: Some(44),
                fraction: Some(0.25),
            }),
            height: None,
            replaces: None,
            current_line: false,
            available: false,
        };
        let response = |pane: PaneRegistration, chord: &str| HandshakeResponse {
            extension_api_version: API_VERSION,
            extension_version: "1.0.0".into(),
            registrations: vec![
                Registration::Pane(pane),
                Registration::Command(CommandRegistration {
                    id: "toggle".into(),
                    title: "Toggle".into(),
                    description: None,
                    default_keys: vec![chord.into()],
                }),
            ],
        };
        assert!(validate_registrations(&manifest, &response(valid_pane.clone(), "ctrl+p")).is_ok());

        let mut wrong_axis = valid_pane.clone();
        wrong_axis.height = Some(ExtensionPaneSize::fixed(2));
        assert!(validate_registrations(&manifest, &response(wrong_axis, "ctrl+p")).is_err());
        let mut invalid_bounds = valid_pane.clone();
        invalid_bounds.width.as_mut().unwrap().min = Some(29);
        assert!(validate_registrations(&manifest, &response(invalid_bounds, "ctrl+p")).is_err());
        let mut invalid_fraction = valid_pane.clone();
        invalid_fraction.width.as_mut().unwrap().fraction = Some(1.1);
        assert!(validate_registrations(&manifest, &response(invalid_fraction, "ctrl+p")).is_err());
        assert!(validate_registrations(&manifest, &response(valid_pane, "ctlr+p")).is_err());
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
        owning.set_phase(ExtensionEventBusPhase::Closing);
        assert!(!lease.is_live());
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

    #[test]
    fn committed_event_context_provider_installs_observably_and_cleans_up() {
        let slot = ExtensionEventContextProviderSlot::default();
        assert!(!slot.has_provider());
        let installation = slot.install(PathBuf::from("/repo"));
        assert!(slot.has_provider());
        let context = slot.context(vec!["summary".into()]).unwrap();
        assert_eq!(context.cwd, PathBuf::from("/repo"));
        assert_eq!(context.panes, context.sidebars);
        assert!(context.panes.is_open("summary"));
        drop(installation);
        assert!(!slot.has_provider());
    }

    #[test]
    fn event_context_registry_replacement_retires_the_predecessor() {
        let first_slot = ExtensionEventContextProviderSlot::default();
        let second_slot = ExtensionEventContextProviderSlot::default();
        let first = first_slot.install(PathBuf::from("/repo/first"));
        assert_eq!(
            first_slot.context(Vec::new()).unwrap().cwd,
            PathBuf::from("/repo/first")
        );
        drop(first);
        assert!(!first_slot.has_provider());

        let second = second_slot.install(PathBuf::from("/repo/second"));
        assert_eq!(
            second_slot.context(Vec::new()).unwrap().cwd,
            PathBuf::from("/repo/second")
        );
        drop(second);
        assert!(!second_slot.has_provider());
    }

    #[test]
    fn stale_event_context_cleanup_cannot_clear_a_successor() {
        let slot = ExtensionEventContextProviderSlot::default();
        let predecessor = slot.install(PathBuf::from("/repo/first"));
        let successor = slot.install(PathBuf::from("/repo/second"));
        drop(predecessor);
        assert_eq!(
            slot.context(Vec::new()).unwrap().cwd,
            PathBuf::from("/repo/second")
        );
        drop(successor);
        assert!(!slot.has_provider());
    }

    #[test]
    fn scoped_epochs_sum_scope_and_item_counters_so_neither_can_mask_the_other() {
        let mut epochs = bump_scoped_epoch(&ScopedEpochState::default(), "ext:view", None);
        epochs = bump_scoped_epoch(&epochs, "ext:view", Some("file-1"));
        epochs = bump_scoped_epoch(&epochs, "ext:view", Some("file-1"));

        assert_eq!(scoped_epoch(&epochs, "ext:view", "file-1"), 3);
        assert_eq!(scoped_epoch(&epochs, "ext:view", "file-2"), 1);
        assert_eq!(scoped_epoch(&epochs, "other", "file-1"), 0);
    }

    #[test]
    fn bumping_scoped_epochs_returns_a_fresh_state_identity() {
        let before = ScopedEpochState::default();
        let after = bump_scoped_epoch(&before, "scope", None);

        assert!(!after.ptr_eq(&before));
        assert_eq!(before.len(), 0);
    }

    #[test]
    fn scoped_epochs_reconcile_orphans_and_keep_identity_when_unchanged() {
        let mut epochs = bump_scoped_epoch(&ScopedEpochState::default(), "kept", None);
        epochs = bump_scoped_epoch(&epochs, "kept", Some("file-1"));
        epochs = bump_scoped_epoch(&epochs, "dropped-scope", None);
        epochs = bump_scoped_epoch(&epochs, "kept", Some("dropped-file"));

        let reconciled = reconcile_scoped_epochs(
            &epochs,
            &["file-1".into()],
            &BTreeSet::from(["kept".into()]),
        );
        assert_eq!(scoped_epoch(&reconciled, "kept", "file-1"), 2);
        assert_eq!(scoped_epoch(&reconciled, "dropped-scope", "file-1"), 0);
        assert_eq!(scoped_epoch(&reconciled, "kept", "dropped-file"), 1);

        let unchanged = reconcile_scoped_epochs(
            &reconciled,
            &["file-1".into()],
            &BTreeSet::from(["kept".into()]),
        );
        assert!(unchanged.ptr_eq(&reconciled));
    }

    #[test]
    fn scoped_epochs_ignore_malformed_external_entries() {
        let polluted = ScopedEpochState::from_encoded_entries([("not-json".into(), 7)]);
        let reconciled = reconcile_scoped_epochs(&polluted, &[], &BTreeSet::new());
        assert_eq!(reconciled.len(), 0);
    }

    #[test]
    fn parses_native_notification_messages_and_defaults_to_info() {
        let parsed = parse_extension_notification(
            r#"{"jsonrpc":"2.0","method":"workdeck/notify","params":{"message":"ready"}}"#,
        )
        .unwrap();
        assert_eq!(parsed.message, "ready");
        assert_eq!(parsed.notification_type, ExtensionNotifyType::Info);

        let warning = parse_extension_notification(
            r#"{"jsonrpc":"2.0","method":"workdeck/notify","params":{"message":"careful","type":"warning"}}"#,
        )
        .unwrap();
        assert_eq!(warning.notification_type, ExtensionNotifyType::Warning);
    }

    #[test]
    fn rejects_responses_unknown_methods_and_invalid_notification_payloads() {
        for line in [
            r#"{"jsonrpc":"2.0","id":1,"method":"workdeck/notify","params":{"message":"response"}}"#,
            r#"{"jsonrpc":"2.0","method":"workdeck/other","params":{"message":"other"}}"#,
            r#"{"jsonrpc":"2.0","method":"workdeck/notify","params":{"message":3}}"#,
            "not-json",
        ] {
            assert!(parse_extension_notification(line).is_none(), "{line}");
        }
    }

    #[test]
    fn response_identity_distinguishes_settled_replies_from_notifications() {
        assert_eq!(
            json_rpc_response_id(r#"{"jsonrpc":"2.0","id":7,"result":{}}"#),
            Some(7)
        );
        assert_eq!(
            json_rpc_response_id(r#"{"jsonrpc":"2.0","method":"workdeck/notify","params":{}}"#),
            None
        );
        assert_eq!(json_rpc_response_id("not-json"), None);
    }

    #[test]
    fn parses_byte_exact_cli_output_and_rejects_response_shaped_messages() {
        let parsed = parse_cli_output_notification(
            r#"{"jsonrpc":"2.0","method":"workdeck/cli/output","params":{"request_id":7,"stream":"stderr","bytes":[0,10,255]}}"#,
        )
        .unwrap();
        assert_eq!(parsed.request_id, 7);
        assert_eq!(parsed.stream, CliOutputStream::Stderr);
        assert_eq!(parsed.bytes, [0, 10, 255]);

        assert!(
            parse_cli_output_notification(
                r#"{"jsonrpc":"2.0","id":7,"method":"workdeck/cli/output","params":{"request_id":7,"stream":"stdout","bytes":[]}}"#,
            )
            .is_none()
        );
    }

    #[test]
    fn parses_lazy_cli_stdin_reads_and_rejects_response_shaped_messages() {
        let parsed = parse_cli_stdin_read_notification(
            r#"{"jsonrpc":"2.0","method":"workdeck/cli/stdin/read","params":{"request_id":7,"read_id":2,"max_bytes":4096}}"#,
        )
        .unwrap();
        assert_eq!(parsed.request_id, 7);
        assert_eq!(parsed.read_id, 2);
        assert_eq!(parsed.max_bytes, 4096);
        assert!(
            parse_cli_stdin_read_notification(
                r#"{"jsonrpc":"2.0","id":7,"method":"workdeck/cli/stdin/read","params":{"request_id":7,"read_id":2}}"#,
            )
            .is_none()
        );
    }

    #[test]
    fn frozen_hunk_cli_runtime_oracle_maps_every_source_test() {
        let oracle: Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/extension-cli-runtime.json"
        ))
        .unwrap();
        assert_eq!(oracle["baselines"][0]["tests"], 9);
        assert_eq!(oracle["baselines"][0]["passed"], 9);
        assert_eq!(oracle["baselines"][0]["failed"], 0);
        assert_eq!(oracle["baselines"][1]["status"], "absent");
        let mappings = oracle["test_mapping"].as_array().unwrap();
        assert_eq!(mappings.len(), 9);
        assert!(mappings.iter().all(|mapping| {
            mapping["source_test"]
                .as_str()
                .is_some_and(|name| !name.is_empty())
                && mapping["rust_tests"]
                    .as_array()
                    .is_some_and(|tests| !tests.is_empty())
        }));
    }

    #[test]
    fn frozen_hunk_event_oracle_maps_every_source_test_at_both_pins() {
        let oracle: Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/extension-events.json"
        ))
        .unwrap();
        for baseline in oracle["baselines"].as_array().unwrap() {
            assert_eq!(baseline["tests"], 26);
            assert_eq!(baseline["passed"], 26);
            assert_eq!(baseline["failed"], 0);
            assert_eq!(baseline["expect_calls"], 72);
        }
        let mappings = oracle["test_mapping"].as_array().unwrap();
        assert_eq!(mappings.len(), 26);
        assert!(mappings.iter().all(|mapping| {
            mapping["source_test"]
                .as_str()
                .is_some_and(|name| !name.is_empty())
                && mapping["rust_tests"]
                    .as_array()
                    .is_some_and(|tests| !tests.is_empty())
        }));
    }

    #[test]
    fn cli_delegation_retains_host_terminal_and_bootstrap_ownership() {
        let delegate = |argv: &[&str], stdin_read_started, stdin_consumed| CliCommandExecution {
            result: CliCommandResult::Delegate {
                argv: argv.iter().map(ToString::to_string).collect(),
            },
            stdin_read_started,
            stdin_consumed,
        };

        assert!(validate_cli_execution(&delegate(&["diff", "HEAD"], false, false), 0).is_ok());
        assert!(validate_cli_execution(&delegate(&[], false, false), 0).is_err());
        assert!(validate_cli_execution(&delegate(&["diff", "bad\0arg"], false, false), 0).is_err());
        assert!(
            validate_cli_execution(&delegate(&["diff", "--no-extensions"], false, false), 0)
                .is_err()
        );
        assert!(validate_cli_execution(&delegate(&["diff"], false, false), 1).is_err());
        assert!(validate_cli_execution(&delegate(&["diff"], true, false), 0).is_err());
        assert!(
            validate_cli_execution(
                &CliCommandExecution {
                    result: CliCommandResult::Exit { code: 2 },
                    stdin_read_started: false,
                    stdin_consumed: true,
                },
                0,
            )
            .is_err()
        );
    }

    #[test]
    fn handshake_validates_cli_command_metadata() {
        let mut manifest = ExtensionManifest {
            id: "demo".into(),
            name: "Demo".into(),
            version: "1.0.0".into(),
            api_version: API_VERSION,
            executable: "demo".into(),
            capabilities: vec![workdeck_extension_api::Capability::CliCommands],
            description: None,
        };
        let response = |name: &str, summary: &str, usage: Option<&str>| HandshakeResponse {
            extension_api_version: API_VERSION,
            extension_version: "1.0.0".into(),
            registrations: vec![Registration::CliCommand(
                workdeck_extension_api::CliCommandRegistration {
                    name: name.into(),
                    summary: summary.into(),
                    usage: usage.map(Into::into),
                },
            )],
        };
        assert!(validate_registrations(&manifest, &response("cli-tools", "Tools", None)).is_ok());
        for invalid in ["", "Cli-Tools", "cli_tools", "1tool", "cli--tools-"] {
            let result = validate_registrations(&manifest, &response(invalid, "Tools", None));
            if invalid == "cli--tools-" {
                // Hunk's grammar permits consecutive/trailing dashes after a lowercase start.
                assert!(result.is_ok());
            } else {
                assert!(result.is_err(), "{invalid:?}");
            }
        }
        assert!(validate_registrations(&manifest, &response("tools", " ", None)).is_err());
        assert!(validate_registrations(&manifest, &response("tools", "Tools", Some(" "))).is_err());

        manifest.capabilities.clear();
        assert!(validate_registrations(&manifest, &response("tools", "Tools", None)).is_err());
    }

    #[test]
    fn custom_event_names_match_hunks_open_non_empty_contract() {
        for valid in [
            "review-triage:decision",
            "vendor.feature:opened",
            "a:b_c-1.2",
            "selection_changed",
            "workdeck:selection_changed",
            "missing space:event 🧭",
            ":",
        ] {
            assert!(valid_custom_event_name(valid), "{valid}");
        }
        for invalid in ["", " \t"] {
            assert!(!valid_custom_event_name(invalid), "{invalid}");
        }

        let manifest = ExtensionManifest {
            id: "events".into(),
            name: "Events".into(),
            version: "1.0.0".into(),
            api_version: API_VERSION,
            executable: "events".into(),
            capabilities: vec![workdeck_extension_api::Capability::Events],
            description: None,
        };
        let lifecycle_response = |names: &[&str]| HandshakeResponse {
            extension_api_version: API_VERSION,
            extension_version: "1.0.0".into(),
            registrations: vec![Registration::EventSubscription {
                names: names.iter().map(|name| (*name).into()).collect(),
            }],
        };
        assert!(
            validate_registrations(
                &manifest,
                &lifecycle_response(&["selection_changed", "shutdown"])
            )
            .is_ok()
        );
        assert!(validate_registrations(&manifest, &lifecycle_response(&[])).is_err());
        for unknown in ["Startup", "changesetLoaded", "review-triage:open", "", " "] {
            assert!(
                validate_registrations(&manifest, &lifecycle_response(&[unknown])).is_err(),
                "{unknown:?}"
            );
        }

        let custom_response = |names: &[&str]| HandshakeResponse {
            extension_api_version: API_VERSION,
            extension_version: "1.0.0".into(),
            registrations: vec![Registration::CustomEventSubscription {
                names: names.iter().map(|name| (*name).into()).collect(),
            }],
        };
        assert!(
            validate_registrations(
                &manifest,
                &custom_response(&["selection_changed", "review-triage:open", "bad name"])
            )
            .is_ok()
        );
        assert!(validate_registrations(&manifest, &custom_response(&[])).is_err());
        assert!(validate_registrations(&manifest, &custom_response(&["same", "same"])).is_ok());
        assert!(validate_registrations(&manifest, &custom_response(&[" "])).is_err());
    }

    fn transform_fixture() -> Changeset {
        workdeck_diff::parse_patch(
            concat!(
                "diff --git a/a.rs b/a.rs\n",
                "--- a/a.rs\n",
                "+++ b/a.rs\n",
                "@@ -1 +1 @@\n",
                "-old a\n",
                "+new a\n",
                "diff --git a/b.rs b/b.rs\n",
                "--- a/b.rs\n",
                "+++ b/b.rs\n",
                "@@ -1 +1 @@\n",
                "-old b\n",
                "+new b\n",
            ),
            "transform-fixture",
            "Transform fixture",
            workdeck_core::ChangesetSource::Patch {
                label: "fixture".into(),
            },
        )
        .unwrap()
    }

    #[test]
    fn transform_response_validation_rejects_every_renderer_critical_near_miss() {
        let original = transform_fixture();
        let mut duplicate = original.clone();
        duplicate.files[1].runtime_id = duplicate.files[0].runtime_id.clone();
        let mut empty = original.clone();
        empty.files[0].runtime_id.clear();

        for value in [
            Value::Null,
            serde_json::json!({}),
            serde_json::json!({ "changeset": null }),
            serde_json::json!({ "changeset": { "files": null } }),
            serde_json::json!({ "changeset": { "files": [null] } }),
            serde_json::to_value(TransformResponse {
                changeset: duplicate,
            })
            .unwrap(),
            serde_json::to_value(TransformResponse { changeset: empty }).unwrap(),
        ] {
            assert!(decode_transform_response(value).is_err());
        }

        let mut malformed = serde_json::to_value(TransformResponse {
            changeset: original,
        })
        .unwrap();
        malformed["changeset"]["files"][0]["stats"] = Value::Null;
        assert!(decode_transform_response(malformed).is_err());
    }

    #[test]
    fn valid_transform_responses_can_filter_reorder_and_compose() {
        let mut first = transform_fixture();
        first.files.reverse();
        first.title = "reordered".into();
        let first = decode_transform_response(
            serde_json::to_value(TransformResponse { changeset: first }).unwrap(),
        )
        .unwrap();
        assert_eq!(first.files[0].path, "b.rs");

        let mut second = first;
        second.files.truncate(1);
        second.title = "filtered".into();
        let second = decode_transform_response(
            serde_json::to_value(TransformResponse { changeset: second }).unwrap(),
        )
        .unwrap();
        assert_eq!(second.title, "filtered");
        assert_eq!(second.files.len(), 1);
        assert_eq!(second.files[0].path, "b.rs");
    }

    #[test]
    fn failed_and_invalid_transform_attempts_keep_the_previous_value_and_warn() {
        let original = transform_fixture();
        let hub = ExtensionNotificationHub::new();
        let received = Arc::new(Mutex::new(Vec::new()));
        let capture = Arc::clone(&received);
        let _subscription = hub.subscribe(move |notice| capture.lock().unwrap().push(notice));

        let after_failure = settle_transform_attempt(
            "broken",
            &hub,
            original.clone(),
            Err("sync or async failure".into()),
        );
        assert_eq!(after_failure, original);
        let after_invalid = settle_transform_attempt(
            "near-miss",
            &hub,
            after_failure,
            Ok(serde_json::json!({ "changeset": { "files": null } })),
        );
        assert_eq!(after_invalid, original);

        let messages = received.lock().unwrap();
        assert_eq!(messages.len(), 2);
        assert!(messages[0].message.contains("Extension broken failed"));
        assert!(
            messages[1]
                .message
                .contains("Extension near-miss returned an invalid changeset")
        );
        assert!(
            messages
                .iter()
                .all(|notice| notice.notification_type == ExtensionNotifyType::Warning)
        );
    }

    #[test]
    fn absent_transforms_leave_the_input_untouched_and_declared_order_is_stable() {
        let mut handshake = HandshakeResponse {
            extension_api_version: API_VERSION,
            extension_version: "1.0.0".into(),
            registrations: Vec::new(),
        };
        assert!(changeset_transform_ids(&handshake).is_empty());

        handshake.registrations.extend([
            Registration::ChangesetTransform { id: "first".into() },
            Registration::Theme(workdeck_extension_api::ThemeRegistration {
                id: "ignored".into(),
                base: None,
                colors: BTreeMap::new(),
            }),
            Registration::ChangesetTransform {
                id: "second".into(),
            },
        ]);
        assert_eq!(changeset_transform_ids(&handshake), ["first", "second"]);
    }
}
