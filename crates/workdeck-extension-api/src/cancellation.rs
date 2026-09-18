//! Typed metadata for native JSON-RPC cancellation notifications.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionCancellationCause {
    Settled,
    Cancelled,
    TimedOut,
}

/// Parameters of `$/cancelRequest`. Old id-only notifications remain valid.
/// `reason` can retain a JSON-compatible upstream reason without flattening it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionRequestCancellation {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<ExtensionCancellationCause>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<Value>,
}

impl ExtensionRequestCancellation {
    #[must_use]
    pub fn new(id: u64, cause: ExtensionCancellationCause) -> Self {
        Self {
            id,
            cause: Some(cause),
            reason: None,
        }
    }
}

impl std::fmt::Display for ExtensionRequestCancellation {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(output, "extension request {} cancelled", self.id)
    }
}
impl std::error::Error for ExtensionRequestCancellation {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn legacy_id_only_payload_round_trips_without_inventing_a_reason() {
        let value = json!({"id":7});
        let cancellation: ExtensionRequestCancellation =
            serde_json::from_value(value.clone()).unwrap();
        assert_eq!(cancellation.cause, None);
        assert_eq!(cancellation.reason, None);
        assert_eq!(serde_json::to_value(cancellation).unwrap(), value);
    }

    #[test]
    fn every_cause_and_structured_reason_survives_round_trip() {
        for (cause, wire) in [
            (ExtensionCancellationCause::Settled, "settled"),
            (ExtensionCancellationCause::Cancelled, "cancelled"),
            (ExtensionCancellationCause::TimedOut, "timed_out"),
        ] {
            let mut cancellation = ExtensionRequestCancellation::new(9, cause);
            cancellation.reason = Some(json!({"message":"stop", "details":[1, true, null]}));
            let value = serde_json::to_value(&cancellation).unwrap();
            assert_eq!(value["cause"], wire);
            assert_eq!(
                serde_json::from_value::<ExtensionRequestCancellation>(value).unwrap(),
                cancellation
            );
        }
    }
}
