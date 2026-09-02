//! Subprocess host for trusted native Workdeck extensions.

mod extension_document_reader;
mod extension_trust;
mod file_view_host;
mod file_view_mode;
mod file_view_state;
mod file_views;
mod line_highlights;
mod synchronous_callbacks;

pub use extension_document_reader::*;
pub use extension_trust::*;
pub use file_view_host::*;
pub use file_view_mode::*;
pub use file_view_state::*;
pub use file_views::*;
pub use line_highlights::*;
pub use synchronous_callbacks::*;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;
use workdeck_core::{Changeset, ReviewSnapshot};
use workdeck_diff::{SanitizeOptions, sanitize_terminal_text};
use workdeck_extension_api::{
    API_VERSION, CliCommandExecution, CliCommandInvocation, CliCommandResult,
    CliOutputNotification, CliOutputStream, CommandExecution, CommandInvocation,
    ConfirmDialogSubmission, DEFAULT_HANDSHAKE_TIMEOUT_MS, DEFAULT_REQUEST_TIMEOUT_MS,
    ExtensionDiffFile, ExtensionEventContext, ExtensionFileSide, ExtensionHostAction,
    ExtensionKeyEvent, ExtensionManifest, ExtensionNotificationHub, ExtensionNotifyType,
    ExtensionPaneView, ExtensionWorkspaceSnapshot, ExtensionWorkspaceWriteCompletion,
    FileViewLayoutRequest, FileViewMatchRequest, FileViewModeKeyRequest,
    FileViewModeLifecycleRequest, HandshakeRequest, HandshakeResponse, InputDialogSubmission,
    JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, KeyboardModeExecution,
    KeyboardModeKeyRequest, KeyboardModeLifecycleRequest, MAX_MESSAGE_BYTES, ManifestError,
    PaneActionInvocation, PaneRenderRequest, PaneRenderResponse, Registration, ReviewEvent,
    SelectDialogSubmission, TransformRequest, TransformResponse, ValidatedFileViewLayout,
    extension_pane_size, is_vertical_pane_placement, parse_key_chord, validate_view,
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

#[derive(Debug)]
pub struct LoadedExtension {
    pub manifest: ExtensionManifest,
    pub handshake: HandshakeResponse,
    child: Child,
    stdin: ChildStdin,
    responses: mpsc::Receiver<Result<String, std::io::Error>>,
    next_id: u64,
    pending_command: Option<PendingCommandRequest>,
    registry: Arc<ExtensionRuntimeRegistry>,
    notifications: ExtensionNotificationHub,
}

#[derive(Debug, Clone, Copy)]
struct PendingCommandRequest {
    id: u64,
    deadline: Instant,
}

impl LoadedExtension {
    pub fn spawn(manifest_path: &Path, host_version: &str) -> Result<Self, HostError> {
        Self::spawn_with_notifications(manifest_path, host_version, ExtensionNotificationHub::new())
    }

    pub fn spawn_with_notifications(
        manifest_path: &Path,
        host_version: &str,
        notifications: ExtensionNotificationHub,
    ) -> Result<Self, HostError> {
        let manifest = ExtensionManifest::load(manifest_path)?;
        let directory = manifest_path.parent().unwrap_or_else(|| Path::new("."));
        let directory = fs::canonicalize(directory).unwrap_or_else(|_| directory.to_owned());
        let mut executable = directory.join(&manifest.executable);
        if !executable.is_file() && !std::env::consts::EXE_SUFFIX.is_empty() {
            let mut name = executable.as_os_str().to_owned();
            name.push(std::env::consts::EXE_SUFFIX);
            executable = PathBuf::from(name);
        }
        if !executable.is_file() {
            return Err(HostError::MissingExecutable(executable));
        }
        let mut child = Command::new(&executable)
            .current_dir(&directory)
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
            pending_command: None,
            registry: Arc::new(ExtensionRuntimeRegistry::new()),
            notifications,
        };
        let result = loaded.request(
            "workdeck/handshake",
            HandshakeRequest {
                host_api_version: API_VERSION,
                host_version: host_version.to_owned(),
                extension_id: loaded.manifest.id.clone(),
                granted_capabilities: loaded.manifest.capabilities.clone(),
            },
            Duration::from_millis(DEFAULT_HANDSHAKE_TIMEOUT_MS),
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

    #[must_use]
    pub fn notifications(&self) -> ExtensionNotificationHub {
        self.notifications.clone()
    }

    pub fn request(
        &mut self,
        method: &str,
        params: impl Serialize,
        timeout: Duration,
    ) -> Result<Value, HostError> {
        if self.pending_command.is_some() {
            return Err(HostError::Busy(self.manifest.id.clone()));
        }
        let id = self.send_request(method, params)?;
        let line = self.receive_protocol_line(Instant::now() + timeout)?;
        self.decode_response(id, &line)
    }

    fn send_request(&mut self, method: &str, params: impl Serialize) -> Result<u64, HostError> {
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
        Ok(id)
    }

    fn send_notification(&mut self, method: &str, params: impl Serialize) -> Result<(), HostError> {
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
        self.stdin
            .write_all(&encoded)
            .map_err(|source| HostError::Io {
                id: self.manifest.id.clone(),
                source,
            })?;
        self.stdin.flush().map_err(|source| HostError::Io {
            id: self.manifest.id.clone(),
            source,
        })
    }

    fn receive_protocol_line(&self, deadline: Instant) -> Result<String, HostError> {
        let timeout = deadline.saturating_duration_since(Instant::now());
        self.responses
            .recv_timeout(timeout)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => HostError::Timeout(self.manifest.id.clone()),
                mpsc::RecvTimeoutError::Disconnected => HostError::Closed(self.manifest.id.clone()),
            })?
            .map_err(|source| HostError::Io {
                id: self.manifest.id.clone(),
                source,
            })
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
        self.invoke_cli_command_cancellable(
            command_name,
            args,
            cwd,
            timeout,
            &AtomicBool::new(false),
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
        if !self.handshake.registrations.iter().any(|registration| {
            matches!(registration, Registration::CliCommand(command) if command.name == command_name)
        }) {
            return Err(HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "CLI command",
                message: format!("command {command_name:?} is not registered"),
            });
        }

        let id = self.send_request(
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

        let value = loop {
            if cancelled.load(Ordering::Acquire) && !cancellation_sent {
                self.send_notification("$/cancelRequest", serde_json::json!({ "id": id }))?;
                cancellation_sent = true;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(HostError::Timeout(self.manifest.id.clone()));
            }
            let wait = remaining.min(Duration::from_millis(25));
            let line = match self.responses.recv_timeout(wait) {
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
                    return Err(HostError::InvalidPayload {
                        id: self.manifest.id.clone(),
                        kind: "CLI output",
                        message: format!(
                            "output request id {} did not match active request {id}",
                            output.request_id
                        ),
                    });
                }
                match output.stream {
                    CliOutputStream::Stdout => {
                        stdout
                            .write_all(&output.bytes)
                            .map_err(|source| HostError::Io {
                                id: self.manifest.id.clone(),
                                source,
                            })?;
                        stdout.flush().map_err(|source| HostError::Io {
                            id: self.manifest.id.clone(),
                            source,
                        })?;
                        stdout_bytes = stdout_bytes.saturating_add(output.bytes.len());
                    }
                    CliOutputStream::Stderr => {
                        stderr
                            .write_all(&output.bytes)
                            .map_err(|source| HostError::Io {
                                id: self.manifest.id.clone(),
                                source,
                            })?;
                        stderr.flush().map_err(|source| HostError::Io {
                            id: self.manifest.id.clone(),
                            source,
                        })?;
                    }
                }
                continue;
            }
            break self.decode_response(id, &line)?;
        };

        let execution: CliCommandExecution =
            serde_json::from_value(value).map_err(|error| HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "CLI command",
                message: error.to_string(),
            })?;
        validate_cli_execution(&execution, stdout_bytes).map_err(|message| {
            HostError::InvalidPayload {
                id: self.manifest.id.clone(),
                kind: "CLI command",
                message,
            }
        })?;
        Ok(execution)
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
            matches!(registration, Registration::EventSubscription { names } if names.iter().any(|candidate| candidate == name))
        })
    }

    /// Deliver one host lifecycle or namespaced extension event to a declared subscriber.
    pub fn deliver_event(&mut self, event: ReviewEvent) -> Result<CommandExecution, HostError> {
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
        let value = self.request(
            "workdeck/command/invoke",
            CommandInvocation {
                command_id: command_id.to_owned(),
                snapshot,
                cwd,
                review,
                open_panes,
                active_keyboard_mode,
                workspace,
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
        workspace: Option<ExtensionWorkspaceSnapshot>,
    ) -> Result<(), HostError> {
        if self.pending_command.is_some() {
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
        let id = self.send_request(
            "workdeck/command/invoke",
            CommandInvocation {
                command_id: command_id.to_owned(),
                snapshot,
                cwd,
                review,
                open_panes,
                active_keyboard_mode,
                workspace,
            },
        )?;
        self.pending_command = Some(PendingCommandRequest {
            id,
            deadline: Instant::now() + Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS),
        });
        Ok(())
    }

    #[must_use]
    pub const fn command_pending(&self) -> bool {
        self.pending_command.is_some()
    }

    /// Poll the in-flight command once without blocking the host event loop.
    pub fn poll_command(&mut self) -> Option<Result<CommandExecution, HostError>> {
        let pending = self.pending_command?;
        let line = match self.responses.try_recv() {
            Ok(Ok(line)) => line,
            Ok(Err(source)) => {
                self.pending_command = None;
                return Some(Err(HostError::Io {
                    id: self.manifest.id.clone(),
                    source,
                }));
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.pending_command = None;
                return Some(Err(HostError::Closed(self.manifest.id.clone())));
            }
            Err(mpsc::TryRecvError::Empty) if Instant::now() >= pending.deadline => {
                self.pending_command = None;
                return Some(Err(HostError::Timeout(self.manifest.id.clone())));
            }
            Err(mpsc::TryRecvError::Empty) => return None,
        };
        self.pending_command = None;
        let result = self.decode_response(pending.id, &line).and_then(|value| {
            let execution: CommandExecution =
                serde_json::from_value(value).map_err(|error| HostError::InvalidPayload {
                    id: self.manifest.id.clone(),
                    kind: "command",
                    message: error.to_string(),
                })?;
            self.validate_host_actions(&execution.actions, "command")?;
            Ok(execution)
        });
        Some(result)
    }

    pub fn enter_keyboard_mode(
        &mut self,
        mode_id: &str,
        snapshot: ReviewSnapshot,
    ) -> Result<CommandExecution, HostError> {
        self.keyboard_mode_lifecycle("workdeck/keyboard-mode/enter", mode_id, snapshot)
    }

    pub fn exit_keyboard_mode(
        &mut self,
        mode_id: &str,
        snapshot: ReviewSnapshot,
    ) -> Result<CommandExecution, HostError> {
        self.keyboard_mode_lifecycle("workdeck/keyboard-mode/exit", mode_id, snapshot)
    }

    fn keyboard_mode_lifecycle(
        &mut self,
        method: &str,
        mode_id: &str,
        snapshot: ReviewSnapshot,
    ) -> Result<CommandExecution, HostError> {
        self.require_keyboard_mode(mode_id)?;
        let value = self.request(
            method,
            KeyboardModeLifecycleRequest {
                mode_id: mode_id.to_owned(),
                snapshot,
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
        self.require_keyboard_mode(mode_id)?;
        let value = self.request(
            "workdeck/keyboard-mode/key",
            KeyboardModeKeyRequest {
                mode_id: mode_id.to_owned(),
                key,
                snapshot,
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
        )
    }

    pub fn submit_input_dialog_with_context(
        &mut self,
        action_id: &str,
        value: Option<String>,
        snapshot: ReviewSnapshot,
        active_keyboard_mode: Option<String>,
        cwd: PathBuf,
        review: Option<workdeck_extension_api::ExtensionReviewSnapshot>,
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

    pub fn submit_select_dialog_with_context(
        &mut self,
        action_id: &str,
        value: Option<String>,
        snapshot: ReviewSnapshot,
        active_keyboard_mode: Option<String>,
        cwd: PathBuf,
        review: Option<workdeck_extension_api::ExtensionReviewSnapshot>,
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

    pub fn submit_confirm_dialog_with_context(
        &mut self,
        action_id: &str,
        confirmed: bool,
        snapshot: ReviewSnapshot,
        active_keyboard_mode: Option<String>,
        cwd: PathBuf,
        review: Option<workdeck_extension_api::ExtensionReviewSnapshot>,
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
                        && options
                            .iter()
                            .all(|option| !option.trim().is_empty() && option.len() <= 4 * 1_024)
                }
                ExtensionHostAction::OpenConfirmDialog {
                    id,
                    title,
                    body,
                    confirm_label,
                } => {
                    self.manifest
                        .capabilities
                        .contains(&workdeck_extension_api::Capability::Dialogs)
                        && !id.trim().is_empty()
                        && !title.trim().is_empty()
                        && !body.trim().is_empty()
                        && !confirm_label.trim().is_empty()
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
    let Some((namespace, event)) = name.split_once(':') else {
        return false;
    };
    !namespace.is_empty()
        && !event.is_empty()
        && name.len() <= 256
        && !name.starts_with("workdeck:")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
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

fn valid_cli_command_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(first) if first.is_ascii_lowercase())
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
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
        if let Registration::CliCommand(command) = registration {
            if !valid_cli_command_name(&command.name) {
                return Err(HostError::Handshake {
                    id: manifest.id.clone(),
                    message: format!(
                        "CLI command {:?} must use lowercase kebab case and start with a letter",
                        command.name
                    ),
                });
            }
            if command.summary.trim().is_empty() {
                return Err(HostError::Handshake {
                    id: manifest.id.clone(),
                    message: format!(
                        "CLI command {:?} requires a non-empty summary",
                        command.name
                    ),
                });
            }
            if command
                .usage
                .as_ref()
                .is_some_and(|usage| usage.trim().is_empty())
            {
                return Err(HostError::Handshake {
                    id: manifest.id.clone(),
                    message: format!(
                        "CLI command {:?} usage must be non-empty when provided",
                        command.name
                    ),
                });
            }
        }
        if let Registration::Command(command) = registration {
            if command.id.trim().is_empty() || command.title.trim().is_empty() {
                return Err(HostError::Handshake {
                    id: manifest.id.clone(),
                    message: "commands require non-empty ids and titles".into(),
                });
            }
            for chord in &command.default_keys {
                parse_key_chord(chord).map_err(|error| HostError::Handshake {
                    id: manifest.id.clone(),
                    message: format!("command {:?} has invalid key chord: {error}", command.id),
                })?;
            }
        }
        if let Registration::Pane(pane) = registration {
            if pane.id.trim().is_empty() || pane.title.trim().is_empty() || pane.id.contains(':') {
                return Err(HostError::Handshake {
                    id: manifest.id.clone(),
                    message: "panes require non-empty local ids and titles".into(),
                });
            }
            let vertical = is_vertical_pane_placement(pane.placement);
            if vertical && pane.height.is_some() || !vertical && pane.width.is_some() {
                return Err(HostError::Handshake {
                    id: manifest.id.clone(),
                    message: format!(
                        "pane {:?} uses the dimension opposite its {:?} placement",
                        pane.id, pane.placement
                    ),
                });
            }
            let size = extension_pane_size(pane, None);
            let min = size.min.unwrap_or(1);
            let max = size.max.unwrap_or(u16::MAX);
            if size.preferred == 0
                || min == 0
                || max == 0
                || min > size.preferred
                || size.preferred > max
            {
                return Err(HostError::Handshake {
                    id: manifest.id.clone(),
                    message: format!(
                        "pane {:?} size must satisfy 0 < min <= preferred <= max",
                        pane.id
                    ),
                });
            }
            if size
                .fraction
                .is_some_and(|fraction| !fraction.is_finite() || fraction <= 0.0 || fraction > 1.0)
            {
                return Err(HostError::Handshake {
                    id: manifest.id.clone(),
                    message: format!(
                        "pane {:?} fraction must be greater than 0 and at most 1",
                        pane.id
                    ),
                });
            }
        }
        if let Registration::KeyboardMode(mode) = registration
            && (mode.id.trim().is_empty() || mode.id.contains(':') || mode.title.trim().is_empty())
        {
            return Err(HostError::Handshake {
                id: manifest.id.clone(),
                message: "keyboard modes require non-empty local ids and titles".into(),
            });
        }
        if let Registration::FileView { id, title, .. } = registration
            && (id.trim().is_empty() || id.contains(':') || title.trim().is_empty())
        {
            return Err(HostError::Handshake {
                id: manifest.id.clone(),
                message: "file views require non-empty local ids and titles".into(),
            });
        }
        if let Registration::EventSubscription { names } = registration {
            let unique = names.iter().collect::<BTreeSet<_>>();
            if names.is_empty()
                || unique.len() != names.len()
                || names.iter().any(|name| {
                    name.trim().is_empty()
                        || name.len() > 256
                        || !name.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric()
                                || matches!(byte, b'-' | b'_' | b'.' | b':')
                        })
                })
            {
                return Err(HostError::Handshake {
                    id: manifest.id.clone(),
                    message: "event subscriptions require unique, non-empty protocol names".into(),
                });
            }
        }
    }
    Ok(())
}

impl Drop for LoadedExtension {
    fn drop(&mut self) {
        self.registry.set_phase(ExtensionEventBusPhase::Closing);
        if self.subscribes_to_event("shutdown") {
            let notification = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "workdeck/shutdown",
                "params": {},
            });
            if serde_json::to_writer(&mut self.stdin, &notification).is_ok()
                && self.stdin.write_all(b"\n").is_ok()
                && self.stdin.flush().is_ok()
            {
                let deadline = Instant::now() + Duration::from_millis(100);
                while Instant::now() < deadline {
                    if self.child.try_wait().ok().flatten().is_some() {
                        self.registry.set_phase(ExtensionEventBusPhase::Closed);
                        return;
                    }
                    thread::sleep(Duration::from_millis(2));
                }
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.registry.set_phase(ExtensionEventBusPhase::Closed);
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManifestDiscovery {
    pub manifests: Vec<PathBuf>,
    pub pending_trust_repo_root: Option<PathBuf>,
}

pub fn discover_manifests(
    global_directory: Option<&Path>,
    repo_root: Option<&Path>,
    trust: &TrustStore,
    explicit: &[PathBuf],
) -> Result<Vec<PathBuf>, HostError> {
    Ok(discover_manifests_with_status(global_directory, repo_root, trust, explicit)?.manifests)
}

/// Discover explicit and global extensions while nonfatally trust-gating repository extensions.
pub fn discover_manifests_with_status(
    global_directory: Option<&Path>,
    repo_root: Option<&Path>,
    trust: &TrustStore,
    explicit: &[PathBuf],
) -> Result<ManifestDiscovery, HostError> {
    let mut manifests = Vec::new();
    let mut seen = BTreeSet::new();
    let mut pending_trust_repo_root = None;
    let mut explicit = explicit
        .iter()
        .map(|path| {
            if path.is_dir() {
                path.join("workdeck-extension.toml")
            } else {
                path.clone()
            }
        })
        .collect::<Vec<_>>();
    explicit.sort();
    append_manifests(explicit, &mut manifests, &mut seen);
    if let Some(global) = global_directory {
        append_manifests(scan_manifests(global), &mut manifests, &mut seen);
    }
    if let Some(repo) = repo_root {
        let directory = repo.join(".agents/workdeck/extensions");
        if directory.exists() {
            match trust.decision(repo) {
                Some(TrustDecision::Trusted) => {
                    append_manifests(scan_manifests(&directory), &mut manifests, &mut seen);
                }
                Some(TrustDecision::Denied) => {}
                Some(TrustDecision::Legacy) | None => {
                    pending_trust_repo_root = Some(repo.to_owned());
                }
            }
        }
    }
    Ok(ManifestDiscovery {
        manifests,
        pending_trust_repo_root,
    })
}

fn append_manifests(
    candidates: impl IntoIterator<Item = PathBuf>,
    manifests: &mut Vec<PathBuf>,
    seen: &mut BTreeSet<PathBuf>,
) {
    for path in candidates {
        let identity = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if path.is_file() && seen.insert(identity) {
            manifests.push(path);
        }
    }
}

fn scan_manifests(directory: &Path) -> Vec<PathBuf> {
    let mut manifests = BTreeSet::new();
    let direct = directory.join("workdeck-extension.toml");
    if direct.is_file() {
        manifests.insert(direct);
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return manifests.into_iter().collect();
    };
    for entry in entries.flatten() {
        let manifest = entry.path().join("workdeck-extension.toml");
        if manifest.is_file() {
            manifests.insert(manifest);
        }
    }
    manifests.into_iter().collect()
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
        let pending = discover_manifests_with_status(None, Some(repo.path()), &trust, &[]).unwrap();
        assert!(pending.manifests.is_empty());
        assert_eq!(
            pending.pending_trust_repo_root.as_deref(),
            Some(repo.path())
        );

        let mut trust = trust;
        trust.grant(repo.path(), TrustDecision::Trusted);
        let loaded = discover_manifests_with_status(None, Some(repo.path()), &trust, &[]).unwrap();
        assert_eq!(loaded.manifests.len(), 1);
        assert!(loaded.pending_trust_repo_root.is_none());

        trust.grant(repo.path(), TrustDecision::Denied);
        let denied = discover_manifests_with_status(None, Some(repo.path()), &trust, &[]).unwrap();
        assert!(denied.manifests.is_empty());
        assert!(denied.pending_trust_repo_root.is_none());
    }

    #[test]
    fn discovery_orders_explicit_then_global_then_repo_and_deduplicates_paths() {
        let root = TempDir::new().unwrap();
        let explicit_a = root.path().join("explicit-a/workdeck-extension.toml");
        let explicit_b = root.path().join("explicit-b/workdeck-extension.toml");
        let global = root.path().join("global");
        let repo = root.path().join("repo");
        let global_manifest = global.join("global/workdeck-extension.toml");
        let repo_manifest = repo.join(".agents/workdeck/extensions/repo/workdeck-extension.toml");
        for manifest in [&explicit_a, &explicit_b, &global_manifest, &repo_manifest] {
            fs::create_dir_all(manifest.parent().unwrap()).unwrap();
            fs::write(manifest, "id = 'placeholder'").unwrap();
        }
        let mut trust = TrustStore::default();
        trust.grant(&repo, TrustDecision::Trusted);
        let manifests = discover_manifests(
            Some(&global),
            Some(&repo),
            &trust,
            &[explicit_b.clone(), explicit_a.clone(), explicit_b.clone()],
        )
        .unwrap();
        assert_eq!(
            manifests,
            [explicit_a, explicit_b, global_manifest, repo_manifest]
        );
    }

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
    fn event_names_distinguish_host_lifecycle_subscriptions_from_extension_emissions() {
        for valid in [
            "review-triage:decision",
            "vendor.feature:opened",
            "a:b_c-1.2",
        ] {
            assert!(valid_custom_event_name(valid), "{valid}");
        }
        for invalid in [
            "selection_changed",
            "workdeck:selection_changed",
            "missing space:event",
            ":",
            "",
        ] {
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
        let response = |names: &[&str]| HandshakeResponse {
            extension_api_version: API_VERSION,
            extension_version: "1.0.0".into(),
            registrations: vec![Registration::EventSubscription {
                names: names.iter().map(|name| (*name).into()).collect(),
            }],
        };
        assert!(
            validate_registrations(
                &manifest,
                &response(&["selection_changed", "review-triage:open"])
            )
            .is_ok()
        );
        assert!(validate_registrations(&manifest, &response(&[])).is_err());
        assert!(validate_registrations(&manifest, &response(&["same", "same"])).is_err());
        assert!(validate_registrations(&manifest, &response(&["bad name"])).is_err());
    }
}
