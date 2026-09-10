//! Provenance checks for executable highlighter lifecycle fixtures.
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::path::Path;

const PINS: [&str; 2] = [
    "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2",
    "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd",
];
const SOURCE: &str = "src/ui/highlights/useLineHighlights.ts";
const FIXTURES: [&str; 2] = [
    "port/hunk/oracles/highlighter-registration-reorder.json",
    "port/hunk/oracles/highlighter-registration-removal.json",
];

fn validate(value: &Value, mut blob: impl FnMut(&str, &str) -> Result<String>) -> Result<()> {
    for field in ["runtime", "capture"] {
        ensure!(
            value[field]
                .as_str()
                .is_some_and(|text| !text.trim().is_empty()),
            "highlighter oracle {field} metadata missing"
        );
    }
    ensure!(
        value["resultsIdenticalAtBothPins"] == Value::Bool(true),
        "shared highlighter trace requires identical results at both pins"
    );
    ensure!(
        value["trace"]
            .as_object()
            .is_some_and(|trace| !trace.is_empty()),
        "highlighter oracle executable trace missing"
    );
    ensure!(
        value["baselines"] == serde_json::json!(PINS),
        "highlighter oracle must identify both exact source pins"
    );
    ensure!(
        value["source"] == SOURCE,
        "highlighter oracle source path mismatch"
    );
    let recorded = value["sourceBlobAtBothPins"]
        .as_str()
        .context("highlighter source blob missing")?;
    for pin in PINS {
        ensure!(
            recorded == blob(pin, SOURCE)?,
            "highlighter oracle blob does not match {pin}"
        );
    }
    Ok(())
}

pub(super) fn verify(repo: &Path, baseline: &str) -> Result<()> {
    // The general inventory/audit tooling also accepts unrelated test trees.
    if baseline != PINS[0] {
        return Ok(());
    }
    for path in FIXTURES {
        let value = serde_json::from_slice(&std::fs::read(repo.join(path))?)?;
        validate(&value, |pin, source| {
            Ok(
                super::git_stdout(repo, ["rev-parse", "--verify", &format!("{pin}:{source}")])?
                    .trim()
                    .into(),
            )
        })
        .with_context(|| format!("invalid oracle provenance: {path}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pinned_lifecycle_provenance_rejects_changed_pins_paths_and_blobs() {
        for text in [
            include_str!("../../port/hunk/oracles/highlighter-registration-reorder.json"),
            include_str!("../../port/hunk/oracles/highlighter-registration-removal.json"),
        ] {
            let value: Value = serde_json::from_str(text).unwrap();
            let resolve = |pin: &str, source: &str| {
                assert!(PINS.contains(&pin));
                assert_eq!(source, SOURCE);
                Ok("5488cdaae5ccfd72e980deb6c5cadd90441a3024".into())
            };
            validate(&value, resolve).unwrap();
            for key in ["baselines", "source", "sourceBlobAtBothPins"] {
                let mut changed = value.clone();
                changed[key] = Value::Null;
                assert!(validate(&changed, resolve).is_err());
            }
            assert!(validate(&value, |_, _| Ok("wrong-object".into())).is_err());
            for key in ["runtime", "capture", "resultsIdenticalAtBothPins", "trace"] {
                let mut missing = value.clone();
                missing.as_object_mut().unwrap().remove(key);
                assert!(validate(&missing, resolve).is_err(), "missing {key}");
                missing[key] = Value::Null;
                assert!(validate(&missing, resolve).is_err(), "null {key}");
            }
            for key in ["runtime", "capture"] {
                for invalid in [
                    serde_json::json!(" \n\t"),
                    serde_json::json!(42),
                    serde_json::json!({}),
                ] {
                    let mut changed = value.clone();
                    changed[key] = invalid;
                    assert!(validate(&changed, resolve).is_err(), "invalid {key}");
                }
            }
            for invalid in [
                serde_json::json!(false),
                serde_json::json!("true"),
                serde_json::json!(1),
            ] {
                let mut changed = value.clone();
                changed["resultsIdenticalAtBothPins"] = invalid;
                assert!(validate(&changed, resolve).is_err());
            }
            for invalid in [
                serde_json::json!({}),
                serde_json::json!([]),
                serde_json::json!("trace"),
            ] {
                let mut changed = value.clone();
                changed["trace"] = invalid;
                assert!(validate(&changed, resolve).is_err());
            }
        }
    }
}
