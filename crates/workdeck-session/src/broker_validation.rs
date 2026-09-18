//! Strict, redacted validation for every value entering the session broker boundary.

use crate::{
    SessionSelector, is_valid_broker_app_id, is_valid_broker_identifier, is_valid_broker_revision,
    parse_caller_sequence,
};
use serde_json::{Map, Value};
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

pub const MAX_BROKER_STRING_BYTES: usize = 4_096;
pub const MAX_BROKER_ERROR_BYTES: usize = 1_024;
pub const MAX_BROKER_DEADLINE_MS: u64 = 5 * 60_000;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrokerProtocolFailureCode {
    InvalidJson,
    InvalidRecord,
    InvalidKeys,
    InvalidDiscriminant,
    InvalidField,
    InvalidSelector,
    InvalidDeadline,
    InvalidContract,
    UnknownCommand,
    InvalidAppPayload,
    AppParserFailed,
}

impl BrokerProtocolFailureCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidJson => "invalid-json",
            Self::InvalidRecord => "invalid-record",
            Self::InvalidKeys => "invalid-keys",
            Self::InvalidDiscriminant => "invalid-discriminant",
            Self::InvalidField => "invalid-field",
            Self::InvalidSelector => "invalid-selector",
            Self::InvalidDeadline => "invalid-deadline",
            Self::InvalidContract => "invalid-contract",
            Self::UnknownCommand => "unknown-command",
            Self::InvalidAppPayload => "invalid-app-payload",
            Self::AppParserFailed => "app-parser-failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrokerProtocolError {
    pub code: BrokerProtocolFailureCode,
}

impl fmt::Display for BrokerProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Session broker protocol validation failed.")
    }
}

impl std::error::Error for BrokerProtocolError {}

pub type BrokerProtocolResult<T> = Result<T, BrokerProtocolError>;

const fn protocol_failure(code: BrokerProtocolFailureCode) -> BrokerProtocolError {
    BrokerProtocolError { code }
}

pub fn fail_broker_protocol<T>(code: BrokerProtocolFailureCode) -> BrokerProtocolResult<T> {
    Err(protocol_failure(code))
}

pub fn parse_broker_record(value: &Value) -> BrokerProtocolResult<&Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| protocol_failure(BrokerProtocolFailureCode::InvalidRecord))
}

pub fn parse_exact_broker_record<'a>(
    value: &'a Value,
    required: &[&str],
    optional: &[&str],
) -> BrokerProtocolResult<&'a Map<String, Value>> {
    let record = parse_broker_record(value)?;
    if required.iter().any(|key| !record.contains_key(*key))
        || record
            .keys()
            .any(|key| !required.contains(&key.as_str()) && !optional.contains(&key.as_str()))
    {
        return fail_broker_protocol(BrokerProtocolFailureCode::InvalidKeys);
    }
    Ok(record)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrokerStringOptions {
    pub min_bytes: usize,
    pub max_bytes: usize,
}

impl Default for BrokerStringOptions {
    fn default() -> Self {
        Self {
            min_bytes: 1,
            max_bytes: MAX_BROKER_STRING_BYTES,
        }
    }
}

pub fn parse_broker_string(
    value: &Value,
    options: BrokerStringOptions,
) -> BrokerProtocolResult<&str> {
    let string = value
        .as_str()
        .ok_or_else(|| protocol_failure(BrokerProtocolFailureCode::InvalidField))?;
    if string.len() < options.min_bytes || string.len() > options.max_bytes {
        return fail_broker_protocol(BrokerProtocolFailureCode::InvalidField);
    }
    Ok(string)
}

pub fn parse_broker_identifier(value: &Value) -> BrokerProtocolResult<&str> {
    let identifier = value
        .as_str()
        .filter(|value| is_valid_broker_identifier(value))
        .ok_or_else(|| protocol_failure(BrokerProtocolFailureCode::InvalidField))?;
    Ok(identifier)
}

pub fn parse_broker_app_id(value: &Value) -> BrokerProtocolResult<&str> {
    let app_id = value
        .as_str()
        .filter(|value| is_valid_broker_app_id(value))
        .ok_or_else(|| protocol_failure(BrokerProtocolFailureCode::InvalidField))?;
    Ok(app_id)
}

pub fn parse_broker_safe_integer(
    value: &Value,
    minimum: u64,
    maximum: u64,
) -> BrokerProtocolResult<u64> {
    let integer = value
        .as_u64()
        .filter(|integer| *integer <= MAX_SAFE_INTEGER)
        .ok_or_else(|| protocol_failure(BrokerProtocolFailureCode::InvalidField))?;
    if integer < minimum || integer > maximum {
        return fail_broker_protocol(BrokerProtocolFailureCode::InvalidField);
    }
    Ok(integer)
}

pub fn parse_broker_revision(value: &Value) -> BrokerProtocolResult<u64> {
    let Some(revision) = value
        .as_u64()
        .filter(|value| is_valid_broker_revision(*value))
    else {
        return fail_broker_protocol(BrokerProtocolFailureCode::InvalidContract);
    };
    Ok(revision)
}

pub fn parse_broker_uint64(value: &Value, allow_zero: bool) -> BrokerProtocolResult<String> {
    let Some(canonical) = value.as_str() else {
        return fail_broker_protocol(BrokerProtocolFailureCode::InvalidField);
    };
    let Some(parsed) = parse_caller_sequence(canonical) else {
        return fail_broker_protocol(BrokerProtocolFailureCode::InvalidField);
    };
    if !allow_zero && parsed == 0 {
        return fail_broker_protocol(BrokerProtocolFailureCode::InvalidField);
    }
    Ok(canonical.into())
}

pub fn parse_broker_selector(value: &Value) -> BrokerProtocolResult<SessionSelector> {
    let record = parse_exact_broker_record(
        value,
        &[],
        &["sessionId", "sessionPath", "repoRoot", "repoBoundary"],
    )?;
    let session_id = record
        .get("sessionId")
        .map(parse_broker_identifier)
        .transpose()?
        .map(str::to_owned);
    let parse_path = |key: &str| -> BrokerProtocolResult<Option<PathBuf>> {
        record
            .get(key)
            .map(|value| parse_broker_string(value, BrokerStringOptions::default()))
            .transpose()
            .map(|path| path.map(PathBuf::from))
    };
    Ok(SessionSelector {
        session_id,
        session_path: parse_path("sessionPath")?,
        repo_root: parse_path("repoRoot")?,
        repo_boundary: parse_path("repoBoundary")?,
    })
}

pub fn parse_broker_timeout(value: &Value) -> BrokerProtocolResult<u64> {
    parse_broker_safe_integer(value, 1, MAX_BROKER_DEADLINE_MS)
        .map_err(|_| protocol_failure(BrokerProtocolFailureCode::InvalidDeadline))
}

pub fn parse_broker_deadline(value: &Value) -> BrokerProtocolResult<u64> {
    value
        .as_u64()
        .filter(|deadline| *deadline > 0 && *deadline <= MAX_SAFE_INTEGER)
        .ok_or_else(|| protocol_failure(BrokerProtocolFailureCode::InvalidDeadline))
}

pub fn parse_broker_app_payload<T>(
    parser: impl FnOnce(&Value) -> Option<T>,
    value: &Value,
) -> BrokerProtocolResult<T> {
    match catch_unwind(AssertUnwindSafe(|| parser(value))) {
        Ok(Some(parsed)) => Ok(parsed),
        Ok(None) => fail_broker_protocol(BrokerProtocolFailureCode::InvalidAppPayload),
        Err(_) => fail_broker_protocol(BrokerProtocolFailureCode::AppParserFailed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn code<T>(result: BrokerProtocolResult<T>) -> Option<BrokerProtocolFailureCode> {
        result.err().map(|error| error.code)
    }

    #[test]
    fn requires_exact_plain_records_and_strict_optional_fields() {
        for value in [
            Value::Null,
            json!([]),
            json!("record"),
            json!({
                "required": 1,
                "extra": true
            }),
        ] {
            assert!(parse_exact_broker_record(&value, &["required"], &[]).is_err());
        }
        assert_eq!(
            parse_exact_broker_record(&json!({ "required": 1 }), &["required"], &[]).unwrap()["required"],
            1
        );
        assert_eq!(
            code(parse_broker_selector(
                &json!({ "sessionId": "session-1", "extra": true })
            )),
            Some(BrokerProtocolFailureCode::InvalidKeys)
        );
        assert_eq!(
            code(parse_broker_selector(&json!({ "repoRoot": null }))),
            Some(BrokerProtocolFailureCode::InvalidField)
        );
    }

    #[test]
    fn dangerous_prototype_names_are_only_data_and_never_inherited_fields() {
        for key in ["__proto__", "constructor", "toString"] {
            let value = json!({ "required": 1, key: true });
            assert_eq!(
                code(parse_exact_broker_record(&value, &["required"], &[])),
                Some(BrokerProtocolFailureCode::InvalidKeys)
            );
        }
        // `serde_json::Value` has no prototype chain: a missing own key stays missing.
        assert_eq!(
            code(parse_exact_broker_record(&json!({}), &["required"], &[])),
            Some(BrokerProtocolFailureCode::InvalidKeys)
        );
    }

    #[test]
    fn bounds_identifiers_revisions_uint64_values_and_deadlines() {
        assert_eq!(parse_broker_app_id(&json!("dev.hunk")).unwrap(), "dev.hunk");
        assert_eq!(
            parse_broker_identifier(&json!("session-1")).unwrap(),
            "session-1"
        );
        for value in [
            json!(0),
            json!(-1),
            json!(1.5),
            json!(9_007_199_254_740_992_u64),
            Value::Null,
        ] {
            assert_eq!(
                code(parse_broker_revision(&value)),
                Some(BrokerProtocolFailureCode::InvalidContract)
            );
        }
        assert_eq!(
            parse_broker_uint64(&json!(u64::MAX.to_string()), false).unwrap(),
            u64::MAX.to_string()
        );
        assert_eq!(
            code(parse_broker_uint64(&json!("01"), false)),
            Some(BrokerProtocolFailureCode::InvalidField)
        );
        assert_eq!(parse_broker_timeout(&json!(300_000)).unwrap(), 300_000);
        assert_eq!(
            code(parse_broker_timeout(&json!(300_001))),
            Some(BrokerProtocolFailureCode::InvalidDeadline)
        );
        assert_eq!(
            code(parse_broker_deadline(&json!(f64::INFINITY))),
            Some(BrokerProtocolFailureCode::InvalidDeadline)
        );
    }

    #[test]
    fn redacts_app_parser_rejection_and_panics() {
        assert_eq!(
            code(parse_broker_app_payload::<Value>(|_| None, &json!({}))),
            Some(BrokerProtocolFailureCode::InvalidAppPayload)
        );
        let error =
            parse_broker_app_payload::<Value>(|_| panic!("secret parser internals"), &json!({}))
                .unwrap_err();
        assert_eq!(error.code, BrokerProtocolFailureCode::AppParserFailed);
        assert!(!error.to_string().contains("secret parser internals"));
    }

    #[test]
    fn bounds_strings_by_utf8_bytes_instead_of_scalar_count() {
        let value = json!("🦀");
        assert_eq!(
            parse_broker_string(
                &value,
                BrokerStringOptions {
                    min_bytes: 4,
                    max_bytes: 4,
                }
            )
            .unwrap(),
            "🦀"
        );
        assert_eq!(
            code(parse_broker_string(
                &value,
                BrokerStringOptions {
                    min_bytes: 1,
                    max_bytes: 3,
                }
            )),
            Some(BrokerProtocolFailureCode::InvalidField)
        );
    }
}
