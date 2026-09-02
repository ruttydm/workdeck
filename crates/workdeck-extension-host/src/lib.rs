//! Subprocess host for trusted native Workdeck extensions.

mod extension_document_reader;
mod file_view_host;
mod file_view_mode;
mod file_view_state;
mod file_views;
mod line_highlights;
mod synchronous_callbacks;

pub use extension_document_reader::*;
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
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;
use workdeck_core::{Changeset, ReviewSnapshot};
use workdeck_diff::{SanitizeOptions, sanitize_terminal_text};
use workdeck_extension_api::{
    API_VERSION, CliCommandExecution, CliCommandInvocation, CliCommandResult,
    CliOutputNotification, CliOutputStream, DEFAULT_REQUEST_TIMEOUT_MS, ExtensionManifest,
    ExtensionNotificationHub, ExtensionNotifyType, ExtensionPaneView, HandshakeRequest,
    HandshakeResponse, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, MAX_MESSAGE_BYTES,
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
    registry: Arc<ExtensionRuntimeRegistry>,
    notifications: ExtensionNotificationHub,
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
    let mut manifests = Vec::new();
    let mut seen = BTreeSet::new();
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
            if trust.decision(repo) != Some(TrustDecision::Trusted) {
                return Err(HostError::Untrusted(directory));
            }
            append_manifests(scan_manifests(&directory), &mut manifests, &mut seen);
        }
    }
    Ok(manifests)
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
}
