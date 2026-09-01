//! Transport-neutral live-session ownership, selection, and command scheduling.

use crate::{
    BrokerCapacityCode, BrokerCapacityError, BrokerProtocolError, BrokerProtocolFailureCode,
    BudgetReservation, ReservationGroup, ResourceBudget, SelectableSession,
    SessionBrokerLimitOptions, SessionBrokerLimits, SessionRegistration, SessionSelector,
    SessionSnapshot, is_valid_broker_revision, matches_session_selector, repo_selector_distance,
    resolve_session_broker_limits,
};
use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::sync::{Arc, Mutex, Weak, mpsc};
use std::thread;
use std::time::Duration;

const RETAINED_SESSION_OVERHEAD_BYTES: u64 = 256;
const QUEUED_COMMAND_OVERHEAD_BYTES: u64 = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionBrokerStateError {
    message: String,
    pub capacity: Option<BrokerCapacityError>,
    pub protocol: Option<BrokerProtocolError>,
}

impl SessionBrokerStateError {
    #[must_use]
    pub fn message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            capacity: None,
            protocol: None,
        }
    }

    #[must_use]
    pub fn capacity(code: BrokerCapacityCode, resource: impl Into<String>) -> Self {
        let capacity = BrokerCapacityError {
            code,
            resource: resource.into(),
        };
        Self {
            message: capacity.to_string(),
            capacity: Some(capacity),
            protocol: None,
        }
    }

    #[must_use]
    pub fn protocol(code: BrokerProtocolFailureCode) -> Self {
        let protocol = BrokerProtocolError { code };
        Self {
            message: protocol.to_string(),
            capacity: None,
            protocol: Some(protocol),
        }
    }
}

impl fmt::Display for SessionBrokerStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for SessionBrokerStateError {}

impl From<BrokerCapacityError> for SessionBrokerStateError {
    fn from(error: BrokerCapacityError) -> Self {
        Self {
            message: error.to_string(),
            capacity: Some(error),
            protocol: None,
        }
    }
}

impl From<BrokerProtocolError> for SessionBrokerStateError {
    fn from(error: BrokerProtocolError) -> Self {
        Self {
            message: error.to_string(),
            capacity: None,
            protocol: Some(error),
        }
    }
}

/// Minimal transport surface owned by the generic broker state.
pub trait DaemonSessionSocket: Send + Sync {
    /// Return `Ok(false)` when transport backpressure rejects the write.
    fn send(&self, data: &str) -> Result<bool, SessionBrokerStateError>;
}

pub type SharedDaemonSessionSocket = Arc<dyn DaemonSessionSocket>;

/// One live broker session plus the transport that owns it.
#[derive(Clone)]
pub struct SessionBrokerEntry<Info, State> {
    pub registration: SessionRegistration<Info>,
    pub snapshot: SessionSnapshot<State>,
    pub socket: SharedDaemonSessionSocket,
    pub connected_at: String,
    pub last_seen_at: String,
    last_seen_unix_ms: i64,
}

/// Minimum projected session shape shared by target selection and listings.
pub trait SessionBrokerListedSession: Clone {
    fn selectable_session(&self) -> SelectableSession;
    fn title(&self) -> &str;
    fn snapshot_updated_at(&self) -> &str;
}

/// App-owned strict parsing and projections, kept outside the generic broker core.
pub trait SessionBrokerViewAdapter: Send + Sync + 'static {
    type Info: Clone + Serialize + Send + Sync + 'static;
    type State: Clone + Serialize + Send + Sync + 'static;
    type CommandInput: Serialize + Send + Sync + 'static;
    type CommandResult: Clone + Serialize + Send + 'static;
    type ListedSession: SessionBrokerListedSession + Send + 'static;
    type SelectedContext;
    type SessionReview;
    type SessionCommentSummary;

    fn parse_registration(&self, value: &Value) -> Option<SessionRegistration<Self::Info>>;
    fn parse_snapshot(&self, value: &Value) -> Option<SessionSnapshot<Self::State>>;
    fn parse_command_input(
        &self,
        command: &str,
        version: u64,
        value: &Value,
    ) -> Option<Self::CommandInput>;
    fn parse_command_result(
        &self,
        command: &str,
        version: u64,
        value: &Value,
    ) -> Option<Self::CommandResult>;
    fn build_listed_session(
        &self,
        entry: &SessionBrokerEntry<Self::Info, Self::State>,
    ) -> Self::ListedSession;
    fn build_selected_context(&self, session: &Self::ListedSession) -> Self::SelectedContext;
    fn build_session_review(
        &self,
        entry: &SessionBrokerEntry<Self::Info, Self::State>,
        options: BrokerSessionReviewOptions,
    ) -> Self::SessionReview;
    fn list_comments(
        &self,
        session: &Self::ListedSession,
        filter: BrokerSessionCommentFilter,
    ) -> Vec<Self::SessionCommentSummary>;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BrokerSessionReviewOptions {
    pub include_patch: bool,
    pub include_notes: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BrokerSessionCommentFilter {
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterSessionResult {
    Registered,
    Invalid,
    AlreadyConnected,
    CapacityExceeded,
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateSnapshotResult {
    Updated,
    Invalid,
    NotOwner,
    CapacityExceeded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkSessionSeenResult {
    Seen,
    NotOwner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleCommandResult {
    Handled,
    NotFound,
    NotOwner,
    Invalid,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RegisterSessionOptions {
    pub replace_owner: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DispatchSessionCommand {
    pub selector: SessionSelector,
    pub command: String,
    pub command_version: u64,
    pub input: Value,
    pub timeout_message: String,
    pub timeout_ms: Option<u64>,
}

impl DispatchSessionCommand {
    #[must_use]
    pub fn new(
        selector: SessionSelector,
        command: impl Into<String>,
        input: Value,
        timeout_message: impl Into<String>,
    ) -> Self {
        Self {
            selector,
            command: command.into(),
            command_version: 1,
            input,
            timeout_message: timeout_message.into(),
            timeout_ms: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BrokerCommandOutcome {
    Success(Value),
    Failure(String),
}

#[derive(Debug)]
pub struct PendingCommandResult<ResultValue> {
    receiver: mpsc::Receiver<Result<ResultValue, SessionBrokerStateError>>,
}

impl<ResultValue> PendingCommandResult<ResultValue> {
    pub fn receive(self) -> Result<ResultValue, SessionBrokerStateError> {
        self.receiver.recv().unwrap_or_else(|_| {
            Err(SessionBrokerStateError::message(
                "The session command result channel closed.",
            ))
        })
    }

    pub fn receive_timeout(
        &self,
        timeout: Duration,
    ) -> Result<Option<ResultValue>, SessionBrokerStateError> {
        match self.receiver.recv_timeout(timeout) {
            Ok(result) => result.map(Some),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(SessionBrokerStateError::message(
                "The session command result channel closed.",
            )),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RetainedSession<'a, Info, State> {
    registration: &'a SessionRegistration<Info>,
    snapshot: &'a SessionSnapshot<State>,
}

fn retained_json_bytes(value: &impl Serialize, overhead: u64) -> Result<u64, ()> {
    let bytes = serde_json::to_vec(value).map_err(|_| ())?;
    u64::try_from(bytes.len())
        .ok()
        .and_then(|length| length.checked_add(overhead))
        .ok_or(())
}

fn describe_session_choices<S: SessionBrokerListedSession>(sessions: &[S]) -> String {
    sessions
        .iter()
        .map(|session| {
            format!(
                "{} ({})",
                session.selectable_session().session_id,
                session.title()
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Resolve one live target with Hunk's id, cwd, nearest-root, and sole-session precedence.
pub fn resolve_session_target<S: SessionBrokerListedSession>(
    sessions: &[S],
    selector: &SessionSelector,
) -> Result<S, SessionBrokerStateError> {
    if let Some(session_id) = selector
        .session_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        return sessions
            .iter()
            .find(|session| matches_session_selector(&session.selectable_session(), Some(selector)))
            .cloned()
            .ok_or_else(|| {
                SessionBrokerStateError::message(format!(
                    "No active session matches sessionId {session_id}."
                ))
            });
    }

    if let Some(session_path) = selector
        .session_path
        .as_deref()
        .filter(|value| !value.as_os_str().is_empty())
    {
        let matches = sessions
            .iter()
            .filter(|session| {
                matches_session_selector(&session.selectable_session(), Some(selector))
            })
            .cloned()
            .collect::<Vec<_>>();
        return match matches.as_slice() {
            [] => Err(SessionBrokerStateError::message(format!(
                "No active session matches session path {}.",
                session_path.display()
            ))),
            [session] => Ok(session.clone()),
            _ => Err(SessionBrokerStateError::message(format!(
                "Multiple active sessions match session path {}; specify sessionId instead. Matches: {}.",
                session_path.display(),
                describe_session_choices(&matches)
            ))),
        };
    }

    if let Some(repo_root) = selector
        .repo_root
        .as_deref()
        .filter(|value| !value.as_os_str().is_empty())
    {
        let candidates = sessions
            .iter()
            .filter_map(|session| {
                repo_selector_distance(
                    &session.selectable_session(),
                    repo_root,
                    selector.repo_boundary.as_deref(),
                )
                .map(|distance| (session.clone(), distance))
            })
            .collect::<Vec<_>>();
        let Some(nearest) = candidates.iter().map(|(_, distance)| *distance).min() else {
            return Err(SessionBrokerStateError::message(format!(
                "No active session matches repoRoot {}.",
                repo_root.display()
            )));
        };
        let matches = candidates
            .into_iter()
            .filter_map(|(session, distance)| (distance == nearest).then_some(session))
            .collect::<Vec<_>>();
        return match matches.as_slice() {
            [session] => Ok(session.clone()),
            _ => Err(SessionBrokerStateError::message(format!(
                "Multiple active sessions match repoRoot {}; specify sessionId instead. Matches: {}.",
                repo_root.display(),
                describe_session_choices(&matches)
            ))),
        };
    }

    match sessions {
        [session] => Ok(session.clone()),
        [] => Err(SessionBrokerStateError::message(
            "No active sessions are registered with the broker. Open the app and wait for it to connect.",
        )),
        _ => Err(SessionBrokerStateError::message(format!(
            "Multiple active sessions are registered; specify sessionId, sessionPath, or repoRoot. Sessions: {}.",
            describe_session_choices(sessions)
        ))),
    }
}

struct PendingCommand<ResultValue> {
    request_id: String,
    session_id: String,
    socket_key: usize,
    command: String,
    command_version: u64,
    serialized_message: String,
    reservation: ReservationGroup,
    sender: mpsc::Sender<Result<ResultValue, SessionBrokerStateError>>,
    active: bool,
}

type SessionEntries<V> = Vec<(
    String,
    SessionBrokerEntry<
        <V as SessionBrokerViewAdapter>::Info,
        <V as SessionBrokerViewAdapter>::State,
    >,
)>;

struct BrokerStateInner<V: SessionBrokerViewAdapter> {
    view: Arc<V>,
    limits: SessionBrokerLimits,
    sessions: SessionEntries<V>,
    session_ids_by_socket: BTreeMap<usize, String>,
    pending_commands: Vec<PendingCommand<V::CommandResult>>,
    command_queues: BTreeMap<String, VecDeque<String>>,
    retained_reservations: BTreeMap<String, BudgetReservation>,
    session_reservations: BTreeMap<String, BudgetReservation>,
    session_budget: ResourceBudget,
    command_budget: ResourceBudget,
    queued_command_byte_budget: ResourceBudget,
    retained_byte_budget: ResourceBudget,
    last_prune_at: Option<i64>,
    shutdown_error: Option<SessionBrokerStateError>,
}

#[derive(Clone)]
pub struct SessionBrokerState<V: SessionBrokerViewAdapter> {
    inner: Arc<Mutex<BrokerStateInner<V>>>,
}

impl<V: SessionBrokerViewAdapter> SessionBrokerState<V> {
    pub fn new(
        view: V,
        limit_options: &SessionBrokerLimitOptions,
    ) -> Result<Self, crate::BrokerLimitError> {
        let limits = resolve_session_broker_limits(limit_options)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(BrokerStateInner {
                view: Arc::new(view),
                limits,
                sessions: Vec::new(),
                session_ids_by_socket: BTreeMap::new(),
                pending_commands: Vec::new(),
                command_queues: BTreeMap::new(),
                retained_reservations: BTreeMap::new(),
                session_reservations: BTreeMap::new(),
                session_budget: ResourceBudget::new(limits.max_sessions, "maxSessions"),
                command_budget: ResourceBudget::with_code(
                    limits.max_commands_total,
                    "maxCommandsTotal",
                    BrokerCapacityCode::QueueFull,
                ),
                queued_command_byte_budget: ResourceBudget::with_code(
                    limits.max_queued_command_bytes,
                    "maxQueuedCommandBytes",
                    BrokerCapacityCode::QueueFull,
                ),
                retained_byte_budget: ResourceBudget::new(
                    limits.max_retained_bytes,
                    "maxRetainedBytes",
                ),
                last_prune_at: None,
                shutdown_error: None,
            })),
        })
    }

    #[must_use]
    pub fn limits(&self) -> SessionBrokerLimits {
        self.lock().limits
    }

    #[must_use]
    pub fn list_sessions(&self) -> Vec<V::ListedSession> {
        let inner = self.lock();
        let mut listed = inner
            .sessions
            .iter()
            .map(|(_, entry)| inner.view.build_listed_session(entry))
            .collect::<Vec<_>>();
        listed.sort_by(|left, right| right.snapshot_updated_at().cmp(left.snapshot_updated_at()));
        listed
    }

    pub fn get_session(
        &self,
        selector: &SessionSelector,
    ) -> Result<V::ListedSession, SessionBrokerStateError> {
        resolve_session_target(&self.list_sessions(), selector)
    }

    pub fn get_session_review(
        &self,
        selector: &SessionSelector,
        options: BrokerSessionReviewOptions,
    ) -> Result<V::SessionReview, SessionBrokerStateError> {
        let inner = self.lock();
        let session = resolve_session_target(&inner.list_sessions(), selector)?;
        let session_id = session.selectable_session().session_id;
        let entry = inner.session(&session_id).ok_or_else(disconnected_error)?;
        Ok(inner.view.build_session_review(entry, options))
    }

    pub fn get_selected_context(
        &self,
        selector: &SessionSelector,
    ) -> Result<V::SelectedContext, SessionBrokerStateError> {
        let session = self.get_session(selector)?;
        let view = Arc::clone(&self.lock().view);
        Ok(view.build_selected_context(&session))
    }

    pub fn list_comments(
        &self,
        selector: &SessionSelector,
        filter: BrokerSessionCommentFilter,
    ) -> Result<Vec<V::SessionCommentSummary>, SessionBrokerStateError> {
        let session = self.get_session(selector)?;
        let view = Arc::clone(&self.lock().view);
        Ok(view.list_comments(&session, filter))
    }

    #[must_use]
    pub fn session_count(&self) -> usize {
        self.lock().sessions.len()
    }

    #[must_use]
    pub fn pending_command_count(&self) -> usize {
        self.lock().pending_commands.len()
    }

    pub fn register_session(
        &self,
        socket: SharedDaemonSessionSocket,
        registration_input: &Value,
        snapshot_input: &Value,
        options: RegisterSessionOptions,
    ) -> RegisterSessionResult {
        let mut inner = self.lock();
        if inner.shutdown_error.is_some() {
            return RegisterSessionResult::Shutdown;
        }
        let Some(registration) = inner.view.parse_registration(registration_input) else {
            return RegisterSessionResult::Invalid;
        };
        let Some(snapshot) = inner.view.parse_snapshot(snapshot_input) else {
            return RegisterSessionResult::Invalid;
        };
        let retained_bytes = match retained_json_bytes(
            &RetainedSession {
                registration: &registration,
                snapshot: &snapshot,
            },
            RETAINED_SESSION_OVERHEAD_BYTES,
        ) {
            Ok(bytes) => bytes,
            Err(()) => return RegisterSessionResult::Invalid,
        };
        if retained_bytes > inner.limits.max_retained_session_bytes {
            return RegisterSessionResult::CapacityExceeded;
        }

        let owner_socket_key = socket_key(&socket);
        let session_id = registration.session_id.clone();
        let existing = inner.session(&session_id).cloned();
        if existing.as_ref().is_some_and(|entry| {
            socket_key(&entry.socket) != owner_socket_key && !options.replace_owner
        }) {
            return RegisterSessionResult::AlreadyConnected;
        }
        let previous_session_id = inner.session_ids_by_socket.get(&owner_socket_key).cloned();
        let transfer_session_id = existing
            .as_ref()
            .map(|_| session_id.clone())
            .or_else(|| previous_session_id.clone());
        let previous_retained = transfer_session_id
            .as_ref()
            .and_then(|id| inner.retained_reservations.get(id))
            .cloned();
        let previous_count = transfer_session_id
            .as_ref()
            .and_then(|id| inner.session_reservations.get(id))
            .cloned();
        let abandoned_id = existing.as_ref().and_then(|_| {
            previous_session_id
                .as_ref()
                .filter(|previous| *previous != &session_id)
                .cloned()
        });
        let abandoned_retained = abandoned_id
            .as_ref()
            .and_then(|id| inner.retained_reservations.get(id))
            .cloned();
        let abandoned_count = abandoned_id
            .as_ref()
            .and_then(|id| inner.session_reservations.get(id))
            .cloned();

        let retained_reservation = match previous_retained.as_ref() {
            Some(previous) => match abandoned_retained.as_ref() {
                Some(credit) => {
                    inner
                        .retained_byte_budget
                        .resize_with_credit(previous, retained_bytes, credit)
                }
                None => inner.retained_byte_budget.resize(previous, retained_bytes),
            },
            None => inner
                .retained_byte_budget
                .reserve(retained_bytes)
                .map_err(crate::BudgetError::Capacity),
        };
        let Ok(retained_reservation) = retained_reservation else {
            return RegisterSessionResult::CapacityExceeded;
        };
        let session_reservation = match previous_count.clone() {
            Some(reservation) => Ok(reservation),
            None => inner.session_budget.reserve(1),
        };
        let session_reservation = match session_reservation {
            Ok(reservation) => reservation,
            Err(_) => {
                retained_reservation.release();
                return RegisterSessionResult::CapacityExceeded;
            }
        };

        let (now, now_ms) = now_timestamp();
        if let Some(existing) = &existing {
            let existing_socket_key = socket_key(&existing.socket);
            if existing_socket_key != owner_socket_key {
                inner.session_ids_by_socket.remove(&existing_socket_key);
                inner.reject_pending_for_session(
                    &session_id,
                    SessionBrokerStateError::message("The session owner reconnected."),
                );
            }
        }
        if let Some(previous_id) = previous_session_id
            .as_ref()
            .filter(|previous| *previous != &session_id)
        {
            inner.remove_session_entry(previous_id);
            inner.retained_reservations.remove(previous_id);
            inner.session_reservations.remove(previous_id);
            if let Some(reservation) = &abandoned_retained {
                reservation.release();
            }
            if let Some(reservation) = &abandoned_count {
                reservation.release();
            }
            inner.reject_pending_for_session(
                previous_id,
                SessionBrokerStateError::message("The session registration was replaced."),
            );
        }
        let connected_at = existing
            .as_ref()
            .map_or_else(|| now.clone(), |entry| entry.connected_at.clone());
        inner.set_session(
            session_id.clone(),
            SessionBrokerEntry {
                registration,
                snapshot,
                socket,
                connected_at,
                last_seen_at: now,
                last_seen_unix_ms: now_ms,
            },
        );
        inner
            .session_ids_by_socket
            .insert(owner_socket_key, session_id.clone());
        inner
            .retained_reservations
            .insert(session_id.clone(), retained_reservation);
        inner
            .session_reservations
            .insert(session_id, session_reservation);
        RegisterSessionResult::Registered
    }

    pub fn update_snapshot(
        &self,
        socket: &SharedDaemonSessionSocket,
        session_id_assertion: &str,
        snapshot_input: &Value,
    ) -> UpdateSnapshotResult {
        let mut inner = self.lock();
        let key = socket_key(socket);
        let Some(owned_session_id) = inner.session_ids_by_socket.get(&key).cloned() else {
            return UpdateSnapshotResult::NotOwner;
        };
        if owned_session_id != session_id_assertion {
            return UpdateSnapshotResult::NotOwner;
        }
        let Some(entry) = inner.session(&owned_session_id).cloned() else {
            return UpdateSnapshotResult::NotOwner;
        };
        if socket_key(&entry.socket) != key {
            return UpdateSnapshotResult::NotOwner;
        }
        let Some(snapshot) = inner.view.parse_snapshot(snapshot_input) else {
            return UpdateSnapshotResult::Invalid;
        };
        let retained_bytes = match retained_json_bytes(
            &RetainedSession {
                registration: &entry.registration,
                snapshot: &snapshot,
            },
            RETAINED_SESSION_OVERHEAD_BYTES,
        ) {
            Ok(bytes) => bytes,
            Err(()) => return UpdateSnapshotResult::Invalid,
        };
        if retained_bytes > inner.limits.max_retained_session_bytes {
            return UpdateSnapshotResult::CapacityExceeded;
        }
        let Some(previous) = inner.retained_reservations.get(&owned_session_id).cloned() else {
            return UpdateSnapshotResult::CapacityExceeded;
        };
        let Ok(reservation) = inner.retained_byte_budget.resize(&previous, retained_bytes) else {
            return UpdateSnapshotResult::CapacityExceeded;
        };
        let (now, now_ms) = now_timestamp();
        inner.set_session(
            owned_session_id.clone(),
            SessionBrokerEntry {
                snapshot,
                last_seen_at: now,
                last_seen_unix_ms: now_ms,
                ..entry
            },
        );
        inner
            .retained_reservations
            .insert(owned_session_id, reservation);
        UpdateSnapshotResult::Updated
    }

    pub fn mark_session_seen(
        &self,
        socket: &SharedDaemonSessionSocket,
        session_id_assertion: &str,
    ) -> MarkSessionSeenResult {
        let mut inner = self.lock();
        let key = socket_key(socket);
        let Some(owned_session_id) = inner.session_ids_by_socket.get(&key).cloned() else {
            return MarkSessionSeenResult::NotOwner;
        };
        if owned_session_id != session_id_assertion {
            return MarkSessionSeenResult::NotOwner;
        }
        let Some(mut entry) = inner.session(&owned_session_id).cloned() else {
            return MarkSessionSeenResult::NotOwner;
        };
        if socket_key(&entry.socket) != key {
            return MarkSessionSeenResult::NotOwner;
        }
        (entry.last_seen_at, entry.last_seen_unix_ms) = now_timestamp();
        inner.set_session(owned_session_id, entry);
        MarkSessionSeenResult::Seen
    }

    pub fn unregister_socket(&self, socket: &SharedDaemonSessionSocket) {
        let mut inner = self.lock();
        let key = socket_key(socket);
        let Some(session_id) = inner.session_ids_by_socket.get(&key).cloned() else {
            return;
        };
        inner.remove_session(
            &session_id,
            SessionBrokerStateError::message("The targeted session disconnected."),
        );
    }

    pub fn prune_stale_sessions(&self, ttl_ms: u64, now_ms: Option<i64>) -> usize {
        let mut inner = self.lock();
        let now = now_ms.unwrap_or_else(|| Utc::now().timestamp_millis());
        let ttl = i64::try_from(ttl_ms).unwrap_or(i64::MAX);
        let wall_clock_jumped = inner
            .last_prune_at
            .is_some_and(|last| now.saturating_sub(last) > ttl);
        inner.last_prune_at = Some(now);
        if wall_clock_jumped {
            return 0;
        }
        let cutoff = now.saturating_sub(ttl);
        let stale = inner
            .sessions
            .iter()
            .filter_map(|(id, entry)| (entry.last_seen_unix_ms <= cutoff).then_some(id.clone()))
            .collect::<Vec<_>>();
        for session_id in &stale {
            inner.remove_session(
                session_id,
                SessionBrokerStateError::message(
                    "The targeted session became stale and was removed from the session broker.",
                ),
            );
        }
        stale.len()
    }

    pub fn dispatch_command(
        &self,
        request: DispatchSessionCommand,
    ) -> Result<PendingCommandResult<V::CommandResult>, SessionBrokerStateError> {
        let mut inner = self.lock();
        if let Some(error) = &inner.shutdown_error {
            return Err(error.clone());
        }
        if !is_valid_broker_revision(request.command_version) {
            return Err(SessionBrokerStateError::message(
                "Command version must be a positive safe integer.",
            ));
        }
        let timeout_ms = request
            .timeout_ms
            .unwrap_or(inner.limits.default_command_timeout_ms);
        if timeout_ms < 1 || timeout_ms > inner.limits.max_command_timeout_ms {
            return Err(SessionBrokerStateError::capacity(
                BrokerCapacityCode::CapacityExceeded,
                "maxCommandTimeoutMs",
            ));
        }
        let session = resolve_session_target(&inner.list_sessions(), &request.selector)?;
        let session_id = session.selectable_session().session_id;
        let Some(entry) = inner.session(&session_id).cloned() else {
            let (sender, receiver) = mpsc::channel();
            let _ = sender.send(Err(disconnected_error()));
            return Ok(PendingCommandResult { receiver });
        };
        let session_count = inner
            .command_queues
            .get(&session_id)
            .map_or(0, VecDeque::len);
        if u64::try_from(session_count).unwrap_or(u64::MAX) >= inner.limits.max_commands_per_session
        {
            return Err(SessionBrokerStateError::capacity(
                BrokerCapacityCode::QueueFull,
                "maxCommandsPerSession",
            ));
        }
        let input_bytes = retained_json_bytes(&request.input, 0).map_err(|()| {
            SessionBrokerStateError::protocol(BrokerProtocolFailureCode::InvalidAppPayload)
        })?;
        if input_bytes > inner.limits.max_command_input_bytes {
            return Err(SessionBrokerStateError::capacity(
                BrokerCapacityCode::CapacityExceeded,
                "maxCommandInputBytes",
            ));
        }
        let mut reservations = ReservationGroup::default();
        reservations
            .add(inner.command_budget.reserve(1)?)
            .map_err(|error| SessionBrokerStateError::message(error.to_string()))?;
        let Some(parsed_input) = inner.view.parse_command_input(
            &request.command,
            request.command_version,
            &request.input,
        ) else {
            reservations.release();
            return Err(SessionBrokerStateError::protocol(
                BrokerProtocolFailureCode::InvalidAppPayload,
            ));
        };
        let parsed_bytes = match retained_json_bytes(&parsed_input, 0) {
            Ok(bytes) => bytes,
            Err(()) => {
                reservations.release();
                return Err(SessionBrokerStateError::protocol(
                    BrokerProtocolFailureCode::InvalidAppPayload,
                ));
            }
        };
        if parsed_bytes > inner.limits.max_command_input_bytes {
            reservations.release();
            return Err(SessionBrokerStateError::capacity(
                BrokerCapacityCode::CapacityExceeded,
                "maxCommandInputBytes",
            ));
        }
        let request_id = match random_uuid() {
            Ok(id) => id,
            Err(error) => {
                reservations.release();
                return Err(error);
            }
        };
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Outgoing<'a, Input> {
            #[serde(rename = "type")]
            kind: &'static str,
            request_id: &'a str,
            command: &'a str,
            command_version: u64,
            input: &'a Input,
        }
        let serialized_message = match serde_json::to_string(&Outgoing {
            kind: "command",
            request_id: &request_id,
            command: &request.command,
            command_version: request.command_version,
            input: &parsed_input,
        }) {
            Ok(message) => message,
            Err(_) => {
                reservations.release();
                return Err(SessionBrokerStateError::protocol(
                    BrokerProtocolFailureCode::InvalidAppPayload,
                ));
            }
        };
        let queued_bytes = u64::try_from(serialized_message.len())
            .unwrap_or(u64::MAX)
            .saturating_add(QUEUED_COMMAND_OVERHEAD_BYTES);
        match inner.queued_command_byte_budget.reserve(queued_bytes) {
            Ok(reservation) => {
                reservations
                    .add(reservation)
                    .map_err(|error| SessionBrokerStateError::message(error.to_string()))?;
            }
            Err(error) => {
                reservations.release();
                return Err(error.into());
            }
        }

        let (sender, receiver) = mpsc::channel();
        inner.pending_commands.push(PendingCommand {
            request_id: request_id.clone(),
            session_id: session_id.clone(),
            socket_key: socket_key(&entry.socket),
            command: request.command,
            command_version: request.command_version,
            serialized_message,
            reservation: reservations,
            sender,
            active: false,
        });
        inner
            .command_queues
            .entry(session_id.clone())
            .or_default()
            .push_back(request_id.clone());
        inner.advance_session_queue(&session_id);

        let weak = Arc::downgrade(&self.inner);
        let timeout_message = request.timeout_message;
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(timeout_ms));
            timeout_pending(&weak, &request_id, timeout_message);
        });
        Ok(PendingCommandResult { receiver })
    }

    pub fn handle_command_result(
        &self,
        socket: &SharedDaemonSessionSocket,
        request_id: &str,
        outcome: BrokerCommandOutcome,
    ) -> HandleCommandResult {
        let mut inner = self.lock();
        let Some(index) = inner.pending_index(request_id) else {
            return HandleCommandResult::NotFound;
        };
        if inner.pending_commands[index].socket_key != socket_key(socket) {
            return HandleCommandResult::NotOwner;
        }
        match outcome {
            BrokerCommandOutcome::Success(value) => {
                let raw_bytes = match retained_json_bytes(&value, 0) {
                    Ok(bytes) => bytes,
                    Err(()) => return HandleCommandResult::Invalid,
                };
                if raw_bytes > inner.limits.max_command_result_bytes {
                    return HandleCommandResult::Invalid;
                }
                let command = inner.pending_commands[index].command.clone();
                let command_version = inner.pending_commands[index].command_version;
                let Some(result) =
                    inner
                        .view
                        .parse_command_result(&command, command_version, &value)
                else {
                    return HandleCommandResult::Invalid;
                };
                let parsed_bytes = match retained_json_bytes(&result, 0) {
                    Ok(bytes) => bytes,
                    Err(()) => return HandleCommandResult::Invalid,
                };
                if parsed_bytes > inner.limits.max_command_result_bytes {
                    return HandleCommandResult::Invalid;
                }
                inner.finish_pending(request_id, Ok(result), true);
            }
            BrokerCommandOutcome::Failure(error) => {
                inner.finish_pending(
                    request_id,
                    Err(SessionBrokerStateError::message(if error.is_empty() {
                        "The session failed to handle the command.".into()
                    } else {
                        error
                    })),
                    true,
                );
            }
        }
        HandleCommandResult::Handled
    }

    pub fn shutdown(&self, error: Option<SessionBrokerStateError>) {
        let mut inner = self.lock();
        if inner.shutdown_error.is_some() {
            return;
        }
        let error = error.unwrap_or_else(|| {
            SessionBrokerStateError::message("The session broker daemon shut down.")
        });
        inner.shutdown_error = Some(error.clone());
        let pending = inner
            .pending_commands
            .iter()
            .map(|pending| pending.request_id.clone())
            .collect::<Vec<_>>();
        for request_id in pending {
            inner.finish_pending(&request_id, Err(error.clone()), false);
        }
        inner.command_queues.clear();
        inner.session_ids_by_socket.clear();
        inner.sessions.clear();
        for reservation in inner.retained_reservations.values() {
            reservation.release();
        }
        for reservation in inner.session_reservations.values() {
            reservation.release();
        }
        inner.retained_reservations.clear();
        inner.session_reservations.clear();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BrokerStateInner<V>> {
        self.inner.lock().unwrap_or_else(|error| error.into_inner())
    }
}

impl<V: SessionBrokerViewAdapter> BrokerStateInner<V> {
    fn list_sessions(&self) -> Vec<V::ListedSession> {
        let mut listed = self
            .sessions
            .iter()
            .map(|(_, entry)| self.view.build_listed_session(entry))
            .collect::<Vec<_>>();
        listed.sort_by(|left, right| right.snapshot_updated_at().cmp(left.snapshot_updated_at()));
        listed
    }

    fn session(&self, session_id: &str) -> Option<&SessionBrokerEntry<V::Info, V::State>> {
        self.sessions
            .iter()
            .find(|(id, _)| id == session_id)
            .map(|(_, entry)| entry)
    }

    fn set_session(&mut self, session_id: String, entry: SessionBrokerEntry<V::Info, V::State>) {
        if let Some((_, current)) = self.sessions.iter_mut().find(|(id, _)| id == &session_id) {
            *current = entry;
        } else {
            self.sessions.push((session_id, entry));
        }
    }

    fn remove_session_entry(&mut self, session_id: &str) {
        if let Some(index) = self.sessions.iter().position(|(id, _)| id == session_id) {
            self.sessions.remove(index);
        }
    }

    fn pending_index(&self, request_id: &str) -> Option<usize> {
        self.pending_commands
            .iter()
            .position(|pending| pending.request_id == request_id)
    }

    fn advance_session_queue(&mut self, session_id: &str) {
        let Some(request_id) = self
            .command_queues
            .get(session_id)
            .and_then(|queue| queue.front())
            .cloned()
        else {
            self.command_queues.remove(session_id);
            return;
        };
        let Some(index) = self.pending_index(&request_id) else {
            return;
        };
        if self.pending_commands[index].active {
            return;
        }
        let Some(entry) = self.session(session_id).cloned() else {
            self.finish_pending(&request_id, Err(disconnected_error()), true);
            return;
        };
        if socket_key(&entry.socket) != self.pending_commands[index].socket_key {
            self.finish_pending(&request_id, Err(disconnected_error()), true);
            return;
        }
        self.pending_commands[index].active = true;
        let serialized = self.pending_commands[index].serialized_message.clone();
        let send_result = entry.socket.send(&serialized);
        match send_result {
            Ok(true) => {}
            Ok(false) => self.finish_pending(
                &request_id,
                Err(SessionBrokerStateError::capacity(
                    BrokerCapacityCode::Busy,
                    "outbound",
                )),
                true,
            ),
            Err(error) => self.finish_pending(&request_id, Err(error), true),
        }
    }

    fn finish_pending(
        &mut self,
        request_id: &str,
        outcome: Result<V::CommandResult, SessionBrokerStateError>,
        advance: bool,
    ) {
        let Some(index) = self.pending_index(request_id) else {
            return;
        };
        let mut pending = self.pending_commands.remove(index);
        if let Some(queue) = self.command_queues.get_mut(&pending.session_id) {
            if let Some(index) = queue.iter().position(|id| id == request_id) {
                queue.remove(index);
            }
            if queue.is_empty() {
                self.command_queues.remove(&pending.session_id);
            }
        }
        pending.reservation.release();
        let session_id = pending.session_id.clone();
        let _ = pending.sender.send(outcome);
        if advance {
            self.advance_session_queue(&session_id);
        }
    }

    fn remove_session(&mut self, session_id: &str, error: SessionBrokerStateError) {
        let Some(entry) = self.session(session_id).cloned() else {
            return;
        };
        self.remove_session_entry(session_id);
        if let Some(reservation) = self.retained_reservations.remove(session_id) {
            reservation.release();
        }
        if let Some(reservation) = self.session_reservations.remove(session_id) {
            reservation.release();
        }
        let key = socket_key(&entry.socket);
        if self
            .session_ids_by_socket
            .get(&key)
            .is_some_and(|owned| owned == session_id)
        {
            self.session_ids_by_socket.remove(&key);
        }
        self.reject_pending_for_session(session_id, error);
    }

    fn reject_pending_for_session(&mut self, session_id: &str, error: SessionBrokerStateError) {
        let pending = self
            .pending_commands
            .iter()
            .filter(|pending| pending.session_id == session_id)
            .map(|pending| pending.request_id.clone())
            .collect::<Vec<_>>();
        for request_id in pending {
            self.finish_pending(&request_id, Err(error.clone()), false);
        }
        self.command_queues.remove(session_id);
    }
}

fn timeout_pending<V: SessionBrokerViewAdapter>(
    weak: &Weak<Mutex<BrokerStateInner<V>>>,
    request_id: &str,
    timeout_message: String,
) {
    let Some(inner) = weak.upgrade() else {
        return;
    };
    let mut inner = inner.lock().unwrap_or_else(|error| error.into_inner());
    inner.finish_pending(
        request_id,
        Err(SessionBrokerStateError::message(timeout_message)),
        true,
    );
}

fn socket_key(socket: &SharedDaemonSessionSocket) -> usize {
    Arc::as_ptr(socket) as *const () as usize
}

fn now_timestamp() -> (String, i64) {
    let now = Utc::now();
    (
        now.to_rfc3339_opts(SecondsFormat::Millis, true),
        now.timestamp_millis(),
    )
}

fn disconnected_error() -> SessionBrokerStateError {
    SessionBrokerStateError::message("The targeted session is no longer connected.")
}

fn random_uuid() -> Result<String, SessionBrokerStateError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|error| SessionBrokerStateError::message(error.to_string()))?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        SESSION_BROKER_REGISTRATION_VERSION, SessionBrokerLimitPatch,
        parse_session_registration_envelope, parse_session_snapshot_envelope,
    };
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct TestInfo {
        title: String,
        files: Vec<String>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct TestState {
        selected_index: u64,
        note_count: u64,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestListedSession {
        selectable: SelectableSession,
        title: String,
        pid: u64,
        launched_at: String,
        file_count: usize,
        snapshot: SessionSnapshot<TestState>,
    }

    impl SessionBrokerListedSession for TestListedSession {
        fn selectable_session(&self) -> SelectableSession {
            self.selectable.clone()
        }

        fn title(&self) -> &str {
            &self.title
        }

        fn snapshot_updated_at(&self) -> &str {
            &self.snapshot.updated_at
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestSelectedContext {
        session_id: String,
        selected_index: u64,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestReview {
        session_id: String,
        title: String,
        file_count: usize,
        include_patch: bool,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestComment {
        id: String,
        file_path: Option<String>,
    }

    struct TestAdapter;

    impl SessionBrokerViewAdapter for TestAdapter {
        type Info = TestInfo;
        type State = TestState;
        type CommandInput = Value;
        type CommandResult = Value;
        type ListedSession = TestListedSession;
        type SelectedContext = TestSelectedContext;
        type SessionReview = TestReview;
        type SessionCommentSummary = TestComment;

        fn parse_registration(&self, value: &Value) -> Option<SessionRegistration<TestInfo>> {
            parse_session_registration_envelope(value, |info| {
                serde_json::from_value(info.clone()).ok()
            })
        }

        fn parse_snapshot(&self, value: &Value) -> Option<SessionSnapshot<TestState>> {
            parse_session_snapshot_envelope(value, |state| {
                serde_json::from_value(state.clone()).ok()
            })
        }

        fn parse_command_input(
            &self,
            _command: &str,
            _version: u64,
            value: &Value,
        ) -> Option<Value> {
            Some(value.clone())
        }

        fn parse_command_result(
            &self,
            _command: &str,
            _version: u64,
            value: &Value,
        ) -> Option<Value> {
            value.is_object().then(|| value.clone())
        }

        fn build_listed_session(
            &self,
            entry: &SessionBrokerEntry<TestInfo, TestState>,
        ) -> TestListedSession {
            TestListedSession {
                selectable: SelectableSession {
                    session_id: entry.registration.session_id.clone(),
                    cwd: PathBuf::from(&entry.registration.cwd),
                    repo_root: entry.registration.repo_root.as_deref().map(PathBuf::from),
                },
                title: entry.registration.info.title.clone(),
                pid: entry.registration.pid,
                launched_at: entry.registration.launched_at.clone(),
                file_count: entry.registration.info.files.len(),
                snapshot: entry.snapshot.clone(),
            }
        }

        fn build_selected_context(&self, session: &TestListedSession) -> TestSelectedContext {
            TestSelectedContext {
                session_id: session.selectable.session_id.clone(),
                selected_index: session.snapshot.state.selected_index,
            }
        }

        fn build_session_review(
            &self,
            entry: &SessionBrokerEntry<TestInfo, TestState>,
            options: BrokerSessionReviewOptions,
        ) -> TestReview {
            TestReview {
                session_id: entry.registration.session_id.clone(),
                title: entry.registration.info.title.clone(),
                file_count: entry.registration.info.files.len(),
                include_patch: options.include_patch,
            }
        }

        fn list_comments(
            &self,
            _session: &TestListedSession,
            filter: BrokerSessionCommentFilter,
        ) -> Vec<TestComment> {
            vec![TestComment {
                id: "note-1".into(),
                file_path: filter.file_path,
            }]
        }
    }

    #[derive(Default)]
    struct TestSocket {
        sent: Mutex<Vec<String>>,
        fail: Mutex<Option<String>>,
        accept: AtomicBool,
    }

    impl TestSocket {
        fn accepting() -> Arc<Self> {
            Arc::new(Self {
                accept: AtomicBool::new(true),
                ..Self::default()
            })
        }

        fn failing(message: &str) -> Arc<Self> {
            Arc::new(Self {
                fail: Mutex::new(Some(message.into())),
                accept: AtomicBool::new(true),
                ..Self::default()
            })
        }

        fn messages(&self) -> Vec<String> {
            self.sent
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone()
        }
    }

    impl DaemonSessionSocket for TestSocket {
        fn send(&self, data: &str) -> Result<bool, SessionBrokerStateError> {
            if let Some(message) = self
                .fail
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone()
            {
                return Err(SessionBrokerStateError::message(message));
            }
            self.sent
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(data.into());
            Ok(self.accept.load(Ordering::Acquire))
        }
    }

    fn shared(socket: &Arc<TestSocket>) -> SharedDaemonSessionSocket {
        Arc::clone(socket) as SharedDaemonSessionSocket
    }

    fn create_state(limits: SessionBrokerLimitPatch) -> SessionBrokerState<TestAdapter> {
        SessionBrokerState::new(
            TestAdapter,
            &SessionBrokerLimitOptions {
                limits,
                ..SessionBrokerLimitOptions::default()
            },
        )
        .unwrap()
    }

    fn default_state() -> SessionBrokerState<TestAdapter> {
        create_state(SessionBrokerLimitPatch::default())
    }

    fn registration(session_id: &str, cwd: &str, repo_root: &str) -> SessionRegistration<TestInfo> {
        SessionRegistration {
            registration_version: SESSION_BROKER_REGISTRATION_VERSION,
            session_id: session_id.into(),
            pid: 123,
            cwd: cwd.into(),
            repo_root: Some(repo_root.into()),
            launched_at: "2026-03-22T00:00:00.000Z".into(),
            terminal: None,
            info: TestInfo {
                title: "repo working tree".into(),
                files: vec!["src/example.ts".into()],
            },
        }
    }

    fn snapshot(updated_at: &str, selected_index: u64) -> SessionSnapshot<TestState> {
        SessionSnapshot {
            updated_at: updated_at.into(),
            state: TestState {
                selected_index,
                note_count: 0,
            },
        }
    }

    fn value(value: &impl Serialize) -> Value {
        serde_json::to_value(value).unwrap()
    }

    fn register(
        state: &SessionBrokerState<TestAdapter>,
        socket: &Arc<TestSocket>,
        registration: &SessionRegistration<TestInfo>,
        snapshot: &SessionSnapshot<TestState>,
    ) -> RegisterSessionResult {
        state.register_session(
            shared(socket),
            &value(registration),
            &value(snapshot),
            RegisterSessionOptions::default(),
        )
    }

    fn selector(session_id: &str) -> SessionSelector {
        SessionSelector {
            session_id: Some(session_id.into()),
            ..SessionSelector::default()
        }
    }

    fn command(session_id: &str, summary: &str) -> DispatchSessionCommand {
        DispatchSessionCommand::new(
            selector(session_id),
            "annotate",
            json!({ "filePath": "a.ts", "summary": summary }),
            "timeout",
        )
    }

    fn outgoing_id(socket: &TestSocket, index: usize) -> String {
        serde_json::from_str::<Value>(&socket.messages()[index]).unwrap()["requestId"]
            .as_str()
            .unwrap()
            .into()
    }

    fn listed(
        session_id: &str,
        cwd: &str,
        repo_root: &str,
        title: &str,
        updated_at: &str,
    ) -> TestListedSession {
        TestListedSession {
            selectable: SelectableSession {
                session_id: session_id.into(),
                cwd: cwd.into(),
                repo_root: Some(repo_root.into()),
            },
            title: title.into(),
            pid: 123,
            launched_at: "2026-03-22T00:00:00.000Z".into(),
            file_count: 1,
            snapshot: snapshot(updated_at, 0),
        }
    }

    #[test]
    fn keeps_shutdown_terminal_against_registration_and_command_readmission() {
        let state = default_state();
        let socket = TestSocket::accepting();
        state.shutdown(Some(SessionBrokerStateError::message("terminal shutdown")));
        state.shutdown(Some(SessionBrokerStateError::message(
            "ignored second shutdown",
        )));
        assert_eq!(
            register(
                &state,
                &socket,
                &registration("session-1", "/repo", "/repo"),
                &snapshot("2026-03-22T00:00:00.000Z", 0)
            ),
            RegisterSessionResult::Shutdown
        );
        assert_eq!(state.session_count(), 0);
        assert_eq!(
            state
                .dispatch_command(command("session-1", "late"))
                .unwrap_err()
                .to_string(),
            "terminal shutdown"
        );
        assert_eq!(state.pending_command_count(), 0);
    }

    #[test]
    fn resolves_target_by_id_path_root_or_sole_session() {
        let one = vec![listed(
            "session-1",
            "/repo",
            "/repo",
            "repo working tree",
            "2026-03-22T00:00:00.000Z",
        )];
        let mut two = one.clone();
        two.push(listed(
            "session-2",
            "/other-session",
            "/repo",
            "repo secondary view",
            "2026-03-22T00:00:01.000Z",
        ));
        assert_eq!(
            resolve_session_target(&one, &SessionSelector::default())
                .unwrap()
                .selectable
                .session_id,
            "session-1"
        );
        assert_eq!(
            resolve_session_target(
                &one,
                &SessionSelector {
                    session_path: Some("/repo".into()),
                    ..SessionSelector::default()
                }
            )
            .unwrap()
            .selectable
            .session_id,
            "session-1"
        );
        assert_eq!(
            resolve_session_target(
                &one,
                &SessionSelector {
                    repo_root: Some("/repo".into()),
                    ..SessionSelector::default()
                }
            )
            .unwrap()
            .selectable
            .session_id,
            "session-1"
        );
        assert_eq!(
            resolve_session_target(&two, &selector("session-2"))
                .unwrap()
                .selectable
                .session_id,
            "session-2"
        );
        assert!(
            resolve_session_target(&two, &SessionSelector::default())
                .unwrap_err()
                .to_string()
                .contains("specify sessionId, sessionPath, or repoRoot")
        );
        assert!(
            resolve_session_target(
                &two,
                &SessionSelector {
                    repo_root: Some("/repo".into()),
                    ..SessionSelector::default()
                }
            )
            .unwrap_err()
            .to_string()
            .contains("specify sessionId instead")
        );
    }

    #[test]
    fn resolves_repo_subdirectories_to_nearest_eligible_root() {
        let outer = listed("outer", "/repo", "/repo", "outer", "2026-03-22T00:00:00Z");
        let inner = listed(
            "inner",
            "/repo/packages/app",
            "/repo/packages/app",
            "inner",
            "2026-03-22T00:00:00Z",
        );
        let target = SessionSelector {
            repo_root: Some("/repo/packages/app/src".into()),
            repo_boundary: Some("/repo/packages/app".into()),
            ..SessionSelector::default()
        };
        assert_eq!(
            resolve_session_target(&[outer.clone(), inner], &target)
                .unwrap()
                .selectable
                .session_id,
            "inner"
        );
        assert!(resolve_session_target(std::slice::from_ref(&outer), &target).is_err());
        let custom = listed(
            "custom",
            "/repo/custom",
            "/repo/custom",
            "custom",
            "2026-03-22T00:00:00Z",
        );
        assert_eq!(
            resolve_session_target(
                &[outer.clone(), custom],
                &SessionSelector {
                    repo_root: Some("/repo/custom/src".into()),
                    repo_boundary: Some("/repo".into()),
                    ..SessionSelector::default()
                }
            )
            .unwrap()
            .selectable
            .session_id,
            "custom"
        );
        assert!(
            resolve_session_target(
                &[outer],
                &SessionSelector {
                    repo_root: Some("/repo/..cache".into()),
                    ..SessionSelector::default()
                }
            )
            .is_ok()
        );
    }

    #[test]
    fn session_path_matching_uses_live_cwd() {
        let sessions = vec![
            listed(
                "session-f",
                "/live-session",
                "/source-f",
                "f",
                "2026-03-22T00:00:00Z",
            ),
            listed(
                "session-a",
                "/other-session",
                "/source-a",
                "a",
                "2026-03-22T00:00:00Z",
            ),
        ];
        let match_path = resolve_session_target(
            &sessions,
            &SessionSelector {
                session_path: Some("/live-session".into()),
                ..SessionSelector::default()
            },
        )
        .unwrap();
        assert_eq!(match_path.selectable.session_id, "session-f");
    }

    #[test]
    fn delegates_session_projections_to_adapter() {
        let state = default_state();
        let socket = TestSocket::accepting();
        let mut current = snapshot("2026-03-22T00:00:00.000Z", 0);
        current.state.note_count = 2;
        register(
            &state,
            &socket,
            &registration("session-1", "/repo", "/repo"),
            &current,
        );
        assert_eq!(
            state.get_selected_context(&selector("session-1")).unwrap(),
            TestSelectedContext {
                session_id: "session-1".into(),
                selected_index: 0
            }
        );
        assert!(
            state
                .get_session_review(
                    &selector("session-1"),
                    BrokerSessionReviewOptions {
                        include_patch: true,
                        include_notes: false
                    }
                )
                .unwrap()
                .include_patch
        );
        assert_eq!(
            state
                .list_comments(
                    &selector("session-1"),
                    BrokerSessionCommentFilter {
                        file_path: Some("src/example.ts".into())
                    }
                )
                .unwrap()[0]
                .file_path
                .as_deref(),
            Some("src/example.ts")
        );
    }

    #[test]
    fn incompatible_registration_does_not_replace_valid_listing() {
        let state = default_state();
        let socket = TestSocket::accepting();
        let registration = registration("session-1", "/repo", "/repo");
        let snapshot = snapshot("2026-03-22T00:00:00.000Z", 0);
        assert_eq!(
            register(&state, &socket, &registration, &snapshot),
            RegisterSessionResult::Registered
        );
        let mut invalid = value(&registration);
        invalid["registrationVersion"] = json!(0);
        assert_eq!(
            state.register_session(
                shared(&socket),
                &invalid,
                &value(&snapshot),
                RegisterSessionOptions::default()
            ),
            RegisterSessionResult::Invalid
        );
        assert_eq!(state.list_sessions().len(), 1);
    }

    #[test]
    fn invalid_snapshot_does_not_replace_last_valid_selection() {
        let state = default_state();
        let socket = TestSocket::accepting();
        register(
            &state,
            &socket,
            &registration("session-1", "/repo", "/repo"),
            &snapshot("2026-03-22T00:00:00.000Z", 0),
        );
        assert_eq!(
            state.update_snapshot(
                &shared(&socket),
                "session-1",
                &json!({"selectedIndex":"oops"})
            ),
            UpdateSnapshotResult::Invalid
        );
        assert_eq!(
            state
                .get_session(&selector("session-1"))
                .unwrap()
                .snapshot
                .state
                .selected_index,
            0
        );
    }

    #[test]
    fn unregistered_peer_cannot_update_or_heartbeat() {
        let state = default_state();
        let socket = TestSocket::accepting();
        assert_eq!(
            state.update_snapshot(&shared(&socket), "missing", &json!({})),
            UpdateSnapshotResult::NotOwner
        );
        assert_eq!(
            state.mark_session_seen(&shared(&socket), "missing"),
            MarkSessionSeenResult::NotOwner
        );
    }

    #[test]
    fn routes_opaque_command_and_resolves_result() {
        let state = default_state();
        let socket = TestSocket::accepting();
        register(
            &state,
            &socket,
            &registration("session-1", "/repo", "/repo"),
            &snapshot("2026-03-22T00:00:00Z", 0),
        );
        let pending = state
            .dispatch_command(command("session-1", "Review note"))
            .unwrap();
        let messages = socket.messages();
        assert_eq!(messages.len(), 1);
        let outgoing: Value = serde_json::from_str(&messages[0]).unwrap();
        assert_eq!(outgoing["command"], "annotate");
        assert_eq!(outgoing["input"]["summary"], "Review note");
        assert_eq!(
            state.handle_command_result(
                &shared(&socket),
                outgoing["requestId"].as_str().unwrap(),
                BrokerCommandOutcome::Success(
                    json!({"kind":"annotated","annotationId":"annotation-1"})
                )
            ),
            HandleCommandResult::Handled
        );
        assert_eq!(pending.receive().unwrap()["annotationId"], "annotation-1");
    }

    #[test]
    fn cross_peer_mutation_is_rejected_and_owner_stays_pending() {
        let state = default_state();
        let owner = TestSocket::accepting();
        let other = TestSocket::accepting();
        register(
            &state,
            &owner,
            &registration("session-1", "/repo", "/repo"),
            &snapshot("2026-03-22T00:00:00Z", 0),
        );
        register(
            &state,
            &other,
            &registration("session-2", "/other", "/other"),
            &snapshot("2026-03-22T00:00:00Z", 0),
        );
        assert_eq!(
            state.update_snapshot(
                &shared(&other),
                "session-1",
                &value(&snapshot("2026-03-22T00:00:01Z", 1))
            ),
            UpdateSnapshotResult::NotOwner
        );
        assert_eq!(
            state.mark_session_seen(&shared(&other), "session-1"),
            MarkSessionSeenResult::NotOwner
        );
        let pending = state
            .dispatch_command(command("session-1", "note"))
            .unwrap();
        let request_id = outgoing_id(&owner, 0);
        assert_eq!(
            state.handle_command_result(
                &shared(&other),
                &request_id,
                BrokerCommandOutcome::Success(json!({"kind":"annotated","annotationId":"forged"}))
            ),
            HandleCommandResult::NotOwner
        );
        assert_eq!(state.pending_command_count(), 1);
        assert_eq!(
            state.handle_command_result(
                &shared(&owner),
                &request_id,
                BrokerCommandOutcome::Success(json!({"kind":"annotated","annotationId":"owned"}))
            ),
            HandleCommandResult::Handled
        );
        assert_eq!(pending.receive().unwrap()["annotationId"], "owned");
    }

    #[test]
    fn disconnect_rejects_in_flight_commands() {
        let state = default_state();
        let socket = TestSocket::accepting();
        register(
            &state,
            &socket,
            &registration("session-1", "/repo", "/repo"),
            &snapshot("2026-03-22T00:00:00Z", 0),
        );
        let pending = state
            .dispatch_command(command("session-1", "note"))
            .unwrap();
        state.unregister_socket(&shared(&socket));
        assert!(
            pending
                .receive()
                .unwrap_err()
                .to_string()
                .contains("disconnected")
        );
    }

    #[test]
    fn second_live_peer_waits_until_owner_closes() {
        let state = default_state();
        let original = TestSocket::accepting();
        let replacement = TestSocket::accepting();
        let registration = registration("session-1", "/repo", "/repo");
        assert_eq!(
            register(
                &state,
                &original,
                &registration,
                &snapshot("2026-03-22T00:00:00Z", 0)
            ),
            RegisterSessionResult::Registered
        );
        assert_eq!(
            register(
                &state,
                &replacement,
                &registration,
                &snapshot("2026-03-22T00:00:01Z", 0)
            ),
            RegisterSessionResult::AlreadyConnected
        );
        state.unregister_socket(&shared(&original));
        assert_eq!(
            register(
                &state,
                &replacement,
                &registration,
                &snapshot("2026-03-22T00:00:01Z", 0)
            ),
            RegisterSessionResult::Registered
        );
        state.unregister_socket(&shared(&original));
        assert_eq!(state.session_count(), 1);
    }

    #[test]
    fn owner_replacement_atomically_transfers_retained_reservations() {
        let first_registration = registration("session-1", "/repo", "/repo");
        let first_snapshot = snapshot("2026-03-22T00:00:00Z", 0);
        let retained = retained_json_bytes(
            &RetainedSession {
                registration: &first_registration,
                snapshot: &first_snapshot,
            },
            256,
        )
        .unwrap();
        let mut expanded = first_registration.clone();
        expanded.info.title = "x".repeat(64);
        let expanded_snapshot = snapshot("2026-03-22T00:00:01Z", 0);
        let expanded_bytes = retained_json_bytes(
            &RetainedSession {
                registration: &expanded,
                snapshot: &expanded_snapshot,
            },
            256,
        )
        .unwrap();
        let state = create_state(SessionBrokerLimitPatch {
            max_sessions: Some(2),
            max_retained_session_bytes: Some(expanded_bytes),
            max_retained_bytes: Some(retained * 2),
            ..SessionBrokerLimitPatch::default()
        });
        let original = TestSocket::accepting();
        let replacement = TestSocket::accepting();
        register(&state, &original, &first_registration, &first_snapshot);
        register(
            &state,
            &replacement,
            &registration("session-2", "/two", "/two"),
            &first_snapshot,
        );
        assert_eq!(
            state.register_session(
                shared(&replacement),
                &value(&expanded),
                &value(&expanded_snapshot),
                RegisterSessionOptions {
                    replace_owner: true
                }
            ),
            RegisterSessionResult::Registered
        );
        assert_eq!(
            state.mark_session_seen(&shared(&original), "session-1"),
            MarkSessionSeenResult::NotOwner
        );
        assert_eq!(
            state.mark_session_seen(&shared(&replacement), "session-1"),
            MarkSessionSeenResult::Seen
        );
        state.unregister_socket(&shared(&original));
        assert_eq!(state.session_count(), 1);
    }

    #[test]
    fn owner_replacement_releases_prior_session_count() {
        let state = create_state(SessionBrokerLimitPatch {
            max_sessions: Some(2),
            ..SessionBrokerLimitPatch::default()
        });
        let original = TestSocket::accepting();
        let replacement = TestSocket::accepting();
        let third = TestSocket::accepting();
        register(
            &state,
            &original,
            &registration("session-1", "/one", "/one"),
            &snapshot("2026-03-22T00:00:00Z", 0),
        );
        register(
            &state,
            &replacement,
            &registration("session-2", "/two", "/two"),
            &snapshot("2026-03-22T00:00:00Z", 0),
        );
        assert_eq!(
            state.register_session(
                shared(&replacement),
                &value(&registration("session-1", "/one", "/one")),
                &value(&snapshot("2026-03-22T00:00:00Z", 0)),
                RegisterSessionOptions {
                    replace_owner: true
                }
            ),
            RegisterSessionResult::Registered
        );
        assert_eq!(
            register(
                &state,
                &third,
                &registration("session-3", "/three", "/three"),
                &snapshot("2026-03-22T00:00:00Z", 0)
            ),
            RegisterSessionResult::Registered
        );
        assert_eq!(state.session_count(), 2);
    }

    #[test]
    fn socket_send_failure_rejects_command_immediately() {
        let state = default_state();
        let socket = TestSocket::failing("socket closed");
        register(
            &state,
            &socket,
            &registration("session-1", "/repo", "/repo"),
            &snapshot("2026-03-22T00:00:00Z", 0),
        );
        let pending = state
            .dispatch_command(command("session-1", "note"))
            .unwrap();
        assert_eq!(pending.receive().unwrap_err().to_string(), "socket closed");
        assert_eq!(state.pending_command_count(), 0);
    }

    #[test]
    fn stale_pruning_removes_session_and_rejects_commands() {
        let state = default_state();
        let socket = TestSocket::accepting();
        register(
            &state,
            &socket,
            &registration("session-1", "/repo", "/repo"),
            &snapshot("2026-03-22T00:00:00Z", 0),
        );
        let pending = state
            .dispatch_command(command("session-1", "note"))
            .unwrap();
        assert_eq!(
            state.prune_stale_sessions(1, Some(Utc::now().timestamp_millis() + 10)),
            1
        );
        assert_eq!(state.session_count(), 0);
        assert!(pending.receive().unwrap_err().to_string().contains("stale"));
    }

    #[test]
    fn heartbeat_keeps_idle_session_live() {
        let state = default_state();
        let socket = TestSocket::accepting();
        register(
            &state,
            &socket,
            &registration("session-1", "/repo", "/repo"),
            &snapshot("2026-03-22T00:00:00Z", 0),
        );
        let registered_at = Utc::now().timestamp_millis();
        assert_eq!(state.prune_stale_sessions(50, Some(registered_at + 25)), 0);
        assert_eq!(
            state.mark_session_seen(&shared(&socket), "session-1"),
            MarkSessionSeenResult::Seen
        );
        assert_eq!(
            state.prune_stale_sessions(50, Some(Utc::now().timestamp_millis() + 25)),
            0
        );
        assert_eq!(state.session_count(), 1);
    }

    #[test]
    fn wall_clock_jump_gets_one_grace_sweep() {
        let state = default_state();
        let socket = TestSocket::accepting();
        register(
            &state,
            &socket,
            &registration("session-1", "/repo", "/repo"),
            &snapshot("2026-03-22T00:00:00Z", 0),
        );
        let last_seen = Utc::now().timestamp_millis();
        state.prune_stale_sessions(45_000, Some(last_seen + 15_000));
        assert_eq!(
            state.prune_stale_sessions(45_000, Some(last_seen + 300_000)),
            0
        );
        assert_eq!(state.session_count(), 1);
    }

    #[test]
    fn silent_session_is_pruned_after_post_wake_grace() {
        let state = default_state();
        let socket = TestSocket::accepting();
        register(
            &state,
            &socket,
            &registration("session-1", "/repo", "/repo"),
            &snapshot("2026-03-22T00:00:00Z", 0),
        );
        let last_seen = Utc::now().timestamp_millis();
        state.prune_stale_sessions(45_000, Some(last_seen + 15_000));
        state.prune_stale_sessions(45_000, Some(last_seen + 300_000));
        assert_eq!(
            state.prune_stale_sessions(45_000, Some(last_seen + 315_000)),
            1
        );
        assert_eq!(state.session_count(), 0);
    }

    #[test]
    fn schedules_fifo_with_one_active_command_per_session() {
        let state = default_state();
        let socket = TestSocket::accepting();
        register(
            &state,
            &socket,
            &registration("session-1", "/repo", "/repo"),
            &snapshot("2026-03-22T00:00:00Z", 0),
        );
        let first = state
            .dispatch_command(command("session-1", "first"))
            .unwrap();
        let second = state
            .dispatch_command(command("session-1", "second"))
            .unwrap();
        assert_eq!(socket.messages().len(), 1);
        let first_id = outgoing_id(&socket, 0);
        state.handle_command_result(
            &shared(&socket),
            &first_id,
            BrokerCommandOutcome::Success(json!({"annotationId":"one"})),
        );
        assert_eq!(socket.messages().len(), 2);
        let second_id = outgoing_id(&socket, 1);
        state.handle_command_result(
            &shared(&socket),
            &second_id,
            BrokerCommandOutcome::Success(json!({"annotationId":"two"})),
        );
        assert_eq!(first.receive().unwrap()["annotationId"], "one");
        assert_eq!(second.receive().unwrap()["annotationId"], "two");
    }

    #[test]
    fn different_sessions_progress_independently() {
        let state = default_state();
        let first_socket = TestSocket::accepting();
        let second_socket = TestSocket::accepting();
        register(
            &state,
            &first_socket,
            &registration("session-1", "/one", "/one"),
            &snapshot("2026-03-22T00:00:00Z", 0),
        );
        register(
            &state,
            &second_socket,
            &registration("session-2", "/two", "/two"),
            &snapshot("2026-03-22T00:00:00Z", 0),
        );
        let first = state.dispatch_command(command("session-1", "one")).unwrap();
        let second = state.dispatch_command(command("session-2", "two")).unwrap();
        assert_eq!(
            (
                first_socket.messages().len(),
                second_socket.messages().len()
            ),
            (1, 1)
        );
        state.handle_command_result(
            &shared(&first_socket),
            &outgoing_id(&first_socket, 0),
            BrokerCommandOutcome::Success(json!({"annotationId":"one"})),
        );
        state.handle_command_result(
            &shared(&second_socket),
            &outgoing_id(&second_socket, 0),
            BrokerCommandOutcome::Success(json!({"annotationId":"two"})),
        );
        assert_eq!(first.receive().unwrap()["annotationId"], "one");
        assert_eq!(second.receive().unwrap()["annotationId"], "two");
    }

    #[test]
    fn exact_command_count_boundary_plus_one_is_rejected() {
        let state = create_state(SessionBrokerLimitPatch {
            max_commands_per_session: Some(2),
            max_commands_total: Some(2),
            ..SessionBrokerLimitPatch::default()
        });
        let socket = TestSocket::accepting();
        register(
            &state,
            &socket,
            &registration("session-1", "/repo", "/repo"),
            &snapshot("2026-03-22T00:00:00Z", 0),
        );
        let first = state.dispatch_command(command("session-1", "one")).unwrap();
        let second = state.dispatch_command(command("session-1", "two")).unwrap();
        assert_eq!(
            state
                .dispatch_command(command("session-1", "three"))
                .unwrap_err()
                .to_string(),
            "queue-full"
        );
        state.shutdown(None);
        assert!(
            first
                .receive()
                .unwrap_err()
                .to_string()
                .contains("shut down")
        );
        assert!(
            second
                .receive()
                .unwrap_err()
                .to_string()
                .contains("shut down")
        );
    }

    #[test]
    fn queued_utf8_bytes_are_accounted_at_exact_boundary() {
        let input = json!({"filePath":"é","summary":"😀"});
        let template = json!({"type":"command","requestId":"0".repeat(36),"command":"annotate","commandVersion":1,"input":input});
        let bytes = serde_json::to_vec(&template).unwrap().len() as u64 + 128;
        let state = create_state(SessionBrokerLimitPatch {
            max_queued_command_bytes: Some(bytes),
            ..SessionBrokerLimitPatch::default()
        });
        let socket = TestSocket::accepting();
        register(
            &state,
            &socket,
            &registration("session-1", "/repo", "/repo"),
            &snapshot("2026-03-22T00:00:00Z", 0),
        );
        let admitted = state
            .dispatch_command(DispatchSessionCommand::new(
                selector("session-1"),
                "annotate",
                input.clone(),
                "timeout",
            ))
            .unwrap();
        assert_eq!(
            state
                .dispatch_command(DispatchSessionCommand::new(
                    selector("session-1"),
                    "annotate",
                    input,
                    "timeout"
                ))
                .unwrap_err()
                .to_string(),
            "queue-full"
        );
        state.shutdown(None);
        assert!(admitted.receive().is_err());
    }

    #[test]
    fn active_timeout_releases_and_advances_fifo() {
        let state = create_state(SessionBrokerLimitPatch {
            default_command_timeout_ms: Some(5),
            max_command_timeout_ms: Some(100),
            ..SessionBrokerLimitPatch::default()
        });
        let socket = TestSocket::accepting();
        register(
            &state,
            &socket,
            &registration("session-1", "/repo", "/repo"),
            &snapshot("2026-03-22T00:00:00Z", 0),
        );
        let first = state.dispatch_command(command("session-1", "one")).unwrap();
        let mut second_request = command("session-1", "two");
        second_request.timeout_ms = Some(100);
        let second = state.dispatch_command(second_request).unwrap();
        assert_eq!(first.receive().unwrap_err().to_string(), "timeout");
        let deadline = Instant::now() + Duration::from_millis(50);
        while socket.messages().len() < 2 && Instant::now() < deadline {
            thread::yield_now();
        }
        assert_eq!(socket.messages().len(), 2);
        state.handle_command_result(
            &shared(&socket),
            &outgoing_id(&socket, 1),
            BrokerCommandOutcome::Success(json!({"annotationId":"two"})),
        );
        assert_eq!(second.receive().unwrap()["annotationId"], "two");
        let mut too_long = command("session-1", "three");
        too_long.timeout_ms = Some(101);
        assert_eq!(
            state.dispatch_command(too_long).unwrap_err().to_string(),
            "capacity-exceeded"
        );
    }

    #[test]
    fn session_capacity_rejects_new_owner_without_eviction() {
        let state = create_state(SessionBrokerLimitPatch {
            max_sessions: Some(1),
            ..SessionBrokerLimitPatch::default()
        });
        let first = TestSocket::accepting();
        let second = TestSocket::accepting();
        assert_eq!(
            register(
                &state,
                &first,
                &registration("session-1", "/one", "/one"),
                &snapshot("2026-03-22T00:00:00Z", 0)
            ),
            RegisterSessionResult::Registered
        );
        assert_eq!(
            register(
                &state,
                &second,
                &registration("session-2", "/two", "/two"),
                &snapshot("2026-03-22T00:00:00Z", 0)
            ),
            RegisterSessionResult::CapacityExceeded
        );
        assert_eq!(state.list_sessions()[0].selectable.session_id, "session-1");
    }

    #[test]
    fn same_socket_transfers_count_when_session_id_changes_at_capacity() {
        let state = create_state(SessionBrokerLimitPatch {
            max_sessions: Some(1),
            ..SessionBrokerLimitPatch::default()
        });
        let socket = TestSocket::accepting();
        assert_eq!(
            register(
                &state,
                &socket,
                &registration("session-1", "/one", "/one"),
                &snapshot("2026-03-22T00:00:00Z", 0)
            ),
            RegisterSessionResult::Registered
        );
        assert_eq!(
            register(
                &state,
                &socket,
                &registration("session-2", "/two", "/two"),
                &snapshot("2026-03-22T00:00:00Z", 0)
            ),
            RegisterSessionResult::Registered
        );
        assert_eq!(state.list_sessions()[0].selectable.session_id, "session-2");
    }

    #[test]
    fn identical_replacement_fits_exact_retained_ceiling() {
        let registration = registration("session-1", "/repo", "/repo");
        let snapshot = snapshot("2026-03-22T00:00:00Z", 0);
        let bytes = retained_json_bytes(
            &RetainedSession {
                registration: &registration,
                snapshot: &snapshot,
            },
            256,
        )
        .unwrap();
        let state = create_state(SessionBrokerLimitPatch {
            max_retained_session_bytes: Some(bytes),
            max_retained_bytes: Some(bytes),
            ..SessionBrokerLimitPatch::default()
        });
        let socket = TestSocket::accepting();
        assert_eq!(
            register(&state, &socket, &registration, &snapshot),
            RegisterSessionResult::Registered
        );
        assert_eq!(
            register(&state, &socket, &registration, &snapshot),
            RegisterSessionResult::Registered
        );
        assert_eq!(
            state.update_snapshot(&shared(&socket), "session-1", &value(&snapshot)),
            UpdateSnapshotResult::Updated
        );
    }

    #[test]
    fn failed_retained_replacement_preserves_state_and_reuses_capacity() {
        let first_registration = registration("session-1", "/repo", "/repo");
        let first_snapshot = snapshot("2026-03-22T00:00:00Z", 0);
        let bytes = retained_json_bytes(
            &RetainedSession {
                registration: &first_registration,
                snapshot: &first_snapshot,
            },
            256,
        )
        .unwrap();
        let state = create_state(SessionBrokerLimitPatch {
            max_retained_session_bytes: Some(bytes),
            max_retained_bytes: Some(bytes),
            ..SessionBrokerLimitPatch::default()
        });
        let socket = TestSocket::accepting();
        register(&state, &socket, &first_registration, &first_snapshot);
        let expanded = snapshot("2026-03-22T00:00:00Z", 123_456);
        assert_eq!(
            state.update_snapshot(&shared(&socket), "session-1", &value(&expanded)),
            UpdateSnapshotResult::CapacityExceeded
        );
        assert_eq!(
            state
                .get_session(&selector("session-1"))
                .unwrap()
                .snapshot
                .state
                .selected_index,
            0
        );
        assert_eq!(
            state.update_snapshot(&shared(&socket), "session-1", &value(&first_snapshot)),
            UpdateSnapshotResult::Updated
        );
        state.unregister_socket(&shared(&socket));
        let second = TestSocket::accepting();
        assert_eq!(
            register(
                &state,
                &second,
                &registration("session-2", "/two", "/two"),
                &first_snapshot
            ),
            RegisterSessionResult::Registered
        );
    }
}
