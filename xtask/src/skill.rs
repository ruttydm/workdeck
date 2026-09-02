//! Generate and verify the bundled review skill from the typed session surface.

use anyhow::{Context, Result, bail};
use std::fs;
use std::path::Path;
use workdeck_session::render_workdeck_review_skill;

const REVIEW_SKILL: &str = "skills/workdeck-review/SKILL.md";

pub(crate) fn generate(repo: &Path) -> Result<()> {
    let destination = repo.join(REVIEW_SKILL);
    let rendered = render_workdeck_review_skill();
    if fs::read_to_string(&destination)
        .ok()
        .is_some_and(|current| normalize_newlines(&current) == rendered)
    {
        println!("{REVIEW_SKILL} is already current");
        return Ok(());
    }
    fs::write(&destination, rendered)
        .with_context(|| format!("write generated review skill {}", destination.display()))?;
    println!("wrote {REVIEW_SKILL}");
    Ok(())
}

pub(crate) fn check(repo: &Path) -> Result<()> {
    let destination = repo.join(REVIEW_SKILL);
    let checked_in = fs::read_to_string(&destination)
        .with_context(|| format!("read generated review skill {}", destination.display()))?;
    let rendered = render_workdeck_review_skill();
    if normalize_newlines(&checked_in) != rendered {
        bail!("{REVIEW_SKILL} is out of date; run `cargo xtask skill generate`");
    }
    println!("Workdeck review skill is current");
    Ok(())
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_checkout_line_endings() {
        assert_eq!(normalize_newlines("one\r\ntwo\r\n"), "one\ntwo\n");
    }

    #[test]
    fn live_checked_in_skill_matches_renderer() {
        check(&crate::repo_root().unwrap()).unwrap();
    }
}
