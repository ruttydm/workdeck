//! Structural verification of canonical review files against their manifest.

use serde_json::Value;
use thiserror::Error;
use workdeck_core::SemanticReviewFile;

use crate::{ReviewContentManifestFile, build_review_content_manifest_file};

pub fn review_canonical_file_mismatches(
    canonical: &SemanticReviewFile,
    expected: &ReviewContentManifestFile,
) -> Vec<String> {
    let expected = serde_json::to_value(expected).expect("manifest is JSON serializable");
    let actual = serde_json::to_value(build_review_content_manifest_file(canonical))
        .expect("manifest is JSON serializable");
    let mut mismatches = Vec::new();
    collect_mismatches("", &expected, &actual, &mut mismatches);
    mismatches
}

#[derive(Debug, Error, PartialEq, Eq)]
#[error("canonical review file disagrees with its manifest at: {paths}")]
pub struct ReviewCanonicalFileMismatchError {
    pub mismatches: Vec<String>,
    paths: String,
}

pub fn assert_canonical_file_matches_manifest(
    canonical: &SemanticReviewFile,
    expected: &ReviewContentManifestFile,
) -> Result<(), ReviewCanonicalFileMismatchError> {
    let mismatches = review_canonical_file_mismatches(canonical, expected);
    if mismatches.is_empty() {
        return Ok(());
    }
    Err(ReviewCanonicalFileMismatchError {
        paths: mismatches.join(", "),
        mismatches,
    })
}

fn collect_mismatches(path: &str, expected: &Value, actual: &Value, into: &mut Vec<String>) {
    match (expected, actual) {
        (Value::Array(expected), Value::Array(actual)) => {
            if expected.len() != actual.len() {
                into.push(format!("{path}.length"));
                return;
            }
            for (index, (expected, actual)) in expected.iter().zip(actual).enumerate() {
                collect_mismatches(&format!("{path}[{index}]"), expected, actual, into);
            }
        }
        (Value::Array(_), _) | (_, Value::Array(_)) => into.push(path.to_owned()),
        (Value::Object(expected), Value::Object(actual)) => {
            let mut keys = expected.keys().chain(actual.keys()).collect::<Vec<_>>();
            keys.sort();
            keys.dedup();
            for key in keys {
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                collect_mismatches(
                    &child,
                    expected.get(key).unwrap_or(&Value::Null),
                    actual.get(key).unwrap_or(&Value::Null),
                    into,
                );
            }
        }
        (Value::Object(_), _) | (_, Value::Object(_)) => into.push(path.to_owned()),
        _ if expected != actual => into.push(path.to_owned()),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use workdeck_core::{
        ReviewFileChangeKind, SemanticReviewFile, SemanticReviewFileFlags, SemanticReviewFileStats,
    };

    use super::*;

    fn file() -> SemanticReviewFile {
        SemanticReviewFile {
            key: "file:alpha".into(),
            runtime_id: "runtime".into(),
            path: "alpha.rs".into(),
            previous_path: None,
            change_kind: ReviewFileChangeKind::Change,
            language: Some("rust".into()),
            agent_summary: None,
            stats: SemanticReviewFileStats {
                additions: 0,
                deletions: 0,
                truncated: false,
            },
            flags: SemanticReviewFileFlags {
                untracked: false,
                binary: false,
                too_large: false,
                partial: false,
            },
            patch: String::new(),
            split_line_count: 0,
            unified_line_count: 0,
            addition_lines: Vec::new(),
            deletion_lines: Vec::new(),
            line_move_kinds: None,
            hunks: Vec::new(),
            content_identity: "content".into(),
            source_identity: Some("source:abc".into()),
            source_attested: Some(true),
        }
    }

    #[test]
    fn accepts_its_source_file_and_reports_changed_fields() {
        let file = file();
        let manifest = build_review_content_manifest_file(&file);
        assert!(review_canonical_file_mismatches(&file, &manifest).is_empty());
        assert_canonical_file_matches_manifest(&file, &manifest).unwrap();

        let mut path = file.clone();
        path.path = "other.rs".into();
        assert_eq!(review_canonical_file_mismatches(&path, &manifest), ["path"]);
        let error = assert_canonical_file_matches_manifest(&path, &manifest).unwrap_err();
        assert_eq!(error.mismatches, ["path"]);

        let mut patch = file.clone();
        patch.patch = "@@ -1 +1 @@".into();
        assert_eq!(
            review_canonical_file_mismatches(&patch, &manifest),
            ["patch"]
        );
    }

    #[test]
    fn structural_comparison_is_key_order_independent_and_reports_lengths() {
        let mut mismatches = Vec::new();
        collect_mismatches(
            "",
            &serde_json::json!({"a": 1, "b": [1, 2]}),
            &serde_json::json!({"b": [1, 2], "a": 1}),
            &mut mismatches,
        );
        assert!(mismatches.is_empty());
        collect_mismatches(
            "",
            &serde_json::json!({"a": [1]}),
            &serde_json::json!({"a": [1, 2]}),
            &mut mismatches,
        );
        assert_eq!(mismatches, ["a.length"]);
    }

    #[test]
    fn optional_manifest_fields_are_not_ignored() {
        let file = file();
        let mut expected = build_review_content_manifest_file(&file);
        expected.source_identity = None;
        assert_eq!(
            review_canonical_file_mismatches(&file, &expected),
            ["sourceIdentity"]
        );
    }
}
