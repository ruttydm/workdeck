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

#[derive(serde::Serialize)]
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
    let bump = highest_bump(&fragments);
    let next = next_stable_version(&package.version, bump)?;
    let notes = render_notes(&next.to_string(), &fragments);
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"current":package.version.to_string(),"next":next.to_string(),"bump":bump,"fragments":fragments,"notes":notes,"applied":false})
        )?
    );
    Ok(())
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
