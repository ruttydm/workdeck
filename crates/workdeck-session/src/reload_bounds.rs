//! Filesystem and revision confinement for daemon-driven live review reloads.

use std::path::{Path, PathBuf};

use thiserror::Error;
use workdeck_core::resolve_canonical_path;

use crate::{DaemonCliInput, DaemonCommonOptions};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionReloadBounds {
    pub roots: Vec<PathBuf>,
    pub exact_files: Vec<PathBuf>,
    pub default_cwd: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedSessionReload {
    pub cwd: PathBuf,
}

#[derive(Debug, Error)]
pub enum SessionReloadBoundsError {
    #[error("could not resolve a reload path: {0}")]
    Path(String),
    #[error("Session reload requires the initial Workdeck session to be rooted in a repository.")]
    UnrootedSession,
    #[error("Session reload refused {description} outside the initial Workdeck root: {candidate}")]
    OutsideRoot {
        description: &'static str,
        candidate: String,
    },
    #[error("Session reload refused source path outside the initial Workdeck root: {0}")]
    OutsideSource(String),
    #[error("Session reload refused {description} that looks like a VCS option: {value}")]
    OptionLikeRevision {
        description: &'static str,
        value: String,
    },
    #[error("Session reload does not support `--agent-context -`.")]
    StdinAgentContext,
    #[error("Session reload does not support stdin-backed patch input.")]
    StdinPatch,
    #[error(
        "Session reload requires repository-backed input to stay inside the initial Workdeck root."
    )]
    RepositoryInputWithoutRoot,
}

fn canonical(path: impl AsRef<Path>) -> Result<PathBuf, SessionReloadBoundsError> {
    resolve_canonical_path(path).map_err(|error| SessionReloadBoundsError::Path(error.to_string()))
}

fn within_root(root: &Path, candidate: &Path) -> bool {
    candidate == root || candidate.strip_prefix(root).is_ok()
}

fn normalize_roots(roots: Vec<PathBuf>) -> Result<Vec<PathBuf>, SessionReloadBoundsError> {
    let mut unique = Vec::<PathBuf>::new();
    for root in roots {
        let root = canonical(root)?;
        if unique.iter().any(|existing| within_root(existing, &root)) {
            continue;
        }
        unique.retain(|existing| !within_root(&root, existing));
        unique.push(root);
    }
    Ok(unique)
}

fn resolve_repo_reload_roots(
    initial_cwd: &Path,
    paths: &[String],
    find_root: &impl Fn(&Path) -> Option<PathBuf>,
) -> Result<Vec<PathBuf>, SessionReloadBoundsError> {
    let Some(root) = find_root(initial_cwd) else {
        return Ok(Vec::new());
    };
    let root = canonical(root)?;
    let files = paths
        .iter()
        .map(|path| canonical(initial_cwd.join(path)))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(files
        .iter()
        .all(|path| within_root(&root, path))
        .then_some(vec![root])
        .unwrap_or_default())
}

/// Derive reload authority from the files and repository made available at startup.
pub fn create_session_reload_bounds(
    initial_input: &DaemonCliInput,
    reload_repo_root: Option<&Path>,
    cwd: &Path,
) -> Result<SessionReloadBounds, SessionReloadBoundsError> {
    create_session_reload_bounds_with_root_finder(
        initial_input,
        reload_repo_root,
        cwd,
        workdeck_vcs::find_project_root_candidate,
    )
}

pub fn create_session_reload_bounds_with_root_finder(
    initial_input: &DaemonCliInput,
    reload_repo_root: Option<&Path>,
    cwd: &Path,
    find_root: impl Fn(&Path) -> Option<PathBuf>,
) -> Result<SessionReloadBounds, SessionReloadBoundsError> {
    let initial_cwd = canonical(cwd)?;
    let mut roots = Vec::new();
    let mut exact_files = Vec::new();
    match initial_input {
        DaemonCliInput::Vcs { .. }
        | DaemonCliInput::Show { .. }
        | DaemonCliInput::StashShow { .. } => {
            roots.push(reload_repo_root.unwrap_or(&initial_cwd).to_path_buf());
        }
        DaemonCliInput::Diff { left, right, .. } | DaemonCliInput::Difftool { left, right, .. } => {
            roots = resolve_repo_reload_roots(
                &initial_cwd,
                &[left.clone(), right.clone()],
                &find_root,
            )?;
        }
        DaemonCliInput::Patch { file, .. } => {
            if let Some(file) = file.as_deref().filter(|file| *file != "-") {
                roots = resolve_repo_reload_roots(
                    &initial_cwd,
                    std::slice::from_ref(&file.to_owned()),
                    &find_root,
                )?;
                if roots.is_empty() {
                    exact_files.push(canonical(initial_cwd.join(file))?);
                }
            }
        }
    }
    let mut exact_unique = Vec::new();
    for file in exact_files {
        let file = canonical(file)?;
        if !exact_unique.contains(&file) {
            exact_unique.push(file);
        }
    }
    Ok(SessionReloadBounds {
        roots: normalize_roots(roots)?,
        exact_files: exact_unique,
        default_cwd: initial_cwd,
    })
}

fn assert_reloadable(bounds: &SessionReloadBounds) -> Result<(), SessionReloadBoundsError> {
    if bounds.roots.is_empty() && bounds.exact_files.is_empty() {
        return Err(SessionReloadBoundsError::UnrootedSession);
    }
    Ok(())
}

fn assert_file(
    bounds: &SessionReloadBounds,
    cwd: &Path,
    path: &str,
    description: &'static str,
    allow_exact: bool,
) -> Result<PathBuf, SessionReloadBoundsError> {
    let candidate = canonical(cwd.join(path))?;
    let within = bounds
        .roots
        .iter()
        .any(|root| within_root(root, &candidate));
    let exact = allow_exact && bounds.exact_files.contains(&candidate);
    if !within && !exact {
        return Err(SessionReloadBoundsError::OutsideRoot {
            description,
            candidate: candidate.display().to_string(),
        });
    }
    Ok(candidate)
}

fn assert_source(
    bounds: &SessionReloadBounds,
    cwd: &Path,
    path: &str,
) -> Result<PathBuf, SessionReloadBoundsError> {
    let candidate = canonical(cwd.join(path))?;
    if !bounds
        .roots
        .iter()
        .any(|root| within_root(root, &candidate))
    {
        return Err(SessionReloadBoundsError::OutsideSource(
            candidate.display().to_string(),
        ));
    }
    Ok(candidate)
}

fn assert_revision(description: &'static str, value: &str) -> Result<(), SessionReloadBoundsError> {
    if value.starts_with('-') {
        return Err(SessionReloadBoundsError::OptionLikeRevision {
            description,
            value: value.to_owned(),
        });
    }
    Ok(())
}

fn validate_options(
    bounds: &SessionReloadBounds,
    cwd: &Path,
    options: &DaemonCommonOptions,
) -> Result<(), SessionReloadBoundsError> {
    let Some(agent_context) = options.agent_context.as_deref() else {
        return Ok(());
    };
    if agent_context == "-" {
        return Err(SessionReloadBoundsError::StdinAgentContext);
    }
    assert_file(bounds, cwd, agent_context, "agent context path", false)?;
    Ok(())
}

/// Validate every path and revision before any daemon-driven reload can touch the filesystem.
pub fn validate_session_reload_within_bounds(
    bounds: &SessionReloadBounds,
    next_input: &DaemonCliInput,
    source_path: Option<&str>,
) -> Result<ValidatedSessionReload, SessionReloadBoundsError> {
    assert_reloadable(bounds)?;
    let source_cwd = source_path.map_or_else(
        || Ok(bounds.default_cwd.clone()),
        |path| assert_source(bounds, &bounds.default_cwd, path),
    )?;
    let options = match next_input {
        DaemonCliInput::Vcs { options, .. }
        | DaemonCliInput::Show { options, .. }
        | DaemonCliInput::StashShow { options, .. }
        | DaemonCliInput::Diff { options, .. }
        | DaemonCliInput::Patch { options, .. }
        | DaemonCliInput::Difftool { options, .. } => options,
    };
    validate_options(bounds, &source_cwd, options)?;
    match next_input {
        DaemonCliInput::Diff { left, right, .. } | DaemonCliInput::Difftool { left, right, .. } => {
            assert_file(bounds, &source_cwd, left, "left file", false)?;
            assert_file(bounds, &source_cwd, right, "right file", false)?;
        }
        DaemonCliInput::Patch { file, text, .. } => {
            if let Some(file) = file.as_deref().filter(|file| *file != "-") {
                assert_file(bounds, &source_cwd, file, "patch file", true)?;
            } else if text.is_none() {
                return Err(SessionReloadBoundsError::StdinPatch);
            }
        }
        DaemonCliInput::Vcs {
            range,
            range_endpoints,
            ..
        } => {
            if bounds.roots.is_empty() {
                return Err(SessionReloadBoundsError::RepositoryInputWithoutRoot);
            }
            if let Some(range) = range {
                assert_revision("diff range", range)?;
            }
            if let Some(endpoints) = range_endpoints {
                assert_revision("diff from revision", &endpoints.from)?;
                assert_revision("diff to revision", &endpoints.to)?;
            }
        }
        DaemonCliInput::Show { reference, .. } => {
            if bounds.roots.is_empty() {
                return Err(SessionReloadBoundsError::RepositoryInputWithoutRoot);
            }
            if let Some(reference) = reference {
                assert_revision("show ref", reference)?;
            }
        }
        DaemonCliInput::StashShow { reference, .. } => {
            if bounds.roots.is_empty() {
                return Err(SessionReloadBoundsError::RepositoryInputWithoutRoot);
            }
            if let Some(reference) = reference {
                assert_revision("stash-show ref", reference)?;
            }
        }
    }
    Ok(ValidatedSessionReload { cwd: source_cwd })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use crate::DaemonRangeEndpoints;

    fn options() -> DaemonCommonOptions {
        DaemonCommonOptions::default()
    }

    fn vcs() -> DaemonCliInput {
        DaemonCliInput::Vcs {
            range: None,
            range_endpoints: None,
            staged: false,
            pathspecs: None,
            options: options(),
        }
    }

    fn show(reference: &str) -> DaemonCliInput {
        DaemonCliInput::Show {
            reference: Some(reference.into()),
            pathspecs: None,
            options: options(),
        }
    }

    fn files(left: impl Into<String>, right: impl Into<String>) -> DaemonCliInput {
        DaemonCliInput::Diff {
            left: left.into(),
            right: right.into(),
            options: options(),
        }
    }

    fn patch(file: Option<String>, text: Option<String>) -> DaemonCliInput {
        DaemonCliInput::Patch {
            file,
            text,
            options: options(),
        }
    }

    fn repository() -> tempfile::TempDir {
        let repo = tempdir().unwrap();
        fs::create_dir(repo.path().join(".git")).unwrap();
        repo
    }

    fn bounds(input: &DaemonCliInput, repo_root: Option<&Path>, cwd: &Path) -> SessionReloadBounds {
        create_session_reload_bounds(input, repo_root, cwd).unwrap()
    }

    #[test]
    fn vcs_reload_stays_inside_root_and_accepts_nested_source_directories() {
        let repo = repository();
        let nested = repo.path().join("packages/app");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("before.ts"), "before\n").unwrap();
        fs::write(nested.join("after.ts"), "after\n").unwrap();
        let bounds = bounds(&vcs(), Some(repo.path()), &nested);
        assert_eq!(
            validate_session_reload_within_bounds(&bounds, &show("HEAD"), None)
                .unwrap()
                .cwd,
            canonical(&nested).unwrap()
        );
        assert_eq!(
            validate_session_reload_within_bounds(
                &bounds,
                &files("before.ts", "after.ts"),
                Some(nested.to_str().unwrap()),
            )
            .unwrap()
            .cwd,
            canonical(&nested).unwrap()
        );
    }

    #[test]
    fn option_like_ranges_endpoints_and_refs_are_rejected_but_pathspecs_are_exempt() {
        let repo = repository();
        let bounds = bounds(&vcs(), Some(repo.path()), repo.path());
        let malformed = [
            DaemonCliInput::Vcs {
                range: Some("--output=/tmp/workdeck-poc".into()),
                range_endpoints: None,
                staged: false,
                pathspecs: None,
                options: options(),
            },
            DaemonCliInput::Vcs {
                range: None,
                range_endpoints: Some(DaemonRangeEndpoints {
                    from: "main".into(),
                    to: "--output=/tmp/workdeck-poc".into(),
                }),
                staged: false,
                pathspecs: None,
                options: options(),
            },
            DaemonCliInput::Vcs {
                range: None,
                range_endpoints: Some(DaemonRangeEndpoints {
                    from: "-R".into(),
                    to: "feature".into(),
                }),
                staged: false,
                pathspecs: None,
                options: options(),
            },
            show("--output=/tmp/workdeck-poc"),
            DaemonCliInput::StashShow {
                reference: Some("-R".into()),
                options: options(),
            },
        ];
        for input in malformed {
            assert!(matches!(
                validate_session_reload_within_bounds(&bounds, &input, None),
                Err(SessionReloadBoundsError::OptionLikeRevision { .. })
            ));
        }
        for input in [
            DaemonCliInput::Vcs {
                range: Some("main..feature".into()),
                range_endpoints: None,
                staged: false,
                pathspecs: Some(vec!["--help".into()]),
                options: options(),
            },
            DaemonCliInput::Vcs {
                range: None,
                range_endpoints: Some(DaemonRangeEndpoints {
                    from: "main".into(),
                    to: "feature".into(),
                }),
                staged: false,
                pathspecs: None,
                options: options(),
            },
            DaemonCliInput::StashShow {
                reference: Some("stash@{1}".into()),
                options: options(),
            },
        ] {
            validate_session_reload_within_bounds(&bounds, &input, None).unwrap();
        }
    }

    #[test]
    fn source_paths_outside_root_and_parent_traversal_are_rejected() {
        let repo = repository();
        let outside = tempdir().unwrap();
        let bounds = bounds(&vcs(), Some(repo.path()), repo.path());
        for source in [outside.path().to_str().unwrap(), ".."] {
            assert!(matches!(
                validate_session_reload_within_bounds(&bounds, &vcs(), Some(source)),
                Err(SessionReloadBoundsError::OutsideSource(_))
            ));
        }
    }

    #[test]
    fn direct_files_outside_repo_are_unreloadable_but_inside_repo_use_the_repo_root() {
        let standalone = tempdir().unwrap();
        let left = standalone.path().join("before.ts");
        let right = standalone.path().join("after.ts");
        fs::write(&left, "before\n").unwrap();
        fs::write(&right, "after\n").unwrap();
        let input = files(left.to_string_lossy(), right.to_string_lossy());
        let standalone_bounds = bounds(&input, None, standalone.path());
        assert!(matches!(
            validate_session_reload_within_bounds(&standalone_bounds, &input, None),
            Err(SessionReloadBoundsError::UnrootedSession)
        ));

        let repo = repository();
        let nested = repo.path().join("src");
        fs::create_dir(&nested).unwrap();
        let left = nested.join("before.ts");
        let right = nested.join("after.ts");
        let other = repo.path().join("other.ts");
        for path in [&left, &right, &other] {
            fs::write(path, "content\n").unwrap();
        }
        let input = files(left.to_string_lossy(), right.to_string_lossy());
        let repo_bounds = bounds(&input, None, repo.path());
        validate_session_reload_within_bounds(&repo_bounds, &show("HEAD"), None).unwrap();
        validate_session_reload_within_bounds(
            &repo_bounds,
            &files(left.to_string_lossy(), other.to_string_lossy()),
            None,
        )
        .unwrap();
    }

    #[cfg(unix)]
    fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::windows::fs::symlink_dir(target, link)
    }

    #[test]
    fn existing_and_missing_leaf_symlink_escapes_are_rejected() {
        let repo = repository();
        let outside = tempdir().unwrap();
        fs::write(repo.path().join("safe.ts"), "safe\n").unwrap();
        fs::write(outside.path().join("secret.ts"), "secret\n").unwrap();
        let link = repo.path().join("outside-link");
        if symlink_dir(outside.path(), &link).is_err() {
            return;
        }
        let bounds = bounds(&vcs(), Some(repo.path()), repo.path());
        for left in [link.join("secret.ts"), link.join("missing.ts")] {
            assert!(matches!(
                validate_session_reload_within_bounds(
                    &bounds,
                    &files(
                        left.to_string_lossy(),
                        repo.path().join("safe.ts").to_string_lossy()
                    ),
                    None,
                ),
                Err(SessionReloadBoundsError::OutsideRoot {
                    description: "left file",
                    ..
                })
            ));
        }
    }

    #[test]
    fn patch_file_inside_repo_uses_root_while_standalone_allows_only_exact_initial_file() {
        let repo = repository();
        let outside = tempdir().unwrap();
        let initial = repo.path().join("changes.patch");
        let other = repo.path().join("other.patch");
        let secret = outside.path().join("secret.patch");
        for path in [&initial, &other, &secret] {
            fs::write(path, "diff\n").unwrap();
        }
        let input = patch(Some(initial.to_string_lossy().into()), None);
        let repo_bounds = bounds(&input, None, repo.path());
        validate_session_reload_within_bounds(
            &repo_bounds,
            &patch(Some(other.to_string_lossy().into()), None),
            None,
        )
        .unwrap();
        assert!(matches!(
            validate_session_reload_within_bounds(
                &repo_bounds,
                &patch(Some(secret.to_string_lossy().into()), None),
                None,
            ),
            Err(SessionReloadBoundsError::OutsideRoot {
                description: "patch file",
                ..
            })
        ));

        let standalone = tempdir().unwrap();
        let initial = standalone.path().join("changes.patch");
        let other = standalone.path().join("other.patch");
        fs::write(&initial, "diff\n").unwrap();
        fs::write(&other, "diff\n").unwrap();
        let input = patch(Some(initial.to_string_lossy().into()), None);
        let standalone_bounds = bounds(&input, None, standalone.path());
        assert!(standalone_bounds.roots.is_empty());
        assert_eq!(
            standalone_bounds.exact_files,
            [canonical(&initial).unwrap()]
        );
        validate_session_reload_within_bounds(&standalone_bounds, &input, None).unwrap();
        assert!(matches!(
            validate_session_reload_within_bounds(
                &standalone_bounds,
                &patch(Some(other.to_string_lossy().into()), None),
                None,
            ),
            Err(SessionReloadBoundsError::OutsideRoot { .. })
        ));
        assert!(matches!(
            validate_session_reload_within_bounds(&standalone_bounds, &vcs(), None),
            Err(SessionReloadBoundsError::RepositoryInputWithoutRoot)
        ));
    }

    #[test]
    fn patch_difftool_and_agent_context_cannot_escape_initial_root() {
        let repo = repository();
        let outside = tempdir().unwrap();
        let safe = repo.path().join("safe.ts");
        let secret = outside.path().join("secret.ts");
        let sidecar = outside.path().join("notes.json");
        for path in [&safe, &secret, &sidecar] {
            fs::write(path, "content\n").unwrap();
        }
        let bounds = bounds(&vcs(), Some(repo.path()), repo.path());
        assert!(
            validate_session_reload_within_bounds(
                &bounds,
                &patch(Some(secret.to_string_lossy().into()), None),
                None,
            )
            .is_err()
        );
        let difftool = DaemonCliInput::Difftool {
            left: secret.to_string_lossy().into(),
            right: safe.to_string_lossy().into(),
            path: Some("safe.ts".into()),
            options: options(),
        };
        assert!(matches!(
            validate_session_reload_within_bounds(&bounds, &difftool, None),
            Err(SessionReloadBoundsError::OutsideRoot {
                description: "left file",
                ..
            })
        ));
        let mut common = options();
        common.agent_context = Some(sidecar.to_string_lossy().into());
        let agent = DaemonCliInput::Vcs {
            range: None,
            range_endpoints: None,
            staged: false,
            pathspecs: None,
            options: common,
        };
        assert!(matches!(
            validate_session_reload_within_bounds(&bounds, &agent, None),
            Err(SessionReloadBoundsError::OutsideRoot {
                description: "agent context path",
                ..
            })
        ));
    }

    #[test]
    fn agent_context_symlink_and_stdin_backed_inputs_are_rejected() {
        let repo = repository();
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("notes.json"), "{}\n").unwrap();
        let link = repo.path().join("agent-link");
        if symlink_dir(outside.path(), &link).is_ok() {
            let bounds = bounds(&vcs(), Some(repo.path()), repo.path());
            let mut common = options();
            common.agent_context = Some(link.join("notes.json").to_string_lossy().into());
            let input = DaemonCliInput::Vcs {
                range: None,
                range_endpoints: None,
                staged: false,
                pathspecs: None,
                options: common,
            };
            assert!(validate_session_reload_within_bounds(&bounds, &input, None).is_err());
        }
        let bounds = bounds(&vcs(), Some(repo.path()), repo.path());
        assert!(matches!(
            validate_session_reload_within_bounds(&bounds, &patch(Some("-".into()), None), None),
            Err(SessionReloadBoundsError::StdinPatch)
        ));
        let mut common = options();
        common.agent_context = Some("-".into());
        let input = DaemonCliInput::Vcs {
            range: None,
            range_endpoints: None,
            staged: false,
            pathspecs: None,
            options: common,
        };
        assert!(matches!(
            validate_session_reload_within_bounds(&bounds, &input, None),
            Err(SessionReloadBoundsError::StdinAgentContext)
        ));
    }

    #[test]
    fn extension_aware_root_finder_can_supply_nonstandard_repository_roots() {
        let root = tempdir().unwrap();
        let nested = root.path().join("nested");
        fs::create_dir(&nested).unwrap();
        let left = nested.join("a");
        let right = nested.join("b");
        let input = files(left.to_string_lossy(), right.to_string_lossy());
        let bounds = create_session_reload_bounds_with_root_finder(&input, None, &nested, |_| {
            Some(root.path().to_path_buf())
        })
        .unwrap();
        assert_eq!(bounds.roots, [canonical(root.path()).unwrap()]);
    }
}
