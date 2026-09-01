//! Runtime-neutral broker facade types shared by native listeners and clients.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{CallerOperation, CallerPrincipal, SessionSelector, SignedBrokerAppContract};

pub const DEFAULT_SESSION_BROKER_HEALTH_PATH: &str = "/health";
pub const DEFAULT_SESSION_BROKER_API_PATH: &str = "/broker";
pub const DEFAULT_SESSION_BROKER_CAPABILITIES_PATH: &str = "/broker/capabilities";
pub const DEFAULT_SESSION_BROKER_SOCKET_PATH: &str = "/session";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionBrokerCapabilities {
    pub version: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features: Option<Vec<String>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionBrokerHttpPaths {
    pub health: String,
    pub socket: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum SessionBrokerDaemonRequest<CommandName = String, CommandInput = Value> {
    #[serde(rename = "list")]
    List,
    #[serde(rename = "get")]
    Get { selector: SessionSelector },
    #[serde(rename = "dispatch")]
    Dispatch {
        selector: SessionSelector,
        command: CommandName,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        command_version: Option<u64>,
        input: CommandInput,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_message: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SessionBrokerDaemonResponse<SessionView = Value, CommandResult = Value> {
    Sessions { sessions: Vec<SessionView> },
    Session { session: SessionView },
    Result { result: CommandResult },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionBrokerResponseAuthentication {
    pub generation: String,
    pub broker_revision: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_contract: Option<SignedBrokerAppContractWire>,
    pub caller_session_id: String,
    pub request_id: String,
    pub sequence: String,
    pub http_status: u16,
    pub body_digest: String,
    pub daemon_key_id: String,
    pub daemon_signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignedBrokerAppContractWire {
    pub app_revision: u32,
    pub features: Vec<String>,
}

impl From<&SignedBrokerAppContract> for SignedBrokerAppContractWire {
    fn from(value: &SignedBrokerAppContract) -> Self {
        Self {
            app_revision: value.app_revision,
            features: value.features.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionBrokerAuthenticatedResponse<Body = Value> {
    pub body: Body,
    pub authentication: SessionBrokerResponseAuthentication,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionBrokerTargetContract {
    pub app_revision: u32,
    pub features: Vec<String>,
}

impl SessionBrokerTargetContract {
    #[must_use]
    pub const fn new(app_revision: u32) -> Self {
        Self {
            app_revision,
            features: Vec::new(),
        }
    }

    #[must_use]
    pub fn is_phase_one(&self) -> bool {
        self.features.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionBrokerHealth {
    pub ok: bool,
    pub pid: u64,
    pub sessions: u64,
    pub pending_commands: u64,
    pub started_at: String,
    pub uptime_ms: u64,
    pub stale_session_ttl_ms: u64,
    pub paths: SessionBrokerHttpPaths,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionBrokerSocketCloseEvent {
    pub code: u16,
    pub reason: String,
    pub authenticated: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionBrokerSocketMessageEvent {
    pub data: Value,
}

pub type SessionBrokerSocketOpenHandler = Arc<dyn Fn() + Send + Sync>;
pub type SessionBrokerSocketMessageHandler =
    Arc<dyn Fn(SessionBrokerSocketMessageEvent) + Send + Sync>;
pub type SessionBrokerSocketCloseHandler = Arc<dyn Fn(SessionBrokerSocketCloseEvent) + Send + Sync>;
pub type SessionBrokerSocketErrorHandler = Arc<dyn Fn() + Send + Sync>;

/// Browser-shaped socket boundary without tying the core to one websocket implementation.
pub trait SessionBrokerSocketLike: Send + Sync {
    fn ready_state(&self) -> u16;
    fn send(&self, data: &str) -> Result<(), String>;
    fn close(&self, code: Option<u16>, reason: Option<&str>);
    fn set_on_open(&self, handler: Option<SessionBrokerSocketOpenHandler>);
    fn set_on_message(&self, handler: Option<SessionBrokerSocketMessageHandler>);
    fn set_on_close(&self, handler: Option<SessionBrokerSocketCloseHandler>);
    fn set_on_error(&self, handler: Option<SessionBrokerSocketErrorHandler>);
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionBrokerConnectionCloseDirective {
    pub reconnect: Option<bool>,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SessionBrokerCancellation(Arc<AtomicBool>);

impl SessionBrokerCancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Debug, Clone)]
pub struct SessionBrokerAuthorizationContext {
    pub principal: CallerPrincipal,
    pub operation: CallerOperation,
    pub session_id: Option<String>,
    pub command: Option<String>,
    pub command_version: Option<u64>,
    pub request_id: Option<String>,
    pub cancellation: SessionBrokerCancellation,
}

pub trait SessionBrokerAuthorizer: Send + Sync {
    fn authorize<'a>(
        &'a self,
        context: &'a SessionBrokerAuthorizationContext,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionBrokerAuditOperation {
    Caller(CallerOperation),
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionBrokerAuditDecision {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionBrokerAuditOutcome {
    Authenticated,
    AuthenticationFailed,
    AuthorizationFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionBrokerAuditEvent {
    pub app_id: String,
    pub principal_id: Option<String>,
    pub key_id: Option<String>,
    pub session_id: Option<String>,
    pub operation: SessionBrokerAuditOperation,
    pub command: Option<String>,
    pub command_version: Option<u64>,
    pub request_id: Option<String>,
    pub decision: SessionBrokerAuditDecision,
    pub outcome: SessionBrokerAuditOutcome,
    pub timestamp: u64,
}

pub trait SessionBrokerAuditHook: Send + Sync {
    fn record<'a>(
        &'a self,
        event: &'a SessionBrokerAuditEvent,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_paths_and_phase_one_contract_match_the_runtime_neutral_facade() {
        assert_eq!(DEFAULT_SESSION_BROKER_HEALTH_PATH, "/health");
        assert_eq!(DEFAULT_SESSION_BROKER_API_PATH, "/broker");
        assert_eq!(
            DEFAULT_SESSION_BROKER_CAPABILITIES_PATH,
            "/broker/capabilities"
        );
        assert_eq!(DEFAULT_SESSION_BROKER_SOCKET_PATH, "/session");
        assert!(SessionBrokerTargetContract::new(7).is_phase_one());
    }

    #[test]
    fn daemon_unions_and_signed_response_keep_exact_wire_shapes() {
        let request: SessionBrokerDaemonRequest = serde_json::from_value(serde_json::json!({
            "action": "dispatch",
            "selector": {"sessionId": "session-1"},
            "command": "navigate_to_hunk",
            "commandVersion": 1,
            "input": {"filePath": "src/main.rs"},
            "timeoutMs": 1000,
            "timeoutMessage": "late"
        }))
        .unwrap();
        assert_eq!(serde_json::to_value(request).unwrap()["action"], "dispatch");

        let response: SessionBrokerAuthenticatedResponse =
            serde_json::from_value(serde_json::json!({
                "body": {"sessions": []},
                "authentication": {
                    "generation": "generation-1",
                    "brokerRevision": 1,
                    "callerSessionId": "caller-1",
                    "requestId": "request-1",
                    "sequence": "1",
                    "httpStatus": 200,
                    "bodyDigest": "digest",
                    "daemonKeyId": "daemon-1",
                    "daemonSignature": "signature"
                }
            }))
            .unwrap();
        assert_eq!(response.authentication.http_status, 200);
    }

    #[test]
    fn capability_extensions_round_trip_and_cancellation_is_shared() {
        let capability: SessionBrokerCapabilities = serde_json::from_value(serde_json::json!({
            "version": 1,
            "name": "workdeck",
            "vendorFlag": true
        }))
        .unwrap();
        assert_eq!(capability.extra.get("vendorFlag"), Some(&Value::Bool(true)));

        let first = SessionBrokerCancellation::default();
        let second = first.clone();
        first.cancel();
        assert!(second.is_cancelled());
    }
}
