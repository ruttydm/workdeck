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
const COMPILED_FIXTURES: [(&str, usize, &str); 2] = [
    (
        "test/cli/fixtures/compiled-highlight-worker-control.ts",
        1333,
        "crates/workdeck-tui/src/highlighted_diff_runtime.rs",
    ),
    (
        "test/cli/fixtures/compiled-opentui-positive-control.ts",
        97,
        "docs/native-extension-runtime-boundary.md",
    ),
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
    verify_compiled_fixtures(repo)?;
    Ok(())
}

fn verify_compiled_fixtures(repo: &Path) -> Result<()> {
    for (path, expected_bytes, destination) in COMPILED_FIXTURES {
        let source = crate::git_stdout_bytes(repo, ["show", &format!("{}:{path}", PINS[0])])?;
        ensure!(
            source.len() == expected_bytes,
            "pinned compiled fixture {path} changed size"
        );
        let text = std::str::from_utf8(&source)?;
        match path {
            "test/cli/fixtures/compiled-highlight-worker-control.ts" => {
                for marker in [
                    "supportsHighlightWorkerOffload",
                    "worker.postMessage({ version: 0, id: 1 })",
                    "response.version !== 3",
                    "compiled highlight worker ready",
                ] {
                    ensure!(
                        text.contains(marker),
                        "compiled worker fixture lost {marker}"
                    );
                }
                let native = std::fs::read_to_string(repo.join(destination))?;
                for marker in [
                    "prefetch_highlighted_diff_shared",
                    "highlight_with_syntax_theme_live",
                    "inline_is_default_and_worker_offload_requires_explicit_request",
                ] {
                    ensure!(
                        native.contains(marker),
                        "native worker replacement lost {marker}"
                    );
                }
            }
            "test/cli/fixtures/compiled-opentui-positive-control.ts" => {
                ensure!(
                    text == "import { RGBA } from \"@opentui/core\";\n\nprocess.stdout.write(RGBA.fromHex(\"#ffffff\").toString());\n",
                    "OpenTUI positive-control fixture changed"
                );
                let native = std::fs::read_to_string(repo.join(destination))?;
                ensure!(
                    native.contains("JavaScript and TypeScript files")
                        && native.contains("never executed"),
                    "OpenTUI positive-control replacement boundary is undocumented"
                );
            }
            other => ensure!(false, "unknown compiled fixture {other}"),
        }
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

    #[test]
    fn compiled_cli_fixtures_have_native_worker_or_boundary_replacements() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        verify_compiled_fixtures(repo).unwrap();
    }
}
