//! Filesystem-backed synthesis of untracked files into reviewable additions.

use std::fs;
use std::path::Path;

use workdeck_core::{ChangesetSource, DiffFile, FileSourceSnapshots, SourceOrigin, SourceSnapshot};
use workdeck_diff::parse_patch;

use crate::{BINARY_SNIFF_BYTES, VcsError, inspect_large_untracked_file};

/// Build one provider-neutral untracked file from its current filesystem representation.
pub fn build_filesystem_untracked_diff_file(
    repo_root: &Path,
    file_path: &Path,
    index: usize,
    source_prefix: &str,
) -> Result<DiffFile, VcsError> {
    let absolute_path = repo_root.join(file_path);
    let metadata = fs::symlink_metadata(&absolute_path).map_err(|source| VcsError::ReadFile {
        path: absolute_path.clone(),
        source,
    })?;
    let display_path = file_path.to_string_lossy();
    let safe_path = escape_untracked_patch_path(&display_path);

    if metadata.file_type().is_symlink() {
        let target = fs::read_link(&absolute_path).map_err(|source| VcsError::ReadFile {
            path: absolute_path,
            source,
        })?;
        let patch = build_untracked_patch_text(&safe_path, "120000", &target.to_string_lossy());
        return parsed_untracked_file(&patch, &display_path, index, source_prefix);
    }

    let large = inspect_large_untracked_file(repo_root, file_path);
    if large.should_skip {
        let transport = binary_transport_patch(&safe_path, "100644");
        let mut file = parsed_untracked_file(&transport, &display_path, index, source_prefix)?;
        file.patch.clear();
        file.hunks.clear();
        file.split_row_count = 0;
        file.stack_row_count = 0;
        file.flags.binary = false;
        file.flags.too_large = true;
        if let Some(stats) = large.stats {
            file.stats = stats;
        }
        file.refresh_identity();
        return Ok(file);
    }

    let contents = fs::read(&absolute_path).map_err(|source| VcsError::ReadFile {
        path: absolute_path.clone(),
        source,
    })?;
    let mode = regular_file_mode(&metadata);
    if is_probably_binary(&contents) {
        let transport = binary_transport_patch(&safe_path, mode);
        let mut file = parsed_untracked_file(&transport, &display_path, index, source_prefix)?;
        file.patch = format!("Binary file skipped: {display_path}\n");
        file.flags.binary = true;
        file.refresh_identity();
        return Ok(file);
    }

    let contents = String::from_utf8_lossy(&contents).into_owned();
    let patch = build_untracked_patch_text(&safe_path, mode, &contents);
    let mut file = parsed_untracked_file(&patch, &display_path, index, source_prefix)?;
    file.set_sources(FileSourceSnapshots {
        old: None,
        new: Some(SourceSnapshot::new(
            contents,
            SourceOrigin::WorkingTree,
            true,
        )),
    });
    Ok(file)
}

fn parsed_untracked_file(
    patch: &str,
    file_path: &str,
    index: usize,
    source_prefix: &str,
) -> Result<DiffFile, VcsError> {
    let mut changeset = parse_patch(
        patch,
        source_prefix,
        file_path,
        ChangesetSource::WorkingTree { staged: false },
    )?;
    let mut file = changeset
        .files
        .pop()
        .expect("a synthesized patch always contains one file");
    file.runtime_id = format!("{source_prefix}:{index}:{file_path}");
    file.flags.untracked = true;
    file.refresh_identity();
    Ok(file)
}

fn build_untracked_patch_text(safe_path: &str, mode: &str, contents: &str) -> String {
    let normalized = contents.replace("\r\n", "\n");
    let ends_with_newline = normalized.ends_with('\n');
    let mut lines = if normalized.is_empty() {
        Vec::new()
    } else {
        normalized.split('\n').collect::<Vec<_>>()
    };
    if ends_with_newline {
        lines.pop();
    }

    let old_path = quote_git_path(&format!("a/{safe_path}"));
    let new_path = quote_git_path(&format!("b/{safe_path}"));
    let mut patch = format!(
        "diff --git {old_path} {new_path}\nnew file mode {mode}\n--- /dev/null\t\n+++ {new_path}\n"
    );
    if !lines.is_empty() {
        patch.push_str(&format!("@@ -0,0 +1,{} @@\n", lines.len()));
        for line in lines {
            patch.push('+');
            patch.push_str(line);
            patch.push('\n');
        }
        if !ends_with_newline {
            patch.push_str("\\ No newline at end of file\n");
        }
    }
    patch
}

fn binary_transport_patch(safe_path: &str, mode: &str) -> String {
    let old_path = quote_git_path(&format!("a/{safe_path}"));
    let new_path = quote_git_path(&format!("b/{safe_path}"));
    format!(
        "diff --git {old_path} {new_path}\nnew file mode {mode}\nBinary files /dev/null and {new_path} differ\n"
    )
}

fn regular_file_mode(metadata: &fs::Metadata) -> &'static str {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 != 0 {
            return "100755";
        }
    }
    "100644"
}

pub(crate) fn is_probably_binary(contents: &[u8]) -> bool {
    let prefix = &contents[..contents.len().min(BINARY_SNIFF_BYTES)];
    if prefix.is_empty() {
        return false;
    }
    if prefix.contains(&0) {
        return true;
    }
    let signals = prefix
        .iter()
        .filter(|byte| **byte < 0x07 || (**byte > 0x0d && **byte < 0x20) || **byte == 0x7f)
        .count();
    signals * 10 >= prefix.len() * 3
}

fn escape_untracked_patch_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn quote_git_path(path: &str) -> String {
    if path
        .bytes()
        .all(|byte| !byte.is_ascii_whitespace() && byte != b'"' && byte != b'\\')
    {
        return path.to_owned();
    }
    let mut quoted = String::from("\"");
    for byte in path.as_bytes() {
        match byte {
            b'"' => quoted.push_str("\\\""),
            b'\\' => quoted.push_str("\\\\"),
            b'\t' => quoted.push_str("\\t"),
            b'\n' => quoted.push_str("\\n"),
            b'\r' => quoted.push_str("\\r"),
            0x20..=0x7e => quoted.push(char::from(*byte)),
            _ => quoted.push_str(&format!("\\{byte:03o}")),
        }
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn keeps_an_empty_file_as_a_zero_line_addition() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("empty.txt"), "").unwrap();

        let file = build_filesystem_untracked_diff_file(
            directory.path(),
            Path::new("empty.txt"),
            0,
            "test",
        )
        .unwrap();

        assert!(file.hunks.is_empty());
        assert_eq!(
            file.patch,
            "diff --git a/empty.txt b/empty.txt\nnew file mode 100644\n--- /dev/null\t\n+++ b/empty.txt\n"
        );
    }

    #[test]
    fn normalizes_crlf_and_marks_only_a_missing_final_newline() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("notes.txt");
        fs::write(&path, "first\r\nsecond").unwrap();
        let unterminated = build_filesystem_untracked_diff_file(
            directory.path(),
            Path::new("notes.txt"),
            0,
            "test",
        )
        .unwrap();
        assert!(
            unterminated
                .patch
                .contains("@@ -0,0 +1,2 @@\n+first\n+second\n")
        );
        assert!(
            unterminated
                .patch
                .ends_with("\\ No newline at end of file\n")
        );
        assert!(!unterminated.patch.contains('\r'));

        fs::write(&path, "first\r\nsecond\r\n").unwrap();
        let terminated = build_filesystem_untracked_diff_file(
            directory.path(),
            Path::new("notes.txt"),
            0,
            "test",
        )
        .unwrap();
        assert!(
            terminated
                .patch
                .contains("@@ -0,0 +1,2 @@\n+first\n+second\n")
        );
        assert!(!terminated.patch.contains("No newline at end of file"));
        assert!(!terminated.patch.contains('\r'));
    }

    #[cfg(unix)]
    #[test]
    fn preserves_executable_mode_and_diffs_symlink_targets_without_dereferencing() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let directory = TempDir::new().unwrap();
        let executable = directory.path().join("run.sh");
        fs::write(&executable, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        let executable_file =
            build_filesystem_untracked_diff_file(directory.path(), Path::new("run.sh"), 0, "test")
                .unwrap();
        assert!(executable_file.patch.contains("new file mode 100755"));

        symlink("missing-target", directory.path().join("link")).unwrap();
        let link =
            build_filesystem_untracked_diff_file(directory.path(), Path::new("link"), 1, "test")
                .unwrap();
        assert!(link.patch.contains("new file mode 120000"));
        assert!(link.patch.contains("+missing-target"));
        assert!(link.sources.new.is_none());
    }

    #[test]
    fn binary_and_large_files_are_bounded_placeholders() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("binary.dat"), [0, 1, 2, 3]).unwrap();
        let binary = build_filesystem_untracked_diff_file(
            directory.path(),
            Path::new("binary.dat"),
            0,
            "test",
        )
        .unwrap();
        assert!(binary.flags.binary && binary.flags.untracked);
        assert_eq!(binary.patch, "Binary file skipped: binary.dat\n");
        assert!(binary.sources.new.is_none());

        fs::write(
            directory.path().join("large.txt"),
            vec![b'x'; crate::LARGE_DIFF_FILE_MAX_BYTES as usize + 1],
        )
        .unwrap();
        let large = build_filesystem_untracked_diff_file(
            directory.path(),
            Path::new("large.txt"),
            1,
            "test",
        )
        .unwrap();
        assert!(large.flags.too_large && large.flags.untracked);
        assert!(!large.flags.binary);
        assert!(large.patch.is_empty() && large.hunks.is_empty());
        assert!(large.stats.truncated);
    }
}
