//! MIT translation of Hunk's `src/core/changeset/fileSource.ts` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`.
//! Provider-neutral, per-file source fetchers; terminal scheduling belongs to the host.

use std::{path::PathBuf, sync::Mutex};
use workdeck_core::ReviewSide;

use crate::{
    DEFAULT_SOURCE_TEXT_MAX_BYTES, LimitedSourceTextResult, SourceTextError,
    read_file_text_with_limit,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileSourceSpec {
    None,
    Fs { absolute_path: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSourceSpecs {
    pub old: FileSourceSpec,
    pub new: FileSourceSpec,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileSourceFetcherOptions {
    pub max_source_bytes: usize,
}

impl Default for FileSourceFetcherOptions {
    fn default() -> Self {
        Self {
            max_source_bytes: DEFAULT_SOURCE_TEXT_MAX_BYTES,
        }
    }
}

/// A source capability independent of the original diff operation.
pub trait FileSourceFetcher: Send + Sync {
    /// An absent key cannot attest unchanged source across a reload.
    fn cache_key(&self) -> Option<&str> {
        None
    }

    fn get_full_text(&self, side: ReviewSide) -> Result<Option<String>, SourceTextError>;
}

pub fn read_file_source_spec(
    spec: &FileSourceSpec,
    options: FileSourceFetcherOptions,
) -> Result<Option<String>, SourceTextError> {
    match spec {
        FileSourceSpec::None => Ok(None),
        FileSourceSpec::Fs { absolute_path } => {
            match read_file_text_with_limit(absolute_path, options.max_source_bytes) {
                LimitedSourceTextResult::Text(text) => Ok(Some(text)),
                LimitedSourceTextResult::Missing => Ok(None),
                LimitedSourceTextResult::TooLarge { max_bytes } => {
                    Err(SourceTextError::TooLarge { max_bytes })
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct FilesystemSourceFetcher {
    specs: FileSourceSpecs,
    options: FileSourceFetcherOptions,
    // Outer None is unresolved; Some(None) is a cached missing side.
    resolved: Mutex<[Option<Option<String>>; 2]>,
}

pub fn create_file_source_fetcher(
    specs: FileSourceSpecs,
    options: FileSourceFetcherOptions,
) -> FilesystemSourceFetcher {
    FilesystemSourceFetcher {
        specs,
        options,
        resolved: Mutex::new([None, None]),
    }
}

impl FileSourceFetcher for FilesystemSourceFetcher {
    fn get_full_text(&self, side: ReviewSide) -> Result<Option<String>, SourceTextError> {
        let (index, spec) = match side {
            ReviewSide::Old => (0, &self.specs.old),
            ReviewSide::New => (1, &self.specs.new),
        };
        if let Some(cached) = &self
            .resolved
            .lock()
            .unwrap_or_else(|error| error.into_inner())[index]
        {
            return Ok(cached.clone());
        }
        // Cache resolved values, not in-flight operations or errors, matching the
        // source fetcher. Do not hold the cache lock across filesystem I/O.
        let text = read_file_source_spec(spec, self.options)?;
        self.resolved
            .lock()
            .unwrap_or_else(|error| error.into_inner())[index] = Some(text.clone());
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fs_spec(path: impl Into<PathBuf>) -> FileSourceSpec {
        FileSourceSpec::Fs {
            absolute_path: path.into(),
        }
    }

    #[test]
    fn reads_fs_paths_for_old_and_new_sides() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("before.txt");
        let new = dir.path().join("after.txt");
        fs::write(&old, "old contents\n").unwrap();
        fs::write(&new, "new contents\n").unwrap();
        let fetcher = create_file_source_fetcher(
            FileSourceSpecs {
                old: fs_spec(old),
                new: fs_spec(new),
            },
            Default::default(),
        );
        assert_eq!(
            fetcher.get_full_text(ReviewSide::Old).unwrap().as_deref(),
            Some("old contents\n")
        );
        assert_eq!(
            fetcher.get_full_text(ReviewSide::New).unwrap().as_deref(),
            Some("new contents\n")
        );
        assert_eq!(fetcher.cache_key(), None);
    }

    #[test]
    fn returns_none_for_none_specs() {
        let fetcher = create_file_source_fetcher(
            FileSourceSpecs {
                old: FileSourceSpec::None,
                new: FileSourceSpec::None,
            },
            Default::default(),
        );
        assert_eq!(fetcher.get_full_text(ReviewSide::Old).unwrap(), None);
        assert_eq!(fetcher.get_full_text(ReviewSide::New).unwrap(), None);
    }

    #[test]
    fn returns_none_when_an_fs_path_cannot_be_read() {
        let dir = tempfile::tempdir().unwrap();
        let fetcher = create_file_source_fetcher(
            FileSourceSpecs {
                old: fs_spec(dir.path().join("missing")),
                new: FileSourceSpec::None,
            },
            Default::default(),
        );
        assert_eq!(fetcher.get_full_text(ReviewSide::Old).unwrap(), None);
    }

    #[test]
    fn rejects_fs_source_reads_that_exceed_the_configured_byte_cap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("large");
        fs::write(&path, "0123456789\n").unwrap();
        let fetcher = create_file_source_fetcher(
            FileSourceSpecs {
                old: fs_spec(path),
                new: FileSourceSpec::None,
            },
            FileSourceFetcherOptions {
                max_source_bytes: 5,
            },
        );
        assert!(matches!(
            fetcher.get_full_text(ReviewSide::Old),
            Err(SourceTextError::TooLarge { max_bytes: 5 })
        ));
    }

    #[test]
    fn caches_resolved_text_per_side() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("value");
        fs::write(&path, "first\n").unwrap();
        let fetcher = create_file_source_fetcher(
            FileSourceSpecs {
                old: fs_spec(&path),
                new: fs_spec(&path),
            },
            Default::default(),
        );
        assert_eq!(
            fetcher.get_full_text(ReviewSide::New).unwrap().as_deref(),
            Some("first\n")
        );
        fs::write(&path, "rewritten\n").unwrap();
        assert_eq!(
            fetcher.get_full_text(ReviewSide::New).unwrap().as_deref(),
            Some("first\n")
        );
        assert_eq!(
            fetcher.get_full_text(ReviewSide::Old).unwrap().as_deref(),
            Some("rewritten\n")
        );
        fs::remove_file(path).unwrap();
        assert_eq!(
            fetcher.get_full_text(ReviewSide::Old).unwrap().as_deref(),
            Some("rewritten\n")
        );
    }

    #[test]
    fn missing_results_are_cached_but_size_errors_are_retryable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("value");
        let fetcher = create_file_source_fetcher(
            FileSourceSpecs {
                old: fs_spec(&path),
                new: fs_spec(&path),
            },
            FileSourceFetcherOptions {
                max_source_bytes: 5,
            },
        );
        assert_eq!(fetcher.get_full_text(ReviewSide::Old).unwrap(), None);
        fs::write(&path, "too large").unwrap();
        assert_eq!(fetcher.get_full_text(ReviewSide::Old).unwrap(), None);
        assert!(matches!(
            fetcher.get_full_text(ReviewSide::New),
            Err(SourceTextError::TooLarge { max_bytes: 5 })
        ));
        fs::write(&path, "okay").unwrap();
        assert_eq!(
            fetcher.get_full_text(ReviewSide::New).unwrap().as_deref(),
            Some("okay")
        );
    }
}
