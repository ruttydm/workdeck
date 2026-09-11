//! Status-line contributions and inline prompt requests from native extensions.
//!
//! Hunk's API 26 made the bottom status row host-owned: extensions describe
//! persistent text items with symbolic tones and ask for one line of text
//! through a real focused input the host draws. This module owns those wire
//! shapes. Measurement, truncation, and painting live in `workdeck-tui`; the
//! host only validates and namespaces what crosses the boundary.

use serde::{Deserialize, Serialize};

use crate::file_views::{ExtensionFileViewSpan, ExtensionFileViewTone, ExtensionTextAttribute};

/// One symbolic run of status text; the same span vocabulary file views use.
pub type ExtensionStatusSpan = ExtensionFileViewSpan;

/// The symbolic tone vocabulary, resolved against the active theme at paint time.
pub type ExtensionStatusTone = ExtensionFileViewTone;

/// Generic emphasis names a status span may stack.
pub type ExtensionStatusAttribute = ExtensionTextAttribute;

/// Which side of the keyboard-mode badge an item paints on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtensionStatusAlignment {
    Left,
    Right,
}

/// One persistent, text-only contribution to the bottom status row.
///
/// Items are declarative: the host measures them without a theme, paints them
/// with the active one, and decides what survives a narrow terminal. Setting an
/// item keeps the row on screen, exactly like a non-empty file filter does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionStatusItem {
    /// Extension-local id; the host namespaces it to `<ext-prefix><extensionId>:<id>`.
    pub id: String,
    pub spans: Vec<ExtensionStatusSpan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alignment: Option<ExtensionStatusAlignment>,
    /// Higher survives longer when the row overflows. Defaults to 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
}

impl ExtensionStatusItem {
    #[must_use]
    pub const fn alignment_or_default(&self) -> ExtensionStatusAlignment {
        match self.alignment {
            Some(ExtensionStatusAlignment::Right) => ExtensionStatusAlignment::Right,
            _ => ExtensionStatusAlignment::Left,
        }
    }

    #[must_use]
    pub const fn priority_or_default(&self) -> i32 {
        match self.priority {
            Some(priority) => priority,
            None => 0,
        }
    }
}

/// Options for one host-drawn inline prompt, mirroring `ctx.prompts.line()`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionPromptLineOptions {
    /// Painted before the input, e.g. "/" or "filter:". Not part of the value.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub prefix: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub placeholder: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub initial: String,
    /// Ask the host to report every edit back while the user types.
    #[serde(default, skip_serializing_if = "is_false")]
    pub on_change: bool,
}

const fn is_false(value: &bool) -> bool {
    !*value
}

/// The submitted text, or `None` on Escape, reload, or teardown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionPromptLineCompletion {
    pub request_id: String,
    pub value: Option<String>,
}

/// One live edit of an opted-in prompt, delivered while the user types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionPromptLineChange {
    pub request_id: String,
    pub value: String,
}

/// Bounded input contract for status items crossing the native boundary.
pub const MAX_STATUS_ITEM_SPANS: usize = 16;
/// Maximum bytes of terminal text one status span may carry.
pub const MAX_STATUS_SPAN_TEXT_BYTES: usize = 256;
/// Extension ids remain one terminal line so namespaced keys stay parseable.
pub const MAX_STATUS_ITEM_ID_BYTES: usize = 256;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_items_round_trip_through_the_wire_shape_with_kebab_tones() {
        let value = serde_json::json!({
            "id": "count",
            "spans": [
                { "text": "3 files ", "tone": "accent", "attributes": ["bold"] },
                { "text": "viewed", "tone": "removed" }
            ],
            "alignment": "right",
            "priority": 2
        });
        let item: ExtensionStatusItem = serde_json::from_value(value).unwrap();
        assert_eq!(item.alignment_or_default(), ExtensionStatusAlignment::Right);
        assert_eq!(item.priority_or_default(), 2);
        assert_eq!(item.spans.len(), 2);
        assert_eq!(item.spans[0].attributes, [ExtensionStatusAttribute::Bold]);
        let encoded = serde_json::to_value(&item).unwrap();
        assert_eq!(encoded["spans"][0]["tone"], "accent");
        assert_eq!(encoded["spans"][1]["tone"], "removed");
        assert!(encoded["spans"][1]["attributes"].is_null());
    }

    #[test]
    fn defaults_keep_items_left_aligned_at_priority_zero_and_prompts_minimal() {
        let item: ExtensionStatusItem =
            serde_json::from_value(serde_json::json!({ "id": "x", "spans": [] })).unwrap();
        assert_eq!(item.alignment_or_default(), ExtensionStatusAlignment::Left);
        assert_eq!(item.priority_or_default(), 0);
        assert!(serde_json::to_value(&item).unwrap()["alignment"].is_null());

        let options: ExtensionPromptLineOptions =
            serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(options, ExtensionPromptLineOptions::default());
        assert!(!options.on_change);
        assert_eq!(
            serde_json::to_value(&options).unwrap(),
            serde_json::json!({})
        );
    }

    #[test]
    fn prompt_completions_and_changes_use_camel_case_request_ids() {
        let completion: ExtensionPromptLineCompletion =
            serde_json::from_value(serde_json::json!({ "requestId": "q1", "value": "needle" }))
                .unwrap();
        assert_eq!(completion.request_id, "q1");
        assert_eq!(completion.value.as_deref(), Some("needle"));
        let change: ExtensionPromptLineChange =
            serde_json::from_value(serde_json::json!({ "requestId": "q1", "value": "nee" }))
                .unwrap();
        assert_eq!(change.value, "nee");
        let cancelled = serde_json::to_value(&ExtensionPromptLineCompletion {
            request_id: "q1".into(),
            value: None,
        })
        .unwrap();
        assert_eq!(cancelled["requestId"], "q1");
        assert_eq!(cancelled["value"], serde_json::Value::Null);
    }
}
