//! Bounded inspection for files that are too expensive to synthesize as full diffs.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use workdeck_core::FileStats;

pub const LARGE_DIFF_FILE_MAX_BYTES: u64 = 1_000_000;
pub const LARGE_DIFF_FILE_MAX_LINES: usize = 20_000;
const LARGE_DIFF_FILE_SNIFF_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LargeFileCheck {
    pub should_skip: bool,
    pub stats: Option<FileStats>,
    pub stats_truncated: Option<bool>,
}

#[derive(Debug, Clone, Copy)]
struct CountedLines {
    complete: bool,
    lines: usize,
}

/// Inspect a whole-file addition using bounded I/O and return placeholder statistics if skipped.
pub fn inspect_large_untracked_file(repo_root: &Path, file_path: &Path) -> LargeFileCheck {
    let absolute_path = repo_root.join(file_path);
    let Ok(metadata) = fs::metadata(&absolute_path) else {
        return not_skipped();
    };
    let size = metadata.len();
    let byte_limit = if size > LARGE_DIFF_FILE_MAX_BYTES {
        LARGE_DIFF_FILE_MAX_BYTES
    } else {
        LARGE_DIFF_FILE_SNIFF_BYTES
    };
    let counted = count_lines_in_file(&absolute_path, byte_limit, size);
    let should_skip = size > LARGE_DIFF_FILE_MAX_BYTES || counted.lines > LARGE_DIFF_FILE_MAX_LINES;
    if should_skip {
        LargeFileCheck {
            should_skip: true,
            stats: Some(FileStats {
                additions: counted.lines,
                deletions: 0,
                truncated: !counted.complete,
            }),
            stats_truncated: Some(!counted.complete),
        }
    } else {
        not_skipped()
    }
}

fn not_skipped() -> LargeFileCheck {
    LargeFileCheck {
        should_skip: false,
        stats: None,
        stats_truncated: None,
    }
}

fn count_lines_in_file(path: &Path, max_bytes: u64, size: u64) -> CountedLines {
    let Ok(mut file) = File::open(path) else {
        return CountedLines {
            complete: true,
            lines: 0,
        };
    };
    let mut buffer = vec![0_u8; usize::try_from((64 * 1024_u64).min(max_bytes)).unwrap_or(0)];
    let mut position = 0_u64;
    let mut line_count = 0_usize;
    let mut last_byte = None;
    while position < max_bytes {
        let bytes_to_read = usize::try_from((max_bytes - position).min(buffer.len() as u64))
            .unwrap_or(buffer.len());
        let Ok(bytes_read) = file.read(&mut buffer[..bytes_to_read]) else {
            return CountedLines {
                complete: true,
                lines: 0,
            };
        };
        if bytes_read == 0 {
            break;
        }
        position = position.saturating_add(bytes_read as u64);
        for byte in &buffer[..bytes_read] {
            last_byte = Some(*byte);
            if *byte == b'\n' {
                line_count = line_count.saturating_add(1);
            }
        }
    }
    CountedLines {
        complete: position >= size,
        lines: if last_byte.is_some_and(|byte| byte != b'\n') {
            line_count.saturating_add(1)
        } else {
            line_count
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn skips_by_byte_limit_with_bounded_truncated_stats() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("large.txt");
        let mut file = File::create(&path).unwrap();
        file.write_all(&vec![b'x'; LARGE_DIFF_FILE_MAX_BYTES as usize + 1])
            .unwrap();

        let check = inspect_large_untracked_file(directory.path(), Path::new("large.txt"));
        assert!(check.should_skip);
        assert_eq!(
            check.stats,
            Some(FileStats {
                additions: 1,
                deletions: 0,
                truncated: true
            })
        );
        assert_eq!(check.stats_truncated, Some(true));
    }

    #[test]
    fn skips_by_line_limit_and_keeps_complete_stats() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("many-lines.txt");
        let mut file = File::create(&path).unwrap();
        for _ in 0..=LARGE_DIFF_FILE_MAX_LINES {
            file.write_all(b"x\n").unwrap();
        }

        let check = inspect_large_untracked_file(directory.path(), Path::new("many-lines.txt"));
        assert!(check.should_skip);
        assert_eq!(
            check.stats.as_ref().map(|stats| stats.additions),
            Some(LARGE_DIFF_FILE_MAX_LINES + 1)
        );
        assert_eq!(check.stats_truncated, Some(false));
    }

    #[test]
    fn small_and_missing_files_are_not_skipped() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("small.txt"), "one\ntwo\n").unwrap();
        assert_eq!(
            inspect_large_untracked_file(directory.path(), Path::new("small.txt")),
            not_skipped()
        );
        assert_eq!(
            inspect_large_untracked_file(directory.path(), Path::new("missing.txt")),
            not_skipped()
        );
    }
}
