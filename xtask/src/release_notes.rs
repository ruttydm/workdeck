//! Partial MIT translation of Hunk's scripts/verify-pr-release-notes.ts.
//! Generated-state validation is native; PR routing and normal fragment status remain separate work.

use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn stable_parts(value: &str) -> Option<[f64; 3]> {
    let parts = value.split('.').collect::<Vec<_>>();
    if parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    Some([
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ])
}

fn latest_stable(changelog: &str) -> Option<&str> {
    let mut versions = changelog
        .split(['\r', '\n', '\u{2028}', '\u{2029}'])
        .filter_map(|line| {
            let raw = line.strip_prefix("## ")?;
            Some((raw, stable_parts(raw)?))
        })
        .collect::<Vec<_>>();
    versions.sort_by(|(_, left), (_, right)| {
        for index in 0..3 {
            let difference = right[index] - left[index];
            if difference != 0.0 {
                return difference.partial_cmp(&0.0).unwrap_or(Ordering::Equal);
            }
        }
        Ordering::Equal
    });
    versions.first().map(|(raw, _)| *raw)
}

fn validate_generated(
    package: &str,
    version: &str,
    pre: &Value,
    changelog: &str,
    on_disk: &BTreeSet<String>,
) -> Result<()> {
    if pre.get("mode").and_then(Value::as_str) != Some("pre") {
        bail!(
            "Expected release pre mode, received {}",
            pre.get("mode")
                .map(Value::to_string)
                .unwrap_or_else(|| "undefined".into())
        );
    }
    let tag = pre
        .get("tag")
        .and_then(Value::as_str)
        .filter(|tag| valid_identifier(tag))
        .context("Release prerelease tag must be a non-empty channel tag")?;
    let initial = pre
        .get("initialVersions")
        .filter(|value| value.is_object() || value.is_array())
        .context("Release prerelease state is missing initialVersions")?;
    let initial_version = initial
        .get(package)
        .and_then(Value::as_str)
        .filter(|value| stable_parts(value).is_some())
        .with_context(|| format!("Missing stable initial version for package {package}"))?;
    let latest =
        latest_stable(changelog).context("Changelog does not contain a stable release heading")?;
    if initial_version != latest {
        bail!("Initial version {initial_version} does not match latest stable release {latest}");
    }
    let pattern = regex::Regex::new(&format!(
        r"^[0-9]+\.[0-9]+\.[0-9]+-{}\.[0-9]+$",
        regex::escape(tag)
    ))?;
    if !pattern.is_match(version) {
        bail!("Package version {version} does not match prerelease tag {tag}");
    }
    let changesets = pre
        .get("changesets")
        .and_then(Value::as_array)
        .filter(|values| !values.is_empty())
        .context("Release prerelease state must record at least one consumed changeset")?;
    let ids = changesets
        .iter()
        .filter_map(Value::as_str)
        .filter(|id| valid_identifier(id))
        .collect::<Vec<_>>();
    if ids.len() != changesets.len()
        || ids.iter().copied().collect::<BTreeSet<_>>().len() != ids.len()
    {
        bail!("Release prerelease state contains invalid or duplicate changeset IDs");
    }
    let missing = ids
        .iter()
        .filter(|id| !on_disk.contains(**id))
        .copied()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        bail!("Missing consumed changeset files: {}", missing.join(", "));
    }
    let heading = format!("## {version}");
    let has_heading = changelog.split_inclusive('\n').any(|line| {
        let line = if let Some(line) = line.strip_suffix('\n') {
            line.strip_suffix('\r').unwrap_or(line)
        } else {
            line
        };
        line == heading
    });
    if !has_heading {
        bail!("Changelog is missing the {version} release heading");
    }
    Ok(())
}

pub(super) fn validate_local(repo: &Path, mut args: impl Iterator<Item = String>) -> Result<()> {
    if args.next().is_some() {
        bail!("release validate-prerelease accepts no arguments");
    }
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(repo.join("Cargo.toml"))
        .no_deps()
        .exec()?;
    let package = metadata
        .packages
        .iter()
        .find(|package| package.name.as_str() == "workdeck-cli")
        .context("workspace does not contain workdeck-cli")?;
    let pre = serde_json::from_slice(&fs::read(repo.join("release/prerelease.json"))?)?;
    let changelog = fs::read_to_string(repo.join("CHANGELOG.md"))?;
    let mut on_disk = BTreeSet::new();
    for entry in fs::read_dir(repo.join("release/fragments"))? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && let Some(name) = entry.file_name().to_str()
            && let Some(id) = name.strip_suffix(".md")
        {
            on_disk.insert(id.to_owned());
        }
    }
    validate_generated(
        "workdeck-cli",
        &package.version.to_string(),
        &pre,
        &changelog,
        &on_disk,
    )?;
    println!(
        "Validated generated prerelease notes for {}.",
        package.version
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> (Value, String, BTreeSet<String>) {
        (
            serde_json::json!({"mode":"pre","tag":"beta","initialVersions":{"workdeck-cli":"0.17.7"},"changesets":["new-feature","old-fix"]}),
            "# Changelog\n\n## 0.18.0-beta.0\n\n- Added a feature.\n\n## 0.17.7\n".into(),
            ["new-feature".into(), "old-fix".into()]
                .into_iter()
                .collect(),
        )
    }

    fn check(pre: &Value, changelog: &str, on_disk: &BTreeSet<String>) -> Result<()> {
        validate_generated("workdeck-cli", "0.18.0-beta.0", pre, changelog, on_disk)
    }

    #[test]
    fn accepts_coherent_generated_prerelease_state() {
        let (pre, changelog, on_disk) = input();
        check(&pre, &changelog, &on_disk).unwrap();
    }

    #[test]
    fn requires_package_version_and_tag_agreement() {
        let (pre, changelog, on_disk) = input();
        assert!(
            validate_generated("workdeck-cli", "0.18.0-next.0", &pre, &changelog, &on_disk)
                .unwrap_err()
                .to_string()
                .contains("does not match prerelease tag beta")
        );
    }

    #[test]
    fn requires_a_stable_initial_package_version() {
        let (mut pre, changelog, on_disk) = input();
        pre["initialVersions"]["workdeck-cli"] = "0.17.7-beta.1".into();
        assert!(
            check(&pre, &changelog, &on_disk)
                .unwrap_err()
                .to_string()
                .contains("Missing stable initial version for package workdeck-cli")
        );
    }

    #[test]
    fn requires_the_latest_stable_changelog_version() {
        let (mut pre, mut changelog, on_disk) = input();
        pre["initialVersions"]["workdeck-cli"] = "0.17.6".into();
        changelog.push_str("\n## 0.17.6\n");
        assert!(
            check(&pre, &changelog, &on_disk)
                .unwrap_err()
                .to_string()
                .contains("Initial version 0.17.6 does not match latest stable release 0.17.7")
        );
    }

    #[test]
    fn requires_every_consumed_changeset_to_remain_on_disk() {
        let (pre, changelog, mut on_disk) = input();
        on_disk.remove("old-fix");
        assert!(
            check(&pre, &changelog, &on_disk)
                .unwrap_err()
                .to_string()
                .contains("Missing consumed changeset files: old-fix")
        );
    }

    #[test]
    fn rejects_duplicate_changeset_ids() {
        let (mut pre, changelog, on_disk) = input();
        pre["changesets"] = serde_json::json!(["old-fix", "old-fix"]);
        assert!(
            check(&pre, &changelog, &on_disk)
                .unwrap_err()
                .to_string()
                .contains("invalid or duplicate changeset IDs")
        );
    }

    #[test]
    fn requires_the_exact_package_version_heading() {
        let (pre, _, on_disk) = input();
        assert!(
            check(
                &pre,
                "# Changelog\n\n## 0.18.0-beta.1\n\n## 0.17.7\n",
                &on_disk
            )
            .unwrap_err()
            .to_string()
            .contains("Changelog is missing the 0.18.0-beta.0 release heading")
        );
    }

    #[test]
    fn malformed_prerelease_metadata_fails_before_release_validation() {
        let (pre, changelog, on_disk) = input();
        for (key, value, expected) in [
            (
                "mode",
                serde_json::json!("exit"),
                "Expected release pre mode",
            ),
            ("tag", serde_json::json!("beta.1"), "non-empty channel tag"),
            ("tag", serde_json::Value::Null, "non-empty channel tag"),
            (
                "initialVersions",
                serde_json::Value::Null,
                "missing initialVersions",
            ),
            ("changesets", serde_json::json!([]), "at least one consumed"),
            ("changesets", serde_json::json!([1]), "invalid or duplicate"),
            (
                "changesets",
                serde_json::json!(["../escape"]),
                "invalid or duplicate",
            ),
        ] {
            let mut altered = pre.clone();
            altered[key] = value;
            assert!(
                check(&altered, &changelog, &on_disk)
                    .unwrap_err()
                    .to_string()
                    .contains(expected)
            );
        }
        assert!(
            check(&pre, "## 0.18.0-beta.0\n", &on_disk)
                .unwrap_err()
                .to_string()
                .contains("does not contain a stable release")
        );
    }

    #[test]
    fn stable_headings_use_numeric_order_and_preserve_source_line_terminators() {
        assert_eq!(
            latest_stable("## 0.9.9\r\n## 0.10.0\n## 0.8.0\u{2028}## 0.11.0"),
            Some("0.11.0")
        );
        assert_eq!(
            latest_stable("## 0.17.7-beta.1\n## 00.17.7\n## 0.17.7"),
            Some("00.17.7")
        );
        let (pre, changelog, on_disk) = input();
        check(&pre, &changelog.replace('\n', "\r\n"), &on_disk).unwrap();
        assert!(check(&pre, "## 0.17.7\n## 0.18.0-beta.0\r", &on_disk).is_err());
    }

    #[test]
    fn local_validator_reads_a_real_cargo_prerelease_without_creating_repository_state() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("src")).unwrap();
        fs::create_dir_all(root.path().join("release/fragments")).unwrap();
        fs::write(root.path().join("Cargo.toml"), "[package]\nname = \"workdeck-cli\"\nversion = \"0.18.0-beta.0\"\nedition = \"2024\"\npublish = false\n").unwrap();
        fs::write(root.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        let (pre, changelog, ids) = input();
        fs::write(
            root.path().join("release/prerelease.json"),
            serde_json::to_vec(&pre).unwrap(),
        )
        .unwrap();
        fs::write(root.path().join("CHANGELOG.md"), changelog).unwrap();
        for id in ids {
            fs::write(
                root.path().join(format!("release/fragments/{id}.md")),
                "---\n---\n",
            )
            .unwrap();
        }
        validate_local(root.path(), std::iter::empty()).unwrap();
        assert!(!root.path().join(".agents").exists());
        assert!(!root.path().join("Cargo.lock").exists());
        assert!(!root.path().join("target").exists());
        fs::remove_file(root.path().join("release/fragments/old-fix.md")).unwrap();
        assert!(
            validate_local(root.path(), std::iter::empty())
                .unwrap_err()
                .to_string()
                .contains("Missing consumed changeset files: old-fix")
        );
        assert!(validate_local(root.path(), ["unexpected".into()].into_iter()).is_err());
    }
}
