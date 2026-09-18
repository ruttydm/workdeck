//! Session-broker resource ceilings and reserve-before-work accounting.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionBrokerLimits {
    pub max_sessions: u64,
    pub max_commands_per_session: u64,
    pub max_commands_total: u64,
    pub max_pre_bridge_commands: u64,
    pub max_command_input_bytes: u64,
    pub max_command_result_bytes: u64,
    pub max_queued_command_bytes: u64,
    pub max_retained_session_bytes: u64,
    pub max_retained_bytes: u64,
    pub default_command_timeout_ms: u64,
    pub max_command_timeout_ms: u64,
    pub max_concurrent_http_controls: u64,
    pub max_in_flight_http_body_bytes: u64,
    pub max_http_body_bytes: u64,
    pub max_http_response_bytes: u64,
    pub max_in_flight_http_response_bytes: u64,
    pub max_ws_message_bytes: u64,
    pub max_in_flight_ws_bytes: u64,
    pub max_outbound_bytes_per_peer: u64,
    pub max_outbound_bytes_total: u64,
    pub max_unauthenticated_sockets: u64,
    pub max_handshake_duration_ms: u64,
    pub max_incomplete_handshakes: u64,
    pub max_incomplete_handshake_bytes: u64,
    pub max_handshake_proposal_bytes: u64,
    pub challenge_ttl_ms: u64,
    pub caller_session_ttl_ms: u64,
    pub max_caller_sessions: u64,
    pub max_caller_session_bytes: u64,
    pub max_caller_sessions_bytes: u64,
}

pub const DEFAULT_SESSION_BROKER_LIMITS: SessionBrokerLimits = SessionBrokerLimits {
    max_sessions: 256,
    max_commands_per_session: 64,
    max_commands_total: 1_024,
    max_pre_bridge_commands: 32,
    max_command_input_bytes: 1_024 * 1_024,
    max_command_result_bytes: 1_024 * 1_024,
    max_queued_command_bytes: 64 * 1_024 * 1_024,
    max_retained_session_bytes: 4 * 1_024 * 1_024,
    max_retained_bytes: 256 * 1_024 * 1_024,
    default_command_timeout_ms: 15_000,
    max_command_timeout_ms: 5 * 60_000,
    max_concurrent_http_controls: 32,
    max_in_flight_http_body_bytes: 64 * 1_024 * 1_024,
    max_http_body_bytes: 4 * 1_024 * 1_024,
    max_http_response_bytes: 8 * 1_024 * 1_024,
    max_in_flight_http_response_bytes: 64 * 1_024 * 1_024,
    max_ws_message_bytes: 8 * 1_024 * 1_024,
    max_in_flight_ws_bytes: 64 * 1_024 * 1_024,
    max_outbound_bytes_per_peer: 8 * 1_024 * 1_024,
    max_outbound_bytes_total: 64 * 1_024 * 1_024,
    max_unauthenticated_sockets: 64,
    max_handshake_duration_ms: 15_000,
    max_incomplete_handshakes: 128,
    max_incomplete_handshake_bytes: 4 * 1_024 * 1_024,
    max_handshake_proposal_bytes: 64 * 1_024,
    challenge_ttl_ms: 15_000,
    caller_session_ttl_ms: 5 * 60_000,
    max_caller_sessions: 256,
    max_caller_session_bytes: 8 * 1_024,
    max_caller_sessions_bytes: 2 * 1_024 * 1_024,
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionBrokerLimitPatch {
    pub max_sessions: Option<u64>,
    pub max_commands_per_session: Option<u64>,
    pub max_commands_total: Option<u64>,
    pub max_pre_bridge_commands: Option<u64>,
    pub max_command_input_bytes: Option<u64>,
    pub max_command_result_bytes: Option<u64>,
    pub max_queued_command_bytes: Option<u64>,
    pub max_retained_session_bytes: Option<u64>,
    pub max_retained_bytes: Option<u64>,
    pub default_command_timeout_ms: Option<u64>,
    pub max_command_timeout_ms: Option<u64>,
    pub max_concurrent_http_controls: Option<u64>,
    pub max_in_flight_http_body_bytes: Option<u64>,
    pub max_http_body_bytes: Option<u64>,
    pub max_http_response_bytes: Option<u64>,
    pub max_in_flight_http_response_bytes: Option<u64>,
    pub max_ws_message_bytes: Option<u64>,
    pub max_in_flight_ws_bytes: Option<u64>,
    pub max_outbound_bytes_per_peer: Option<u64>,
    pub max_outbound_bytes_total: Option<u64>,
    pub max_unauthenticated_sockets: Option<u64>,
    pub max_handshake_duration_ms: Option<u64>,
    pub max_incomplete_handshakes: Option<u64>,
    pub max_incomplete_handshake_bytes: Option<u64>,
    pub max_handshake_proposal_bytes: Option<u64>,
    pub challenge_ttl_ms: Option<u64>,
    pub caller_session_ttl_ms: Option<u64>,
    pub max_caller_sessions: Option<u64>,
    pub max_caller_session_bytes: Option<u64>,
    pub max_caller_sessions_bytes: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionBrokerLimitOptions {
    pub limits: SessionBrokerLimitPatch,
    pub unsafe_limits: SessionBrokerLimitPatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokerLimitError(String);

impl fmt::Display for BrokerLimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for BrokerLimitError {}

pub fn merge_session_broker_limits(
    base: SessionBrokerLimits,
    options: &SessionBrokerLimitOptions,
) -> Result<SessionBrokerLimits, BrokerLimitError> {
    let mut resolved = base;
    macro_rules! apply {
        ($field:ident, $wire:literal) => {{
            if let Some(value) = options.limits.$field {
                if value > base.$field {
                    return Err(BrokerLimitError(format!(
                        "Session broker limit {} may only be raised through unsafeLimits.",
                        $wire
                    )));
                }
                resolved.$field = value;
            }
            if let Some(value) = options.unsafe_limits.$field {
                resolved.$field = value;
            }
        }};
    }
    apply!(max_sessions, "maxSessions");
    apply!(max_commands_per_session, "maxCommandsPerSession");
    apply!(max_commands_total, "maxCommandsTotal");
    apply!(max_pre_bridge_commands, "maxPreBridgeCommands");
    apply!(max_command_input_bytes, "maxCommandInputBytes");
    apply!(max_command_result_bytes, "maxCommandResultBytes");
    apply!(max_queued_command_bytes, "maxQueuedCommandBytes");
    apply!(max_retained_session_bytes, "maxRetainedSessionBytes");
    apply!(max_retained_bytes, "maxRetainedBytes");
    apply!(default_command_timeout_ms, "defaultCommandTimeoutMs");
    apply!(max_command_timeout_ms, "maxCommandTimeoutMs");
    apply!(max_concurrent_http_controls, "maxConcurrentHttpControls");
    apply!(max_in_flight_http_body_bytes, "maxInFlightHttpBodyBytes");
    apply!(max_http_body_bytes, "maxHttpBodyBytes");
    apply!(max_http_response_bytes, "maxHttpResponseBytes");
    apply!(
        max_in_flight_http_response_bytes,
        "maxInFlightHttpResponseBytes"
    );
    apply!(max_ws_message_bytes, "maxWsMessageBytes");
    apply!(max_in_flight_ws_bytes, "maxInFlightWsBytes");
    apply!(max_outbound_bytes_per_peer, "maxOutboundBytesPerPeer");
    apply!(max_outbound_bytes_total, "maxOutboundBytesTotal");
    apply!(max_unauthenticated_sockets, "maxUnauthenticatedSockets");
    apply!(max_handshake_duration_ms, "maxHandshakeDurationMs");
    apply!(max_incomplete_handshakes, "maxIncompleteHandshakes");
    apply!(
        max_incomplete_handshake_bytes,
        "maxIncompleteHandshakeBytes"
    );
    apply!(max_handshake_proposal_bytes, "maxHandshakeProposalBytes");
    apply!(challenge_ttl_ms, "challengeTtlMs");
    apply!(caller_session_ttl_ms, "callerSessionTtlMs");
    apply!(max_caller_sessions, "maxCallerSessions");
    apply!(max_caller_session_bytes, "maxCallerSessionBytes");
    apply!(max_caller_sessions_bytes, "maxCallerSessionsBytes");

    if resolved.default_command_timeout_ms > resolved.max_command_timeout_ms {
        return Err(BrokerLimitError(
            "Session broker defaultCommandTimeoutMs must not exceed maxCommandTimeoutMs.".into(),
        ));
    }
    if resolved.max_retained_session_bytes > resolved.max_retained_bytes {
        return Err(BrokerLimitError(
            "Session broker per-session retained bytes must not exceed daemon bytes.".into(),
        ));
    }
    if resolved.max_caller_session_bytes > resolved.max_caller_sessions_bytes {
        return Err(BrokerLimitError(
            "Session broker per-caller retained bytes must not exceed daemon bytes.".into(),
        ));
    }
    if resolved.max_http_body_bytes > resolved.max_in_flight_http_body_bytes / 2 {
        return Err(BrokerLimitError(
            "Session broker in-flight HTTP body bytes must cover the source-plus-copy peak.".into(),
        ));
    }
    if resolved.max_http_response_bytes > resolved.max_in_flight_http_response_bytes / 2 {
        return Err(BrokerLimitError(
            "Session broker in-flight HTTP response bytes must cover the source-plus-copy peak."
                .into(),
        ));
    }
    if resolved.max_ws_message_bytes > resolved.max_in_flight_ws_bytes {
        return Err(BrokerLimitError(
            "Session broker WebSocket message bytes must not exceed in-flight bytes.".into(),
        ));
    }
    Ok(resolved)
}

pub fn resolve_session_broker_limits(
    options: &SessionBrokerLimitOptions,
) -> Result<SessionBrokerLimits, BrokerLimitError> {
    merge_session_broker_limits(DEFAULT_SESSION_BROKER_LIMITS, options)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrokerCapacityCode {
    Busy,
    QueueFull,
    CapacityExceeded,
}

impl BrokerCapacityCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Busy => "busy",
            Self::QueueFull => "queue-full",
            Self::CapacityExceeded => "capacity-exceeded",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokerCapacityError {
    pub code: BrokerCapacityCode,
    pub resource: String,
}

impl fmt::Display for BrokerCapacityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code.as_str())
    }
}

impl std::error::Error for BrokerCapacityError {}

#[derive(Debug)]
struct ResourceBudgetState {
    capacity: u64,
    resource: String,
    code: BrokerCapacityCode,
    used: u64,
    next_id: u64,
    active: BTreeMap<u64, u64>,
}

#[derive(Debug, Clone)]
pub struct ResourceBudget {
    state: Arc<Mutex<ResourceBudgetState>>,
}

impl ResourceBudget {
    #[must_use]
    pub fn new(capacity: u64, resource: impl Into<String>) -> Self {
        Self::with_code(capacity, resource, BrokerCapacityCode::CapacityExceeded)
    }

    #[must_use]
    pub fn with_code(capacity: u64, resource: impl Into<String>, code: BrokerCapacityCode) -> Self {
        Self {
            state: Arc::new(Mutex::new(ResourceBudgetState {
                capacity,
                resource: resource.into(),
                code,
                used: 0,
                next_id: 1,
                active: BTreeMap::new(),
            })),
        }
    }

    #[must_use]
    pub fn capacity(&self) -> u64 {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .capacity
    }

    #[must_use]
    pub fn used(&self) -> u64 {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .used
    }

    pub fn try_reserve(&self, amount: u64) -> Option<BudgetReservation> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if amount > state.capacity - state.used {
            return None;
        }
        Some(new_reservation(&self.state, &mut state, amount))
    }

    pub fn reserve(&self, amount: u64) -> Result<BudgetReservation, BrokerCapacityError> {
        self.try_reserve(amount).ok_or_else(|| {
            let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            BrokerCapacityError {
                code: state.code,
                resource: state.resource.clone(),
            }
        })
    }

    pub fn resize(
        &self,
        previous: &BudgetReservation,
        amount: u64,
    ) -> Result<BudgetReservation, BudgetError> {
        if !previous.belongs_to(&self.state) {
            return Err(self.inactive_error("Cannot resize an inactive"));
        }
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let Some(previous_amount) = state.active.get(&previous.id).copied() else {
            return Err(BudgetError::Inactive(format!(
                "Cannot resize an inactive {} reservation.",
                state.resource
            )));
        };
        let retained = state.used - previous_amount;
        if amount > state.capacity - retained {
            return Err(BudgetError::Capacity(BrokerCapacityError {
                code: state.code,
                resource: state.resource.clone(),
            }));
        }
        state.active.remove(&previous.id);
        previous.mark_released();
        state.used = retained;
        Ok(new_reservation(&self.state, &mut state, amount))
    }

    pub fn resize_with_credit(
        &self,
        previous: &BudgetReservation,
        amount: u64,
        credit: &BudgetReservation,
    ) -> Result<BudgetReservation, BudgetError> {
        if previous.id == credit.id
            || !previous.belongs_to(&self.state)
            || !credit.belongs_to(&self.state)
        {
            return Err(self.inactive_error("Cannot combine inactive"));
        }
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let Some(previous_amount) = state.active.get(&previous.id).copied() else {
            return Err(BudgetError::Inactive(format!(
                "Cannot combine inactive {} reservations.",
                state.resource
            )));
        };
        let Some(credit_amount) = state.active.get(&credit.id).copied() else {
            return Err(BudgetError::Inactive(format!(
                "Cannot combine inactive {} reservations.",
                state.resource
            )));
        };
        let retained = state.used - previous_amount - credit_amount;
        if amount > state.capacity - retained {
            return Err(BudgetError::Capacity(BrokerCapacityError {
                code: state.code,
                resource: state.resource.clone(),
            }));
        }
        state.active.remove(&previous.id);
        state.active.remove(&credit.id);
        previous.mark_released();
        credit.mark_released();
        state.used = retained;
        Ok(new_reservation(&self.state, &mut state, amount))
    }

    fn inactive_error(&self, prefix: &str) -> BudgetError {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        BudgetError::Inactive(format!("{prefix} {} reservation.", state.resource))
    }
}

fn new_reservation(
    budget: &Arc<Mutex<ResourceBudgetState>>,
    state: &mut ResourceBudgetState,
    amount: u64,
) -> BudgetReservation {
    let id = state.next_id;
    state.next_id = state.next_id.wrapping_add(1).max(1);
    state.used += amount;
    state.active.insert(id, amount);
    BudgetReservation {
        id,
        amount,
        budget: Arc::downgrade(budget),
        released: Arc::new(AtomicBool::new(false)),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetError {
    Capacity(BrokerCapacityError),
    Inactive(String),
    ReleasedGroup,
}

impl fmt::Display for BudgetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Capacity(error) => error.fmt(formatter),
            Self::Inactive(message) => formatter.write_str(message),
            Self::ReleasedGroup => {
                formatter.write_str("Cannot add to a released reservation group.")
            }
        }
    }
}

impl std::error::Error for BudgetError {}

#[derive(Debug, Clone)]
pub struct BudgetReservation {
    id: u64,
    amount: u64,
    budget: Weak<Mutex<ResourceBudgetState>>,
    released: Arc<AtomicBool>,
}

impl BudgetReservation {
    #[must_use]
    pub const fn amount(&self) -> u64 {
        self.amount
    }

    #[must_use]
    pub fn released(&self) -> bool {
        self.released.load(Ordering::Acquire)
    }

    pub fn release(&self) {
        if self.released.swap(true, Ordering::AcqRel) {
            return;
        }
        let Some(budget) = self.budget.upgrade() else {
            return;
        };
        let mut state = budget.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(amount) = state.active.remove(&self.id) {
            state.used -= amount;
        }
    }

    fn belongs_to(&self, budget: &Arc<Mutex<ResourceBudgetState>>) -> bool {
        self.budget
            .upgrade()
            .is_some_and(|owner| Arc::ptr_eq(&owner, budget))
            && !self.released()
    }

    fn mark_released(&self) {
        self.released.store(true, Ordering::Release);
    }
}

#[derive(Debug, Default)]
pub struct ReservationGroup {
    reservations: Vec<BudgetReservation>,
    done: bool,
}

impl ReservationGroup {
    #[must_use]
    pub fn amount(&self) -> u64 {
        self.reservations
            .iter()
            .map(BudgetReservation::amount)
            .sum()
    }

    #[must_use]
    pub const fn released(&self) -> bool {
        self.done
    }

    pub fn add(&mut self, reservation: BudgetReservation) -> Result<(), BudgetError> {
        if self.done {
            reservation.release();
            return Err(BudgetError::ReleasedGroup);
        }
        self.reservations.push(reservation);
        Ok(())
    }

    pub fn release(&mut self) {
        if self.done {
            return;
        }
        self.done = true;
        for reservation in self.reservations.drain(..) {
            reservation.release();
        }
    }
}

impl Drop for ReservationGroup {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publishes_the_complete_immutable_contract_defaults() {
        assert_eq!(DEFAULT_SESSION_BROKER_LIMITS.max_sessions, 256);
        assert_eq!(DEFAULT_SESSION_BROKER_LIMITS.max_commands_per_session, 64);
        assert_eq!(DEFAULT_SESSION_BROKER_LIMITS.max_commands_total, 1_024);
        assert_eq!(DEFAULT_SESSION_BROKER_LIMITS.max_pre_bridge_commands, 32);
        assert_eq!(
            DEFAULT_SESSION_BROKER_LIMITS.max_command_input_bytes,
            1_024 * 1_024
        );
        assert_eq!(
            DEFAULT_SESSION_BROKER_LIMITS.max_queued_command_bytes,
            64 * 1_024 * 1_024
        );
        assert_eq!(
            DEFAULT_SESSION_BROKER_LIMITS.max_retained_session_bytes,
            4 * 1_024 * 1_024
        );
        assert_eq!(
            DEFAULT_SESSION_BROKER_LIMITS.max_retained_bytes,
            256 * 1_024 * 1_024
        );
        assert_eq!(
            DEFAULT_SESSION_BROKER_LIMITS.default_command_timeout_ms,
            15_000
        );
        assert_eq!(
            DEFAULT_SESSION_BROKER_LIMITS.max_command_timeout_ms,
            300_000
        );
        assert_eq!(
            DEFAULT_SESSION_BROKER_LIMITS.max_concurrent_http_controls,
            32
        );
        assert_eq!(
            DEFAULT_SESSION_BROKER_LIMITS.max_in_flight_http_body_bytes,
            64 * 1_024 * 1_024
        );
        assert_eq!(
            DEFAULT_SESSION_BROKER_LIMITS.max_http_body_bytes,
            4 * 1_024 * 1_024
        );
        assert_eq!(
            DEFAULT_SESSION_BROKER_LIMITS.max_http_response_bytes,
            8 * 1_024 * 1_024
        );
        assert_eq!(
            DEFAULT_SESSION_BROKER_LIMITS.max_ws_message_bytes,
            8 * 1_024 * 1_024
        );
        assert_eq!(DEFAULT_SESSION_BROKER_LIMITS.challenge_ttl_ms, 15_000);
        assert_eq!(DEFAULT_SESSION_BROKER_LIMITS.caller_session_ttl_ms, 300_000);
        assert_eq!(
            DEFAULT_SESSION_BROKER_LIMITS.max_handshake_duration_ms,
            15_000
        );
    }

    #[test]
    fn merges_supported_lowerings_and_explicit_unsafe_raises() {
        let broker = resolve_session_broker_limits(&SessionBrokerLimitOptions {
            limits: SessionBrokerLimitPatch {
                max_sessions: Some(4),
                max_ws_message_bytes: Some(64),
                ..SessionBrokerLimitPatch::default()
            },
            ..SessionBrokerLimitOptions::default()
        })
        .unwrap();
        let merged = merge_session_broker_limits(
            broker,
            &SessionBrokerLimitOptions {
                limits: SessionBrokerLimitPatch {
                    max_sessions: Some(2),
                    ..SessionBrokerLimitPatch::default()
                },
                unsafe_limits: SessionBrokerLimitPatch {
                    max_http_body_bytes: Some(broker.max_http_body_bytes + 1),
                    ..SessionBrokerLimitPatch::default()
                },
            },
        )
        .unwrap();
        assert_eq!(merged.max_sessions, 2);
        assert_eq!(merged.max_ws_message_bytes, 64);
        assert_eq!(merged.max_http_body_bytes, broker.max_http_body_bytes + 1);
        let error = merge_session_broker_limits(
            broker,
            &SessionBrokerLimitOptions {
                limits: SessionBrokerLimitPatch {
                    max_sessions: Some(5),
                    ..SessionBrokerLimitPatch::default()
                },
                ..SessionBrokerLimitOptions::default()
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("unsafeLimits"));
    }

    #[test]
    fn validates_cross_limit_invariants_and_unknown_serialized_keys() {
        let options = |limits: SessionBrokerLimitPatch| SessionBrokerLimitOptions {
            limits,
            ..SessionBrokerLimitOptions::default()
        };
        assert!(
            resolve_session_broker_limits(&options(SessionBrokerLimitPatch {
                max_ws_message_bytes: Some(8),
                max_in_flight_ws_bytes: Some(7),
                ..SessionBrokerLimitPatch::default()
            }))
            .unwrap_err()
            .to_string()
            .contains("WebSocket")
        );
        assert_eq!(
            resolve_session_broker_limits(&options(SessionBrokerLimitPatch {
                max_http_body_bytes: Some(4),
                max_in_flight_http_body_bytes: Some(8),
                ..SessionBrokerLimitPatch::default()
            }))
            .unwrap()
            .max_http_body_bytes,
            4
        );
        assert!(
            resolve_session_broker_limits(&options(SessionBrokerLimitPatch {
                max_http_response_bytes: Some(4),
                max_in_flight_http_response_bytes: Some(7),
                ..SessionBrokerLimitPatch::default()
            }))
            .unwrap_err()
            .to_string()
            .contains("source-plus-copy peak")
        );
        assert!(
            serde_json::from_value::<SessionBrokerLimitPatch>(serde_json::json!({ "unknown": 1 }))
                .is_err()
        );
        assert!(
            serde_json::from_value::<SessionBrokerLimitPatch>(
                serde_json::json!({ "maxSessions": -1 })
            )
            .is_err()
        );
    }

    #[test]
    fn exact_boundary_release_is_idempotent() {
        let budget = ResourceBudget::new(4, "test");
        let reservation = budget.reserve(4).unwrap();
        assert_eq!(budget.used(), 4);
        assert!(budget.reserve(1).is_err());
        reservation.release();
        reservation.release();
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn combines_replacement_and_credit_without_transient_over_admission() {
        let budget = ResourceBudget::new(10, "bytes");
        let target = budget.reserve(6).unwrap();
        let credit = budget.reserve(4).unwrap();
        let replacement = budget.resize_with_credit(&target, 9, &credit).unwrap();
        assert_eq!(budget.used(), 9);
        assert!(target.released());
        assert!(credit.released());
        replacement.release();
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn resize_charges_only_delta_and_transfers_release_ownership() {
        let budget = ResourceBudget::new(4, "bytes");
        let original = budget.reserve(4).unwrap();
        let identical = budget.resize(&original, 4).unwrap();
        assert_eq!(budget.used(), 4);
        original.release();
        assert_eq!(budget.used(), 4);
        let smaller = budget.resize(&identical, 2).unwrap();
        assert_eq!(budget.used(), 2);
        smaller.release();
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn grouped_failures_release_capacity_for_reuse_exactly_once() {
        let count = ResourceBudget::new(1, "count");
        let bytes = ResourceBudget::new(4, "bytes");
        let mut group = ReservationGroup::default();
        group.add(count.reserve(1).unwrap()).unwrap();
        assert!(bytes.reserve(5).is_err());
        group.release();
        group.release();
        let reused_count = count.reserve(1).unwrap();
        let reused_bytes = bytes.reserve(4).unwrap();
        assert_eq!((count.used(), bytes.used()), (1, 4));
        reused_count.release();
        reused_bytes.release();
    }

    #[test]
    fn released_group_rejects_and_releases_late_reservations() {
        let budget = ResourceBudget::new(1, "count");
        let mut group = ReservationGroup::default();
        group.release();
        assert_eq!(
            group.add(budget.reserve(1).unwrap()),
            Err(BudgetError::ReleasedGroup)
        );
        assert_eq!(budget.used(), 0);
    }
}
