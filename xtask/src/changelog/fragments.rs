//! Native authoring replacement for the Hunk Changesets workflow (MIT).
use anyhow::{Result, bail, ensure};
use std::io::Write;
use std::path::Path;

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
}
