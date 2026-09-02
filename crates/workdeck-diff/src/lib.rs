//! Unified patch parsing and renderer-neutral row geometry.
//!
//! The parser accepts Git-format and ordinary unified patches, normalizes terminal-pager input,
//! and preserves each original per-file patch for review/session consumers.

mod alignment;
mod bundled_theme_assets;
mod geometry;
mod git_format;
mod git_log;
mod language;
mod syntax;
mod terminal;
mod word_diff;

pub use alignment::{SplitLinePair, plan_split_line_pairs};
pub use geometry::{TextSegment, clip_segments, segments_width, wrap_segments};
pub use git_format::{SanitizedGitPatch, SanitizedGitPatchFilePaths, sanitize_git_patch};
pub use git_log::strip_git_log_metadata;
pub use language::{LanguageMatcher, LanguageRegistration, LanguageRegistry};
pub use syntax::{
    HighlightAppearance, HighlightCache, PIERRE_DARK_THEME, PIERRE_LIGHT_THEME, SyntaxColor,
    SyntaxToken, highlight_worker_cache_key, syntax_highlight_theme_name,
};
pub use terminal::{
    SanitizeOptions, TerminalSpan, format_terminal_path, sanitize_terminal_line,
    sanitize_terminal_spans, sanitize_terminal_text,
};
pub use word_diff::{WordDiffRanges, word_diff_ranges};

use thiserror::Error;
use workdeck_core::{
    Changeset, ChangesetSource, DiffFile, DiffHunk, DiffLine, DiffLineKind, FileChangeKind,
    FileFlags, FileStats,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileSnapshot<'a> {
    pub cache_key: &'a str,
    pub contents: &'a str,
    pub name: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileComparisonOptions {
    pub context_radius: usize,
}

impl Default for FileComparisonOptions {
    fn default() -> Self {
        Self { context_radius: 3 }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct DiffLineMoveKinds {
    addition_lines: Vec<bool>,
    deletion_lines: Vec<bool>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PatchError {
    #[error("patch contains no file headers")]
    NoFiles,
    #[error("invalid hunk header {0:?}")]
    InvalidHunkHeader(String),
    #[error("file patch does not identify either side")]
    MissingPath,
    #[error("malformed quoted Git path {0:?}")]
    MalformedQuotedPath(String),
    #[error("expected one parsed file for patch {path:?}, got {count}")]
    ExpectedOneFile { path: String, count: usize },
}

pub fn parse_patch(
    patch: &str,
    id: impl Into<String>,
    title: impl Into<String>,
    source: ChangesetSource,
) -> Result<Changeset, PatchError> {
    let line_move_kinds = collect_line_move_kinds(patch);
    let normalized = patch.replace("\r\n", "\n");
    let stripped = strip_terminal_control(&normalized);
    let without_log = strip_git_log_metadata(&stripped);
    let sanitized = sanitize_git_patch(&without_log);
    let chunks = split_patch_into_file_chunks(&sanitized.text);
    if chunks.is_empty() {
        return Err(PatchError::NoFiles);
    }
    let source_prefix = id.into();
    let files = chunks
        .iter()
        .enumerate()
        .map(|(index, chunk)| {
            parse_file_chunk(
                chunk,
                index,
                &source_prefix,
                sanitized.file_paths.get(index).and_then(Option::as_ref),
                line_move_kinds.get(index),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut changeset = Changeset {
        id: source_prefix,
        title: title.into(),
        source,
        files,
    };
    changeset.refresh_review_identities();
    Ok(changeset)
}

/// Compare two text snapshots and project the result through Workdeck's canonical patch parser.
/// Snapshot cache keys participate in the source identity but never alter displayed paths.
pub fn diff_from_file_snapshots(
    before: FileSnapshot<'_>,
    after: FileSnapshot<'_>,
    options: FileComparisonOptions,
) -> Result<DiffFile, PatchError> {
    let diff = similar::TextDiff::from_lines(before.contents, after.contents);
    let unified = diff
        .unified_diff()
        .context_radius(options.context_radius)
        .header(before.name, after.name)
        .to_string();
    let source_id = format!("{}:{}", before.cache_key, after.cache_key);
    let patch = format!(
        "diff --git a/{before_name} b/{after_name}\n{unified}",
        before_name = before.name,
        after_name = after.name,
    );
    let mut changeset = parse_patch(
        &patch,
        &source_id,
        after.name,
        ChangesetSource::Files {
            left: before.name.to_owned(),
            right: after.name.to_owned(),
        },
    )?;
    let mut file = changeset.files.remove(0);
    for hunk in &mut file.hunks {
        hunk.header = format!(
            "@@ -{},{} +{},{} @@{}",
            hunk.old_start,
            hunk.old_count,
            hunk.new_start,
            hunk.new_count,
            hunk.context
                .as_deref()
                .filter(|context| !context.is_empty())
                .map_or_else(String::new, |context| format!(" {context}")),
        );
    }
    file.previous_path = (before.name != after.name).then(|| before.name.to_owned());
    file.path = after.name.to_owned();
    file.refresh_identity();
    file.refresh_address(&source_id, 0);
    Ok(file)
}

pub fn sanitize_patch(patch: &str) -> String {
    let normalized = patch.replace("\r\n", "\n");
    let stripped = strip_terminal_control(&normalized);
    let without_log = strip_git_log_metadata(&stripped);
    sanitize_git_patch(&without_log).text
}

/// Parse a synthetic patch that must describe exactly one file and relabel it with the producer's
/// authoritative repository-relative path.
pub fn parse_single_file_patch(
    patch: &str,
    file_path: &str,
    previous_path: Option<&str>,
) -> Result<DiffFile, PatchError> {
    let mut changeset = parse_patch(
        patch,
        "single-file",
        file_path,
        ChangesetSource::Patch {
            label: file_path.to_owned(),
        },
    )?;
    if changeset.files.len() != 1 {
        return Err(PatchError::ExpectedOneFile {
            path: file_path.to_owned(),
            count: changeset.files.len(),
        });
    }
    let mut file = changeset.files.remove(0);
    file.path = file_path.to_owned();
    file.previous_path = previous_path.map(str::to_owned);
    file.language = language_for_path(file_path);
    file.refresh_identity();
    file.refresh_address("single-file", 0);
    Ok(file)
}

pub fn strip_terminal_control(text: &str) -> String {
    sanitize_terminal_text(text, SanitizeOptions::default())
}

pub fn split_patch_into_file_chunks(raw_patch: &str) -> Vec<String> {
    let patch = raw_patch.replace("\r\n", "\n");
    let lines = patch.split('\n').collect::<Vec<_>>();
    let has_git_headers = lines.iter().any(|line| line.starts_with("diff --git "));
    let mut chunks = Vec::new();
    let mut current = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index];
        let starts_file = if has_git_headers {
            line.starts_with("diff --git ")
        } else {
            line.starts_with("--- ")
                && lines
                    .get(index + 1)
                    .is_some_and(|next| next.starts_with("+++ "))
        };
        if starts_file {
            flush_chunk(&mut chunks, &mut current);
            current.push(line);
            if !has_git_headers {
                current.push(lines[index + 1]);
                index += 1;
            }
        } else if !current.is_empty() {
            current.push(line);
        }
        index += 1;
    }
    flush_chunk(&mut chunks, &mut current);
    chunks
}

pub fn normalize_diff_path(path: Option<&str>) -> Option<String> {
    path.map(|path| path.trim_end_matches(['\r', '\n']).to_owned())
}

pub fn find_patch_chunk(
    current_path: Option<&str>,
    previous_path: Option<&str>,
    chunks: &[String],
    index: usize,
) -> String {
    if let Some(chunk) = chunks.get(index) {
        return chunk.clone();
    }
    [current_path, previous_path]
        .into_iter()
        .flatten()
        .filter_map(|path| normalize_diff_path(Some(path)))
        .map(|path| strip_side_prefix(&path).to_owned())
        .find_map(|path| {
            chunks
                .iter()
                .find(|chunk| {
                    chunk.contains(&format!("a/{path}"))
                        || chunk.contains(&format!("b/{path}"))
                        || chunk.contains(&path)
                })
                .cloned()
        })
        .unwrap_or_default()
}

pub fn escape_untracked_patch_path(path: &str) -> String {
    path.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn flush_chunk(chunks: &mut Vec<String>, current: &mut Vec<&str>) {
    if current.is_empty() {
        return;
    }
    let text = current.join("\n");
    chunks.push(format!("{}\n", text.trim_end()));
    current.clear();
}

fn parse_file_chunk(
    chunk: &str,
    index: usize,
    source_prefix: &str,
    exact_paths: Option<&SanitizedGitPatchFilePaths>,
    line_move_kinds: Option<&DiffLineMoveKinds>,
) -> Result<DiffFile, PatchError> {
    let lines = chunk.lines().collect::<Vec<_>>();
    let header_pair = lines.iter().find_map(|line| {
        line.strip_prefix("diff --git ")
            .and_then(parse_git_header_pair)
    });
    let old_header = lines.iter().find_map(|line| line.strip_prefix("--- "));
    let new_header = lines.iter().find_map(|line| line.strip_prefix("+++ "));
    let rename_from = metadata_path(&lines, "rename from ")?;
    let rename_to = metadata_path(&lines, "rename to ")?;
    let copy_from = metadata_path(&lines, "copy from ")?;
    let copy_to = metadata_path(&lines, "copy to ")?;

    let header_old = old_header
        .map(parse_unified_header_path)
        .transpose()?
        .flatten();
    let header_new = new_header
        .map(parse_unified_header_path)
        .transpose()?
        .flatten();
    let pair_old = header_pair.as_ref().map(|pair| pair.0.clone());
    let pair_new = header_pair.as_ref().map(|pair| pair.1.clone());
    let old_path = rename_from
        .clone()
        .or(copy_from.clone())
        .or(header_old)
        .or(pair_old);
    let new_path = rename_to
        .clone()
        .or(copy_to.clone())
        .or(header_new)
        .or(pair_new);
    let parsed_path = new_path
        .clone()
        .or_else(|| old_path.clone())
        .ok_or(PatchError::MissingPath)?;
    let parsed_previous_path = old_path.filter(|old| old != &parsed_path);
    let path = exact_paths
        .map(|paths| paths.path.clone())
        .unwrap_or(parsed_path);
    let previous_path = exact_paths
        .and_then(|paths| paths.previous_path.clone())
        .or(parsed_previous_path);

    let change_kind = if lines.iter().any(|line| line.starts_with("new file mode "))
        || old_header.is_some_and(|path| path.starts_with("/dev/null"))
    {
        FileChangeKind::Added
    } else if lines
        .iter()
        .any(|line| line.starts_with("deleted file mode "))
        || new_header.is_some_and(|path| path.starts_with("/dev/null"))
    {
        FileChangeKind::Deleted
    } else if rename_from.is_some() || rename_to.is_some() {
        FileChangeKind::Renamed
    } else if copy_from.is_some() || copy_to.is_some() {
        FileChangeKind::Copied
    } else if lines.iter().any(|line| line.starts_with("old mode "))
        && lines.iter().any(|line| line.starts_with("new mode "))
    {
        FileChangeKind::TypeChanged
    } else {
        FileChangeKind::Modified
    };

    let mut hunks = Vec::new();
    let mut cursor = 0;
    let mut split_rows = 0;
    let mut stack_rows = 0;
    let mut additions = 0;
    let mut deletions = 0;
    let mut addition_line_index = 0;
    let mut deletion_line_index = 0;
    while cursor < lines.len() {
        if !lines[cursor].starts_with("@@ ") && !lines[cursor].starts_with("@@-") {
            cursor += 1;
            continue;
        }
        let (old_start, old_count, new_start, new_count, context) =
            parse_hunk_header(lines[cursor])?;
        let header = lines[cursor].to_owned();
        cursor += 1;
        let mut old_line = old_start;
        let mut new_line = new_start;
        let mut hunk_lines = Vec::new();
        while cursor < lines.len() && !lines[cursor].starts_with("@@ ") {
            let line = lines[cursor];
            if line.starts_with("diff --git ") {
                break;
            }
            match line.as_bytes().first().copied() {
                Some(b' ') => {
                    hunk_lines.push(DiffLine {
                        kind: DiffLineKind::Context,
                        content: line[1..].to_owned(),
                        old_line: Some(old_line),
                        new_line: Some(new_line),
                        moved: false,
                        no_newline_at_eof: false,
                    });
                    old_line = old_line.saturating_add(1);
                    new_line = new_line.saturating_add(1);
                    addition_line_index += 1;
                    deletion_line_index += 1;
                }
                Some(b'-') => {
                    hunk_lines.push(DiffLine {
                        kind: DiffLineKind::Deletion,
                        content: line[1..].to_owned(),
                        old_line: Some(old_line),
                        new_line: None,
                        moved: line_move_kinds
                            .and_then(|kinds| kinds.deletion_lines.get(deletion_line_index))
                            .copied()
                            .unwrap_or(false),
                        no_newline_at_eof: false,
                    });
                    old_line = old_line.saturating_add(1);
                    deletions += 1;
                    deletion_line_index += 1;
                }
                Some(b'+') => {
                    hunk_lines.push(DiffLine {
                        kind: DiffLineKind::Addition,
                        content: line[1..].to_owned(),
                        old_line: None,
                        new_line: Some(new_line),
                        moved: line_move_kinds
                            .and_then(|kinds| kinds.addition_lines.get(addition_line_index))
                            .copied()
                            .unwrap_or(false),
                        no_newline_at_eof: false,
                    });
                    new_line = new_line.saturating_add(1);
                    additions += 1;
                    addition_line_index += 1;
                }
                Some(b'\\') if line == "\\ No newline at end of file" => {
                    if let Some(previous) = hunk_lines.last_mut() {
                        previous.no_newline_at_eof = true;
                    }
                }
                _ if line.is_empty() => {}
                _ => break,
            }
            cursor += 1;
        }
        let hunk_split_rows = split_row_count(&hunk_lines);
        let hunk_stack_rows = hunk_lines.len();
        let hunk_index = hunks.len();
        hunks.push(DiffHunk {
            index: hunk_index,
            header,
            context,
            old_start,
            old_count,
            new_start,
            new_count,
            split_row_start: split_rows,
            split_row_count: hunk_split_rows,
            stack_row_start: stack_rows,
            stack_row_count: hunk_stack_rows,
            lines: hunk_lines,
        });
        split_rows += hunk_split_rows;
        stack_rows += hunk_stack_rows;
    }

    let binary = lines.iter().any(|line| {
        line == &"GIT binary patch"
            || line.starts_with("Binary files ")
            || line.starts_with("Binary file ")
    });
    let language = language_for_path(&path);
    let mut file = DiffFile {
        key: String::new(),
        runtime_id: format!("{source_prefix}:{index}:{path}"),
        path,
        previous_path,
        change_kind,
        language,
        stats: FileStats {
            additions,
            deletions,
            truncated: false,
        },
        flags: FileFlags {
            binary,
            partial: true,
            ..FileFlags::default()
        },
        patch: chunk.to_owned(),
        split_row_count: split_rows,
        stack_row_count: stack_rows,
        hunks,
        content_identity: String::new(),
        sources: workdeck_core::FileSourceSnapshots::default(),
        source_identity: None,
        source_attested: false,
        agent: None,
    };
    file.refresh_identity();
    Ok(file)
}

/// Capture Git's deterministic color-moved classes before the terminal sanitizer removes SGR.
fn collect_line_move_kinds(patch_text: &str) -> Vec<DiffLineMoveKinds> {
    let normalized = patch_text.replace("\r\n", "\n");
    let mut files = Vec::new();
    let mut current: Option<usize> = None;
    let mut in_hunk = false;
    let mut addition_line_index = 0;
    let mut deletion_line_index = 0;

    let begin_file = |files: &mut Vec<DiffLineMoveKinds>| {
        files.push(DiffLineMoveKinds::default());
        files.len() - 1
    };

    for raw_line in normalized.split('\n') {
        let plain_line = strip_terminal_control(raw_line);
        if plain_line.starts_with("diff --git ") {
            current = Some(begin_file(&mut files));
            in_hunk = false;
            addition_line_index = 0;
            deletion_line_index = 0;
            continue;
        }
        if current.is_none() && (plain_line.starts_with("--- ") || plain_line.starts_with("@@ ")) {
            current = Some(begin_file(&mut files));
            in_hunk = false;
            addition_line_index = 0;
            deletion_line_index = 0;
        }
        let Some(file_index) = current else {
            continue;
        };
        if plain_line.starts_with("@@ ") {
            in_hunk = true;
            continue;
        }
        if !in_hunk {
            continue;
        }
        if plain_line.starts_with('+') && !plain_line.starts_with("+++") {
            set_move_kind(
                &mut files[file_index].addition_lines,
                addition_line_index,
                moved_line_from_ansi(raw_line, b'+', "36"),
            );
            addition_line_index += 1;
        } else if plain_line.starts_with('-') && !plain_line.starts_with("---") {
            set_move_kind(
                &mut files[file_index].deletion_lines,
                deletion_line_index,
                moved_line_from_ansi(raw_line, b'-', "35"),
            );
            deletion_line_index += 1;
        } else if plain_line.starts_with(' ') {
            addition_line_index += 1;
            deletion_line_index += 1;
        }
    }
    files
}

fn set_move_kind(target: &mut Vec<bool>, index: usize, moved: bool) {
    if target.len() <= index {
        target.resize(index + 1, false);
    }
    target[index] = moved;
}

fn moved_line_from_ansi(raw_line: &str, expected_sign: u8, color_code: &str) -> bool {
    let bytes = raw_line.as_bytes();
    let mut index = 0;
    let mut parameters = Vec::new();
    while bytes.get(index) == Some(&0x1b) {
        if bytes.get(index + 1) != Some(&b'[') {
            return false;
        }
        let start = index + 2;
        index = start;
        while bytes
            .get(index)
            .is_some_and(|byte| !(0x40..=0x7e).contains(byte))
        {
            index += 1;
        }
        let Some(final_byte) = bytes.get(index).copied() else {
            return false;
        };
        if final_byte == b'm' {
            parameters.push(&raw_line[start..index]);
        }
        index += 1;
    }
    bytes.get(index) == Some(&expected_sign)
        && parameters
            .iter()
            .flat_map(|parameter| parameter.split(';'))
            .any(|parameter| parameter == color_code)
}

fn metadata_path(lines: &[&str], prefix: &str) -> Result<Option<String>, PatchError> {
    lines
        .iter()
        .find_map(|line| line.strip_prefix(prefix))
        .map(parse_git_path)
        .transpose()
}

fn parse_unified_header_path(value: &str) -> Result<Option<String>, PatchError> {
    let path = if value.starts_with('"') {
        let closing =
            quoted_path_end(value).ok_or_else(|| PatchError::MalformedQuotedPath(value.into()))?;
        parse_git_path(&value[..=closing])?
    } else {
        value.split('\t').next().unwrap_or(value).to_owned()
    };
    if path == "/dev/null" {
        Ok(None)
    } else {
        Ok(Some(strip_side_prefix(&path).to_owned()))
    }
}

fn parse_git_header_pair(value: &str) -> Option<(String, String)> {
    if value.starts_with('"') {
        let first_end = quoted_path_end(value)?;
        let first = parse_git_path(&value[..=first_end]).ok()?;
        let rest = value[first_end + 1..].trim_start();
        let second_end = quoted_path_end(rest)?;
        let second = parse_git_path(&rest[..=second_end]).ok()?;
        return Some((
            strip_side_prefix(&first).to_owned(),
            strip_side_prefix(&second).to_owned(),
        ));
    }
    let tokens = value.split_whitespace().collect::<Vec<_>>();
    if tokens.len() < 2 || tokens.len() % 2 != 0 {
        return None;
    }
    let middle = tokens.len() / 2;
    let first = tokens[..middle].join(" ");
    let second = tokens[middle..].join(" ");
    Some((
        strip_side_prefix(&first).to_owned(),
        strip_side_prefix(&second).to_owned(),
    ))
}

fn parse_git_path(value: &str) -> Result<String, PatchError> {
    if !value.starts_with('"') {
        return Ok(value.to_owned());
    }
    let end =
        quoted_path_end(value).ok_or_else(|| PatchError::MalformedQuotedPath(value.into()))?;
    if end + 1 != value.len() {
        return Err(PatchError::MalformedQuotedPath(value.into()));
    }
    let inner = &value[1..end];
    let mut bytes = Vec::with_capacity(inner.len());
    let raw = inner.as_bytes();
    let mut index = 0;
    while index < raw.len() {
        if raw[index] != b'\\' {
            bytes.push(raw[index]);
            index += 1;
            continue;
        }
        index += 1;
        let Some(escaped) = raw.get(index).copied() else {
            return Err(PatchError::MalformedQuotedPath(value.into()));
        };
        if (b'0'..=b'7').contains(&escaped) {
            let mut value = 0_u16;
            let mut digits = 0;
            while digits < 3 && index < raw.len() && (b'0'..=b'7').contains(&raw[index]) {
                value = value * 8 + u16::from(raw[index] - b'0');
                index += 1;
                digits += 1;
            }
            if value > 255 {
                return Err(PatchError::MalformedQuotedPath(inner.into()));
            }
            bytes.push(value as u8);
            continue;
        }
        let decoded = match escaped {
            b'a' => 0x07,
            b'b' => 0x08,
            b't' => b'\t',
            b'n' => b'\n',
            b'v' => 0x0b,
            b'f' => 0x0c,
            b'r' => b'\r',
            b'\\' => b'\\',
            b'"' => b'"',
            _ => return Err(PatchError::MalformedQuotedPath(inner.into())),
        };
        bytes.push(decoded);
        index += 1;
    }
    String::from_utf8(bytes).map_err(|_| PatchError::MalformedQuotedPath(value.into()))
}

fn quoted_path_end(value: &str) -> Option<usize> {
    let bytes = value.as_bytes();
    if bytes.first() != Some(&b'"') {
        return None;
    }
    let mut index = 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'"' => return Some(index),
            _ => index += 1,
        }
    }
    None
}

fn strip_side_prefix(path: &str) -> &str {
    path.strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .or_else(|| path.strip_prefix("i/"))
        .or_else(|| path.strip_prefix("w/"))
        .or_else(|| path.strip_prefix("c/"))
        .or_else(|| path.strip_prefix("o/"))
        .unwrap_or(path)
}

fn parse_hunk_header(line: &str) -> Result<(u32, u32, u32, u32, Option<String>), PatchError> {
    let rest = line
        .strip_prefix("@@")
        .ok_or_else(|| PatchError::InvalidHunkHeader(line.into()))?;
    let closing = rest
        .find("@@")
        .ok_or_else(|| PatchError::InvalidHunkHeader(line.into()))?;
    let ranges = rest[..closing].split_whitespace().collect::<Vec<_>>();
    if ranges.len() != 2 {
        return Err(PatchError::InvalidHunkHeader(line.into()));
    }
    let (old_start, old_count) = parse_range(ranges[0], '-')?;
    let (new_start, new_count) = parse_range(ranges[1], '+')?;
    let context = rest[closing + 2..].trim();
    Ok((
        old_start,
        old_count,
        new_start,
        new_count,
        (!context.is_empty()).then(|| context.to_owned()),
    ))
}

fn parse_range(value: &str, marker: char) -> Result<(u32, u32), PatchError> {
    let range = value
        .strip_prefix(marker)
        .ok_or_else(|| PatchError::InvalidHunkHeader(value.into()))?;
    let (start, count) = range.split_once(',').unwrap_or((range, "1"));
    Ok((
        start
            .parse()
            .map_err(|_| PatchError::InvalidHunkHeader(value.into()))?,
        count
            .parse()
            .map_err(|_| PatchError::InvalidHunkHeader(value.into()))?,
    ))
}

fn split_row_count(lines: &[DiffLine]) -> usize {
    let mut rows = 0;
    let mut index = 0;
    while index < lines.len() {
        if lines[index].kind == DiffLineKind::Context {
            rows += 1;
            index += 1;
            continue;
        }
        let mut additions = 0;
        let mut deletions = 0;
        while index < lines.len() && lines[index].kind != DiffLineKind::Context {
            match lines[index].kind {
                DiffLineKind::Addition => additions += 1,
                DiffLineKind::Deletion => deletions += 1,
                DiffLineKind::Context => unreachable!(),
            }
            index += 1;
        }
        rows += additions.max(deletions);
    }
    rows
}

fn language_for_path(path: &str) -> Option<String> {
    let language = LanguageRegistry::default().language_for_path(path);
    (language != "text").then_some(language)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATCH: &str = "diff --git a/src/main.rs b/src/main.rs\nindex 111..222 100644\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,3 +1,4 @@ fn main() {\n context\n-old\n+new\n+later\n tail\n";

    #[test]
    fn splits_git_and_plain_unified_patches() {
        let two = format!("{PATCH}{}", PATCH.replace("main.rs", "lib.rs"));
        assert_eq!(split_patch_into_file_chunks(&two).len(), 2);
        let plain = "--- old.txt\n+++ new.txt\n@@ -1 +1 @@\n-a\n+b\n";
        assert_eq!(split_patch_into_file_chunks(plain).len(), 1);
    }

    #[test]
    fn finds_fallback_chunks_by_current_or_previous_normalized_path() {
        let chunks = vec![
            "diff --git a/old-name.ts b/new-name.ts\n--- a/old-name.ts\n+++ b/new-name.ts\n"
                .to_owned(),
        ];
        assert_eq!(
            find_patch_chunk(Some("b/new-name.ts\r\n"), None, &chunks, 3),
            chunks[0]
        );
        assert_eq!(
            find_patch_chunk(None, Some("a/old-name.ts"), &chunks, 3),
            chunks[0]
        );
        assert!(find_patch_chunk(Some("missing.ts"), None, &chunks, 2).is_empty());
    }

    #[test]
    fn escapes_only_patch_header_breaking_path_characters() {
        assert_eq!(escape_untracked_patch_path("a\\\tb"), "a\\\\\\tb");
        assert_eq!(escape_untracked_patch_path("a\tb"), "a\\tb");
        assert_eq!(escape_untracked_patch_path("a\nb\rc"), "a\\nb\\rc");
        assert_eq!(
            escape_untracked_patch_path("src/foo bar.ts"),
            "src/foo bar.ts"
        );
    }

    #[test]
    fn parses_file_stats_lines_and_layout_geometry() {
        let changeset = parse_patch(
            PATCH,
            "working",
            "Working tree",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap();
        let file = &changeset.files[0];
        assert_eq!(file.path, "src/main.rs");
        assert_eq!(file.language.as_deref(), Some("rust"));
        assert_eq!(file.stats.additions, 2);
        assert_eq!(file.stats.deletions, 1);
        assert_eq!(file.stack_row_count, 5);
        assert_eq!(file.split_row_count, 4);
        assert_eq!(file.hunks[0].context.as_deref(), Some("fn main() {"));
        assert_eq!(file.hunks[0].lines[1].old_line, Some(2));
        assert_eq!(file.hunks[0].lines[2].new_line, Some(2));
    }

    #[test]
    fn strips_crlf_and_terminal_color_sequences() {
        let dirty = "\x1b[31mdiff --git a/a b/a\x1b[0m\r\n--- a/a\r\n+++ b/a\r\n@@ -1 +1 @@\r\n-a\r\n+b\r\n";
        let sanitized = sanitize_patch(dirty);
        assert!(!sanitized.contains('\x1b'));
        assert!(!sanitized.contains('\r'));
        assert_eq!(split_patch_into_file_chunks(&sanitized).len(), 1);
    }

    #[test]
    fn decodes_git_quoted_utf8_paths() {
        let patch = "diff --git \"a/caf\\303\\251.rs\" \"b/caf\\303\\251.rs\"\n--- \"a/caf\\303\\251.rs\"\n+++ \"b/caf\\303\\251.rs\"\n@@ -1 +1 @@\n-a\n+b\n";
        let changeset = parse_patch(
            patch,
            "quoted",
            "Quoted",
            ChangesetSource::Patch {
                label: "quoted".into(),
            },
        )
        .unwrap();
        assert_eq!(changeset.files[0].path, "café.rs");
    }

    #[test]
    fn recognizes_binary_and_rename_metadata() {
        let patch = "diff --git a/old.png b/new.png\nsimilarity index 100%\nrename from old.png\nrename to new.png\nBinary files a/old.png and b/new.png differ\n";
        let changeset = parse_patch(
            patch,
            "binary",
            "Binary",
            ChangesetSource::Patch {
                label: "binary".into(),
            },
        )
        .unwrap();
        let file = &changeset.files[0];
        assert_eq!(file.change_kind, FileChangeKind::Renamed);
        assert_eq!(file.previous_path.as_deref(), Some("old.png"));
        assert_eq!(file.path, "new.png");
        assert!(file.flags.binary);
    }

    #[test]
    fn single_file_patch_uses_authoritative_paths_and_rejects_multiple_files() {
        let file = parse_single_file_patch(PATCH, "actual/name.mts", Some("old/name.mts"))
            .expect("one file parses");
        assert_eq!(file.path, "actual/name.mts");
        assert_eq!(file.previous_path.as_deref(), Some("old/name.mts"));
        assert_eq!(file.language.as_deref(), Some("typescript"));

        let twice = format!("{PATCH}{}", PATCH.replace("main.rs", "lib.rs"));
        assert!(matches!(
            parse_single_file_patch(&twice, "actual.rs", None),
            Err(PatchError::ExpectedOneFile { count: 2, .. })
        ));
    }

    #[test]
    fn captures_deterministic_git_moved_line_colors_before_sanitizing() {
        let patch = concat!(
            "diff --git a/file.txt b/file.txt\n",
            "--- a/file.txt\n",
            "+++ b/file.txt\n",
            "@@ -1,2 +1,2 @@\n",
            "\x1b[1;35m-old moved\x1b[0m\n",
            "-old plain\n",
            "\x1b[36m+new moved\x1b[0m\n",
            "+new plain\n",
        );
        let changeset = parse_patch(
            patch,
            "moved",
            "Moved",
            ChangesetSource::Patch {
                label: "moved".into(),
            },
        )
        .unwrap();
        let lines = &changeset.files[0].hunks[0].lines;
        assert!(lines[0].moved);
        assert!(!lines[1].moved);
        assert!(lines[2].moved);
        assert!(!lines[3].moved);
        assert!(!changeset.files[0].patch.contains('\x1b'));
    }

    #[test]
    fn compares_text_snapshots_with_pierre_compatible_explicit_hunk_counts() {
        let file = diff_from_file_snapshots(
            FileSnapshot {
                cache_key: "old-cache",
                contents: "one\ntwo\n",
                name: "old name.txt",
            },
            FileSnapshot {
                cache_key: "new-cache",
                contents: "one\nthree\nadded\n",
                name: "new name.txt",
            },
            FileComparisonOptions { context_radius: 3 },
        )
        .unwrap();

        assert_eq!(file.path, "new name.txt");
        assert_eq!(file.previous_path.as_deref(), Some("old name.txt"));
        assert_eq!(file.stats.additions, 2);
        assert_eq!(file.stats.deletions, 1);
        assert_eq!(file.hunks[0].formatted_header(), "@@ -1,2 +1,3 @@");
        assert!(file.runtime_id.starts_with("old-cache:new-cache:"));
    }

    #[test]
    fn identical_text_snapshots_remain_an_addressable_empty_file() {
        let file = diff_from_file_snapshots(
            FileSnapshot {
                cache_key: "same-before",
                contents: "same\n",
                name: "same.txt",
            },
            FileSnapshot {
                cache_key: "same-after",
                contents: "same\n",
                name: "same.txt",
            },
            FileComparisonOptions::default(),
        )
        .unwrap();

        assert_eq!(file.path, "same.txt");
        assert!(file.hunks.is_empty());
        assert_eq!(file.stats, FileStats::default());
    }
}
