//! Canonical paths for Workdeck configuration, state, extensions, and bundled skills.

use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};

use thiserror::Error;

pub const BUNDLED_SKILL_NAMES: &[&str] = &[
    "workdeck-review",
    "workdeck-extensions",
    "workdeck-release",
    "workdeck-launch-video",
];
pub const DEFAULT_BUNDLED_SKILL_NAME: &str = "workdeck-review";
pub const INSTALLED_EXTENSIONS_DIR_NAME: &str = "installed";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserPathEnvironment {
    pub xdg_config_home: Option<OsString>,
    pub home: Option<OsString>,
    pub user_profile: Option<OsString>,
}

impl UserPathEnvironment {
    #[must_use]
    pub fn current() -> Self {
        Self {
            xdg_config_home: std::env::var_os("XDG_CONFIG_HOME"),
            home: std::env::var_os("HOME"),
            user_profile: std::env::var_os("USERPROFILE"),
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PathResolutionError {
    #[error("Could not locate the bundled Workdeck {0} skill.")]
    MissingBundledSkill(String),
    #[error("cannot resolve an absolute path because the current directory is unavailable: {0}")]
    CurrentDirectory(String),
}

#[must_use]
pub fn resolve_bundled_skill_name(value: &str) -> Option<&'static str> {
    let normalized = value.trim().to_ascii_lowercase();
    BUNDLED_SKILL_NAMES
        .iter()
        .copied()
        .find(|name| *name == normalized)
        .or(match normalized.as_str() {
            "review" => Some("workdeck-review"),
            "extensions" => Some("workdeck-extensions"),
            "release" => Some("workdeck-release"),
            "launch-video" => Some("workdeck-launch-video"),
            _ => None,
        })
}

/// Resolve symlinked existing ancestors even when the requested leaf is absent.
pub fn resolve_canonical_path(path: impl AsRef<Path>) -> Result<PathBuf, PathResolutionError> {
    let path = path.as_ref();
    let absolute = if path.is_absolute() {
        normalize_path(path)
    } else {
        let cwd = std::env::current_dir()
            .map_err(|error| PathResolutionError::CurrentDirectory(error.to_string()))?;
        normalize_path(&cwd.join(path))
    };
    if let Ok(canonical) = fs::canonicalize(&absolute) {
        return Ok(canonical);
    }

    let mut current = absolute.as_path();
    let mut missing = Vec::<OsString>::new();
    loop {
        if let Some(name) = current.file_name() {
            missing.push(name.to_os_string());
        }
        let Some(parent) = current.parent() else {
            return Ok(absolute);
        };
        if parent == current {
            return Ok(absolute);
        }
        current = parent;
        if let Ok(mut canonical) = fs::canonicalize(current) {
            for segment in missing.iter().rev() {
                canonical.push(segment);
            }
            return Ok(canonical);
        }
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

#[must_use]
pub fn resolve_user_config_dir_with(environment: &UserPathEnvironment) -> Option<PathBuf> {
    environment
        .xdg_config_home
        .as_ref()
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            environment
                .home
                .as_ref()
                .filter(|value| !value.is_empty())
                .or_else(|| {
                    environment
                        .user_profile
                        .as_ref()
                        .filter(|value| !value.is_empty())
                })
                .map(PathBuf::from)
                .map(|home| home.join(".config"))
        })
}

#[must_use]
pub fn resolve_user_config_dir() -> Option<PathBuf> {
    resolve_user_config_dir_with(&UserPathEnvironment::current())
}

#[must_use]
pub fn resolve_global_config_path_with(environment: &UserPathEnvironment) -> Option<PathBuf> {
    resolve_user_config_dir_with(environment).map(|root| root.join("workdeck/config.toml"))
}

#[must_use]
pub fn resolve_global_config_path() -> Option<PathBuf> {
    resolve_global_config_path_with(&UserPathEnvironment::current())
}

#[must_use]
pub fn resolve_app_state_path_with(environment: &UserPathEnvironment) -> Option<PathBuf> {
    resolve_user_config_dir_with(environment).map(|root| root.join("workdeck/state.json"))
}

#[must_use]
pub fn resolve_app_state_path() -> Option<PathBuf> {
    resolve_app_state_path_with(&UserPathEnvironment::current())
}

#[must_use]
pub fn resolve_global_extensions_dir_with(environment: &UserPathEnvironment) -> Option<PathBuf> {
    resolve_user_config_dir_with(environment).map(|root| root.join("workdeck/extensions"))
}

#[must_use]
pub fn resolve_global_extensions_dir() -> Option<PathBuf> {
    resolve_global_extensions_dir_with(&UserPathEnvironment::current())
}

#[must_use]
pub fn resolve_installed_extensions_root_with(
    environment: &UserPathEnvironment,
) -> Option<PathBuf> {
    resolve_global_extensions_dir_with(environment)
        .map(|root| root.join(INSTALLED_EXTENSIONS_DIR_NAME))
}

#[must_use]
pub fn resolve_installed_extensions_root() -> Option<PathBuf> {
    resolve_installed_extensions_root_with(&UserPathEnvironment::current())
}

fn find_relative_path_from_ancestors(start: &Path, relative: &Path) -> Option<PathBuf> {
    let mut current = if start.is_absolute() {
        start.to_owned()
    } else {
        std::env::current_dir().ok()?.join(start)
    };
    if current.is_file() {
        current.pop();
    }
    loop {
        let candidate = current.join(relative);
        if candidate.exists() {
            return Some(candidate);
        }
        if !current.pop() {
            return None;
        }
    }
}

pub fn resolve_bundled_skill_path_from(
    name: &str,
    search_roots: &[PathBuf],
) -> Result<PathBuf, PathResolutionError> {
    let name = resolve_bundled_skill_name(name)
        .ok_or_else(|| PathResolutionError::MissingBundledSkill(name.to_owned()))?;
    let skill = PathBuf::from("skills").join(name).join("SKILL.md");
    let candidates = [
        skill.clone(),
        PathBuf::from("workdeck").join(&skill),
        PathBuf::from("share/workdeck").join(&skill),
    ];
    for root in search_roots {
        for candidate in &candidates {
            if let Some(path) = find_relative_path_from_ancestors(root, candidate) {
                return Ok(path);
            }
        }
    }
    Err(PathResolutionError::MissingBundledSkill(name.into()))
}

pub fn resolve_bundled_skill_path(name: Option<&str>) -> Result<PathBuf, PathResolutionError> {
    let roots = [
        PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        std::env::current_exe().unwrap_or_default(),
    ];
    resolve_bundled_skill_path_from(name.unwrap_or(DEFAULT_BUNDLED_SKILL_NAME), &roots)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn environment(
        xdg: Option<&str>,
        home: Option<&str>,
        profile: Option<&str>,
    ) -> UserPathEnvironment {
        UserPathEnvironment {
            xdg_config_home: xdg.map(Into::into),
            home: home.map(Into::into),
            user_profile: profile.map(Into::into),
        }
    }

    #[test]
    fn resolves_xdg_home_and_profile_paths_under_workdeck() {
        let xdg = environment(Some("/tmp/xdg-home"), None, None);
        assert_eq!(
            resolve_global_config_path_with(&xdg),
            Some(PathBuf::from("/tmp/xdg-home/workdeck/config.toml"))
        );
        assert_eq!(
            resolve_app_state_path_with(&xdg),
            Some(PathBuf::from("/tmp/xdg-home/workdeck/state.json"))
        );
        let home = environment(None, Some("/tmp/home"), None);
        assert_eq!(
            resolve_global_config_path_with(&home),
            Some(PathBuf::from("/tmp/home/.config/workdeck/config.toml"))
        );
        let profile = environment(None, None, Some("/tmp/windows-profile"));
        assert_eq!(
            resolve_global_config_path_with(&profile),
            Some(PathBuf::from(
                "/tmp/windows-profile/.config/workdeck/config.toml"
            ))
        );
        assert_eq!(
            resolve_app_state_path_with(&profile),
            Some(PathBuf::from(
                "/tmp/windows-profile/.config/workdeck/state.json"
            ))
        );
    }

    #[test]
    fn resolves_every_bundled_skill_and_alias() {
        for name in BUNDLED_SKILL_NAMES {
            let path =
                resolve_bundled_skill_path_from(name, &[PathBuf::from(env!("CARGO_MANIFEST_DIR"))])
                    .unwrap();
            assert!(path.ends_with(Path::new("skills").join(name).join("SKILL.md")));
        }
        assert_eq!(
            resolve_bundled_skill_name(" extensions "),
            Some("workdeck-extensions")
        );
        assert_eq!(
            resolve_bundled_skill_name("Review"),
            Some("workdeck-review")
        );
        assert_eq!(resolve_bundled_skill_name("missing"), None);
    }

    #[test]
    fn missing_skill_error_names_the_requested_skill() {
        let directory = TempDir::new().unwrap();
        let error =
            resolve_bundled_skill_path_from("workdeck-extensions", &[directory.path().to_owned()])
                .unwrap_err();
        assert!(error.to_string().contains("workdeck-extensions"));
    }

    #[test]
    fn locates_packaged_skill_through_a_nested_share_directory() {
        let directory = TempDir::new().unwrap();
        let skill = directory
            .path()
            .join("share/workdeck/skills/workdeck-review/SKILL.md");
        fs::create_dir_all(skill.parent().unwrap()).unwrap();
        fs::write(&skill, "# skill\n").unwrap();
        let binary = directory.path().join("bin/workdeck");
        fs::create_dir_all(binary.parent().unwrap()).unwrap();
        fs::write(&binary, "binary\n").unwrap();
        assert_eq!(
            resolve_bundled_skill_path_from("workdeck-review", &[binary]).unwrap(),
            skill
        );
    }

    #[cfg(unix)]
    #[test]
    fn canonicalizes_existing_and_missing_paths_through_a_symlinked_ancestor() {
        use std::os::unix::fs::symlink;

        let directory = TempDir::new().unwrap();
        let target = directory.path().join("target");
        let link = directory.path().join("link");
        fs::create_dir_all(&target).unwrap();
        symlink(&target, &link).unwrap();
        let target = fs::canonicalize(target).unwrap();
        assert_eq!(resolve_canonical_path(&link).unwrap(), target);
        assert_eq!(
            resolve_canonical_path(link.join("nested/missing.txt")).unwrap(),
            target.join("nested/missing.txt")
        );
    }
}
