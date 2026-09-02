//! Host policy and filesystem guard for native-extension document access.
//!
//! This is a Rust translation of Hunk's MIT-licensed
//! `extensionWorkspace.ts` and `workspaceWriteGuard.ts`. The pure policy is the
//! only place that decides whether a reviewed file may be replaced. The
//! filesystem guard then verifies that the lexical target is still a regular
//! file inside the link-resolved review root, both before consent and before
//! the write.

use std::fs;
use std::path::{Component, Path, PathBuf};

use serde_json::Value;
use workdeck_core::{CliInput, DiffFile, FileChangeKind};
use workdeck_diff::normalize_diff_path;
use workdeck_extension_api::{
    ExtensionFileSide, ExtensionWorkspaceDocument, ExtensionWorkspaceSnapshot,
};

use crate::can_reload_cli_input;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceWriteRequestFields {
    pub file_id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionWorkspaceWriteTarget {
    Writable {
        path: String,
        absolute_path: PathBuf,
    },
    Unavailable {
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceWriteFailure {
    Unavailable(String),
    Failed(String),
}

impl WorkspaceWriteFailure {
    #[must_use]
    pub fn into_extension_result(self) -> workdeck_extension_api::ExtensionWorkspaceWriteResult {
        match self {
            Self::Unavailable(detail) => {
                workdeck_extension_api::ExtensionWorkspaceWriteResult::Unavailable { detail }
            }
            Self::Failed(detail) => {
                workdeck_extension_api::ExtensionWorkspaceWriteResult::Failed { detail }
            }
        }
    }
}

impl ExtensionWorkspaceWriteTarget {
    #[must_use]
    pub const fn writable(&self) -> bool {
        matches!(self, Self::Writable { .. })
    }

    #[must_use]
    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::Writable { .. } => None,
            Self::Unavailable { detail } => Some(detail),
        }
    }
}

/// Reject malformed extension input before it can become a user-facing refusal.
pub fn normalize_workspace_write_request(
    request: &Value,
) -> Result<WorkspaceWriteRequestFields, String> {
    let file_id = request
        .get("fileId")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "workspace.writeDocument requires a non-empty fileId.".to_owned())?;
    let text = request
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| "workspace.writeDocument requires text to be a string.".to_owned())?;
    Ok(WorkspaceWriteRequestFields {
        file_id: file_id.to_owned(),
        text: text.to_owned(),
    })
}

fn non_working_tree_review(input: &CliInput) -> Option<&'static str> {
    match input {
        CliInput::Vcs(input) if input.range.is_some() || input.range_endpoints.is_some() => {
            Some("a revision range")
        }
        CliInput::Vcs(input) if input.staged => Some("staged changes"),
        CliInput::Vcs(_) => None,
        CliInput::Show(_) => Some("a single revision"),
        CliInput::StashShow(_) => Some("a stash entry"),
        CliInput::Patch(_) => Some("patch input"),
        CliInput::Files(_) | CliInput::DiffTool(_) => Some("a file comparison"),
    }
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut output = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                output.pop();
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                output.push(component.as_os_str());
            }
        }
    }
    output
}

fn lexical_resolve(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        lexical_normalize(path)
    } else {
        lexical_normalize(&root.join(path))
    }
}

/// Whether `candidate` is the root itself or one of its descendants.
#[must_use]
pub fn is_within_root(root: &Path, candidate: &Path) -> bool {
    candidate.strip_prefix(root).is_ok()
}

/// Resolve a reviewed file into the only working-tree path an extension may replace.
#[must_use]
pub fn resolve_extension_workspace_write_target(
    file_id: &str,
    files: &[DiffFile],
    input: &CliInput,
    root: &Path,
) -> ExtensionWorkspaceWriteTarget {
    if let Some(reviewing) = non_working_tree_review(input) {
        return ExtensionWorkspaceWriteTarget::Unavailable {
            detail: format!(
                "Workspace writes are working-tree only; this session is reviewing {reviewing}."
            ),
        };
    }
    if !can_reload_cli_input(input) {
        return ExtensionWorkspaceWriteTarget::Unavailable {
            detail: "Workspace writes need a session that can reload; this one cannot, because part of its input came from stdin (--agent-context -).".into(),
        };
    }
    let Some(file) = files
        .iter()
        .find(|candidate| candidate.runtime_id == file_id)
    else {
        return ExtensionWorkspaceWriteTarget::Unavailable {
            detail: format!("No reviewed file has the id \"{file_id}\"."),
        };
    };
    let path = normalize_diff_path(Some(&file.path)).unwrap_or_else(|| file.path.clone());
    if file.change_kind == FileChangeKind::Deleted {
        return ExtensionWorkspaceWriteTarget::Unavailable {
            detail: format!("{path} was deleted in this review; it has no new side."),
        };
    }
    if file.flags.binary {
        return ExtensionWorkspaceWriteTarget::Unavailable {
            detail: format!("{path} is binary; workspace writes are text-only."),
        };
    }
    if file.flags.too_large {
        return ExtensionWorkspaceWriteTarget::Unavailable {
            detail: format!("{path} was skipped as too large to load."),
        };
    }
    let absolute_root = if root.is_absolute() {
        lexical_normalize(root)
    } else {
        std::env::current_dir()
            .map(|cwd| lexical_normalize(&cwd.join(root)))
            .unwrap_or_else(|_| lexical_normalize(root))
    };
    let absolute_path = lexical_resolve(&absolute_root, Path::new(&path));
    if absolute_path == absolute_root || !is_within_root(&absolute_root, &absolute_path) {
        return ExtensionWorkspaceWriteTarget::Unavailable {
            detail: format!("{path} resolves outside the reviewed repository."),
        };
    }
    ExtensionWorkspaceWriteTarget::Writable {
        path,
        absolute_path,
    }
}

/// Resolve one immutable reviewed source side, independent of write policy.
#[must_use]
pub fn resolve_extension_workspace_read<'a>(
    file_id: &str,
    files: &'a [DiffFile],
    side: ExtensionFileSide,
) -> Option<&'a str> {
    let file = files
        .iter()
        .find(|candidate| candidate.runtime_id == file_id)?;
    let snapshot = match side {
        ExtensionFileSide::Old => file.sources.old.as_ref(),
        ExtensionFileSide::New => file.sources.new.as_ref(),
    }?;
    Some(&snapshot.content)
}

/// Freeze Hunk's command-context workspace reads and optimistic write probes for native RPC.
#[must_use]
pub fn build_extension_workspace_snapshot(
    files: &[DiffFile],
    input: &CliInput,
    root: &Path,
    review_generation: u64,
) -> ExtensionWorkspaceSnapshot {
    let documents = files
        .iter()
        .map(|file| {
            let target =
                resolve_extension_workspace_write_target(&file.runtime_id, files, input, root);
            ExtensionWorkspaceDocument {
                file_id: file.runtime_id.clone(),
                path: normalize_diff_path(Some(&file.path)).unwrap_or_else(|| file.path.clone()),
                old: file
                    .sources
                    .old
                    .as_ref()
                    .map(|source| source.content.clone()),
                new: file
                    .sources
                    .new
                    .as_ref()
                    .map(|source| source.content.clone()),
                writable: target.writable(),
                unavailable_detail: target.detail().map(str::to_owned),
            }
        })
        .collect();
    ExtensionWorkspaceSnapshot {
        review_generation,
        documents,
    }
}

/// Verify an already-resolved target against the current filesystem state.
#[must_use]
pub fn verify_workspace_write_target(
    absolute_path: &Path,
    path: &str,
    root: &Path,
) -> Option<String> {
    let target = match fs::symlink_metadata(absolute_path) {
        Ok(target) => target,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Some(format!(
                "{path} is no longer in the working tree; workspace writes replace reviewed files rather than recreate them."
            ));
        }
        Err(error) => {
            return Some(format!(
                "{path} could not be checked before writing • {error}"
            ));
        }
    };
    if target.file_type().is_symlink() {
        return Some(format!(
            "{path} is a symlink; workspace writes refuse to follow links."
        ));
    }
    if !target.is_file() {
        return Some(format!("{path} is not a regular file."));
    }
    let real_target_parent = match absolute_path.parent().map(fs::canonicalize) {
        Some(Ok(parent)) => parent,
        Some(Err(error)) => {
            return Some(format!(
                "{path} could not be resolved inside the reviewed repository • {error}"
            ));
        }
        None => {
            return Some(format!(
                "{path} could not be resolved inside the reviewed repository • target has no parent"
            ));
        }
    };
    let real_root = match fs::canonicalize(root) {
        Ok(root) => root,
        Err(error) => {
            return Some(format!(
                "{path} could not be resolved inside the reviewed repository • {error}"
            ));
        }
    };
    if !is_within_root(&real_root, &real_target_parent) {
        return Some(format!(
            "{path} sits outside the reviewed repository once links resolve."
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;
    use workdeck_core::{
        CliInput, CommonOptions, DiffFile, DiffToolCommandInput, FileChangeKind, FileCommandInput,
        FileFlags, FileSourceSnapshots, FileStats, PatchCommandInput, SourceOrigin, SourceSnapshot,
        VcsDiffCommandInput, VcsRangeEndpoints, VcsShowCommandInput, VcsStashShowCommandInput,
    };

    use super::*;

    fn file(path: &str) -> DiffFile {
        DiffFile {
            key: "alpha-key".into(),
            runtime_id: "alpha".into(),
            path: path.into(),
            previous_path: None,
            change_kind: FileChangeKind::Modified,
            language: None,
            stats: FileStats::default(),
            flags: FileFlags::default(),
            patch: String::new(),
            split_row_count: 0,
            stack_row_count: 0,
            hunks: Vec::new(),
            content_identity: "content".into(),
            sources: FileSourceSnapshots {
                old: Some(SourceSnapshot::new(
                    "old text".into(),
                    SourceOrigin::Revision {
                        revision: "HEAD".into(),
                    },
                    true,
                )),
                new: Some(SourceSnapshot::new(
                    "new text".into(),
                    SourceOrigin::WorkingTree,
                    true,
                )),
            },
            source_identity: None,
            source_attested: true,
            agent: None,
        }
    }

    fn working_tree(options: CommonOptions) -> CliInput {
        CliInput::Vcs(VcsDiffCommandInput {
            range: None,
            range_endpoints: None,
            staged: false,
            pathspecs: Vec::new(),
            options,
        })
    }

    fn unavailable(target: ExtensionWorkspaceWriteTarget) -> String {
        match target {
            ExtensionWorkspaceWriteTarget::Unavailable { detail } => detail,
            ExtensionWorkspaceWriteTarget::Writable { .. } => panic!("target was writable"),
        }
    }

    fn target_value(target: ExtensionWorkspaceWriteTarget) -> Value {
        match target {
            ExtensionWorkspaceWriteTarget::Writable {
                path,
                absolute_path,
            } => serde_json::json!({
                "writable": true,
                "path": path,
                "absolutePath": absolute_path,
            }),
            ExtensionWorkspaceWriteTarget::Unavailable { detail } => {
                serde_json::json!({ "writable": false, "detail": detail })
            }
        }
    }

    fn test_root() -> PathBuf {
        std::env::current_dir()
            .unwrap()
            .join("workspace-policy-root")
    }

    #[cfg(unix)]
    #[test]
    fn matches_frozen_hunk_workspace_policy_oracle() {
        let oracle: Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/extension-workspace.json"
        ))
        .unwrap();
        let expected = oracle["cases"].as_object().unwrap();
        let root = Path::new("/repo");
        let default_input = working_tree(CommonOptions::default());
        let resolve = |file_id: &str, files: Vec<DiffFile>, input: &CliInput| {
            target_value(resolve_extension_workspace_write_target(
                file_id, &files, input, root,
            ))
        };
        let mut range = match default_input.clone() {
            CliInput::Vcs(input) => input,
            _ => unreachable!(),
        };
        range.range = Some("main..HEAD".into());
        let mut staged = range.clone();
        staged.range = None;
        staged.staged = true;
        let stdin_context = working_tree(CommonOptions {
            agent_context: Some("-".into()),
            ..CommonOptions::default()
        });
        let mut deleted = file("src/alpha.ts");
        deleted.change_kind = FileChangeKind::Deleted;
        let mut binary = file("src/alpha.ts");
        binary.flags.binary = true;
        let mut large = file("src/alpha.ts");
        large.flags.too_large = true;
        let actual = serde_json::json!({
            "writable": resolve("alpha", vec![file("src/alpha.ts")], &default_input),
            "range": resolve("alpha", vec![file("src/alpha.ts")], &CliInput::Vcs(range)),
            "staged": resolve("alpha", vec![file("src/alpha.ts")], &CliInput::Vcs(staged)),
            "stdinContext": resolve("alpha", vec![file("src/alpha.ts")], &stdin_context),
            "missing": resolve("missing", vec![file("src/alpha.ts")], &default_input),
            "deleted": resolve("alpha", vec![deleted], &default_input),
            "binary": resolve("alpha", vec![binary], &default_input),
            "large": resolve("alpha", vec![large], &default_input),
            "escape": resolve("alpha", vec![file("../outside/secret.ts")], &default_input),
            "root": resolve("alpha", vec![file(".")], &default_input),
            "crlf": resolve("alpha", vec![file("src/alpha.ts\r")], &default_input),
        });
        assert_eq!(actual.as_object().unwrap(), expected);
    }

    #[test]
    fn resolves_plain_working_tree_files_and_normalizes_diff_suffixes() {
        let root = test_root();
        assert_eq!(
            resolve_extension_workspace_write_target(
                "alpha",
                &[file("src/alpha.rs\r")],
                &working_tree(CommonOptions::default()),
                &root,
            ),
            ExtensionWorkspaceWriteTarget::Writable {
                path: "src/alpha.rs".into(),
                absolute_path: root.join("src/alpha.rs"),
            }
        );
        assert!(
            resolve_extension_workspace_write_target(
                "alpha",
                &[file("..config/alpha.rs")],
                &working_tree(CommonOptions::default()),
                &root,
            )
            .writable()
        );
    }

    #[test]
    fn refuses_every_non_working_tree_review_with_specific_context() {
        let options = CommonOptions::default();
        let cases = [
            (
                CliInput::Vcs(VcsDiffCommandInput {
                    range: Some("main..HEAD".into()),
                    range_endpoints: None,
                    staged: false,
                    pathspecs: Vec::new(),
                    options: options.clone(),
                }),
                "a revision range",
            ),
            (
                CliInput::Vcs(VcsDiffCommandInput {
                    range: None,
                    range_endpoints: Some(VcsRangeEndpoints {
                        from: "main".into(),
                        to: "feature".into(),
                    }),
                    staged: false,
                    pathspecs: Vec::new(),
                    options: options.clone(),
                }),
                "a revision range",
            ),
            (
                CliInput::Vcs(VcsDiffCommandInput {
                    range: None,
                    range_endpoints: None,
                    staged: true,
                    pathspecs: Vec::new(),
                    options: options.clone(),
                }),
                "staged changes",
            ),
            (
                CliInput::Show(VcsShowCommandInput {
                    reference: Some("HEAD".into()),
                    pathspecs: Vec::new(),
                    options: options.clone(),
                }),
                "a single revision",
            ),
            (
                CliInput::StashShow(VcsStashShowCommandInput {
                    reference: None,
                    options: options.clone(),
                }),
                "a stash entry",
            ),
            (
                CliInput::Patch(PatchCommandInput {
                    file: Some("change.patch".into()),
                    text: None,
                    options: options.clone(),
                }),
                "patch input",
            ),
            (
                CliInput::Files(FileCommandInput {
                    left: "before.rs".into(),
                    right: "after.rs".into(),
                    options: options.clone(),
                }),
                "a file comparison",
            ),
            (
                CliInput::DiffTool(DiffToolCommandInput {
                    left: "before.rs".into(),
                    right: "after.rs".into(),
                    path: None,
                    options,
                }),
                "a file comparison",
            ),
        ];
        for (input, expected) in cases {
            let detail = unavailable(resolve_extension_workspace_write_target(
                "alpha",
                &[file("alpha.rs")],
                &input,
                &test_root(),
            ));
            assert!(detail.contains("working-tree only"));
            assert!(detail.contains(expected));
        }
    }

    #[test]
    fn refuses_non_reloadable_missing_deleted_binary_large_and_escaping_targets() {
        let stdin_options = CommonOptions {
            agent_context: Some("-".into()),
            ..CommonOptions::default()
        };
        assert!(
            unavailable(resolve_extension_workspace_write_target(
                "alpha",
                &[file("alpha.rs")],
                &working_tree(stdin_options),
                &test_root(),
            ))
            .contains("session that can reload")
        );
        assert!(
            unavailable(resolve_extension_workspace_write_target(
                "missing",
                &[file("alpha.rs")],
                &working_tree(CommonOptions::default()),
                &test_root(),
            ))
            .contains("No reviewed file")
        );

        let mut deleted = file("deleted.rs");
        deleted.change_kind = FileChangeKind::Deleted;
        let mut binary = file("binary.dat");
        binary.flags.binary = true;
        let mut large = file("large.rs");
        large.flags.too_large = true;
        for (candidate, expected) in [
            (deleted, "was deleted"),
            (binary, "is binary"),
            (large, "too large"),
        ] {
            assert!(
                unavailable(resolve_extension_workspace_write_target(
                    "alpha",
                    &[candidate],
                    &working_tree(CommonOptions::default()),
                    &test_root(),
                ))
                .contains(expected)
            );
        }
        for path in ["../outside/secret.rs", "."] {
            assert!(
                unavailable(resolve_extension_workspace_write_target(
                    "alpha",
                    &[file(path)],
                    &working_tree(CommonOptions::default()),
                    &test_root(),
                ))
                .contains("outside the reviewed repository")
            );
        }
        let file_options = CommonOptions {
            agent_context: Some("notes.json".into()),
            ..CommonOptions::default()
        };
        assert!(
            resolve_extension_workspace_write_target(
                "alpha",
                &[file("alpha.rs")],
                &working_tree(file_options),
                &test_root(),
            )
            .writable()
        );
    }

    #[test]
    fn reads_both_document_sides_without_applying_write_policy() {
        let mut reviewed = file("deleted.rs");
        reviewed.change_kind = FileChangeKind::Deleted;
        let files = [reviewed];
        assert_eq!(
            resolve_extension_workspace_read("alpha", &files, ExtensionFileSide::New),
            Some("new text")
        );
        assert_eq!(
            resolve_extension_workspace_read("alpha", &files, ExtensionFileSide::Old),
            Some("old text")
        );
        assert_eq!(
            resolve_extension_workspace_read("missing", &files, ExtensionFileSide::New),
            None
        );
        let mut without_source = file("missing-source.rs");
        without_source.sources = FileSourceSnapshots::default();
        assert_eq!(
            resolve_extension_workspace_read("alpha", &[without_source], ExtensionFileSide::New,),
            None
        );
        for invalid in [
            Value::Null,
            serde_json::json!("both"),
            serde_json::json!("New"),
        ] {
            assert!(serde_json::from_value::<ExtensionFileSide>(invalid).is_err());
        }
    }

    #[test]
    fn native_command_snapshot_exposes_all_reviewed_reads_and_write_probes() {
        let mut hidden = file("hidden.txt");
        hidden.runtime_id = "hidden".into();
        let mut deleted = file("deleted.txt");
        deleted.runtime_id = "deleted".into();
        deleted.change_kind = FileChangeKind::Deleted;
        let files = [file("alpha.txt"), hidden, deleted];
        let snapshot = build_extension_workspace_snapshot(
            &files,
            &working_tree(CommonOptions::default()),
            &test_root(),
            7,
        );
        assert_eq!(snapshot.review_generation, 7);
        assert_eq!(snapshot.documents.len(), 3);
        assert_eq!(
            snapshot.read_document("hidden", ExtensionFileSide::New),
            Some("new text")
        );
        assert!(snapshot.can_write_document("alpha"));
        assert!(!snapshot.can_write_document("deleted"));
        assert!(!snapshot.can_write_document("missing"));
        assert!(
            snapshot
                .documents
                .iter()
                .find(|document| document.file_id == "deleted")
                .and_then(|document| document.unavailable_detail.as_deref())
                .is_some_and(|detail| detail.contains("was deleted"))
        );
    }

    #[test]
    fn normalizes_only_well_formed_write_requests() {
        assert_eq!(
            normalize_workspace_write_request(&serde_json::json!({
                "fileId": "alpha",
                "text": ""
            })),
            Ok(WorkspaceWriteRequestFields {
                file_id: "alpha".into(),
                text: String::new(),
            })
        );
        for malformed in [
            Value::Null,
            serde_json::json!({ "text": "x" }),
            serde_json::json!({ "fileId": "", "text": "x" }),
        ] {
            assert!(
                normalize_workspace_write_request(&malformed)
                    .unwrap_err()
                    .contains("non-empty fileId")
            );
        }
        for malformed in [
            serde_json::json!({ "fileId": "alpha" }),
            serde_json::json!({ "fileId": "alpha", "text": 12 }),
        ] {
            assert!(
                normalize_workspace_write_request(&malformed)
                    .unwrap_err()
                    .contains("text to be a string")
            );
        }
    }

    fn verify(root: &Path, path: &str) -> Option<String> {
        verify_workspace_write_target(&root.join(path), path, root)
    }

    #[test]
    fn filesystem_guard_accepts_regular_files_and_refuses_missing_or_non_files() {
        let root = TempDir::new().unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        fs::write(root.path().join("src/alpha.rs"), "one\n").unwrap();
        assert_eq!(verify(root.path(), "src/alpha.rs"), None);
        assert!(
            verify(root.path(), "gone.rs")
                .unwrap()
                .contains("no longer in the working tree")
        );
        assert!(
            verify(root.path(), "src")
                .unwrap()
                .contains("not a regular file")
        );
    }

    #[cfg(unix)]
    #[test]
    fn filesystem_guard_handles_target_parent_and_root_links() {
        use std::os::unix::fs::symlink;

        let root = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        fs::write(outside.path().join("secret.txt"), "secret\n").unwrap();
        symlink(
            outside.path().join("secret.txt"),
            root.path().join("linked.txt"),
        )
        .unwrap();
        assert!(
            verify(root.path(), "linked.txt")
                .unwrap()
                .contains("is a symlink")
        );
        fs::write(root.path().join("real.txt"), "one\n").unwrap();
        symlink(root.path().join("real.txt"), root.path().join("alias.txt")).unwrap();
        assert!(
            verify(root.path(), "alias.txt")
                .unwrap()
                .contains("is a symlink")
        );

        symlink(outside.path(), root.path().join("vendor")).unwrap();
        assert!(
            verify(root.path(), "vendor/secret.txt")
                .unwrap()
                .contains("outside the reviewed repository once links resolve")
        );

        fs::create_dir(root.path().join("packages")).unwrap();
        fs::write(root.path().join("packages/alpha.rs"), "one\n").unwrap();
        symlink(root.path().join("packages"), root.path().join("inside")).unwrap();
        assert_eq!(verify(root.path(), "inside/alpha.rs"), None);

        let linked_parent = TempDir::new().unwrap();
        symlink(root.path(), linked_parent.path().join("root")).unwrap();
        fs::write(root.path().join("root-file.rs"), "one\n").unwrap();
        assert_eq!(
            verify(&linked_parent.path().join("root"), "root-file.rs"),
            None
        );
    }
}
