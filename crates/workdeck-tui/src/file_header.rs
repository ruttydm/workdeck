//! Cell-measured file labels and stats for review stream headers.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use workdeck_core::{DiffFile, FileChangeKind};
use workdeck_diff::format_terminal_path;

pub const FILE_HEADER_OVERFLOW_MARKER: &str = "...";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHeaderStats {
    pub additions_text: String,
    pub deletions_text: String,
    pub text: String,
    pub width: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHeaderLabel {
    pub filename: String,
    pub state_label: Option<&'static str>,
}

/// Build rendered count fragments, including Hunk's extra `+` for truncated
/// additions, and measure the complete stats cell in terminal columns.
#[must_use]
pub fn file_header_stats(file: &DiffFile) -> FileHeaderStats {
    let additions_text = format!(
        "+{}{}",
        file.stats.additions,
        if file.stats.truncated { "+" } else { "" }
    );
    let deletions_text = format!("-{}", file.stats.deletions);
    let text = format!("{additions_text} {deletions_text} ");
    let width = text.width();
    FileHeaderStats {
        additions_text,
        deletions_text,
        text,
        width,
    }
}

#[must_use]
pub fn max_file_header_stats_width(files: &[DiffFile]) -> usize {
    files
        .iter()
        .map(|file| file_header_stats(file).width)
        .max()
        .unwrap_or_default()
}

/// Split the terminal-safe path/rename identity from its semantic state suffix.
#[must_use]
pub fn file_header_label_parts(file: &DiffFile) -> (String, Option<&'static str>) {
    let path = format_terminal_path(&file.path);
    let filename = file
        .previous_path
        .as_ref()
        .map(|previous| format_terminal_path(previous))
        .filter(|previous| previous != &path)
        .map_or_else(|| path.clone(), |previous| format!("{previous} -> {path}"));
    let state_label = if file.flags.untracked || file.change_kind == FileChangeKind::Untracked {
        Some(" (untracked)")
    } else {
        match file.change_kind {
            FileChangeKind::Added => Some(" (new)"),
            FileChangeKind::Deleted => Some(" (deleted)"),
            FileChangeKind::Modified
            | FileChangeKind::Renamed
            | FileChangeKind::Copied
            | FileChangeKind::TypeChanged
            | FileChangeKind::Untracked
            | FileChangeKind::Conflicted => None,
        }
    };
    (filename, state_label)
}

/// Fit the path while preserving a suffix only when at least one path cell
/// remains. Overflow always uses Hunk's explicit three-dot marker.
#[must_use]
pub fn fit_file_header_label(file: &DiffFile, width: usize) -> FileHeaderLabel {
    let (filename, state_label) = file_header_label_parts(file);
    let state_width = state_label.map_or(0, UnicodeWidthStr::width);
    let visible_state_label = state_label.filter(|_| state_width < width);
    let visible_state_width = visible_state_label.map_or(0, UnicodeWidthStr::width);
    FileHeaderLabel {
        filename: fit_text(
            &filename,
            width.saturating_sub(visible_state_width),
            FILE_HEADER_OVERFLOW_MARKER,
        ),
        state_label: visible_state_label,
    }
}

fn fit_text(text: &str, width: usize, marker: &str) -> String {
    if width == 0 {
        return String::new();
    }
    if text.width() <= width {
        return text.to_owned();
    }
    let marker = take_width(marker, width);
    let text_width = width.saturating_sub(marker.width());
    format!("{}{}", take_width(text, text_width), marker)
}

fn take_width(text: &str, width: usize) -> String {
    let mut used = 0_usize;
    text.chars()
        .take_while(|character| {
            let character_width = character.width().unwrap_or_default();
            if used.saturating_add(character_width) > width {
                false
            } else {
                used = used.saturating_add(character_width);
                true
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_core::ChangesetSource;
    use workdeck_diff::parse_patch;

    fn test_file(path: &str) -> DiffFile {
        parse_patch(
            &format!(
                "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1 +1 @@\n-old\n+new\n"
            ),
            "test",
            "test",
            ChangesetSource::Patch {
                label: "test".into(),
            },
        )
        .unwrap()
        .files
        .remove(0)
    }

    #[test]
    fn reserves_only_the_widest_rendered_stats_text() {
        let mut small = test_file("small.ts");
        let mut large = test_file("large.ts");
        small.stats.additions = 1;
        small.stats.deletions = 0;
        large.stats.additions = 1_234;
        large.stats.deletions = 56;
        large.stats.truncated = true;

        assert_eq!(file_header_stats(&small).text, "+1 -0 ");
        assert_eq!(file_header_stats(&small).width, 6);
        assert_eq!(file_header_stats(&large).text, "+1234+ -56 ");
        assert_eq!(file_header_stats(&large).width, 11);
        assert_eq!(max_file_header_stats_width(&[small, large]), 11);
    }

    #[test]
    fn fits_long_paths_with_three_dots_at_terminal_cell_width() {
        let file = test_file("packages/visual-studio-code-vscode/extension-postgres.ts");
        let label = fit_file_header_label(&file, 29);
        assert_eq!(
            label.filename,
            format!("packages/visual-studio-cod{FILE_HEADER_OVERFLOW_MARKER}")
        );
        assert_eq!(label.filename.width(), 29);
    }

    #[test]
    fn keeps_state_labels_outside_the_path_truncation_budget() {
        let mut file = test_file("longer-filename.ts");
        file.change_kind = FileChangeKind::Added;
        assert_eq!(
            fit_file_header_label(&file, 14),
            FileHeaderLabel {
                filename: "longe...".into(),
                state_label: Some(" (new)"),
            }
        );
    }

    #[test]
    fn drops_state_before_it_can_displace_the_path_or_stats() {
        let mut file = test_file("longer-filename.ts");
        file.change_kind = FileChangeKind::Added;
        assert_eq!(
            fit_file_header_label(&file, 5),
            FileHeaderLabel {
                filename: "lo...".into(),
                state_label: None,
            }
        );
        assert_eq!(
            fit_file_header_label(&file, 0),
            FileHeaderLabel {
                filename: String::new(),
                state_label: None,
            }
        );
    }
}
