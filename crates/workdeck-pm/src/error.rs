use serde::{Deserialize, Serialize};
use std::path::Path;

pub type Result<T> = std::result::Result<T, PmError>;

/// Stable machine categories. Human-readable messages may improve without
/// changing this versioned contract.
#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidInput,
    InvalidSchema,
    UnsupportedSchema,
    NotInitialized,
    LegacyStore,
    AmbiguousSource,
    CorruptStore,
    NotFound,
    AmbiguousReference,
    Conflict,
    StaleSource,
    IdempotencyConflict,
    RecoveryRequired,
    PolicyBlocked,
    ClaimLost,
    Locked,
    UnsafePath,
    Unsupported,
    Canceled,
    Io,
}

impl ErrorCode {
    pub const fn exit_code(self) -> u8 {
        match self {
            Self::InvalidInput | Self::InvalidSchema | Self::UnsupportedSchema => 2,
            Self::NotFound | Self::NotInitialized => 3,
            Self::Conflict
            | Self::StaleSource
            | Self::IdempotencyConflict
            | Self::ClaimLost
            | Self::Locked => 4,
            Self::PolicyBlocked => 5,
            Self::RecoveryRequired
            | Self::AmbiguousSource
            | Self::CorruptStore
            | Self::LegacyStore => 6,
            Self::Canceled => 130,
            Self::Io | Self::UnsafePath | Self::Unsupported | Self::AmbiguousReference => 1,
        }
    }

    /// Retryable permits reload/re-evaluation, never a blind stale write.
    pub const fn retryable(self) -> bool {
        matches!(
            self,
            Self::Conflict | Self::StaleSource | Self::Locked | Self::Io
        )
    }
}

#[derive(
    schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error,
)]
#[error("{message}")]
pub struct PmError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// Structured diagnostics and explicit partial outcomes for automation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Box<serde_json::Value>>,
}

impl PmError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            path: None,
            line: None,
            column: None,
            hint: None,
            details: None,
        }
    }
    pub fn at(mut self, path: impl AsRef<Path>) -> Self {
        self.path = Some(path.as_ref().to_string_lossy().into_owned());
        self
    }
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
    pub fn details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(Box::new(details));
        self
    }
    pub fn io(path: impl AsRef<Path>, error: impl std::fmt::Display) -> Self {
        Self::new(ErrorCode::Io, error.to_string()).at(path)
    }
}

impl From<crate::documents::DocumentError> for PmError {
    fn from(error: crate::documents::DocumentError) -> Self {
        let mut result = Self::new(ErrorCode::InvalidSchema, error.message).at(error.path);
        result.line = Some(error.line);
        result.column = Some(error.column);
        result
    }
}
