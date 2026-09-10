//! Generate and verify the bundled review skill from the typed session surface.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use workdeck_session::render_workdeck_review_skill;

const REVIEW_SKILL: &str = "skills/workdeck-review/SKILL.md";
const WEB_REVIEW_SKILL: &str = "site/static/docs/workdeck-review-skill.md";
const BUNDLED_SKILLS_ORACLE: &str = "port/hunk/oracles/bundled-skills.json";

#[derive(Debug, Deserialize)]
struct BundledSkillsOracle {
    schema_version: u32,
    captured_from: String,
    sources: Vec<BundledSkillSource>,
    executable_evidence: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct BundledSkillSource {
    source_path: String,
    source_blob: String,
    source_sha256: String,
    source_bytes: usize,
    source_lines: usize,
    destination_path: String,
    destination_sha256: String,
    sections: Vec<BundledSkillSection>,
    evidence: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct BundledSkillSection {
    source_lines: [usize; 2],
    source_bytes: [usize; 2],
    source_semantics: String,
    destination_headings: Vec<String>,
    adaptation: String,
}

pub(crate) fn generate(repo: &Path) -> Result<()> {
    let rendered = render_workdeck_review_skill();
    for path in [REVIEW_SKILL, WEB_REVIEW_SKILL] {
        let destination = repo.join(path);
        if fs::read_to_string(&destination)
            .ok()
            .is_some_and(|current| normalize_newlines(&current) == rendered)
        {
            println!("{path} is already current");
            continue;
        }
        fs::create_dir_all(
            destination
                .parent()
                .context("skill destination needs a parent")?,
        )?;
        fs::write(&destination, &rendered)
            .with_context(|| format!("write generated review skill {}", destination.display()))?;
        println!("wrote {path}");
    }
    Ok(())
}

pub(crate) fn check(repo: &Path) -> Result<()> {
    check_generated_skills(repo)?;
    println!("Workdeck review skill is current");
    check_static_skills(repo)?;
    Ok(())
}

pub(crate) fn check_generated_skills(repo: &Path) -> Result<()> {
    let rendered = render_workdeck_review_skill();
    for path in [REVIEW_SKILL, WEB_REVIEW_SKILL] {
        let destination = repo.join(path);
        let checked_in = fs::read_to_string(&destination)
            .with_context(|| format!("read generated review skill {}", destination.display()))?;
        if normalize_newlines(&checked_in) != rendered {
            bail!("{path} is out of date; run `cargo xtask skill generate`");
        }
    }
    Ok(())
}

fn check_static_skills(repo: &Path) -> Result<()> {
    let oracle_path = repo.join(BUNDLED_SKILLS_ORACLE);
    let encoded = fs::read_to_string(&oracle_path)
        .with_context(|| format!("read bundled skill oracle {}", oracle_path.display()))?;
    let oracle: BundledSkillsOracle = serde_json::from_str(&encoded)
        .with_context(|| format!("parse bundled skill oracle {}", oracle_path.display()))?;
    if oracle.schema_version != 1 {
        bail!(
            "{BUNDLED_SKILLS_ORACLE} has unsupported schema version {}",
            oracle.schema_version
        );
    }
    if !is_lower_hex(&oracle.captured_from, 40) {
        bail!("{BUNDLED_SKILLS_ORACLE} has an invalid source commit");
    }
    if oracle.sources.len() != 2 {
        bail!("{BUNDLED_SKILLS_ORACLE} must map exactly two bundled source skills");
    }
    if oracle.executable_evidence.is_empty()
        || oracle
            .executable_evidence
            .iter()
            .any(|evidence| evidence.trim().is_empty())
    {
        bail!("{BUNDLED_SKILLS_ORACLE} must name executable evidence");
    }

    for source in &oracle.sources {
        validate_source_mapping(repo, source)?;
    }
    println!("Workdeck extension and release skills match their complete source mappings");
    Ok(())
}

fn validate_source_mapping(repo: &Path, source: &BundledSkillSource) -> Result<()> {
    if !is_lower_hex(&source.source_blob, 40) || !is_lower_hex(&source.source_sha256, 64) {
        bail!(
            "{} has invalid frozen source identities",
            source.source_path
        );
    }
    if source.source_bytes == 0 || source.source_lines == 0 || source.sections.is_empty() {
        bail!("{} has an empty source mapping", source.source_path);
    }

    let destination_path = repo.join(&source.destination_path);
    let destination = fs::read_to_string(&destination_path)
        .with_context(|| format!("read bundled skill {}", destination_path.display()))?;
    let destination_hash = format!("{:x}", Sha256::digest(destination.as_bytes()));
    if destination_hash != source.destination_sha256 {
        bail!(
            "{} changed without updating {BUNDLED_SKILLS_ORACLE}; expected {}, found {}",
            source.destination_path,
            source.destination_sha256,
            destination_hash
        );
    }
    validate_skill_frontmatter(&source.destination_path, &destination)?;
    validate_native_only_guidance(&source.destination_path, &destination)?;

    let mut next_line = 1;
    let mut next_byte = 0;
    for section in &source.sections {
        if section.source_lines[0] != next_line
            || section.source_bytes[0] != next_byte
            || section.source_lines[1] < section.source_lines[0]
            || section.source_bytes[1] <= section.source_bytes[0]
        {
            bail!(
                "{} has incomplete or overlapping section coverage at line {} / byte {}",
                source.source_path,
                next_line,
                next_byte
            );
        }
        if section.source_semantics.trim().is_empty()
            || section.adaptation.trim().is_empty()
            || section.destination_headings.is_empty()
        {
            bail!(
                "{} has a blanket or empty section mapping",
                source.source_path
            );
        }
        for heading in &section.destination_headings {
            if heading.trim().is_empty() || !destination.lines().any(|line| line == heading) {
                bail!(
                    "{} is missing mapped heading {heading:?} for {}",
                    source.destination_path,
                    source.source_path
                );
            }
        }
        next_line = section.source_lines[1] + 1;
        next_byte = section.source_bytes[1];
    }
    if next_line != source.source_lines + 1 || next_byte != source.source_bytes {
        bail!(
            "{} maps through line {} / byte {}, expected line {} / byte {}",
            source.source_path,
            next_line.saturating_sub(1),
            next_byte,
            source.source_lines,
            source.source_bytes
        );
    }

    if source.evidence.is_empty() {
        bail!("{} has no implementation evidence", source.source_path);
    }
    for evidence in &source.evidence {
        if !repo.join(evidence).exists() {
            bail!(
                "{} names missing implementation evidence {evidence}",
                source.source_path
            );
        }
    }
    Ok(())
}

fn validate_skill_frontmatter(path: &str, skill: &str) -> Result<()> {
    let mut lines = skill.lines();
    if lines.next() != Some("---") {
        bail!("{path} must start with frontmatter");
    }
    let name = lines
        .next()
        .and_then(|line| line.strip_prefix("name: "))
        .filter(|name| !name.trim().is_empty())
        .with_context(|| format!("{path} needs one non-empty name field"))?;
    let _description = lines
        .next()
        .and_then(|line| line.strip_prefix("description: "))
        .filter(|description| !description.trim().is_empty())
        .with_context(|| format!("{path} needs one non-empty description field"))?;
    if lines.next() != Some("---") {
        bail!("{path} frontmatter must contain only name and description");
    }
    let directory_name = Path::new(path)
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if name != directory_name {
        bail!("{path} name {name:?} must match its skill directory");
    }
    Ok(())
}

fn validate_native_only_guidance(path: &str, skill: &str) -> Result<()> {
    let lower = skill.to_ascii_lowercase();
    for forbidden in [
        "`bun",
        "\nbun ",
        "`npm",
        "\nnpm ",
        "node_modules",
        ".hunk/",
        "hunkdiff",
        "@opentui/",
    ] {
        if lower.contains(forbidden) {
            bail!("{path} retains forbidden source-runtime guidance {forbidden:?}");
        }
    }
    Ok(())
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_installed_and_web_skills_are_identical_and_repairable() {
        let repo = tempfile::tempdir().unwrap();
        generate(repo.path()).unwrap();
        check_generated_skills(repo.path()).unwrap();
        let installed = fs::read(repo.path().join(REVIEW_SKILL)).unwrap();
        assert_eq!(
            installed,
            fs::read(repo.path().join(WEB_REVIEW_SKILL)).unwrap()
        );
        fs::write(repo.path().join(WEB_REVIEW_SKILL), "stale website skill").unwrap();
        let error = check_generated_skills(repo.path()).unwrap_err().to_string();
        assert!(error.contains(WEB_REVIEW_SKILL));
        assert!(error.contains("out of date"));
        assert_eq!(
            fs::read(repo.path().join(WEB_REVIEW_SKILL)).unwrap(),
            b"stale website skill"
        );
        generate(repo.path()).unwrap();
        check_generated_skills(repo.path()).unwrap();
        assert_eq!(installed, fs::read(repo.path().join(REVIEW_SKILL)).unwrap());
        assert_eq!(
            installed,
            fs::read(repo.path().join(WEB_REVIEW_SKILL)).unwrap()
        );
        fs::remove_file(repo.path().join(WEB_REVIEW_SKILL)).unwrap();
        assert!(
            check_generated_skills(repo.path())
                .unwrap_err()
                .to_string()
                .contains(WEB_REVIEW_SKILL)
        );
    }

    #[test]
    fn normalizes_checkout_line_endings() {
        assert_eq!(normalize_newlines("one\r\ntwo\r\n"), "one\ntwo\n");
    }

    #[test]
    fn live_checked_in_skill_matches_renderer() {
        check(&crate::repo_root().unwrap()).unwrap();
    }

    #[test]
    fn bundled_static_skills_match_the_complete_source_mapping() {
        check_static_skills(&crate::repo_root().unwrap()).unwrap();
    }
}
