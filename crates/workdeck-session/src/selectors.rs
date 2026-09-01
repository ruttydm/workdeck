//! Session selector precedence and repository containment matching.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectableSession {
    pub session_id: String,
    pub cwd: PathBuf,
    pub repo_root: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSelector {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_boundary: Option<PathBuf>,
}

fn containment_distance(root: &Path, candidate: &Path) -> Option<usize> {
    let relative = candidate.strip_prefix(root).ok()?;
    Some(
        relative
            .components()
            .filter(|component| !matches!(component, Component::CurDir))
            .count(),
    )
}

pub fn repo_selector_distance(
    session: &SelectableSession,
    selector_path: &Path,
    repo_boundary: Option<&Path>,
) -> Option<usize> {
    let session_root = session.repo_root.as_deref()?;
    if repo_boundary.is_some_and(|boundary| containment_distance(boundary, session_root).is_none())
    {
        return None;
    }
    containment_distance(session_root, selector_path)
}

pub fn matches_session_selector(
    session: &SelectableSession,
    selector: Option<&SessionSelector>,
) -> bool {
    let Some(selector) = selector else {
        return true;
    };
    if let Some(session_id) = &selector.session_id {
        return &session.session_id == session_id;
    }
    if let Some(session_path) = &selector.session_path {
        return &session.cwd == session_path;
    }
    if let Some(repo_root) = &selector.repo_root {
        return repo_selector_distance(session, repo_root, selector.repo_boundary.as_deref())
            .is_some();
    }
    true
}

pub fn normalize_session_selector(selector: &SessionSelector) -> std::io::Result<SessionSelector> {
    Ok(SessionSelector {
        session_id: selector.session_id.clone(),
        session_path: selector
            .session_path
            .as_deref()
            .map(absolute_lexical)
            .transpose()?,
        repo_root: selector
            .repo_root
            .as_deref()
            .map(absolute_lexical)
            .transpose()?,
        repo_boundary: selector
            .repo_boundary
            .as_deref()
            .map(absolute_lexical)
            .transpose()?,
    })
}

/// Attach the nearest recognized project boundary to a repo-path selector.
pub fn resolve_session_selector_boundary(
    selector: &SessionSelector,
    find_boundary: impl FnOnce(&Path) -> Option<PathBuf>,
) -> SessionSelector {
    let Some(repo_root) = selector.repo_root.as_deref() else {
        return selector.clone();
    };
    let Some(repo_boundary) = find_boundary(repo_root) else {
        return selector.clone();
    };
    SessionSelector {
        repo_boundary: Some(repo_boundary),
        ..selector.clone()
    }
}

pub fn describe_session_selector(selector: &SessionSelector) -> String {
    if let Some(session_id) = &selector.session_id {
        return format!("session {session_id}");
    }
    if let Some(session_path) = &selector.session_path {
        return format!("session path {}", session_path.display());
    }
    if let Some(repo_root) = &selector.repo_root {
        return format!("repo {}", repo_root.display());
    }
    "session".into()
}

fn absolute_lexical(path: &Path) -> std::io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_repo_paths_and_optional_boundaries() {
        let selector = normalize_session_selector(&SessionSelector {
            repo_root: Some("repo/src".into()),
            repo_boundary: Some("repo".into()),
            ..SessionSelector::default()
        })
        .unwrap();
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(selector.repo_root, Some(cwd.join("repo/src")));
        assert_eq!(selector.repo_boundary, Some(cwd.join("repo")));
        assert_eq!(selector.session_path, None);
    }

    #[test]
    fn rejects_session_roots_outside_a_repository_boundary() {
        let cwd = std::env::current_dir().unwrap();
        let boundary = cwd.join("repo/nested");
        let selector_path = boundary.join("src");
        let outer = SelectableSession {
            session_id: "outer".into(),
            cwd: cwd.join("repo"),
            repo_root: Some(cwd.join("repo")),
        };
        let inner = SelectableSession {
            session_id: "inner".into(),
            cwd: boundary.clone(),
            repo_root: Some(boundary.clone()),
        };
        assert_eq!(
            repo_selector_distance(&outer, &selector_path, Some(&boundary)),
            None
        );
        assert_eq!(
            repo_selector_distance(&inner, &selector_path, Some(&boundary)),
            Some(1)
        );
    }

    #[test]
    fn selector_precedence_is_id_then_path_then_repo() {
        let session = SelectableSession {
            session_id: "one".into(),
            cwd: PathBuf::from("/repo/sub"),
            repo_root: Some(PathBuf::from("/repo")),
        };
        assert!(matches_session_selector(
            &session,
            Some(&SessionSelector {
                session_id: Some("one".into()),
                session_path: Some("/wrong".into()),
                repo_root: Some("/wrong".into()),
                repo_boundary: None,
            })
        ));
        assert_eq!(
            describe_session_selector(&SessionSelector {
                repo_root: Some("/repo".into()),
                ..SessionSelector::default()
            }),
            "repo /repo"
        );
    }

    #[test]
    fn boundary_resolution_only_changes_addressable_repo_selectors() {
        let selector = SessionSelector {
            repo_root: Some(PathBuf::from("/repo/src/deep")),
            ..SessionSelector::default()
        };
        assert_eq!(
            resolve_session_selector_boundary(&selector, |_| Some(PathBuf::from("/repo")))
                .repo_boundary,
            Some(PathBuf::from("/repo"))
        );
        assert_eq!(
            resolve_session_selector_boundary(&selector, |_| None),
            selector
        );
        let by_id = SessionSelector {
            session_id: Some("one".into()),
            ..SessionSelector::default()
        };
        assert_eq!(
            resolve_session_selector_boundary(&by_id, |_| Some(PathBuf::from("/ignored"))),
            by_id
        );
    }
}
