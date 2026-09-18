//! Direct filesystem comparison shared by `workdeck diff --files` and Git difftool mode.

use std::fs;
use std::path::{Path, PathBuf};

use workdeck_core::{
    Changeset, ChangesetSource, DiffFile, FileChangeKind, FileFlags, FileSourceSnapshots,
    FileStats, SourceOrigin, SourceSnapshot,
};
use workdeck_diff::{
    FileComparisonOptions, FileSnapshot, create_two_files_patch, diff_from_file_snapshots,
};

use crate::{VcsError, untracked::is_probably_binary};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComparisonKind {
    Files,
    DiffTool,
}

/// Compare the exact paths selected by `workdeck diff --files`.
pub fn load_file_comparison(cwd: &Path, left: &Path, right: &Path) -> Result<Changeset, VcsError> {
    load_comparison(cwd, left, right, None, ComparisonKind::Files)
}

/// Compare the temporary pair supplied by Git while displaying the repository path.
pub fn load_difftool_comparison(
    cwd: &Path,
    left: &Path,
    right: &Path,
    display_path: Option<&Path>,
) -> Result<Changeset, VcsError> {
    load_comparison(cwd, left, right, display_path, ComparisonKind::DiffTool)
}

fn load_comparison(
    cwd: &Path,
    left: &Path,
    right: &Path,
    requested_display_path: Option<&Path>,
    kind: ComparisonKind,
) -> Result<Changeset, VcsError> {
    let left_bytes = read_comparison_file(&absolute_or_join(cwd, left))?;
    let right_bytes = read_comparison_file(&absolute_or_join(cwd, right))?;
    comparison_from_bytes(
        cwd,
        left,
        right,
        requested_display_path,
        kind,
        &left_bytes,
        &right_bytes,
    )
}

/// Build a comparison from bytes already read by a source-bound adapter. The
/// paths identify the snapshots; this function never opens them again.
pub fn load_file_comparison_from_bytes(
    cwd: &Path,
    left: &Path,
    right: &Path,
    left_bytes: &[u8],
    right_bytes: &[u8],
) -> Result<Changeset, VcsError> {
    comparison_from_bytes(
        cwd,
        left,
        right,
        None,
        ComparisonKind::Files,
        left_bytes,
        right_bytes,
    )
}

fn comparison_from_bytes(
    cwd: &Path,
    left: &Path,
    right: &Path,
    requested_display_path: Option<&Path>,
    kind: ComparisonKind,
    left_bytes: &[u8],
    right_bytes: &[u8],
) -> Result<Changeset, VcsError> {
    let left_path = absolute_or_join(cwd, left);
    let right_path = absolute_or_join(cwd, right);
    let display_path = match (kind, requested_display_path) {
        (ComparisonKind::DiffTool, Some(path)) => path.to_string_lossy().into_owned(),
        _ => display_basename(&right.to_string_lossy()),
    };
    let source_label = match kind {
        ComparisonKind::Files => "file compare",
        ComparisonKind::DiffTool => "git difftool",
    };
    let title = match kind {
        ComparisonKind::DiffTool => format!("git difftool: {display_path}"),
        ComparisonKind::Files if left == right => display_path.clone(),
        ComparisonKind::Files => format!(
            "{} ↔ {}",
            display_basename(&left.to_string_lossy()),
            display_basename(&right.to_string_lossy())
        ),
    };

    let binary = is_probably_binary(left_bytes) || is_probably_binary(right_bytes);
    let left_text = String::from_utf8_lossy(left_bytes).into_owned();
    let right_text = String::from_utf8_lossy(right_bytes).into_owned();
    let previous_path = display_basename(&left.to_string_lossy());
    let mut file = if binary {
        binary_file(
            &display_path,
            &previous_path,
            &format!(
                "Binary file skipped: {} ↔ {}\n",
                display_basename(&left.to_string_lossy()),
                display_basename(&right.to_string_lossy())
            ),
            binary_change_kind(&left_path, &right_path),
        )
    } else if left_text == right_text {
        empty_text_file(&display_path, &previous_path)
    } else {
        let left_cache_key = format!("{}:left", left_path.display());
        let right_cache_key = format!("{}:right", right_path.display());
        let mut file = diff_from_file_snapshots(
            FileSnapshot {
                cache_key: &left_cache_key,
                contents: &left_text,
                name: &display_path,
            },
            FileSnapshot {
                cache_key: &right_cache_key,
                contents: &right_text,
                name: &display_path,
            },
            FileComparisonOptions { context_radius: 3 },
        )?;
        file.previous_path = Some(previous_path.clone());
        file.patch = create_two_files_patch(&display_path, &left_text, &right_text, 3);
        file
    };

    file.runtime_id = format!("{display_path}:0:{display_path}");
    if !binary {
        file.set_sources(FileSourceSnapshots {
            old: Some(SourceSnapshot::new(
                left_text,
                SourceOrigin::File {
                    path: left_path.display().to_string(),
                },
                true,
            )),
            new: Some(SourceSnapshot::new(
                right_text,
                SourceOrigin::File {
                    path: right_path.display().to_string(),
                },
                true,
            )),
        });
    }
    file.refresh_identity();
    file.refresh_address(source_label, 0);

    Ok(Changeset {
        id: format!("pair:{display_path}"),
        source_label: source_label.into(),
        title,
        summary: None,
        agent_summary: None,
        source: ChangesetSource::Files {
            left: left_path.display().to_string(),
            right: right_path.display().to_string(),
        },
        files: vec![file],
    })
}

fn read_comparison_file(path: &Path) -> Result<Vec<u8>, VcsError> {
    fs::read(path).map_err(|source| VcsError::ReadFile {
        path: path.to_owned(),
        source,
    })
}

fn absolute_or_join(cwd: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        cwd.join(path)
    }
}

/// Return the final segment using either POSIX or Windows separators.
pub fn display_basename(path: &str) -> String {
    path.split(['/', '\\'])
        .rfind(|part| !part.is_empty())
        .unwrap_or(path)
        .to_owned()
}

fn binary_change_kind(left: &Path, right: &Path) -> FileChangeKind {
    if left == Path::new("/dev/null") {
        FileChangeKind::Added
    } else if right == Path::new("/dev/null") {
        FileChangeKind::Deleted
    } else {
        FileChangeKind::Modified
    }
}

fn binary_file(
    display_path: &str,
    previous_path: &str,
    patch: &str,
    change_kind: FileChangeKind,
) -> DiffFile {
    let mut file = empty_file(display_path, previous_path, change_kind);
    file.patch = patch.into();
    file.flags.binary = true;
    file.refresh_identity();
    file
}

fn empty_text_file(display_path: &str, previous_path: &str) -> DiffFile {
    let mut file = empty_file(display_path, previous_path, FileChangeKind::Modified);
    file.patch = create_two_files_patch(display_path, "", "", 3);
    file.refresh_identity();
    file
}

fn empty_file(display_path: &str, previous_path: &str, change_kind: FileChangeKind) -> DiffFile {
    DiffFile {
        key: String::new(),
        runtime_id: String::new(),
        path: display_path.into(),
        previous_path: Some(previous_path.into()),
        change_kind,
        language: None,
        stats: FileStats::default(),
        flags: FileFlags::default(),
        patch: String::new(),
        split_row_count: 0,
        stack_row_count: 0,
        hunks: Vec::new(),
        content_identity: String::new(),
        sources: FileSourceSnapshots::default(),
        source_identity: None,
        source_capability: None,
        source_attested: false,
        agent: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn direct_files_use_hunk_identity_title_sources_and_context() {
        let directory = TempDir::new().unwrap();
        fs::create_dir(directory.path().join("nested")).unwrap();
        fs::write(
            directory.path().join("before.rs"),
            "zero\none\ntwo\nthree\nfour\n",
        )
        .unwrap();
        fs::write(
            directory.path().join("nested/after.rs"),
            "zero\none\nchanged\nthree\nfour\n",
        )
        .unwrap();

        let changeset = load_file_comparison(
            directory.path(),
            Path::new("before.rs"),
            Path::new("nested/after.rs"),
        )
        .unwrap();

        assert_eq!(changeset.id, "pair:after.rs");
        assert_eq!(changeset.source_label, "file compare");
        assert_eq!(changeset.title, "before.rs ↔ after.rs");
        assert_eq!(changeset.files[0].path, "after.rs");
        assert_eq!(
            changeset.files[0].previous_path.as_deref(),
            Some("before.rs")
        );
        assert_eq!(changeset.files[0].runtime_id, "after.rs:0:after.rs");
        assert_eq!(changeset.files[0].hunks.len(), 1);
        assert_eq!(changeset.files[0].hunks[0].old_start, 1);
        assert_eq!(changeset.files[0].hunks[0].new_start, 1);
        assert!(changeset.files[0].source_attested);
        assert_eq!(
            changeset.files[0].sources.old.as_ref().unwrap().content,
            "zero\none\ntwo\nthree\nfour\n"
        );
    }

    #[test]
    fn difftool_uses_display_path_and_git_label() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("left.tmp"), "before\n").unwrap();
        fs::write(directory.path().join("right.tmp"), "after\n").unwrap();

        let changeset = load_difftool_comparison(
            directory.path(),
            Path::new("left.tmp"),
            Path::new("right.tmp"),
            Some(Path::new("src/main.rs")),
        )
        .unwrap();

        assert_eq!(changeset.id, "pair:src/main.rs");
        assert_eq!(changeset.source_label, "git difftool");
        assert_eq!(changeset.title, "git difftool: src/main.rs");
        assert_eq!(changeset.files[0].path, "src/main.rs");
        assert_eq!(
            changeset.files[0].previous_path.as_deref(),
            Some("left.tmp")
        );
    }

    #[test]
    fn binary_pairs_are_sniffed_before_text_loading_and_have_no_sources() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("left.bin"), [0, 1, 2, 3]).unwrap();
        fs::write(directory.path().join("right.bin"), [0, 1, 2, 4]).unwrap();

        let changeset = load_file_comparison(
            directory.path(),
            Path::new("left.bin"),
            Path::new("right.bin"),
        )
        .unwrap();

        let file = &changeset.files[0];
        assert!(file.flags.binary);
        assert!(file.hunks.is_empty());
        assert_eq!(file.patch, "Binary file skipped: left.bin ↔ right.bin\n");
        assert_eq!(file.sources, FileSourceSnapshots::default());
        assert!(!file.source_attested);
    }

    #[test]
    fn identical_paths_keep_one_empty_review_file_and_short_title() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("same.txt"), "same\n").unwrap();

        let changeset = load_file_comparison(
            directory.path(),
            Path::new("same.txt"),
            Path::new("same.txt"),
        )
        .unwrap();

        assert_eq!(changeset.title, "same.txt");
        assert_eq!(changeset.files.len(), 1);
        assert!(changeset.files[0].hunks.is_empty());
    }

    #[test]
    fn display_basename_accepts_both_platform_separators() {
        assert_eq!(display_basename("dir/sub/file.rs"), "file.rs");
        assert_eq!(display_basename(r"dir\sub\file.rs"), "file.rs");
        assert_eq!(display_basename("/"), "/");
    }

    fn oracle_projection(changeset: &Changeset) -> serde_json::Value {
        let file = &changeset.files[0];
        serde_json::json!({
            "id": changeset.id,
            "sourceLabel": changeset.source_label,
            "title": changeset.title,
            "file": {
                "id": file.runtime_id,
                "path": file.path,
                "previousPath": file.previous_path,
                "type": match file.change_kind {
                    FileChangeKind::Added => "new",
                    FileChangeKind::Deleted => "deleted",
                    _ => "change",
                },
                "additions": file.stats.additions,
                "deletions": file.stats.deletions,
                "isBinary": file.flags.binary,
                "hunkCount": file.hunks.len(),
                "splitLineCount": file.split_row_count,
                "unifiedLineCount": file.stack_row_count,
                "patch": file.patch,
                "oldSource": file.sources.old.as_ref().map(|source| &source.content),
                "newSource": file.sources.new.as_ref().map(|source| &source.content),
            },
        })
    }

    #[test]
    fn native_direct_loader_matches_both_pinned_hunk_oracles() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/loader-bootstrap.json"
        ))
        .unwrap();
        let expected = &oracle["direct_cases"];
        let directory = TempDir::new().unwrap();
        fs::create_dir(directory.path().join("nested")).unwrap();
        fs::write(
            directory.path().join("before.ts"),
            "zero\none\ntwo\nthree\nfour\n",
        )
        .unwrap();
        fs::write(
            directory.path().join("nested/after.ts"),
            "zero\none\nchanged\nthree\nfour\n",
        )
        .unwrap();
        fs::write(directory.path().join("left.tmp"), "before\n").unwrap();
        fs::write(directory.path().join("right.tmp"), "after\n").unwrap();
        fs::write(directory.path().join("left.bin"), [0, 1, 2, 3]).unwrap();
        fs::write(directory.path().join("right.bin"), [0, 1, 2, 4]).unwrap();
        fs::write(directory.path().join("same.txt"), "same\n").unwrap();

        let changed = load_file_comparison(
            directory.path(),
            Path::new("before.ts"),
            Path::new("nested/after.ts"),
        )
        .unwrap();
        let difftool = load_difftool_comparison(
            directory.path(),
            Path::new("left.tmp"),
            Path::new("right.tmp"),
            Some(Path::new("src/main.rs")),
        )
        .unwrap();
        let binary = load_file_comparison(
            directory.path(),
            Path::new("left.bin"),
            Path::new("right.bin"),
        )
        .unwrap();
        let identical = load_file_comparison(
            directory.path(),
            Path::new("same.txt"),
            Path::new("same.txt"),
        )
        .unwrap();

        assert_eq!(oracle_projection(&changed), expected["changed"]);
        assert_eq!(oracle_projection(&difftool), expected["difftool"]);
        assert_eq!(oracle_projection(&binary), expected["binary"]);
        assert_eq!(oracle_projection(&identical), expected["identical"]);
    }
}
