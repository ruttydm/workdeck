//! Native authoring replacement for the Hunk Changesets workflow (MIT).
use anyhow::{Result, bail, ensure};
use std::io::Write;
use std::path::Path;

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
            let fields: std::collections::BTreeMap<String, String> =
                serde_norway::from_str(frontmatter)?;
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
            id: path
                .file_stem()
                .and_then(|value| value.to_str())
                .ok_or_else(|| anyhow::anyhow!("fragment id is not UTF-8"))?
                .into(),
            bump,
            body: body.trim().into(),
        });
    }
    fragments.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(fragments)
}

pub(super) fn status(repo: &Path, mut args: impl Iterator<Item = String>) -> Result<()> {
    ensure!(
        args.next().is_none(),
        "changelog status does not accept arguments"
    );
    let fragments = pending(repo)?;
    let bump = ["major", "minor", "patch"].into_iter().find(|bump| {
        fragments
            .iter()
            .any(|fragment| fragment.bump.as_deref() == Some(*bump))
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({"bump":bump,"fragments":fragments}))?
    );
    Ok(())
}

pub(super) fn add(repo: &Path, mut args: impl Iterator<Item = String>) -> Result<()> {
    let id = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("fragment id required"))?;
    ensure!(
        !id.is_empty()
            && id.len() <= 100
            && id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
        "fragment id must use lowercase letters, digits and hyphens (1–100 bytes)"
    );
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
    fn create(repo: &Path, args: &[&str]) -> Result<()> {
        add(repo, args.iter().map(|value| (*value).to_owned()))
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
