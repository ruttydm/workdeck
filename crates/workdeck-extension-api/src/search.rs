//! The bundled `/` content search: pure patch-search primitives plus one session.
//!
//! Everything in the first half is a derivation of the changeset the host
//! already owns, so it is testable without a terminal, a host, or a review. The
//! session holds the stateful half: one query, one target list, and one cursor
//! into it, rebuilt whenever the visible-file corpus it was handed changes.

use std::collections::BTreeMap;

use regex::RegexBuilder;
use serde_json::Value;
use workdeck_core::ReviewSide;

use crate::{
    ExtensionDiffFile, ExtensionFileSide, ExtensionStatusSpan, ExtensionStatusTone, HighlightTone,
    ValidatedLineHighlight,
};

/// How a query string is interpreted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SearchMode {
    #[default]
    Literal,
    Regex,
}

/// Which way a repeat search walks the target list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchDirection {
    Forward,
    Backward,
}

/// Where one match sits inside a line: `[start, end)` in UTF-16 code units.
pub type MatchRange = (u64, u64);

/// One matching line inside a hunk, as it is reported back to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatchLine {
    /// Hunk-local position, counted over the hunk's rendered lines.
    pub offset: usize,
    /// The first match's extent within `text`, so the diff can mark it.
    pub match_range: MatchRange,
    /// Line number on `side`, or `None` when the patch carried no usable numbers.
    pub line_number: Option<u32>,
    pub side: ExtensionFileSide,
    /// The line's text with its diff marker stripped.
    pub text: String,
}

/// One place the review can jump to.
///
/// Hunk-granular stepping with a line-exact landing: collapsing every match
/// inside a hunk into one target is what keeps `n` visibly moving on every
/// press, while `line` carries the first match's own position so the jump lands
/// on it rather than on the hunk's anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchTarget {
    pub file_id: String,
    pub path: String,
    pub hunk_index: usize,
    /// The first matching line in this hunk — what the status row quotes.
    pub line: SearchMatchLine,
    /// How many lines in this hunk matched, including `line`.
    pub count: usize,
}

/// A compiled user query: every non-overlapping match on a line, or why it failed.
///
/// `locate` returns ranges in text order, or an empty vector for a
/// non-matching line. The diff marks every range; hunk-granular targets keep
/// only the first range of their first matching line. Zero-width regex matches
/// mark one character.
#[derive(Debug)]
pub enum CompiledQuery {
    Literal {
        needle: String,
        case_sensitive: bool,
    },
    Regex {
        pattern: regex::Regex,
    },
    Invalid {
        error: String,
    },
}

impl CompiledQuery {
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !matches!(self, Self::Invalid { .. })
    }

    #[must_use]
    pub fn error(&self) -> Option<&str> {
        match self {
            Self::Invalid { error } => Some(error),
            _ => None,
        }
    }

    /// Every non-overlapping `[start, end)` range on one line, in UTF-16 units.
    #[must_use]
    pub fn locate(&self, line: &str) -> Vec<MatchRange> {
        match self {
            Self::Invalid { .. } => Vec::new(),
            Self::Literal {
                needle,
                case_sensitive,
            } => locate_literal(line, needle, *case_sensitive),
            Self::Regex { pattern } => locate_regex(pattern, line),
        }
    }
}

/// Compile a user query into a line match locator.
///
/// Smart case in both modes: an all-lowercase query is case-insensitive, and
/// any uppercase character makes the whole query case-sensitive — the
/// convention `less -I`, vim, and ripgrep users already have in their fingers.
#[must_use]
pub fn compile_query(query: &str, mode: SearchMode) -> CompiledQuery {
    if query.trim().is_empty() {
        return CompiledQuery::Invalid {
            error: "empty search".into(),
        };
    }

    let case_sensitive = query.chars().any(char::is_uppercase);

    if mode == SearchMode::Regex {
        return match RegexBuilder::new(query)
            .case_insensitive(!case_sensitive)
            .build()
        {
            Ok(pattern) => CompiledQuery::Regex { pattern },
            Err(error) => CompiledQuery::Invalid {
                error: error.to_string(),
            },
        };
    }

    let needle = if case_sensitive {
        query.to_owned()
    } else {
        query.to_lowercase()
    };
    CompiledQuery::Literal {
        needle,
        case_sensitive,
    }
}

fn locate_literal(line: &str, needle: &str, case_sensitive: bool) -> Vec<MatchRange> {
    if needle.is_empty() {
        return Vec::new();
    }
    let folded;
    let (text, needle) = if case_sensitive {
        (line, needle)
    } else {
        // Case-insensitive ranges live in the folded text, exactly where the
        // match ran; ASCII and near-ASCII lines fold without shifting offsets.
        folded = line.to_lowercase();
        (folded.as_str(), needle)
    };
    let mut ranges = Vec::new();
    let mut cursor = 0;
    while let Some(found) = text[cursor..].find(needle) {
        let start = cursor + found;
        let end = start + needle.len();
        ranges.push((utf16_offset(text, start), utf16_offset(text, end)));
        cursor = end;
    }
    ranges
}

fn locate_regex(pattern: &regex::Regex, line: &str) -> Vec<MatchRange> {
    let mut ranges = Vec::new();
    for found in pattern.find_iter(line) {
        // Give zero-width matches a visible character and advance past them so
        // the next match cannot overlap or loop at the same position.
        let start = utf16_offset(line, found.start());
        let width = found.as_str().encode_utf16().count();
        let end = start + u64::try_from(width.max(1)).unwrap_or(u64::MAX);
        ranges.push((start, end));
    }
    ranges
}

/// UTF-16 code units before one byte offset, assuming `text[..offset]` splits a char boundary.
fn utf16_offset(text: &str, offset: usize) -> u64 {
    u64::try_from(text[..offset].encode_utf16().count()).unwrap_or(u64::MAX)
}

/// One parsed patch line, tagged with the hunk it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchLine {
    pub hunk_index: usize,
    pub offset: usize,
    pub line_number: Option<u32>,
    pub side: ExtensionFileSide,
    pub text: String,
}

/// Parse one unified-diff patch into its searchable lines, hunk by hunk.
#[must_use]
pub fn parse_patch_lines(patch: &str) -> Vec<PatchLine> {
    let mut lines = Vec::new();
    let mut hunk_index = None;
    let mut offset = 0_usize;
    let mut old_line: Option<u32> = None;
    let mut new_line: Option<u32> = None;

    for raw in patch.split('\n') {
        if raw.starts_with("@@") {
            hunk_index = Some(hunk_index.map_or(0, |index| index + 1));
            offset = 0;
            let (old_start, new_start) = read_hunk_starts(raw);
            old_line = old_start;
            new_line = new_start;
            continue;
        }

        // Everything before the first `@@` is file header noise (`diff --git`,
        // `index`, `---`, `+++`), and `\ No newline at end of file` is a marker
        // rather than content.
        let Some(hunk) = hunk_index else {
            continue;
        };
        if raw.starts_with('\\') {
            continue;
        }

        let marker = raw.chars().next();
        let text = marker.map_or(raw, |_| &raw[1..]);

        match marker {
            Some('-') => {
                lines.push(PatchLine {
                    hunk_index: hunk,
                    offset,
                    line_number: old_line,
                    side: ExtensionFileSide::Old,
                    text: text.to_owned(),
                });
                old_line = old_line.map(|line| line.saturating_add(1));
            }
            Some('+') => {
                lines.push(PatchLine {
                    hunk_index: hunk,
                    offset,
                    line_number: new_line,
                    side: ExtensionFileSide::New,
                    text: text.to_owned(),
                });
                new_line = new_line.map(|line| line.saturating_add(1));
            }
            Some(' ') | None => {
                // An empty line in a patch is an unmarked context line.
                lines.push(PatchLine {
                    hunk_index: hunk,
                    offset,
                    line_number: new_line,
                    side: ExtensionFileSide::New,
                    text: text.to_owned(),
                });
                old_line = old_line.map(|line| line.saturating_add(1));
                new_line = new_line.map(|line| line.saturating_add(1));
            }
            // Not a body line (a stray header inside patch text): skip without
            // advancing either counter.
            _ => continue,
        }

        offset += 1;
    }

    lines
}

/// Read the old/new starting line numbers out of an `@@` header.
fn read_hunk_starts(header: &str) -> (Option<u32>, Option<u32>) {
    let Some(rest) = header.strip_prefix("@@ ") else {
        return (None, None);
    };
    let mut parts = rest.split(' ');
    let old = parts
        .next()
        .and_then(|range| range.strip_prefix('-'))
        .and_then(parse_range_start);
    let new = parts
        .next()
        .and_then(|range| range.strip_prefix('+'))
        .and_then(parse_range_start);
    (old, new)
}

fn parse_range_start(range: &str) -> Option<u32> {
    range.split(',').next()?.parse::<u32>().ok()
}

/// Build every jump target for one compiled query, in review-stream order.
///
/// Files are walked in changeset order and hunks in render order, so the
/// resulting vector is already the order `n` should visit — no sorting, and no
/// second notion of "next" to keep in step with the review stream.
#[must_use]
pub fn find_targets(files: &[ExtensionDiffFile], query: &CompiledQuery) -> Vec<SearchTarget> {
    let mut targets: Vec<SearchTarget> = Vec::new();

    for file in files {
        if file.patch.is_empty() {
            continue;
        }

        let mut current: Option<usize> = None;
        for line in parse_patch_lines(&file.patch) {
            let Some(match_range) = query.locate(&line.text).first().copied() else {
                continue;
            };

            if let Some(index) = current
                && targets[index].hunk_index == line.hunk_index
            {
                targets[index].count += 1;
                continue;
            }

            targets.push(SearchTarget {
                file_id: file.id.clone(),
                path: file.path.clone(),
                hunk_index: line.hunk_index,
                line: SearchMatchLine {
                    offset: line.offset,
                    match_range,
                    line_number: line.line_number,
                    side: line.side,
                    text: line.text,
                },
                count: 1,
            });
            current = Some(targets.len() - 1);
        }
    }

    targets
}

/// The landed target a marks pass should single out, as the search tracks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchCurrentTarget {
    pub file_id: String,
    pub hunk_index: usize,
    pub line_offset: usize,
}

/// Build the diff marks for one file's matches, in source coordinates.
///
/// Mark every occurrence, but give only the first range of the active target's
/// quoted line the [`HighlightTone::Current`] tone. Stepping stays
/// hunk-granular, so later ranges on the landed line keep the ordinary
/// [`HighlightTone::Match`] tone.
#[must_use]
pub fn collect_file_match_marks(
    file: &ExtensionDiffFile,
    query: &CompiledQuery,
    current_target: Option<&SearchCurrentTarget>,
) -> Vec<ValidatedLineHighlight> {
    if file.patch.is_empty() {
        return Vec::new();
    }

    let mut marks = Vec::new();
    for line in parse_patch_lines(&file.patch) {
        // A line the patch numbers ambiguously cannot be addressed; skip its
        // mark rather than guessing — the hunk jump still lands nearby.
        let Some(line_number) = line.line_number else {
            continue;
        };
        let is_current = current_target.is_some_and(|current| {
            current.file_id == file.id
                && current.hunk_index == line.hunk_index
                && current.line_offset == line.offset
        });
        for (index, range) in query.locate(&line.text).iter().enumerate() {
            marks.push(ValidatedLineHighlight {
                side: match line.side {
                    ExtensionFileSide::Old => ReviewSide::Old,
                    ExtensionFileSide::New => ReviewSide::New,
                },
                line: u64::from(line_number),
                start: range.0,
                end: range.1,
                tone: if is_current && index == 0 {
                    HighlightTone::Current
                } else {
                    HighlightTone::Match
                },
            });
        }
    }

    marks
}

/// Where the review is pointing, as the search compares positions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchPosition {
    pub file_id: Option<String>,
    pub hunk_index: Option<usize>,
}

/// The result of stepping through the target list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchStep {
    pub index: usize,
    /// True when the step ran off one end and continued from the other.
    pub wrapped: bool,
}

/// Find the next target in `direction`, wrapping like `less` does.
///
/// Both directions are strict: a repeat never answers with the hunk the user is
/// already on, so `n` always moves. When the current hunk is the only match,
/// the wrap brings it back around — which is why wrapping is unconditional
/// here rather than a setting.
#[must_use]
pub fn step_to_target(
    targets: &[SearchTarget],
    file_order: &BTreeMap<String, usize>,
    from: &SearchPosition,
    direction: SearchDirection,
) -> Option<SearchStep> {
    if targets.is_empty() {
        return None;
    }

    let current = position_rank(file_order, from);
    let Some(current) = current else {
        // No usable selection: start at whichever end the direction implies.
        return Some(SearchStep {
            index: match direction {
                SearchDirection::Forward => 0,
                SearchDirection::Backward => targets.len() - 1,
            },
            wrapped: false,
        });
    };

    match direction {
        SearchDirection::Forward => {
            let index = targets
                .iter()
                .position(|target| target_rank(file_order, target) > current);
            Some(match index {
                Some(index) => SearchStep {
                    index,
                    wrapped: false,
                },
                None => SearchStep {
                    index: 0,
                    wrapped: true,
                },
            })
        }
        SearchDirection::Backward => {
            for index in (0..targets.len()).rev() {
                if target_rank(file_order, &targets[index]) < current {
                    return Some(SearchStep {
                        index,
                        wrapped: false,
                    });
                }
            }
            Some(SearchStep {
                index: targets.len() - 1,
                wrapped: true,
            })
        }
    }
}

/// Order files by their changeset position so positions compare as one number.
#[must_use]
pub fn build_file_order(files: &[ExtensionDiffFile]) -> BTreeMap<String, usize> {
    files
        .iter()
        .enumerate()
        .map(|(index, file)| (file.id.clone(), index))
        .collect()
}

/// Collapse a (file, hunk) pair into one comparable rank.
///
/// Hunk counts per file are unbounded in principle, so the rank is a pair
/// flattened with a large stride rather than an arithmetic trick — the stride is
/// only ever compared, never decoded.
const HUNK_RANK_STRIDE: i128 = 1_000_000;

fn target_rank(file_order: &BTreeMap<String, usize>, target: &SearchTarget) -> i128 {
    let file = file_order.get(&target.file_id).copied().unwrap_or(0);
    i128::try_from(file).unwrap_or(i128::MAX) * HUNK_RANK_STRIDE
        + i128::try_from(target.hunk_index).unwrap_or(i128::MAX)
}

fn position_rank(file_order: &BTreeMap<String, usize>, position: &SearchPosition) -> Option<i128> {
    let file_id = position.file_id.as_ref()?;
    let file = file_order.get(file_id).copied()?;

    // A file with no selected hunk ranks just before its first hunk, so a
    // forward search from a freshly opened file finds that file's own first
    // match.
    let hunk = position
        .hunk_index
        .map_or(-1, |index| i128::try_from(index).unwrap_or(i128::MAX));
    Some(i128::try_from(file).unwrap_or(i128::MAX) * HUNK_RANK_STRIDE + hunk)
}

// ---------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------

/// What a search or repeat asks the host to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchOutcome {
    Moved {
        target: SearchTarget,
        index: usize,
        total: usize,
        wrapped: bool,
    },
    NoMatches {
        query: String,
    },
    NoQuery,
    InvalidQuery {
        query: String,
        error: String,
    },
}

/// Options for one search session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SearchSessionOptions {
    pub mode: SearchMode,
}

/// One query, one target list, one cursor into it, for the process's lifetime.
///
/// The session never holds a review of its own: every search and repeat
/// receives the visible files from the invoking command's selection and
/// rebuilds targets when that list changes identity. Hidden files therefore
/// never become targets, and a reload keeps the query while orphaning the old
/// current target, whose file objects and hunk indexes no longer exist.
#[derive(Debug)]
pub struct SearchSession {
    mode: SearchMode,
    /// Identity of the last corpus targets were built from: the slice's start
    /// address and length, the Rust stand-in for upstream's array identity. A
    /// raw address is never dereferenced, only compared.
    corpus: Option<(usize, usize)>,
    file_order: BTreeMap<String, usize>,
    query: Option<String>,
    targets: Vec<SearchTarget>,
    /// The target the review last jumped to — what `marks_for` paints as current.
    current: Option<SearchTarget>,
}

impl SearchSession {
    #[must_use]
    pub fn new(options: SearchSessionOptions) -> Self {
        Self {
            mode: options.mode,
            corpus: None,
            file_order: BTreeMap::new(),
            query: None,
            targets: Vec::new(),
            current: None,
        }
    }

    /// The active query, or `None` before the first successful search or after `clear`.
    #[must_use]
    pub fn query(&self) -> Option<&str> {
        self.query.as_deref()
    }

    /// How many hunks the active query matches in the last corpus searched.
    #[must_use]
    pub fn total(&self) -> usize {
        self.targets.len()
    }

    /// Run a new query over `files` from `position`, like typing `/pattern` in `less`.
    pub fn search(
        &mut self,
        query: &str,
        files: &[ExtensionDiffFile],
        position: &SearchPosition,
    ) -> SearchOutcome {
        let compiled = compile_query(query, self.mode);
        if let Some(error) = compiled.error() {
            // A bad query never clobbers a working one: the previous search
            // stays repeatable with `n`.
            return SearchOutcome::InvalidQuery {
                query: query.to_owned(),
                error: error.to_owned(),
            };
        }

        self.query = Some(query.to_owned());
        self.corpus = Some((files.as_ptr() as usize, files.len()));
        self.file_order = build_file_order(files);
        self.current = None;
        self.targets = find_targets(files, &compiled);
        self.move_to(position, SearchDirection::Forward)
    }

    /// Repeat the active query over `files`, like `n` / `N`.
    pub fn repeat(
        &mut self,
        direction: SearchDirection,
        files: &[ExtensionDiffFile],
        position: &SearchPosition,
    ) -> SearchOutcome {
        self.adopt_corpus(files);
        self.move_to(position, direction)
    }

    /// Forget the query so the diff paints no marks and repeats report no query.
    pub fn clear(&mut self) {
        self.query = None;
        self.targets.clear();
        self.current = None;
    }

    /// The diff marks the active query paints on one file.
    ///
    /// `None` before the first search, so an idle session paints nothing.
    #[must_use]
    pub fn marks_for(&self, file: &ExtensionDiffFile) -> Option<Vec<ValidatedLineHighlight>> {
        let query = self.query.as_ref()?;
        let compiled = compile_query(query, self.mode);
        if !compiled.is_valid() {
            return None;
        }
        let current = self.current.as_ref().map(|current| SearchCurrentTarget {
            file_id: current.file_id.clone(),
            hunk_index: current.hunk_index,
            line_offset: current.line.offset,
        });
        Some(collect_file_match_marks(file, &compiled, current.as_ref()))
    }

    /// Rebuild targets when the corpus changed identity; a rebuilt list orphans
    /// the current target.
    fn adopt_corpus(&mut self, files: &[ExtensionDiffFile]) {
        let identity = (files.as_ptr() as usize, files.len());
        if self.corpus == Some(identity) {
            return;
        }

        self.corpus = Some(identity);
        self.file_order = build_file_order(files);
        self.current = None;
        let Some(query) = self.query.clone() else {
            self.targets.clear();
            return;
        };
        let compiled = compile_query(&query, self.mode);
        self.targets = if compiled.is_valid() {
            find_targets(files, &compiled)
        } else {
            Vec::new()
        };
    }

    fn move_to(&mut self, position: &SearchPosition, direction: SearchDirection) -> SearchOutcome {
        let Some(query) = self.query.clone() else {
            return SearchOutcome::NoQuery;
        };

        if self.targets.is_empty() {
            return SearchOutcome::NoMatches { query };
        }

        let Some(step) = step_to_target(&self.targets, &self.file_order, position, direction)
        else {
            return SearchOutcome::NoMatches { query };
        };
        let target = self.targets[step.index].clone();

        self.current = Some(target.clone());
        SearchOutcome::Moved {
            target,
            index: step.index + 1,
            total: self.targets.len(),
            wrapped: step.wrapped,
        }
    }
}

/// Render one outcome as the status-row spans the user reads, with symbolic tones.
///
/// A hit reads `[i/n] path:line (+k in hunk) • wrapped — quoted text`; misses
/// and bad queries borrow the diff's removal tone so they stand out from the
/// muted location text.
#[must_use]
pub fn format_outcome_spans(outcome: &SearchOutcome) -> Vec<ExtensionStatusSpan> {
    match outcome {
        SearchOutcome::Moved {
            target,
            index,
            total,
            wrapped,
        } => {
            let location = match target.line.line_number {
                Some(line) => format!("{}:{line}", target.path),
                None => target.path.clone(),
            };
            let more = if target.count > 1 {
                format!(" (+{} in hunk)", target.count - 1)
            } else {
                String::new()
            };
            let wrap = if *wrapped { " • wrapped" } else { "" };
            vec![
                status_span(
                    format!("[{index}/{total}] "),
                    Some(ExtensionStatusTone::Accent),
                ),
                status_span(
                    format!("{location}{more}{wrap}"),
                    Some(ExtensionStatusTone::Muted),
                ),
                status_span(
                    format!(" — {}", truncate_search_quote(target.line.text.trim(), 60)),
                    None,
                ),
            ]
        }
        SearchOutcome::NoMatches { query } => vec![status_span(
            format!("No match for \"{query}\""),
            Some(ExtensionStatusTone::Removed),
        )],
        SearchOutcome::NoQuery => vec![status_span(
            "No search yet — press / to search".into(),
            Some(ExtensionStatusTone::Muted),
        )],
        SearchOutcome::InvalidQuery { query, error } => vec![status_span(
            format!("Bad search \"{query}\" • {error}"),
            Some(ExtensionStatusTone::Removed),
        )],
    }
}

fn status_span(text: String, tone: Option<ExtensionStatusTone>) -> ExtensionStatusSpan {
    ExtensionStatusSpan {
        text,
        tone,
        attributes: Vec::new(),
    }
}

/// Clip quoted source text so the status item stays one readable line.
fn truncate_search_quote(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let clipped: String = text.chars().take(max - 1).collect();
    format!("{clipped}…")
}

/// Encode validated search marks in the line-highlighter wire shape.
///
/// The bundled search feeds the host's shared highlight pipeline, which
/// validates every provider's marks as JSON `{side, line, range, tone}` entries.
#[must_use]
pub fn search_marks_wire_value(marks: &[ValidatedLineHighlight]) -> Value {
    Value::Array(
        marks
            .iter()
            .map(|mark| {
                serde_json::json!({
                    "side": match mark.side {
                        ReviewSide::Old => "old",
                        ReviewSide::New => "new",
                    },
                    "line": mark.line,
                    "range": [mark.start, mark.end],
                    "tone": match mark.tone {
                        HighlightTone::Current => "current",
                        _ => "match",
                    },
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search_file(id: &str, path: &str, patch: &str) -> ExtensionDiffFile {
        ExtensionDiffFile {
            id: id.into(),
            path: path.into(),
            previous_path: None,
            patch: patch.into(),
            language: None,
            stats: crate::ExtensionDiffStats {
                additions: 0,
                deletions: 0,
            },
            metadata: serde_json::json!({}),
            change_type: None,
            stats_truncated: false,
            hunks: Vec::new(),
            agent: None,
            is_untracked: false,
            is_binary: false,
            is_too_large: false,
        }
    }

    /// Two hunks: three `readConfig` matches in the first, none in the second.
    fn alpha() -> ExtensionDiffFile {
        search_file(
            "file-0",
            "src/alpha.ts",
            &[
                "diff --git a/src/alpha.ts b/src/alpha.ts",
                "--- a/src/alpha.ts",
                "+++ b/src/alpha.ts",
                "@@ -10,3 +10,4 @@ function alpha() {",
                " const keep = 1;",
                "-const removed = readConfig();",
                "+const added = readConfig();",
                "+const second = readConfig();",
                "@@ -40,2 +41,2 @@",
                " untouched",
                "+const late = 2;",
            ]
            .join("\n"),
        )
    }

    /// One hunk with a single `readConfig` match.
    fn beta() -> ExtensionDiffFile {
        search_file(
            "file-1",
            "src/beta.ts",
            &["@@ -1,2 +1,2 @@", " context", "+readConfig();"].join("\n"),
        )
    }

    /// One hunk with two occurrences per line on both sides.
    fn repeated() -> ExtensionDiffFile {
        search_file(
            "file-repeated",
            "src/repeated.ts",
            &[
                "@@ -1 +1 @@",
                "-readConfig(); readConfig();",
                "+readConfig(); readConfig();",
            ]
            .join("\n"),
        )
    }

    fn files() -> Vec<ExtensionDiffFile> {
        vec![alpha(), beta()]
    }

    fn nowhere() -> SearchPosition {
        SearchPosition::default()
    }

    fn at(file_id: &str, hunk_index: usize) -> SearchPosition {
        SearchPosition {
            file_id: Some(file_id.into()),
            hunk_index: Some(hunk_index),
        }
    }

    #[test]
    fn parse_patch_lines_numbers_both_sides_and_groups_lines_by_hunk() {
        let lines = parse_patch_lines(&alpha().patch);

        let projected = lines
            .iter()
            .map(|line| {
                (
                    line.hunk_index,
                    line.side,
                    line.line_number,
                    line.text.as_str(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            projected,
            [
                (0, ExtensionFileSide::New, Some(10), "const keep = 1;"),
                (
                    0,
                    ExtensionFileSide::Old,
                    Some(11),
                    "const removed = readConfig();"
                ),
                (
                    0,
                    ExtensionFileSide::New,
                    Some(11),
                    "const added = readConfig();"
                ),
                (
                    0,
                    ExtensionFileSide::New,
                    Some(12),
                    "const second = readConfig();"
                ),
                (1, ExtensionFileSide::New, Some(41), "untouched"),
                (1, ExtensionFileSide::New, Some(42), "const late = 2;"),
            ]
        );
    }

    #[test]
    fn parse_patch_lines_skips_file_headers_and_no_newline_markers() {
        let lines = parse_patch_lines(
            &[
                "diff --git a/x b/x",
                "index 1..2",
                "@@ -1 +1 @@",
                "+only",
                "\\ No newline at end of file",
            ]
            .join("\n"),
        );

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "only");
    }

    #[test]
    fn compile_query_is_case_insensitive_until_the_query_carries_uppercase() {
        let lower = compile_query("readconfig", SearchMode::Literal);
        let upper = compile_query("ReadConfig", SearchMode::Literal);

        assert_eq!(
            lower.locate("const x = ReadConfig();"),
            [(10, 20)],
            "smart case matches either spelling"
        );
        assert_eq!(upper.locate("const x = readconfig();"), []);
    }

    #[test]
    fn literal_mode_does_not_interpret_regex_metacharacters() {
        let compiled = compile_query("readConfig(", SearchMode::Literal);

        assert_eq!(compiled.locate("readConfig();"), [(0, 11)]);
    }

    #[test]
    fn regex_mode_compiles_patterns_and_reports_bad_ones() {
        let compiled = compile_query("read(Config|Value)", SearchMode::Regex);
        assert_eq!(compiled.locate("a readValue()"), [(2, 11)]);

        let broken = compile_query("read(", SearchMode::Regex);
        assert!(!broken.is_valid());
    }

    #[test]
    fn a_zero_width_regex_match_still_marks_a_visible_position() {
        let compiled = compile_query("^", SearchMode::Regex);

        assert_eq!(compiled.locate("anything"), [(0, 1)]);
    }

    #[test]
    fn both_modes_return_non_overlapping_ranges() {
        for mode in [SearchMode::Literal, SearchMode::Regex] {
            let compiled = compile_query("aa", mode);
            assert!(compiled.is_valid(), "{mode:?} query should compile");

            assert_eq!(compiled.locate("aaaaa"), [(0, 2), (2, 4)]);
            assert_eq!(compiled.locate("AAaa"), [(0, 2), (2, 4)]);
            assert_eq!(compiled.locate("none"), []);
            assert_eq!(compiled.locate("aa"), [(0, 2)]);
        }
    }

    #[test]
    fn zero_width_regex_matches_advance_and_reset_between_lines_including_at_the_end() {
        // The Rust regex dialect has no look-ahead; `\b` exercises the same
        // advancing contract as upstream's `(?=a)|$`: every zero-width match
        // marks one visible character, none overlap, and repeated calls restart
        // from the beginning.
        let compiled = compile_query("\\b", SearchMode::Regex);
        assert!(compiled.is_valid());

        assert_eq!(compiled.locate("aa bb"), [(0, 1), (2, 3), (3, 4), (5, 6)]);
        assert_eq!(compiled.locate("aa bb"), [(0, 1), (2, 3), (3, 4), (5, 6)]);

        let end = compile_query("$", SearchMode::Regex);
        assert_eq!(end.locate(""), [(0, 1)]);
        assert_eq!(end.locate("aa"), [(2, 3)]);
    }

    #[test]
    fn both_modes_preserve_surrounding_whitespace_and_smart_case() {
        for mode in [SearchMode::Literal, SearchMode::Regex] {
            let compiled = compile_query(" foo ", mode);
            let upper = compile_query(" Foo ", mode);
            assert!(compiled.is_valid() && upper.is_valid());

            assert_eq!(compiled.locate("foo"), []);
            assert_eq!(compiled.locate("foo "), []);
            assert_eq!(compiled.locate(" foo"), []);
            assert_eq!(compiled.locate(" Foo "), [(0, 5)]);
            assert_eq!(upper.locate(" foo "), []);
            assert_eq!(upper.locate(" Foo "), [(0, 5)]);
        }
    }

    #[test]
    fn an_empty_query_is_refused() {
        assert!(!compile_query("   ", SearchMode::Literal).is_valid());
    }

    #[test]
    fn find_targets_collapses_every_match_in_a_hunk_into_one_target_in_stream_order() {
        let compiled = compile_query("readConfig", SearchMode::Literal);
        let corpus = files();

        let targets = find_targets(&corpus, &compiled);

        let projected = targets
            .iter()
            .map(|target| {
                (
                    target.path.as_str(),
                    target.hunk_index,
                    target.count,
                    target.line.line_number,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            projected,
            [
                ("src/alpha.ts", 0, 3, Some(11)),
                ("src/beta.ts", 0, 1, Some(2)),
            ]
        );
    }

    #[test]
    fn find_targets_skips_files_with_no_patch_text() {
        let compiled = compile_query("anything", SearchMode::Literal);

        assert_eq!(
            find_targets(&[search_file("file-0", "bin.png", "")], &compiled),
            []
        );
    }

    #[test]
    fn step_to_target_moves_strictly_forward_from_the_current_hunk() {
        let compiled = compile_query("readConfig", SearchMode::Literal);
        let corpus = files();
        let targets = find_targets(&corpus, &compiled);
        let order = build_file_order(&corpus);

        assert_eq!(
            step_to_target(&targets, &order, &at("file-0", 0), SearchDirection::Forward),
            Some(SearchStep {
                index: 1,
                wrapped: false
            })
        );
    }

    #[test]
    fn step_to_target_wraps_at_the_end_and_reports_it() {
        let compiled = compile_query("readConfig", SearchMode::Literal);
        let corpus = files();
        let targets = find_targets(&corpus, &compiled);
        let order = build_file_order(&corpus);

        assert_eq!(
            step_to_target(&targets, &order, &at("file-1", 0), SearchDirection::Forward),
            Some(SearchStep {
                index: 0,
                wrapped: true
            })
        );
    }

    #[test]
    fn step_to_target_walks_backward_and_wraps_at_the_start() {
        let compiled = compile_query("readConfig", SearchMode::Literal);
        let corpus = files();
        let targets = find_targets(&corpus, &compiled);
        let order = build_file_order(&corpus);

        assert_eq!(
            step_to_target(
                &targets,
                &order,
                &at("file-1", 0),
                SearchDirection::Backward
            ),
            Some(SearchStep {
                index: 0,
                wrapped: false
            })
        );
        assert_eq!(
            step_to_target(
                &targets,
                &order,
                &at("file-0", 0),
                SearchDirection::Backward
            ),
            Some(SearchStep {
                index: 1,
                wrapped: true
            })
        );
    }

    #[test]
    fn a_file_selected_with_no_hunk_finds_that_files_own_first_match() {
        let compiled = compile_query("readConfig", SearchMode::Literal);
        let corpus = files();
        let targets = find_targets(&corpus, &compiled);
        let order = build_file_order(&corpus);

        assert_eq!(
            step_to_target(
                &targets,
                &order,
                &SearchPosition {
                    file_id: Some("file-0".into()),
                    hunk_index: None,
                },
                SearchDirection::Forward
            ),
            Some(SearchStep {
                index: 0,
                wrapped: false
            })
        );
    }

    #[test]
    fn no_selection_starts_at_the_end_the_direction_implies() {
        let compiled = compile_query("readConfig", SearchMode::Literal);
        let corpus = files();
        let targets = find_targets(&corpus, &compiled);
        let order = build_file_order(&corpus);

        assert_eq!(
            step_to_target(&targets, &order, &nowhere(), SearchDirection::Forward),
            Some(SearchStep {
                index: 0,
                wrapped: false
            })
        );
        assert_eq!(
            step_to_target(&targets, &order, &nowhere(), SearchDirection::Backward),
            Some(SearchStep {
                index: 1,
                wrapped: false
            })
        );
    }

    #[test]
    fn an_empty_target_list_has_nowhere_to_go() {
        let order = build_file_order(&[]);
        assert_eq!(
            step_to_target(&[], &order, &at("file-0", 0), SearchDirection::Forward),
            None
        );
    }

    fn current(file_id: &str, hunk_index: usize, line_offset: usize) -> SearchCurrentTarget {
        SearchCurrentTarget {
            file_id: file_id.into(),
            hunk_index,
            line_offset,
        }
    }

    #[test]
    fn both_modes_mark_every_occurrence_but_keep_only_the_first_landed_range_current() {
        for mode in [SearchMode::Literal, SearchMode::Regex] {
            let query = compile_query("readConfig", mode);
            assert!(query.is_valid());

            let file = repeated();
            let marks =
                collect_file_match_marks(&file, &query, Some(&current("file-repeated", 0, 0)));
            let projected = marks
                .iter()
                .map(|mark| (mark.side, mark.line, (mark.start, mark.end), mark.tone))
                .collect::<Vec<_>>();
            assert_eq!(
                projected,
                [
                    (ReviewSide::Old, 1, (0, 10), HighlightTone::Current),
                    (ReviewSide::Old, 1, (14, 24), HighlightTone::Match),
                    (ReviewSide::New, 1, (0, 10), HighlightTone::Match),
                    (ReviewSide::New, 1, (14, 24), HighlightTone::Match),
                ]
            );
            let targets = find_targets(&[file], &query);
            assert_eq!(targets.len(), 1);
            assert_eq!(targets[0].count, 2);
            assert_eq!(targets[0].line.match_range, (0, 10));
            assert_eq!(targets[0].line.text, "readConfig(); readConfig();");
        }
    }

    #[test]
    fn marks_cover_every_matching_line_on_its_own_side_with_the_matched_extent() {
        let compiled = compile_query("readConfig", SearchMode::Literal);

        let marks = collect_file_match_marks(&alpha(), &compiled, None);

        let projected = marks
            .iter()
            .map(|mark| (mark.side, mark.line, (mark.start, mark.end), mark.tone))
            .collect::<Vec<_>>();
        assert_eq!(
            projected,
            [
                (ReviewSide::Old, 11, (16, 26), HighlightTone::Match),
                (ReviewSide::New, 11, (14, 24), HighlightTone::Match),
                (ReviewSide::New, 12, (15, 25), HighlightTone::Match),
            ]
        );
    }

    #[test]
    fn marks_give_the_active_targets_quoted_line_the_one_current_mark() {
        let compiled = compile_query("readConfig", SearchMode::Literal);

        let marks = collect_file_match_marks(&alpha(), &compiled, Some(&current("file-0", 0, 1)));

        assert_eq!(
            marks.iter().map(|mark| mark.tone).collect::<Vec<_>>(),
            [
                HighlightTone::Current,
                HighlightTone::Match,
                HighlightTone::Match
            ]
        );
    }

    #[test]
    fn a_current_target_in_another_file_marks_nothing_current_here() {
        let compiled = compile_query("readConfig", SearchMode::Literal);

        let marks = collect_file_match_marks(&alpha(), &compiled, Some(&current("file-1", 0, 1)));

        assert!(marks.iter().all(|mark| mark.tone == HighlightTone::Match));
    }

    #[test]
    fn a_fresh_search_jumps_forward_from_the_current_position() {
        let corpus = files();
        let mut session = SearchSession::new(SearchSessionOptions::default());

        let outcome = session.search("readConfig", &corpus, &at("file-0", 0));

        assert!(matches!(
            &outcome,
            SearchOutcome::Moved {
                index: 2,
                total: 2,
                wrapped: false,
                ..
            } if &outcome_target(&outcome).path == "src/beta.ts"
        ));
    }

    fn outcome_target(outcome: &SearchOutcome) -> &SearchTarget {
        match outcome {
            SearchOutcome::Moved { target, .. } => target,
            _ => panic!("expected a moved outcome: {outcome:?}"),
        }
    }

    #[test]
    fn n_and_n_walk_the_same_list_in_both_directions() {
        let corpus = files();
        let mut session = SearchSession::new(SearchSessionOptions::default());
        session.search("readConfig", &corpus, &nowhere());

        assert!(matches!(
            session.repeat(SearchDirection::Forward, &corpus, &at("file-0", 0)),
            SearchOutcome::Moved { index: 2, .. }
        ));
        assert!(matches!(
            session.repeat(SearchDirection::Backward, &corpus, &at("file-1", 0)),
            SearchOutcome::Moved { index: 1, .. }
        ));
    }

    #[test]
    fn repeats_wrap_strictly_so_the_only_match_comes_back_around() {
        let corpus = vec![beta()];
        let mut session = SearchSession::new(SearchSessionOptions::default());
        session.search("readConfig", &corpus, &nowhere());

        // Already on the one matching hunk: a strict forward step runs off the
        // end and wraps to the same hunk rather than answering "no movement".
        assert!(matches!(
            session.repeat(SearchDirection::Forward, &corpus, &at("file-1", 0)),
            SearchOutcome::Moved {
                index: 1,
                total: 1,
                wrapped: true,
                ..
            }
        ));
    }

    #[test]
    fn repeats_follow_the_live_selection_not_the_last_landing() {
        let corpus = files();
        let mut session = SearchSession::new(SearchSessionOptions::default());
        session.search("readConfig", &corpus, &nowhere());
        assert!(matches!(
            session.repeat(SearchDirection::Forward, &corpus, &at("file-0", 0)),
            SearchOutcome::Moved { index: 2, .. }
        ));

        // From the landing (beta) another `n` would wrap; the user moved back
        // to alpha's second hunk by hand, so `n` steps from there and does not
        // wrap.
        assert!(matches!(
            session.repeat(SearchDirection::Forward, &corpus, &at("file-0", 1)),
            SearchOutcome::Moved {
                index: 2,
                wrapped: false,
                ..
            }
        ));
        assert!(matches!(
            session.repeat(SearchDirection::Forward, &corpus, &at("file-1", 0)),
            SearchOutcome::Moved {
                index: 1,
                wrapped: true,
                ..
            }
        ));
    }

    #[test]
    fn both_modes_keep_trailing_whitespace_for_prompt_prefill_marks_and_corpus_rebuilds() {
        for mode in [SearchMode::Literal, SearchMode::Regex] {
            let file = search_file("whitespace", "spaces.ts", "@@ -0,0 +1 @@\n+foo fooX");
            let replacement = search_file("whitespace", "spaces.ts", "@@ -0,0 +1 @@\n+fooX");

            let mut session = SearchSession::new(SearchSessionOptions { mode });
            assert!(matches!(
                session.search("foo ", std::slice::from_ref(&file), &nowhere()),
                SearchOutcome::Moved { total: 1, .. }
            ));
            assert_eq!(session.query(), Some("foo "));
            assert_eq!(
                session
                    .marks_for(&file)
                    .unwrap()
                    .iter()
                    .map(|mark| (mark.side, mark.line, (mark.start, mark.end), mark.tone))
                    .collect::<Vec<_>>(),
                [(
                    workdeck_core::ReviewSide::New,
                    1,
                    (0, 4),
                    HighlightTone::Current
                )]
            );
            assert_eq!(
                session.repeat(
                    SearchDirection::Forward,
                    std::slice::from_ref(&replacement),
                    &nowhere()
                ),
                SearchOutcome::NoMatches {
                    query: "foo ".into()
                }
            );
            assert_eq!(session.query(), Some("foo "));
            assert_eq!(session.marks_for(&replacement), Some(Vec::new()));
        }
    }

    #[test]
    fn a_query_with_no_matches_reports_itself_instead_of_moving() {
        let corpus = files();
        let mut session = SearchSession::new(SearchSessionOptions::default());

        assert_eq!(
            session.search("nowhere", &corpus, &nowhere()),
            SearchOutcome::NoMatches {
                query: "nowhere".into()
            }
        );
    }

    #[test]
    fn repeating_before_any_search_says_so() {
        let corpus = files();
        let mut session = SearchSession::new(SearchSessionOptions::default());

        assert_eq!(
            session.repeat(SearchDirection::Forward, &corpus, &nowhere()),
            SearchOutcome::NoQuery
        );
    }

    #[test]
    fn an_invalid_query_leaves_the_previous_one_repeatable() {
        let corpus = files();
        let mut session = SearchSession::new(SearchSessionOptions {
            mode: SearchMode::Regex,
        });
        session.search("readConfig", &corpus, &nowhere());

        assert!(matches!(
            session.search("read(", &corpus, &nowhere()),
            SearchOutcome::InvalidQuery { .. }
        ));
        assert_eq!(session.query(), Some("readConfig"));
        assert!(matches!(
            session.repeat(SearchDirection::Forward, &corpus, &nowhere()),
            SearchOutcome::Moved { .. }
        ));
    }

    #[test]
    fn an_empty_query_is_refused_as_invalid_rather_than_clearing_anything() {
        let corpus = files();
        let mut session = SearchSession::new(SearchSessionOptions::default());
        session.search("readConfig", &corpus, &nowhere());

        assert!(matches!(
            session.search("   ", &corpus, &nowhere()),
            SearchOutcome::InvalidQuery { .. }
        ));
        assert_eq!(session.query(), Some("readConfig"));
    }

    #[test]
    fn clear_forgets_the_query_its_marks_and_its_targets() {
        let corpus = files();
        let mut session = SearchSession::new(SearchSessionOptions::default());
        session.search("readConfig", &corpus, &nowhere());

        session.clear();

        assert_eq!(session.query(), None);
        assert_eq!(session.total(), 0);
        assert_eq!(session.marks_for(&corpus[0]), None);
        assert_eq!(
            session.repeat(SearchDirection::Forward, &corpus, &nowhere()),
            SearchOutcome::NoQuery
        );
    }

    #[test]
    fn a_changed_corpus_rematches_the_live_query_against_the_new_files() {
        let corpus = files();
        let mut session = SearchSession::new(SearchSessionOptions::default());
        session.search("readConfig", &corpus, &nowhere());
        assert_eq!(session.total(), 2);

        // The filter hid alpha, or a reload replaced the files: the next repeat
        // sees only what is visible now.
        let visible = vec![beta()];
        assert!(matches!(
            session.repeat(SearchDirection::Forward, &visible, &nowhere()),
            SearchOutcome::Moved {
                index: 1,
                total: 1,
                ..
            }
        ));
        assert_eq!(session.total(), 1);
    }

    #[test]
    fn hidden_files_never_become_targets() {
        let mut session = SearchSession::new(SearchSessionOptions::default());

        let visible = vec![beta()];
        let outcome = session.search("readConfig", &visible, &at("file-1", 0));

        assert!(matches!(
            &outcome,
            SearchOutcome::Moved {
                index: 1,
                total: 1,
                wrapped: true,
                ..
            }
        ));
        assert_eq!(
            session
                .marks_for(&alpha())
                .unwrap()
                .iter()
                .map(|mark| mark.tone)
                .collect::<Vec<_>>(),
            [
                HighlightTone::Match,
                HighlightTone::Match,
                HighlightTone::Match
            ]
        );
    }

    #[test]
    fn session_marks_paint_nothing_before_the_first_search_and_marks_after_it() {
        let corpus = files();
        let mut session = SearchSession::new(SearchSessionOptions::default());

        assert_eq!(session.marks_for(&corpus[0]), None);

        session.search("readConfig", &corpus, &nowhere());

        assert_eq!(
            session
                .marks_for(&corpus[0])
                .unwrap()
                .iter()
                .map(|mark| mark.tone)
                .collect::<Vec<_>>(),
            [
                HighlightTone::Current,
                HighlightTone::Match,
                HighlightTone::Match
            ]
        );
        assert_eq!(
            session
                .marks_for(&corpus[1])
                .unwrap()
                .iter()
                .map(|mark| mark.tone)
                .collect::<Vec<_>>(),
            [HighlightTone::Match]
        );
    }

    #[test]
    fn both_modes_mark_every_occurrence_while_quoting_the_landed_line_once() {
        for mode in [SearchMode::Literal, SearchMode::Regex] {
            let corpus = vec![repeated()];
            let mut session = SearchSession::new(SearchSessionOptions { mode });

            let outcome = session.search("readConfig", &corpus, &nowhere());

            assert_eq!(session.total(), 1);
            assert_eq!(
                session
                    .marks_for(&corpus[0])
                    .unwrap()
                    .iter()
                    .map(|mark| (mark.side, mark.line, (mark.start, mark.end), mark.tone))
                    .collect::<Vec<_>>(),
                [
                    (ReviewSide::Old, 1, (0, 10), HighlightTone::Current),
                    (ReviewSide::Old, 1, (14, 24), HighlightTone::Match),
                    (ReviewSide::New, 1, (0, 10), HighlightTone::Match),
                    (ReviewSide::New, 1, (14, 24), HighlightTone::Match),
                ]
            );
            let spans = format_outcome_spans(&outcome);
            assert_eq!(spans.len(), 3);
            assert_eq!(spans[0].text, "[1/1] ");
            assert_eq!(spans[1].text, "src/repeated.ts:1 (+1 in hunk)");
            assert_eq!(spans[2].text, " — readConfig(); readConfig();");
        }
    }

    #[test]
    fn the_current_mark_follows_n_across_files() {
        let corpus = files();
        let mut session = SearchSession::new(SearchSessionOptions::default());
        session.search("readConfig", &corpus, &nowhere());

        session.repeat(SearchDirection::Forward, &corpus, &at("file-0", 0));

        assert!(
            session
                .marks_for(&corpus[0])
                .unwrap()
                .iter()
                .all(|mark| mark.tone == HighlightTone::Match)
        );
        assert_eq!(
            session
                .marks_for(&corpus[1])
                .unwrap()
                .iter()
                .map(|mark| mark.tone)
                .collect::<Vec<_>>(),
            [HighlightTone::Current]
        );
    }

    #[test]
    fn a_rebuilt_corpus_keeps_the_query_but_drops_the_orphaned_current_mark() {
        let corpus = files();
        let mut session = SearchSession::new(SearchSessionOptions::default());
        session.search("readConfig", &corpus, &nowhere());
        assert_eq!(
            session.marks_for(&corpus[0]).unwrap()[0].tone,
            HighlightTone::Current
        );

        // Same content, new identity — what a reload hands the next command. The
        // old current target belongs to file objects that no longer exist.
        let rebuilt = vec![alpha(), beta()];
        session.repeat(SearchDirection::Forward, &rebuilt, &at("file-1", 0));

        assert_eq!(session.query(), Some("readConfig"));
        assert_eq!(
            session
                .marks_for(&rebuilt[0])
                .unwrap()
                .iter()
                .map(|mark| mark.tone)
                .collect::<Vec<_>>(),
            [
                HighlightTone::Current,
                HighlightTone::Match,
                HighlightTone::Match
            ]
        );
    }

    #[test]
    fn a_match_reads_like_a_less_status_line() {
        let corpus = files();
        let mut session = SearchSession::new(SearchSessionOptions::default());

        let outcome = session.search("readConfig", &corpus, &nowhere());

        let spans = format_outcome_spans(&outcome);
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].text, "[1/2] ");
        assert_eq!(spans[1].text, "src/alpha.ts:11 (+2 in hunk)");
        assert_eq!(spans[2].text, " — const removed = readConfig();");
    }

    #[test]
    fn a_wrap_and_a_miss_are_both_said_out_loud() {
        let corpus = files();
        let mut session = SearchSession::new(SearchSessionOptions::default());
        session.search("readConfig", &corpus, &nowhere());

        let wrapped = session.repeat(SearchDirection::Forward, &corpus, &at("file-1", 0));
        assert!(
            format_outcome_spans(&wrapped)
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>()
                .contains("• wrapped")
        );
        assert_eq!(
            format_outcome_spans(&SearchOutcome::NoMatches {
                query: "zzz".into()
            })
            .len(),
            1
        );
        assert_eq!(
            format_outcome_spans(&SearchOutcome::NoMatches {
                query: "zzz".into()
            })[0]
                .text,
            "No match for \"zzz\""
        );
        assert_eq!(
            format_outcome_spans(&SearchOutcome::NoQuery)[0].text,
            "No search yet — press / to search"
        );
        assert_eq!(
            format_outcome_spans(&SearchOutcome::InvalidQuery {
                query: "(".into(),
                error: "bad".into()
            })[0]
                .text,
            "Bad search \"(\" • bad"
        );
    }

    #[test]
    fn quoted_source_text_clips_to_one_readable_line() {
        let long = "x".repeat(61);
        assert_eq!(truncate_search_quote(&long, 60).chars().count(), 60);
        assert!(truncate_search_quote(&long, 60).ends_with('…'));
        assert_eq!(truncate_search_quote("short", 60), "short");
    }

    #[test]
    fn search_marks_encode_as_the_line_highlighter_wire_shape() {
        let marks = collect_file_match_marks(
            &repeated(),
            &compile_query("readConfig", SearchMode::Literal),
            Some(&current("file-repeated", 0, 0)),
        );
        let value = search_marks_wire_value(&marks);

        assert_eq!(
            value,
            serde_json::json!([
                {"side": "old", "line": 1, "range": [0, 10], "tone": "current"},
                {"side": "old", "line": 1, "range": [14, 24], "tone": "match"},
                {"side": "new", "line": 1, "range": [0, 10], "tone": "match"},
                {"side": "new", "line": 1, "range": [14, 24], "tone": "match"},
            ])
        );
        assert!(
            marks_match_the_host_wire_contract(&value),
            "search marks must pass the shared highlighter validation"
        );
    }

    fn marks_match_the_host_wire_contract(value: &Value) -> bool {
        // Mirror the host validation contract without a host dependency: an
        // array of {side, line, range: [start, end], tone} with start < end.
        let Some(entries) = value.as_array() else {
            return false;
        };
        entries.iter().all(|entry| {
            let side = entry.get("side").and_then(Value::as_str);
            let line = entry.get("line").and_then(Value::as_u64);
            let range = entry.get("range").and_then(Value::as_array);
            let tone = entry.get("tone").and_then(Value::as_str);
            matches!(side, Some("old") | Some("new"))
                && line.is_some_and(|line| line >= 1)
                && range.is_some_and(|range| {
                    range.len() == 2
                        && range[0]
                            .as_u64()
                            .zip(range[1].as_u64())
                            .is_some_and(|(start, end)| start < end)
                })
                && matches!(tone, Some("match") | Some("current"))
        })
    }
}
