//! Validation and containment for extension-provided line highlights.

use serde_json::{Number, Value};
use std::collections::HashMap;
use workdeck_core::ReviewSide;
use workdeck_extension_api::{HighlightTone, ValidatedLineHighlight};

/// Per-highlighter mark cap for one file.
pub const MAX_LINE_HIGHLIGHTS_PER_FILE: usize = 2_000;
/// Per-highlighter mark cap for one source line.
pub const MAX_LINE_HIGHLIGHTS_PER_LINE: usize = 100;
/// Raw-entry cap checked before inspecting any array members.
pub const MAX_LINE_HIGHLIGHT_INPUT_ENTRIES: usize = 10_000;
/// Aggregate cap enforced by the highlighter coordinator after merging contributors.
pub const MAX_MERGED_LINE_HIGHLIGHTS_PER_FILE: usize = 4_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineHighlightValidation {
    Valid {
        marks: Vec<ValidatedLineHighlight>,
        dropped_invalid: usize,
    },
    Invalid {
        issue: String,
    },
}

impl LineHighlightValidation {
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        matches!(self, Self::Valid { .. })
    }
}

/// Validate one highlighter result for one file.
///
/// `None`, JSON `null`, and an empty array are ordinary no-mark answers.
/// Structurally bad entries are dropped individually. Containment-limit
/// violations reject the whole result so the UI never paints a misleading
/// truncated subset.
#[must_use]
pub fn validate_line_highlights(result: Option<&Value>) -> LineHighlightValidation {
    let Some(result) = result else {
        return valid_empty_highlights();
    };
    if result.is_null() {
        return valid_empty_highlights();
    }
    let Some(entries) = result.as_array() else {
        return LineHighlightValidation::Invalid {
            issue: "returned a non-array result".into(),
        };
    };
    if entries.len() > MAX_LINE_HIGHLIGHT_INPUT_ENTRIES {
        return LineHighlightValidation::Invalid {
            issue: format!(
                "returned more than {MAX_LINE_HIGHLIGHT_INPUT_ENTRIES} entries for one file"
            ),
        };
    }

    let mut marks = Vec::new();
    let mut per_line = HashMap::<(bool, u64), usize>::new();
    let mut dropped_invalid = 0;
    for entry in entries {
        let Some(mark) = validate_entry(entry) else {
            dropped_invalid += 1;
            continue;
        };
        if marks.len() >= MAX_LINE_HIGHLIGHTS_PER_FILE {
            return LineHighlightValidation::Invalid {
                issue: format!(
                    "returned more than {MAX_LINE_HIGHLIGHTS_PER_FILE} ranges for one file"
                ),
            };
        }
        let key = (mark.side == ReviewSide::New, mark.line);
        let line_count = per_line.entry(key).or_default();
        *line_count += 1;
        if *line_count > MAX_LINE_HIGHLIGHTS_PER_LINE {
            return LineHighlightValidation::Invalid {
                issue: format!(
                    "returned more than {MAX_LINE_HIGHLIGHTS_PER_LINE} ranges on one line"
                ),
            };
        }
        marks.push(mark);
    }
    LineHighlightValidation::Valid {
        marks,
        dropped_invalid,
    }
}

fn valid_empty_highlights() -> LineHighlightValidation {
    LineHighlightValidation::Valid {
        marks: Vec::new(),
        dropped_invalid: 0,
    }
}

fn validate_entry(entry: &Value) -> Option<ValidatedLineHighlight> {
    let candidate = entry.as_object()?;
    let side = match candidate.get("side")?.as_str()? {
        "old" => ReviewSide::Old,
        "new" => ReviewSide::New,
        _ => return None,
    };
    let line = non_negative_integer(candidate.get("line")?.as_number()?)?;
    if line < 1 {
        return None;
    }
    let range = candidate.get("range")?.as_array()?;
    if range.len() != 2 {
        return None;
    }
    let start = non_negative_integer(range[0].as_number()?)?;
    let end = non_negative_integer(range[1].as_number()?)?;
    if start >= end {
        return None;
    }
    let tone = match candidate.get("tone") {
        None => HighlightTone::Match,
        Some(Value::String(tone)) => match tone.as_str() {
            "match" => HighlightTone::Match,
            "current" => HighlightTone::Current,
            "info" => HighlightTone::Info,
            "warning" => HighlightTone::Warning,
            "error" => HighlightTone::Error,
            "dim" => HighlightTone::Dim,
            _ => return None,
        },
        Some(_) => return None,
    };
    Some(ValidatedLineHighlight {
        side,
        line,
        start,
        end,
        tone,
    })
}

fn non_negative_integer(number: &Number) -> Option<u64> {
    number.as_u64().or_else(|| {
        let value = number.as_f64()?;
        (value.is_finite() && value >= 0.0 && value.fract() == 0.0 && value <= u64::MAX as f64)
            .then_some(value as u64)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid(
        marks: Vec<ValidatedLineHighlight>,
        dropped_invalid: usize,
    ) -> LineHighlightValidation {
        LineHighlightValidation::Valid {
            marks,
            dropped_invalid,
        }
    }

    #[test]
    fn null_missing_and_empty_are_ordinary_no_mark_answers() {
        assert_eq!(validate_line_highlights(None), valid(Vec::new(), 0));
        assert_eq!(
            validate_line_highlights(Some(&Value::Null)),
            valid(Vec::new(), 0)
        );
        assert_eq!(
            validate_line_highlights(Some(&json!([]))),
            valid(Vec::new(), 0)
        );
    }

    #[test]
    fn rejects_a_non_array_result_whole() {
        assert_eq!(
            validate_line_highlights(Some(&json!({ "side": "new" }))),
            LineHighlightValidation::Invalid {
                issue: "returned a non-array result".into()
            }
        );
    }

    #[test]
    fn accepts_valid_marks_and_applies_the_tone_default() {
        assert_eq!(
            validate_line_highlights(Some(&json!([
                { "side": "new", "line": 3, "range": [2, 8] },
                { "side": "old", "line": 1, "range": [0, 1], "tone": "error" },
                { "side": "new", "line": 4, "range": [0, 10], "tone": "dim" }
            ]))),
            valid(
                vec![
                    ValidatedLineHighlight {
                        side: ReviewSide::New,
                        line: 3,
                        start: 2,
                        end: 8,
                        tone: HighlightTone::Match,
                    },
                    ValidatedLineHighlight {
                        side: ReviewSide::Old,
                        line: 1,
                        start: 0,
                        end: 1,
                        tone: HighlightTone::Error,
                    },
                    ValidatedLineHighlight {
                        side: ReviewSide::New,
                        line: 4,
                        start: 0,
                        end: 10,
                        tone: HighlightTone::Dim,
                    },
                ],
                0,
            )
        );
    }

    #[test]
    fn drops_structurally_invalid_entries_individually_and_counts_them() {
        let result = json!([
            { "side": "new", "line": 2, "range": [1, 4] },
            null,
            { "side": "both", "line": 2, "range": [1, 4] },
            { "side": "new", "line": 0, "range": [1, 4] },
            { "side": "new", "line": 1.5, "range": [1, 4] },
            { "side": "new", "line": 2, "range": [4, 4] },
            { "side": "new", "line": 2, "range": [5, 4] },
            { "side": "new", "line": 2, "range": [-1, 4] },
            { "side": "new", "line": 2, "range": [1] },
            { "side": "new", "line": 2, "range": [1, 4], "tone": "sparkle" }
        ]);
        let LineHighlightValidation::Valid {
            marks,
            dropped_invalid,
        } = validate_line_highlights(Some(&result))
        else {
            panic!("otherwise usable results remain valid")
        };
        assert_eq!(marks.len(), 1);
        assert_eq!(dropped_invalid, 9);
    }

    #[test]
    fn rejects_a_file_exceeding_the_per_file_cap_instead_of_truncating() {
        let result = Value::Array(
            (0..=MAX_LINE_HIGHLIGHTS_PER_FILE)
                .map(|index| json!({ "side": "new", "line": index + 1, "range": [0, 1] }))
                .collect(),
        );
        let LineHighlightValidation::Invalid { issue } = validate_line_highlights(Some(&result))
        else {
            panic!("over-limit files must be rejected")
        };
        assert!(issue.contains(&MAX_LINE_HIGHLIGHTS_PER_FILE.to_string()));
    }

    #[test]
    fn rejects_an_oversized_result_before_validating_entries() {
        let result = Value::Array(
            std::iter::repeat_n(
                Value::String("garbage".into()),
                MAX_LINE_HIGHLIGHT_INPUT_ENTRIES + 1,
            )
            .collect(),
        );
        let LineHighlightValidation::Invalid { issue } = validate_line_highlights(Some(&result))
        else {
            panic!("oversized input must be rejected")
        };
        assert!(issue.contains(&MAX_LINE_HIGHLIGHT_INPUT_ENTRIES.to_string()));
    }

    #[test]
    fn rejects_a_line_exceeding_the_per_line_cap_instead_of_truncating() {
        let result = Value::Array(
            (0..=MAX_LINE_HIGHLIGHTS_PER_LINE)
                .map(|index| json!({ "side": "new", "line": 7, "range": [index, index + 1] }))
                .collect(),
        );
        let LineHighlightValidation::Invalid { issue } = validate_line_highlights(Some(&result))
        else {
            panic!("over-limit lines must be rejected")
        };
        assert!(issue.contains(&MAX_LINE_HIGHLIGHTS_PER_LINE.to_string()));
    }
}
