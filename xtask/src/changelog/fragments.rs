//! Native authoring replacement for the Hunk Changesets workflow (MIT).
use anyhow::{Result, bail, ensure};
use std::io::Write;
use std::path::Path;

fn validate_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 100
            && id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
        "fragment id must use lowercase letters, digits and hyphens (1–100 bytes)"
    );
    Ok(())
}

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
struct PendingFragment {
    id: String,
    bump: Option<String>,
    body: String,
}

fn pending(repo: &Path) -> Result<Vec<PendingFragment>> {
    let directory = repo.join("changes");
    match std::fs::symlink_metadata(&directory) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
        Ok(metadata) => ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "changes must be a real directory"
        ),
    }
    let mut fragments = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("md") {
            continue;
        }
        ensure!(
            entry.file_type()?.is_file(),
            "fragment must be a regular file"
        );
        let text = std::fs::read_to_string(&path)?;
        let id = path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| anyhow::anyhow!("fragment id is not UTF-8"))?;
        validate_id(id)?;
        ensure!(!text.contains('\0'), "fragment contains a NUL byte");
        let normalized = text.replace("\r\n", "\n");
        let rest = normalized
            .strip_prefix("---\n")
            .ok_or_else(|| anyhow::anyhow!("fragment frontmatter missing: {}", path.display()))?;
        let (frontmatter, body) = if let Some(body) = rest.strip_prefix("---\n") {
            ("", body)
        } else {
            rest.split_once("\n---\n").ok_or_else(|| {
                anyhow::anyhow!("fragment frontmatter not closed: {}", path.display())
            })?
        };
        let bump = if frontmatter.trim().is_empty() {
            None
        } else {
            let document: serde_norway::Value = serde_norway::from_str(frontmatter)?;
            let fields: std::collections::BTreeMap<String, String> =
                serde_norway::from_value(document)?;
            ensure!(fields.len() == 1, "fragment must target only workdeck");
            let value = fields
                .get("workdeck")
                .ok_or_else(|| anyhow::anyhow!("fragment targets an unknown product"))?;
            ensure!(
                matches!(value.as_str(), "patch" | "minor" | "major"),
                "invalid fragment bump"
            );
            Some(value.clone())
        };
        ensure!(
            bump.is_none() || !body.trim().is_empty(),
            "user-visible fragment has no release-note text"
        );
        ensure!(
            bump.is_some() || body.trim().is_empty(),
            "maintenance fragment has release-note text"
        );
        fragments.push(PendingFragment {
            id: id.into(),
            bump,
            body: body.trim().into(),
        });
    }
    fragments.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(fragments)
}

fn highest_bump(fragments: &[PendingFragment]) -> Option<&'static str> {
    ["major", "minor", "patch"].into_iter().find(|bump| {
        fragments
            .iter()
            .any(|fragment| fragment.bump.as_deref() == Some(*bump))
    })
}

pub(super) fn status(repo: &Path, mut args: impl Iterator<Item = String>) -> Result<()> {
    ensure!(
        args.next().is_none(),
        "changelog status does not accept arguments"
    );
    let fragments = pending(repo)?;
    let bump = highest_bump(&fragments);
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({"bump":bump,"fragments":fragments}))?
    );
    Ok(())
}

fn next_stable_version(
    current: &cargo_metadata::semver::Version,
    bump: Option<&str>,
) -> Result<cargo_metadata::semver::Version> {
    ensure!(
        current.pre.is_empty() && current.build.is_empty(),
        "stable planning requires a version without prerelease or build metadata"
    );
    let mut next = current.clone();
    match bump {
        None => {}
        Some("major") => {
            next.major = next
                .major
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("major version overflow"))?;
            next.minor = 0;
            next.patch = 0;
        }
        Some("minor") => {
            next.minor = next
                .minor
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("minor version overflow"))?;
            next.patch = 0;
        }
        Some("patch") => {
            next.patch = next
                .patch
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("patch version overflow"))?;
        }
        Some(_) => bail!("invalid version bump"),
    }
    Ok(next)
}

pub(super) fn plan(repo: &Path, mut args: impl Iterator<Item = String>) -> Result<()> {
    ensure!(
        args.next().is_none(),
        "changelog plan does not accept arguments"
    );
    println!("{}", serde_json::to_string_pretty(&build_plan(repo)?)?);
    Ok(())
}

pub(super) fn check_plan(repo: &Path, mut args: impl Iterator<Item = String>) -> Result<()> {
    let path = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("saved plan path required"))?;
    ensure!(
        args.next().is_none(),
        "check-plan accepts one saved plan path"
    );
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
    ensure!(
        saved == build_plan(repo)?,
        "saved release plan is stale or differs from current inputs"
    );
    println!("{}", serde_json::json!({"valid":true,"applied":false}));
    Ok(())
}

fn build_plan(repo: &Path) -> Result<serde_json::Value> {
    let fragments = pending(repo)?;
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(repo.join("Cargo.toml"))
        .no_deps()
        .other_options(vec!["--offline".into(), "--locked".into()])
        .exec()?;
    let package = metadata
        .packages
        .iter()
        .find(|package| {
            package.name == "workdeck-cli" && metadata.workspace_members.contains(&package.id)
        })
        .ok_or_else(|| anyhow::anyhow!("workdeck-cli workspace package missing"))?;
    let manifest = package.manifest_path.as_std_path().strip_prefix(repo)?;
    let inputs = input_fingerprints(repo, manifest, &fragments)?;
    // Discover the manifest first, then read authoritative version metadata
    // between the initial and final byte fingerprints.
    let verified_metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(repo.join("Cargo.toml"))
        .no_deps()
        .other_options(vec!["--offline".into(), "--locked".into()])
        .exec()?;
    let verified_package = verified_metadata
        .packages
        .iter()
        .find(|candidate| {
            candidate.name == "workdeck-cli"
                && verified_metadata.workspace_members.contains(&candidate.id)
        })
        .ok_or_else(|| anyhow::anyhow!("workdeck-cli disappeared during planning"))?;
    ensure!(
        verified_package.manifest_path == package.manifest_path,
        "CLI manifest changed during planning"
    );
    let package = verified_package;
    let bump = highest_bump(&fragments);
    let next = next_stable_version(&package.version, bump)?;
    let notes = render_notes(&next.to_string(), &fragments);
    let mut edits = if next == package.version {
        std::collections::BTreeMap::new()
    } else {
        let manifest_text = std::fs::read_to_string(repo.join(manifest))?;
        let lock_text = std::fs::read_to_string(repo.join("Cargo.lock"))?;
        let (manifest_edit, lock_edit) = prepare_version_edits(
            &manifest_text,
            &lock_text,
            &package.version.to_string(),
            &next.to_string(),
        )?;
        std::collections::BTreeMap::from([
            (
                manifest
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("manifest path is not UTF-8"))?
                    .replace('\\', "/"),
                manifest_edit,
            ),
            ("Cargo.lock".into(), lock_edit),
        ])
    };
    if !notes.is_empty() {
        let history = match std::fs::read_to_string(repo.join("CHANGELOG.md")) {
            Ok(history) => history,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error.into()),
        };
        edits.insert(
            "CHANGELOG.md".into(),
            prepend_release_notes(&history, &notes),
        );
    }
    verify_fragment_snapshot(repo, manifest, &fragments, &inputs)?;
    Ok(
        serde_json::json!({"current":package.version.to_string(),"next":next.to_string(),"bump":bump,"fragments":fragments,"notes":notes,"inputs":inputs,"edits":edits,"applied":false}),
    )
}

fn prepend_release_notes(history: &str, notes: &str) -> String {
    if history.is_empty() {
        return format!("# Changelog\n\n{notes}");
    }
    if history.starts_with("# ") {
        if let Some((heading, rest)) = history.split_once('\n') {
            return format!("{heading}\n\n{notes}{rest}");
        }
        return format!("{history}\n\n{notes}");
    }
    format!("{notes}{history}")
}

fn prepare_version_edits(
    manifest: &str,
    lock: &str,
    current: &str,
    next: &str,
) -> Result<(String, String)> {
    let mut manifest = manifest.parse::<toml_edit::DocumentMut>()?;
    ensure!(
        manifest["package"]["name"].as_str() == Some("workdeck-cli"),
        "version edit targets the wrong package"
    );
    ensure!(
        manifest["package"]["version"].as_str() == Some(current),
        "CLI version must be explicit and match the plan"
    );
    // Preserve decoration (including comments) attached to the version value.
    let decoration = manifest["package"]["version"]
        .as_value()
        .unwrap()
        .decor()
        .clone();
    manifest["package"]["version"] = toml_edit::value(next);
    *manifest["package"]["version"]
        .as_value_mut()
        .unwrap()
        .decor_mut() = decoration;
    let mut lock = lock.parse::<toml_edit::DocumentMut>()?;
    let packages = lock["package"]
        .as_array_of_tables_mut()
        .ok_or_else(|| anyhow::anyhow!("lockfile packages missing"))?;
    let mut matched = 0;
    for package in packages.iter_mut() {
        if package["name"].as_str() == Some("workdeck-cli") {
            ensure!(
                package["version"].as_str() == Some(current) && package.get("source").is_none(),
                "ambiguous CLI lockfile package"
            );
            package["version"] = toml_edit::value(next);
            matched += 1;
        }
        if let Some(dependencies) = package
            .get("dependencies")
            .and_then(toml_edit::Item::as_array)
        {
            ensure!(
                !dependencies
                    .iter()
                    .filter_map(toml_edit::Value::as_str)
                    .any(|value| value.starts_with("workdeck-cli ")),
                "version-qualified CLI dependency requires coordinated lockfile editing"
            );
        }
    }
    ensure!(
        matched == 1,
        "lockfile must contain exactly one local workdeck-cli package"
    );
    Ok((manifest.to_string(), lock.to_string()))
}

fn verify_fragment_snapshot(
    repo: &Path,
    manifest: &Path,
    fragments: &[PendingFragment],
    inputs: &std::collections::BTreeMap<String, String>,
) -> Result<()> {
    ensure!(
        pending(repo)? == fragments,
        "release fragments changed while generating the plan"
    );
    ensure!(
        &input_fingerprints(repo, manifest, fragments)? == inputs,
        "release inputs changed while generating the plan"
    );
    Ok(())
}

fn input_fingerprints(
    repo: &Path,
    manifest: &Path,
    fragments: &[PendingFragment],
) -> Result<std::collections::BTreeMap<String, String>> {
    use sha2::{Digest, Sha256};
    let mut paths = std::collections::BTreeSet::from([
        std::path::PathBuf::from("Cargo.toml"),
        std::path::PathBuf::from("Cargo.lock"),
        std::path::PathBuf::from("CHANGELOG.md"),
        manifest.to_owned(),
    ]);
    for fragment in fragments {
        paths.insert(std::path::PathBuf::from(format!(
            "changes/{}.md",
            fragment.id
        )));
    }
    let mut inputs = std::collections::BTreeMap::new();
    for path in paths {
        ensure!(
            path.components()
                .all(|component| matches!(component, std::path::Component::Normal(_))),
            "plan input is outside repository"
        );
        let full = repo.join(&path);
        let metadata = match std::fs::symlink_metadata(&full) {
            Err(error)
                if path == Path::new("CHANGELOG.md")
                    && error.kind() == std::io::ErrorKind::NotFound =>
            {
                inputs.insert("CHANGELOG.md".into(), "absent".into());
                continue;
            }
            result => result?,
        };
        ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "plan input must be a regular file"
        );
        let bytes = std::fs::read(&full)?;
        let name = path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("plan input path is not UTF-8"))?
            .replace('\\', "/");
        inputs.insert(name, format!("{:x}", Sha256::digest(bytes)));
    }
    Ok(inputs)
}

fn render_notes(version: &str, fragments: &[PendingFragment]) -> String {
    if highest_bump(fragments).is_none() {
        return String::new();
    }
    let mut output = format!("## {version}\n\n");
    for (bump, heading) in [
        ("major", "Major Changes"),
        ("minor", "Minor Changes"),
        ("patch", "Patch Changes"),
    ] {
        let mut entries: Vec<_> = fragments
            .iter()
            .filter(|fragment| fragment.bump.as_deref() == Some(bump))
            .collect();
        entries.sort_by(|left, right| left.id.cmp(&right.id));
        if entries.is_empty() {
            continue;
        }
        output.push_str(&format!("### {heading}\n\n"));
        for entry in entries {
            for (index, line) in entry.body.lines().enumerate() {
                output.push_str(if index == 0 { "- " } else { "  " });
                output.push_str(line);
                output.push('\n');
            }
            output.push('\n');
        }
    }
    output
}

pub(super) fn add(repo: &Path, mut args: impl Iterator<Item = String>) -> Result<()> {
    let id = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("fragment id required"))?;
    validate_id(&id)?;
    let bump = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("fragment bump required"))?;
    ensure!(
        matches!(bump.as_str(), "patch" | "minor" | "major" | "empty"),
        "invalid fragment bump"
    );
    let body = args.next();
    ensure!(args.next().is_none(), "unexpected fragment argument");
    let content = if bump == "empty" {
        ensure!(
            body.is_none(),
            "maintenance-only fragments do not accept release-note text"
        );
        "---\n---\n".to_owned()
    } else {
        let body = body
            .filter(|body| !body.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("user-visible fragments require release-note text"))?;
        ensure!(
            !body.contains('\0'),
            "release-note text contains a NUL byte"
        );
        format!("---\n\"workdeck\": {bump}\n---\n\n{}\n", body.trim_end())
    };
    let directory = repo.join("changes");
    match std::fs::symlink_metadata(&directory) {
        Ok(metadata) => ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "changes must be a real directory"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(&directory)?
        }
        Err(error) => return Err(error.into()),
    }
    let destination = directory.join(format!("{id}.md"));
    let mut temporary = tempfile::NamedTempFile::new_in(&directory)?;
    temporary.write_all(content.as_bytes())?;
    temporary.as_file().sync_all()?;
    if let Err(error) = temporary.persist_noclobber(&destination) {
        bail!(
            "cannot create release fragment {}: {}",
            destination.display(),
            error.error
        );
    }
    println!("changes/{id}.md");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changelog_plan_preserves_existing_history() {
        let notes = "## 2.0.0\n\n- New.\n\n";
        assert_eq!(
            prepend_release_notes("", notes),
            format!("# Changelog\n\n{notes}")
        );
        for history in ["## 1.0.0\n\nOld λ.\n", "Unstructured history\r\n"] {
            assert_eq!(
                prepend_release_notes(history, notes),
                format!("{notes}{history}")
            );
        }
        assert_eq!(
            prepend_release_notes("# History\r\n\r\nOld λ.\r\n", notes),
            format!("# History\r\n\n{notes}\r\nOld λ.\r\n")
        );
        assert_eq!(
            prepend_release_notes("# History", notes),
            format!("# History\n\n{notes}")
        );
    }

    #[test]
    fn version_edits_preserve_manifest_comments_and_unrelated_packages() {
        let manifest = "[package]\nname = \"workdeck-cli\"\nversion = \"1.2.3\" # keep this comment\ndescription = \"Keep me\"\n";
        let lock = "version = 4\n\n[[package]]\nname = \"other\"\nversion = \"9.8.7\"\n\n[[package]]\nname = \"workdeck-cli\"\nversion = \"1.2.3\"\n";
        let (edited_manifest, edited_lock) =
            prepare_version_edits(manifest, lock, "1.2.3", "1.3.0").unwrap();
        assert_eq!(edited_manifest, manifest.replace("1.2.3", "1.3.0"));
        assert_eq!(edited_lock, lock.replace("1.2.3", "1.3.0"));
        assert!(prepare_version_edits(manifest, lock, "1.2.4", "1.3.0").is_err());
        assert!(
            prepare_version_edits(
                &manifest.replace("workdeck-cli", "other"),
                lock,
                "1.2.3",
                "1.3.0"
            )
            .is_err()
        );
        assert!(
            prepare_version_edits(
                manifest,
                &format!("{lock}\n[[package]]\nname = \"workdeck-cli\"\nversion = \"1.2.3\"\n"),
                "1.2.3",
                "1.3.0"
            )
            .is_err()
        );
    }

    #[test]
    fn snapshot_check_detects_changes_between_parsing_and_fingerprinting() {
        let repo = tempfile::tempdir().unwrap();
        for file in ["Cargo.toml", "Cargo.lock"] {
            std::fs::write(repo.path().join(file), "fixture\n").unwrap();
        }
        create(repo.path(), &["fix", "patch", "Original note."]).unwrap();
        let fragments = pending(repo.path()).unwrap();
        let path = repo.path().join("changes/fix.md");
        let original = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, original.replace("Original", "Changed")).unwrap();
        let hashes = input_fingerprints(repo.path(), Path::new("Cargo.toml"), &fragments).unwrap();
        assert!(
            verify_fragment_snapshot(repo.path(), Path::new("Cargo.toml"), &fragments, &hashes)
                .is_err()
        );
        std::fs::write(&path, &original).unwrap();
        let hashes = input_fingerprints(repo.path(), Path::new("Cargo.toml"), &fragments).unwrap();
        verify_fragment_snapshot(repo.path(), Path::new("Cargo.toml"), &fragments, &hashes)
            .unwrap();
        std::fs::write(&path, format!("{original}\n")).unwrap();
        assert_eq!(pending(repo.path()).unwrap(), fragments);
        assert!(
            verify_fragment_snapshot(repo.path(), Path::new("Cargo.toml"), &fragments, &hashes)
                .is_err()
        );
    }

    #[test]
    fn manifest_and_lockfile_drift_invalidates_input_fingerprints() {
        let repo = tempfile::tempdir().unwrap();
        for file in ["Cargo.toml", "Cargo.lock", "cli.toml"] {
            std::fs::write(repo.path().join(file), "original\n").unwrap();
        }
        let hashes = input_fingerprints(repo.path(), Path::new("cli.toml"), &[]).unwrap();
        for file in ["Cargo.toml", "Cargo.lock", "cli.toml"] {
            let path = repo.path().join(file);
            std::fs::write(&path, "changed\n").unwrap();
            assert!(
                verify_fragment_snapshot(repo.path(), Path::new("cli.toml"), &[], &hashes).is_err(),
                "accepted changed {file}"
            );
            std::fs::write(path, "original\n").unwrap();
        }
        verify_fragment_snapshot(repo.path(), Path::new("cli.toml"), &[], &hashes).unwrap();
    }

    #[test]
    fn hand_authored_fragments_obey_authoring_rules() {
        for (name, text) in [
            ("UPPER.md", "---\n---\n"),
            ("with space.md", "---\n---\n"),
            ("nul.md", "---\nworkdeck: patch\n---\n\nNote\0text"),
            (
                "duplicate.md",
                "---\nworkdeck: patch\nworkdeck: major\n---\n\nNote",
            ),
        ] {
            let repo = tempfile::tempdir().unwrap();
            std::fs::create_dir(repo.path().join("changes")).unwrap();
            let path = repo.path().join("changes").join(name);
            std::fs::write(&path, text).unwrap();
            assert!(pending(repo.path()).is_err(), "accepted {name}");
            assert_eq!(std::fs::read_to_string(path).unwrap(), text);
        }
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir(repo.path().join("changes")).unwrap();
        std::fs::write(
            repo.path().join("changes/windows.md"),
            "---\r\nworkdeck: patch\r\n---\r\n\r\nFix λ.\r\n",
        )
        .unwrap();
        assert_eq!(pending(repo.path()).unwrap()[0].body, "Fix λ.");
    }

    #[test]
    fn notes_group_bumps_preserve_multiline_text_and_omit_maintenance() {
        let mut fragments = vec![
            PendingFragment {
                id: "z".into(),
                bump: Some("patch".into()),
                body: "Fix λ.\n\nMore detail.".into(),
            },
            PendingFragment {
                id: "b".into(),
                bump: Some("minor".into()),
                body: "Add feature.".into(),
            },
            PendingFragment {
                id: "a".into(),
                bump: Some("patch".into()),
                body: "First fix.".into(),
            },
            PendingFragment {
                id: "m".into(),
                bump: None,
                body: String::new(),
            },
            PendingFragment {
                id: "breaking".into(),
                bump: Some("major".into()),
                body: "Break protocol.".into(),
            },
        ];
        let expected = "## 2.0.0\n\n### Major Changes\n\n- Break protocol.\n\n### Minor Changes\n\n- Add feature.\n\n### Patch Changes\n\n- First fix.\n\n- Fix λ.\n  \n  More detail.\n\n";
        assert_eq!(render_notes("2.0.0", &fragments), expected);
        fragments.reverse();
        assert_eq!(render_notes("2.0.0", &fragments), expected);
        assert_eq!(render_notes("1.0.0", &[]), "");
        assert_eq!(
            render_notes(
                "1.0.0",
                &[PendingFragment {
                    id: "m".into(),
                    bump: None,
                    body: String::new()
                }]
            ),
            ""
        );
    }
    fn create(repo: &Path, args: &[&str]) -> Result<()> {
        add(repo, args.iter().map(|value| (*value).to_owned()))
    }

    #[test]
    fn bump_precedence_is_independent_of_fragment_order() {
        for mask in 0..8 {
            let mut fragments = vec![PendingFragment {
                id: "maintenance".into(),
                bump: None,
                body: String::new(),
            }];
            for (index, bump) in ["patch", "minor", "major"].iter().enumerate() {
                if mask & (1 << index) != 0 {
                    fragments.push(PendingFragment {
                        id: bump.to_string(),
                        bump: Some(bump.to_string()),
                        body: "Note".into(),
                    });
                }
            }
            let expected = if mask & 4 != 0 {
                Some("major")
            } else if mask & 2 != 0 {
                Some("minor")
            } else if mask & 1 != 0 {
                Some("patch")
            } else {
                None
            };
            assert_eq!(highest_bump(&fragments), expected);
            fragments.reverse();
            assert_eq!(highest_bump(&fragments), expected);
        }
        assert_eq!(highest_bump(&[]), None);
    }

    #[test]
    fn stable_version_plans_reset_lower_components_and_reject_unsafe_inputs() {
        use cargo_metadata::semver::Version;
        let current = Version::parse("1.2.3").unwrap();
        for (bump, expected) in [
            (None, "1.2.3"),
            (Some("patch"), "1.2.4"),
            (Some("minor"), "1.3.0"),
            (Some("major"), "2.0.0"),
        ] {
            assert_eq!(
                next_stable_version(&current, bump).unwrap().to_string(),
                expected
            );
        }
        for version in ["1.2.3-beta.1", "1.2.3+build"] {
            assert!(next_stable_version(&Version::parse(version).unwrap(), Some("patch")).is_err());
        }
        for bump in ["major", "minor", "patch"] {
            assert!(
                next_stable_version(&Version::new(u64::MAX, u64::MAX, u64::MAX), Some(bump))
                    .is_err()
            );
        }
        assert!(next_stable_version(&current, Some("unknown")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_directories_and_fragments_are_never_followed() {
        use std::os::unix::fs::symlink;
        let repo = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), repo.path().join("changes")).unwrap();
        assert!(create(repo.path(), &["safe", "empty"]).is_err());
        assert!(pending(repo.path()).is_err());
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);

        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir(repo.path().join("changes")).unwrap();
        let target = outside.path().join("note.md");
        std::fs::write(&target, "---\n---\n").unwrap();
        symlink(&target, repo.path().join("changes/safe.md")).unwrap();
        assert!(pending(repo.path()).is_err());
        assert!(create(repo.path(), &["safe", "patch", "Overwrite"]).is_err());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "---\n---\n");
    }

    #[test]
    fn creates_each_bump_and_maintenance_fragment_without_overwrite() {
        let repo = tempfile::tempdir().unwrap();
        for bump in ["patch", "minor", "major"] {
            create(repo.path(), &[bump, bump, "Fix Unicode λ output."]).unwrap();
            let path = repo.path().join(format!("changes/{bump}.md"));
            let expected = format!("---\n\"workdeck\": {bump}\n---\n\nFix Unicode λ output.\n");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), expected);
            assert!(create(repo.path(), &[bump, "major", "Overwrite"]).is_err());
            assert_eq!(std::fs::read_to_string(path).unwrap(), expected);
        }
        create(repo.path(), &["maintenance", "empty"]).unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.path().join("changes/maintenance.md")).unwrap(),
            "---\n---\n"
        );
        assert_eq!(
            std::fs::read_dir(repo.path().join("changes"))
                .unwrap()
                .count(),
            4
        );
    }

    #[test]
    fn rejects_invalid_requests_without_creating_repository_state() {
        for args in [
            vec!["../escape", "empty"],
            vec!["UPPER", "empty"],
            vec!["valid", "unknown"],
            vec!["valid", "patch"],
            vec!["valid", "patch", " "],
            vec!["valid", "empty", "text"],
            vec!["valid", "patch", "text", "extra"],
        ] {
            let repo = tempfile::tempdir().unwrap();
            assert!(create(repo.path(), &args).is_err());
            assert_eq!(std::fs::read_dir(repo.path()).unwrap().count(), 0);
        }
    }

    #[test]
    fn reads_pending_fragments_without_creating_state() {
        let repo = tempfile::tempdir().unwrap();
        assert!(pending(repo.path()).unwrap().is_empty());
        assert_eq!(std::fs::read_dir(repo.path()).unwrap().count(), 0);
        create(repo.path(), &["z-fix", "patch", "Fix λ."]).unwrap();
        create(repo.path(), &["a-maintenance", "empty"]).unwrap();
        let records = pending(repo.path()).unwrap();
        assert_eq!(records[0].id, "a-maintenance");
        assert_eq!(records[0].bump, None);
        assert_eq!(records[1].body, "Fix λ.");
        assert_eq!(records[1].bump.as_deref(), Some("patch"));
    }

    #[test]
    fn rejects_malformed_pending_fragments() {
        for content in [
            "missing",
            "---\nworkdeck: patch\n",
            "---\nother: patch\n---\n\nText",
            "---\nworkdeck: unknown\n---\n\nText",
            "---\nworkdeck: patch\n---\n",
            "---\n---\n\nText",
        ] {
            let repo = tempfile::tempdir().unwrap();
            std::fs::create_dir(repo.path().join("changes")).unwrap();
            std::fs::write(repo.path().join("changes/bad.md"), content).unwrap();
            assert!(pending(repo.path()).is_err(), "accepted {content:?}");
        }
    }
}
