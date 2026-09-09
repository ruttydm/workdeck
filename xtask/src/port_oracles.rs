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
        }
    }
}
