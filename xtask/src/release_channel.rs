//! MIT Hunk ports: scripts/resolve-release-channel.ts and scripts/check-release-version.ts.
//! Native release metadata only: this module never publishes a package or release.

use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Resolution {
    channel: String,
    make_latest: bool,
}

fn quoted(value: &str) -> String {
    serde_json::to_string(value).unwrap()
}

fn stable_version(value: &str) -> Result<[f64; 3]> {
    let components = value
        .strip_prefix('v')
        .unwrap_or(value)
        .split('.')
        .collect::<Vec<_>>();
    if components.len() != 3
        || components
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()))
    {
        bail!(
            "Expected a stable semantic version, received {}.",
            quoted(value)
        );
    }
    // Preserve the source's Number comparison, including leading zeros and IEEE-754 rounding.
    Ok([
        components[0].parse()?,
        components[1].parse()?,
        components[2].parse()?,
    ])
}

fn compare(left: [f64; 3], right: [f64; 3]) -> f64 {
    for (left, right) in left[..2].iter().zip(&right[..2]) {
        let difference = left - right;
        if difference != 0.0 && !difference.is_nan() {
            return difference;
        }
    }
    // The final operand of the source's logical-or chain is returned even when NaN.
    left[2] - right[2]
}

fn component_text(value: f64) -> String {
    if value.is_infinite() {
        "Infinity".into()
    } else if value >= 1e21 {
        format!("{value:e}").replace('e', "e+")
    } else {
        value.to_string()
    }
}

pub(super) fn trim_source_whitespace(value: &str) -> &str {
    value.trim_matches(|c| matches!(c, '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'))
}

fn resolve(
    event: &str,
    reference: &str,
    requested: Option<&str>,
    latest: Option<&str>,
) -> Result<Resolution> {
    if event == "workflow_dispatch" {
        let channel = requested
            .map(trim_source_whitespace)
            .filter(|tag| !tag.is_empty())
            .context("Manual release dispatch requires a release channel tag.")?;
        return Ok(Resolution {
            channel: channel.into(),
            make_latest: channel == "latest",
        });
    }
    if event != "push" {
        bail!("Unsupported release event {}.", quoted(event));
    }
    let prerelease = regex::Regex::new(r"-(?:alpha|beta|rc)(?:[.-]|$)").unwrap();
    if prerelease.is_match(reference) {
        return Ok(Resolution {
            channel: "beta".into(),
            make_latest: false,
        });
    }
    let target = stable_version(reference)?;
    let latest = stable_version(latest.unwrap_or(""))?;
    let comparison = compare(target, latest);
    if comparison == 0.0 {
        bail!("Release {reference} is already the latest release.");
    }
    if comparison > 0.0 {
        return Ok(Resolution {
            channel: "latest".into(),
            make_latest: true,
        });
    }
    Ok(Resolution {
        channel: format!(
            "backport-{}.{}",
            component_text(target[0]),
            component_text(target[1])
        ),
        make_latest: false,
    })
}

fn parse(args: impl Iterator<Item = String>) -> Result<BTreeMap<String, String>> {
    let mut args = args;
    let mut values = BTreeMap::new();
    while let Some(name) = args.next() {
        let value = args.next();
        if !name.starts_with("--") || value.is_none() {
            bail!(
                "Usage: cargo xtask release channel --event <event> --ref <ref> --requested-tag <tag> --current-latest <version>"
            );
        }
        // The pinned parser accepts unknown pairs and lets the last duplicate win.
        values.insert(name, value.unwrap());
    }
    Ok(values)
}

pub(super) fn channel(args: impl Iterator<Item = String>) -> Result<()> {
    let values = parse(args)?;
    let resolution = resolve(
        values.get("--event").map(String::as_str).unwrap_or(""),
        values.get("--ref").map(String::as_str).unwrap_or(""),
        values.get("--requested-tag").map(String::as_str),
        values.get("--current-latest").map(String::as_str),
    )?;
    println!("{}", serde_json::to_string(&resolution)?);
    Ok(())
}

fn verify_tag(tag: Option<&str>, version: &str) -> Result<String> {
    let tag = tag
        .filter(|tag| !tag.is_empty())
        .context("Usage: cargo xtask release check-version <tag>")?;
    let expected = format!("v{version}");
    if tag != expected {
        bail!("Tag {tag} does not match workdeck-cli Cargo version {version} ({expected}).");
    }
    Ok(format!(
        "Verified release tag {tag} matches workdeck-cli Cargo version {version}."
    ))
}

pub(super) fn check_version(repo: &Path, mut args: impl Iterator<Item = String>) -> Result<()> {
    let tag = args.next();
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(repo.join("Cargo.toml"))
        .no_deps()
        .exec()?;
    let package = metadata
        .packages
        .iter()
        .find(|package| package.name.as_str() == "workdeck-cli")
        .context("workspace does not contain workdeck-cli")?;
    println!(
        "{}",
        verify_tag(tag.as_deref(), &package.version.to_string())?
    );
    Ok(())
}

/// Verify that the single native release workflow accounts for the complete
/// pinned prebuilt-release pipeline. Platform builds, benchmark gates,
/// checksums, SBOM/provenance, and GitHub publication remain; npm/Bun/Node
/// package publication is intentionally removed by product policy.
pub(crate) fn verify_prebuilt_release_workflow(repo: &Path) -> Result<()> {
    const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
    let source = crate::git_stdout_bytes(
        repo,
        [
            "show",
            &format!("{BASELINE}:.github/workflows/release-prebuilt-npm.yml"),
        ],
    )?;
    anyhow::ensure!(
        source.len() == 13_540,
        "pinned prebuilt release workflow changed size: {} != 13540",
        source.len()
    );
    let source = std::str::from_utf8(&source)?;
    for marker in [
        "name: Release prebuilt npm packages",
        "workflow_dispatch:",
        "publish:",
        "npm_tag:",
        "allow_benchmark_regression:",
        "benchmark_regression_reason:",
        "push:",
        "tags:",
        "SKIP_INSTALL_SIMPLE_GIT_HOOKS",
        "release-prebuilt-${{ github.ref }}",
        "resolve-release-channel:",
        "release-benchmark-gate:",
        "build-binaries:",
        "stage-release:",
        "publish:",
        "create-github-release:",
        "Set up Bun",
        "Set up Node",
        "npm view hunkdiff dist-tags.latest",
        "bun ./scripts/resolve-release-channel.ts",
        "bun run bench:release:compare",
        "bun run ./scripts/check-release-version.ts",
        "bun run ./scripts/build-prebuilt-artifact.ts",
        "bun run stage:prebuilt:release",
        "bun run check:prebuilt-pack",
        "bun run publish:prebuilt:npm",
        "actions/attest-build-provenance@",
        "Create or update GitHub release",
    ] {
        anyhow::ensure!(
            source.contains(marker),
            "pinned prebuilt release workflow lost marker {marker:?}"
        );
    }
    let native = std::fs::read_to_string(repo.join(".github/workflows/release.yml"))?;
    for marker in [
        "name: Release",
        "permissions:",
        "contents: write",
        "id-token: write",
        "attestations: write",
        "preflight:",
        "Semantic-port release gates",
        "cargo xtask port fetch",
        "cargo xtask port audit",
        "cargo fmt --all --check",
        "cargo clippy --locked --workspace --all-targets -- -D warnings",
        "cargo xtask test",
        "cargo-deny-action@v2",
        "build:",
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
        "x86_64-pc-windows-msvc",
        "workdeck",
        "cargo xtask ci-host",
        "cargo build --locked --release --target",
        "actions/attest-build-provenance@v2",
        "cargo xtask release package",
        "dist/workdeck-${{ matrix.target }}.*",
        "publish:",
        "softprops/action-gh-release@v2",
    ] {
        anyhow::ensure!(
            native.contains(marker),
            "native release workflow is missing {marker:?}"
        );
    }
    for forbidden in ["bun", "node", "npm", "opentui", "wasm", "hunkdiff"] {
        let pattern = regex::Regex::new(&format!(
            r"(?i)(?:^|[^a-z]){}(?:$|[^a-z])",
            regex::escape(forbidden)
        ))
        .expect("forbidden runtime token pattern is valid");
        anyhow::ensure!(
            !pattern.is_match(&native),
            "native release workflow retains forbidden runtime token {forbidden:?}"
        );
    }
    let migration = std::fs::read_to_string(repo.join("docs/release-prebuilt-migration.md"))?;
    for marker in [
        "release-prebuilt-npm.yml",
        "single native release workflow",
        "platform builds",
        "benchmark",
        "SBOM",
        "provenance",
        "npm",
        "not published",
    ] {
        anyhow::ensure!(
            migration.contains(marker),
            "prebuilt release migration is missing {marker:?}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_source_channel_cases_match_native_resolution() {
        let oracle: serde_json::Value =
            serde_json::from_str(include_str!("../../port/hunk/oracles/release-channel.json"))
                .unwrap();
        assert_eq!(oracle["baselines"].as_array().unwrap().len(), 2);
        assert_eq!(oracle["test_mapping"].as_array().unwrap().len(), 7);
        let cases = oracle["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 18);
        for case in cases {
            let input = &case["input"];
            let result = resolve(
                input["eventName"].as_str().unwrap(),
                input["refName"].as_str().unwrap(),
                input["requestedNpmTag"].as_str(),
                input["currentLatestVersion"].as_str(),
            );
            if let Some(error) = case["error"].as_str() {
                let expected = error
                    .replace("an npm tag", "a release channel tag")
                    .replace("already npm latest", "already the latest release");
                assert_eq!(result.unwrap_err().to_string(), expected);
            } else {
                assert_eq!(
                    result.unwrap(),
                    Resolution {
                        channel: case["result"]["npmTag"].as_str().unwrap().into(),
                        make_latest: case["result"]["makeLatest"].as_bool().unwrap(),
                    }
                );
            }
        }
    }

    #[test]
    fn native_release_workflow_replaces_the_complete_prebuilt_npm_pipeline() {
        let repo = super::super::repo_root().unwrap();
        super::verify_prebuilt_release_workflow(&repo).unwrap();
    }

    fn stable(reference: &str, latest: &str) -> Result<Resolution> {
        resolve("push", reference, None, Some(latest))
    }

    fn expected(channel: &str, make_latest: bool) -> Resolution {
        Resolution {
            channel: channel.into(),
            make_latest,
        }
    }

    #[test]
    fn newer_stable_release_becomes_latest() {
        assert_eq!(
            stable("v0.19.0", "0.18.2").unwrap(),
            expected("latest", true)
        );
    }

    #[test]
    fn older_series_backport_stays_away_from_latest() {
        assert_eq!(
            stable("v0.17.8", "0.18.2").unwrap(),
            expected("backport-0.17", false)
        );
    }

    #[test]
    fn prereleases_use_beta_without_changing_latest() {
        for reference in ["v0.19.0-alpha.1", "v0.19.0-beta.2", "v0.19.0-rc.1"] {
            assert_eq!(
                stable(reference, "0.18.2").unwrap(),
                expected("beta", false)
            );
        }
    }

    #[test]
    fn explicit_manual_dispatch_tag_is_honored() {
        assert_eq!(
            resolve("workflow_dispatch", "main", Some("backport-0.17"), None).unwrap(),
            expected("backport-0.17", false)
        );
    }

    #[test]
    fn explicit_manual_latest_dispatch_is_latest() {
        assert_eq!(
            resolve("workflow_dispatch", "main", Some("latest"), None).unwrap(),
            expected("latest", true)
        );
    }

    #[test]
    fn republishing_current_latest_is_rejected() {
        assert!(
            stable("v0.18.2", "0.18.2")
                .unwrap_err()
                .to_string()
                .contains("already the latest")
        );
    }

    #[test]
    fn missing_manual_and_stable_version_inputs_are_rejected() {
        assert!(
            resolve("workflow_dispatch", "main", None, None)
                .unwrap_err()
                .to_string()
                .contains("requires a release channel tag")
        );
        assert!(
            stable("v0.19", "0.18.2")
                .unwrap_err()
                .to_string()
                .contains("Expected a stable semantic version")
        );
    }

    #[test]
    fn source_edge_cases_preserve_number_and_prerelease_matching_rules() {
        assert_eq!(
            stable("v01.02.03", "9007199254740992.0.0").unwrap(),
            expected("backport-1.2", false)
        );
        assert!(stable("v9007199254740993.0.0", "9007199254740992.0.0").is_err());
        for suffix in ["\n", "\r\n", "\r", "\u{2028}", "\n\n"] {
            assert!(stable(&format!("v1.2.3{suffix}"), "2.0.0").is_err());
        }
        assert_eq!(
            stable("not-a-version-beta.1", "").unwrap(),
            expected("beta", false)
        );
        assert!(stable("v1.2.3-rc\n", "2.0.0").is_err());
        assert_eq!(
            stable("v1000000000000000000000.0.0", "10000000000000000000000.0.0").unwrap(),
            expected("backport-1e+21.0", false)
        );
        assert!(
            resolve("pull_request", "main", None, None)
                .unwrap_err()
                .to_string()
                .contains("Unsupported release event")
        );
        assert_eq!(
            resolve(
                "workflow_dispatch",
                "main",
                Some("\u{feff} latest \u{feff}"),
                None
            )
            .unwrap(),
            expected("latest", true)
        );
        let overflow = "9".repeat(400);
        assert_eq!(
            stable(&format!("v{overflow}.1.0"), &format!("{overflow}.2.0")).unwrap(),
            expected("backport-Infinity.1", false)
        );
        assert_eq!(
            stable(&format!("v1.2.{overflow}"), &format!("1.2.{overflow}")).unwrap(),
            expected("backport-1.2", false)
        );
    }

    #[test]
    fn argument_pairs_preserve_duplicate_and_unknown_option_semantics() {
        let args = [
            "--event",
            "push",
            "--unknown",
            "kept",
            "--event",
            "workflow_dispatch",
            "--requested-tag",
            "latest",
        ];
        let values = parse(args.into_iter().map(str::to_owned)).unwrap();
        assert_eq!(values["--event"], "workflow_dispatch");
        assert_eq!(values["--unknown"], "kept");
        assert!(parse(["event", "push"].into_iter().map(str::to_owned)).is_err());
        assert!(parse(["--event"].into_iter().map(str::to_owned)).is_err());
    }

    #[test]
    fn cargo_release_tags_require_the_exact_version_including_prerelease() {
        assert_eq!(
            verify_tag(Some("v0.1.0"), "0.1.0").unwrap(),
            "Verified release tag v0.1.0 matches workdeck-cli Cargo version 0.1.0."
        );
        assert!(verify_tag(Some("v0.2.0-beta.1"), "0.2.0-beta.1").is_ok());
        for tag in [None, Some(""), Some("0.1.0"), Some("v0.1.1")] {
            assert!(verify_tag(tag, "0.1.0").is_err());
        }
    }
}
