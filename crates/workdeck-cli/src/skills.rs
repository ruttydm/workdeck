//! Read-only ancestor lookup derived from Hunk's MIT-licensed core/run/paths.ts.
//! Copyright (c) Modem Labs Inc. See THIRD_PARTY_NOTICES.
use anyhow::{Result, anyhow};
use std::path::PathBuf;
use workdeck_core::{
    PathResolutionError, resolve_bundled_skill_name, resolve_bundled_skill_path_from,
};

pub fn canonical_name(value: &str) -> Result<&'static str> {
    resolve_bundled_skill_name(value).ok_or_else(|| anyhow!("unknown bundled skill {value:?}"))
}

/// Search native package and source ancestors without creating user or repo state.
/// npm layouts are intentionally absent from the native Workdeck distribution.
pub fn find_path(name: &str, roots: &[PathBuf]) -> Result<Option<PathBuf>> {
    let name = canonical_name(name)?;
    match resolve_bundled_skill_path_from(name, roots) {
        Ok(path) => Ok(Some(path)),
        Err(PathResolutionError::MissingBundledSkill(_)) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_normalize_case_whitespace_and_preserve_workdeck_additions() {
        assert_eq!(canonical_name(" Review ").unwrap(), "workdeck-review");
        assert_eq!(
            canonical_name("WORKDECK-EXTENSIONS").unwrap(),
            "workdeck-extensions"
        );
        assert_eq!(canonical_name("release").unwrap(), "workdeck-release");
        assert!(canonical_name("../review").is_err());
        assert!(canonical_name("").is_err());
    }

    #[test]
    fn native_binary_and_missing_source_roots_find_existing_skills_without_writes() {
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("bin/workdeck");
        let skill = dir.path().join("skills/workdeck-review/SKILL.md");
        std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
        std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
        std::fs::write(&binary, b"binary").unwrap();
        std::fs::write(&skill, b"skill").unwrap();
        assert_eq!(find_path("review", &[binary]).unwrap(), Some(skill.clone()));
        assert_eq!(
            find_path("review", &[dir.path().join("missing/src")]).unwrap(),
            Some(skill)
        );
        assert!(!dir.path().join("missing").exists());
        assert!(!dir.path().join(".agents").exists());
        assert!(
            find_path("extensions", &[dir.path().to_owned()])
                .unwrap()
                .is_none()
        );
    }
}
