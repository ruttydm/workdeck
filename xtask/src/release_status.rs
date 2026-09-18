//! Native single-product release-fragment gate.
//! Merge-base/status behavior follows MIT-licensed Changesets CLI 2.31.0; see THIRD_PARTY_NOTICES.

use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::Command;

const FRAGMENTS: &str = "release/fragments/";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Status {
    base_revision: String,
    merge_base: String,
    fragments: Vec<String>,
    release_type: Option<String>,
}

fn git(repo: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git").args(args).current_dir(repo).output()?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

fn parse_fragment(contents: &str) -> Result<BTreeMap<String, String>> {
    // Same delimiter structure as the pinned parser, including a maintenance-only empty mapping.
    let whitespace = r"[\x09-\x0d\x20\u{00a0}\u{1680}\u{2000}-\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}]";
    let pattern = regex::Regex::new(&format!(
        r"(?s){whitespace}*---(.*?)\n{whitespace}*---({whitespace}*(?:\n|$).*)"
    ))
    .unwrap();
    let captures = pattern
        .captures(contents)
        .context("release fragment is missing valid YAML frontmatter")?;
    let mut value: serde_norway::Value = serde_norway::from_str(&captures[1])
        .context("invalid YAML in release fragment frontmatter")?;
    value
        .apply_merge()
        .context("invalid YAML merge in release fragment frontmatter")?;
    let mut releases = BTreeMap::new();
    if value.is_null()
        || value.as_bool() == Some(false)
        || value
            .as_f64()
            .is_some_and(|number| number == 0.0 || number.is_nan())
        || value.as_str() == Some("")
    {
        return Ok(releases);
    }
    let mapping = value
        .as_mapping()
        .context("release fragment frontmatter must map package names to version types")?;
    for (name, kind) in mapping {
        let name = name
            .as_str()
            .filter(|name| !super::release_channel::trim_source_whitespace(name).is_empty())
            .context("release fragment has an invalid package name")?;
        let kind = kind
            .as_str()
            .filter(|kind| matches!(*kind, "major" | "minor" | "patch" | "none"))
            .with_context(|| {
                format!(
                    "invalid release type for package {name}; expected major, minor, patch, or none"
                )
            })?;
        releases.insert(name.into(), kind.into());
    }
    Ok(releases)
}

pub(super) fn since(repo: &Path, base: &str) -> Result<Status> {
    if base.is_empty() || base.starts_with('-') {
        bail!("release status requires a non-option base revision");
    }
    let merge_base = String::from_utf8(git(repo, &["merge-base", base, "HEAD"])?)?
        .trim()
        .to_owned();
    let changed = git(
        repo,
        &[
            "diff",
            "--name-only",
            "--no-relative",
            "-z",
            &merge_base,
            "--",
        ],
    )?;
    let changed = String::from_utf8_lossy(&changed)
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let surviving = git(
        repo,
        &[
            "diff",
            "--name-only",
            "--diff-filter=d",
            "--no-relative",
            "-z",
            &merge_base,
            "--",
        ],
    )?;
    let surviving = String::from_utf8_lossy(&surviving)
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(repo.join("Cargo.toml"))
        .no_deps()
        .exec()?;
    let packages = metadata
        .packages
        .iter()
        .map(|package| package.name.as_str())
        .collect::<BTreeSet<_>>();
    if !packages.contains("workdeck-cli") {
        bail!("workspace does not contain workdeck-cli");
    }
    let mut fragments = Vec::new();
    let mut release_type = None;
    let rank = |kind: &str| match kind {
        "major" => 3,
        "minor" => 2,
        "patch" => 1,
        _ => 0,
    };
    for entry in
        fs::read_dir(repo.join(FRAGMENTS)).context("missing release/fragments directory")?
    {
        let entry = entry?;
        let filename = entry.file_name();
        let Some(name) = filename.to_str() else {
            continue;
        };
        if name.starts_with('.')
            || !name.ends_with(".md")
            || name.eq_ignore_ascii_case("README.md")
            || !surviving.contains(&format!("{FRAGMENTS}{name}"))
        {
            continue;
        }
        let bytes = fs::read(entry.path())?;
        let releases = parse_fragment(&String::from_utf8_lossy(&bytes))
            .with_context(|| format!("invalid release fragment {name}"))?;
        for (package, kind) in releases {
            if !packages.contains(package.as_str()) {
                bail!("release fragment {name} references unknown package {package}");
            }
            // Internal crates are not independently shipped. Only the executable gets a release bump.
            if package == "workdeck-cli"
                && rank(&kind) > release_type.as_deref().map(rank).unwrap_or(0)
            {
                release_type = Some(kind);
            }
        }
        fragments.push(name.strip_suffix(".md").unwrap().to_owned());
    }
    fragments.sort();
    if !changed.is_empty() && fragments.is_empty() {
        bail!(
            "tracked files changed but no changed release fragments were found; add a versioned or empty maintenance fragment"
        );
    }
    Ok(Status {
        base_revision: base.into(),
        merge_base,
        fragments,
        release_type,
    })
}

pub(super) fn run(repo: &Path, mut args: impl Iterator<Item = String>) -> Result<()> {
    let option = args
        .next()
        .context("Usage: cargo xtask release status --since=REVISION")?;
    let base = option
        .strip_prefix("--since=")
        .context("Usage: cargo xtask release status --since=REVISION")?;
    if args.next().is_some() {
        bail!("release status accepts exactly one --since=REVISION option");
    }
    println!("{}", serde_json::to_string(&since(repo, base)?)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository() -> (tempfile::TempDir, String) {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("src")).unwrap();
        fs::create_dir_all(root.path().join(FRAGMENTS)).unwrap();
        fs::write(root.path().join("Cargo.toml"), "[package]\nname = \"workdeck-cli\"\nversion = \"0.1.0\"\nedition = \"2024\"\npublish = false\n").unwrap();
        fs::write(root.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(root.path().join("release/fragments/old.md"), "---\n---\n").unwrap();
        git(root.path(), &["init", "-q"]).unwrap();
        git(
            root.path(),
            &["config", "user.name", "Workdeck Release Test"],
        )
        .unwrap();
        git(
            root.path(),
            &["config", "user.email", "release@example.invalid"],
        )
        .unwrap();
        git(root.path(), &["config", "commit.gpgsign", "false"]).unwrap();
        git(root.path(), &["add", "."]).unwrap();
        git(root.path(), &["commit", "-qm", "base"]).unwrap();
        let base = String::from_utf8(git(root.path(), &["rev-parse", "HEAD"]).unwrap())
            .unwrap()
            .trim()
            .to_owned();
        (root, base)
    }

    #[test]
    fn changed_product_requires_a_changed_tracked_fragment_but_an_unchanged_tree_does_not() {
        let (root, base) = repository();
        assert!(since(root.path(), &base).unwrap().fragments.is_empty());
        fs::write(
            root.path().join("src/main.rs"),
            "fn main() { println!(\"changed\"); }\n",
        )
        .unwrap();
        assert!(
            since(root.path(), &base)
                .unwrap_err()
                .to_string()
                .contains("no changed release fragments")
        );
        fs::write(
            root.path().join("release/fragments/fix.md"),
            "---\nworkdeck-cli: patch\n---\n\nFix.\n",
        )
        .unwrap();
        assert!(
            since(root.path(), &base).is_err(),
            "untracked notes must not satisfy the tracked diff gate"
        );
        git(root.path(), &["add", "release/fragments/fix.md"]).unwrap();
        let status = since(root.path(), &base).unwrap();
        assert_eq!(status.fragments, ["fix"]);
        assert_eq!(status.release_type.as_deref(), Some("patch"));
        assert_eq!(status.base_revision, base);
        assert!(!root.path().join(".agents").exists());
        assert!(!root.path().join("Cargo.lock").exists());
        assert!(!root.path().join("target").exists());
    }

    #[test]
    fn modified_existing_maintenance_fragments_count_but_deleted_ones_do_not() {
        let (root, base) = repository();
        fs::write(
            root.path().join("release/fragments/old.md"),
            "---\n---\n\nMaintenance.\n",
        )
        .unwrap();
        let status = since(root.path(), &base).unwrap();
        assert_eq!(status.fragments, ["old"]);
        assert_eq!(status.release_type, None);
        fs::remove_file(root.path().join("release/fragments/old.md")).unwrap();
        assert!(since(root.path(), &base).is_err());
    }

    #[test]
    fn readmes_hidden_fragments_and_unknown_packages_cannot_satisfy_the_gate() {
        let (root, base) = repository();
        for name in ["README.md", ".hidden.md"] {
            fs::write(root.path().join(FRAGMENTS).join(name), "---\n---\n").unwrap();
        }
        git(root.path(), &["add", "."]).unwrap();
        assert!(since(root.path(), &base).is_err());
        fs::write(
            root.path().join("release/fragments/fix.md"),
            "---\nmissing-package: patch\n---\n",
        )
        .unwrap();
        git(root.path(), &["add", "."]).unwrap();
        assert!(
            since(root.path(), &base)
                .unwrap_err()
                .to_string()
                .contains("unknown package missing-package")
        );
        assert!(since(root.path(), "--help").is_err());
    }

    #[test]
    fn status_uses_the_merge_base_when_the_requested_branch_has_diverged() {
        let (root, base) = repository();
        git(root.path(), &["checkout", "-qb", "upstream"]).unwrap();
        fs::write(
            root.path().join("release/fragments/upstream.md"),
            "---\nworkdeck-cli: major\n---\n",
        )
        .unwrap();
        git(root.path(), &["add", "."]).unwrap();
        git(root.path(), &["commit", "-qm", "upstream change"]).unwrap();
        git(root.path(), &["checkout", "-qb", "feature", &base]).unwrap();
        fs::write(
            root.path().join("release/fragments/feature.md"),
            "---\nworkdeck-cli: minor\n---\n",
        )
        .unwrap();
        git(root.path(), &["add", "."]).unwrap();
        git(root.path(), &["commit", "-qm", "feature change"]).unwrap();
        let status = since(root.path(), "upstream").unwrap();
        assert_eq!(status.merge_base, base);
        assert_eq!(status.fragments, ["feature"]);
        assert_eq!(status.release_type.as_deref(), Some("minor"));
    }

    #[test]
    fn parser_accepts_every_exact_pinned_release_fragment() {
        let archive: serde_json::Value =
            serde_json::from_str(include_str!("../../port/hunk/release-fragments.json")).unwrap();
        let fragments = archive["fragments"].as_array().unwrap();
        assert_eq!(fragments.len(), 77);
        for fragment in fragments {
            let bump = fragment["bump"].as_str();
            let header = bump
                .map(|bump| format!("\"hunkdiff\": {bump}\n"))
                .unwrap_or_default();
            let body = fragment["body"].as_str().unwrap();
            let source = format!(
                "---\n{header}---\n{}{body}",
                if body.is_empty() { "" } else { "\n" }
            );
            let releases = parse_fragment(&source).unwrap();
            assert_eq!(releases.get("hunkdiff").map(String::as_str), bump);
            assert_eq!(releases.len(), usize::from(bump.is_some()));
        }
    }

    #[test]
    fn parses_versioned_empty_anchored_and_merged_yaml_fragments() {
        assert!(parse_fragment("---\n---\n").unwrap().is_empty());
        assert!(
            parse_fragment("---\n---\n\nMaintenance.\n")
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            parse_fragment("---\n\"workdeck-cli\": patch\n---\n\nFix.\n").unwrap()["workdeck-cli"],
            "patch"
        );
        assert_eq!(
            parse_fragment("---\n<<: {workdeck-cli: minor}\n---\n\nFeature.\n").unwrap()["workdeck-cli"],
            "minor"
        );
        assert_eq!(
            parse_fragment(
                "---\nworkdeck-core: &kind none\nworkdeck-cli: *kind\n---\n\nMaintenance.\n"
            )
            .unwrap()["workdeck-cli"],
            "none"
        );
    }

    #[test]
    fn malformed_or_duplicate_frontmatter_is_not_silently_accepted() {
        for input in [
            "",
            "No metadata",
            "---\n[\n---\n",
            "---\n- patch\n---\n",
            "---\nworkdeck-cli: true\n---\n",
            "---\nworkdeck-cli: future\n---\n",
            "---\nworkdeck-cli: patch\nworkdeck-cli: minor\n---\n",
        ] {
            assert!(parse_fragment(input).is_err(), "{input}");
        }
    }
}
