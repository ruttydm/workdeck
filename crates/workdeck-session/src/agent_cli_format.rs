//! Terminal-safe human and JSON output for Workdeck's live-session CLI.

use serde::Serialize;
use workdeck_core::{ReviewNoteSource, ReviewSide};
use workdeck_diff::{SanitizeOptions, sanitize_terminal_text};

use crate::{
    AppliedCommentBatchResult, AppliedCommentResult, AppliedHighlightResult, ClearedCommentsResult,
    ClearedHighlightsResult, ListedSession, NavigatedSelectionResult, ReloadedSessionResult,
    RemovedCommentResult, RevealedTarget, SelectedSessionContext, SessionLineHighlightTone,
    SessionLiveCommentSummary, SessionReview, SessionReviewNoteSummary, SessionSelector,
    SessionTerminalLocation, SessionTerminalMetadata, WorkdeckExperimentalFeature,
    WorkdeckSessionInputKind, describe_session_selector,
};

/// Render one serializable result with the CLI's stable two-space JSON indentation and final LF.
pub fn stringify_json<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(value).map(|mut output| {
        output.push('\n');
        output
    })
}

fn input_kind(kind: WorkdeckSessionInputKind) -> &'static str {
    match kind {
        WorkdeckSessionInputKind::Vcs => "vcs",
        WorkdeckSessionInputKind::Diff => "diff",
        WorkdeckSessionInputKind::Show => "show",
        WorkdeckSessionInputKind::StashShow => "stash-show",
        WorkdeckSessionInputKind::Patch => "patch",
        WorkdeckSessionInputKind::Difftool => "difftool",
    }
}

fn feature(feature: WorkdeckExperimentalFeature) -> &'static str {
    match feature {
        WorkdeckExperimentalFeature::Stml => "stml",
    }
}

fn side(side: ReviewSide) -> &'static str {
    match side {
        ReviewSide::Old => "old",
        ReviewSide::New => "new",
    }
}

fn note_source(source: ReviewNoteSource) -> &'static str {
    match source {
        ReviewNoteSource::Ai => "ai",
        ReviewNoteSource::Agent => "agent",
        ReviewNoteSource::User => "user",
    }
}

fn tone(tone: SessionLineHighlightTone) -> &'static str {
    match tone {
        SessionLineHighlightTone::Match => "match",
        SessionLineHighlightTone::Current => "current",
        SessionLineHighlightTone::Info => "info",
        SessionLineHighlightTone::Warning => "warning",
        SessionLineHighlightTone::Error => "error",
        SessionLineHighlightTone::Dim => "dim",
    }
}

fn format_session_path(path: &str) -> String {
    sanitize_terminal_text(
        path,
        SanitizeOptions {
            preserve_newlines: false,
            preserve_tabs: false,
            preserve_ansi_style: false,
        },
    )
}

fn format_session_selector(selector: &SessionSelector) -> String {
    format_session_path(&describe_session_selector(selector))
}

fn format_selected_summary(session: &ListedSession) -> String {
    session
        .snapshot
        .state
        .selected_file_path
        .as_deref()
        .map_or_else(
            || "(none)".into(),
            |path| {
                format!(
                    "{} hunk {}",
                    format_session_path(path),
                    session.snapshot.state.selected_hunk_index + 1
                )
            },
        )
}

fn format_terminal_location(location: &SessionTerminalLocation) -> String {
    let mut parts = Vec::new();
    if let Some(tty) = &location.tty {
        parts.push(tty.clone());
    }
    if let Some(window_id) = &location.window_id {
        parts.push(format!("window {window_id}"));
    }
    if let Some(tab_id) = &location.tab_id {
        parts.push(format!("tab {tab_id}"));
    }
    if let Some(pane_id) = &location.pane_id {
        parts.push(format!("pane {pane_id}"));
    }
    if let Some(terminal_id) = &location.terminal_id {
        parts.push(format!("terminal {terminal_id}"));
    }
    if let Some(session_id) = &location.session_id {
        parts.push(format!("session {session_id}"));
    }
    if parts.is_empty() {
        "present".into()
    } else {
        parts.join(", ")
    }
}

fn format_terminal_lines(
    terminal: Option<&SessionTerminalMetadata>,
    header_label: &str,
    location_label: &str,
) -> Vec<String> {
    let Some(terminal) = terminal else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    if let Some(program) = &terminal.program {
        lines.push(format!("{header_label}: {program}"));
    }
    lines.extend(terminal.locations.iter().map(|location| {
        format!(
            "{location_label}[{}]: {}",
            location.source,
            format_terminal_location(location)
        )
    }));
    lines
}

#[must_use]
pub fn format_list_output(sessions: &[ListedSession]) -> String {
    if sessions.is_empty() {
        return "No active Workdeck sessions.\n".into();
    }
    let entries = sessions
        .iter()
        .map(|session| {
            let mut lines = vec![
                format!(
                    "{}  {}",
                    session.session_id,
                    format_session_path(&session.title)
                ),
                format!("  path: {}", format_session_path(&session.cwd)),
                format!(
                    "  repo: {}",
                    session
                        .repo_root
                        .as_deref()
                        .map_or_else(|| "-".into(), format_session_path)
                ),
            ];
            lines.extend(format_terminal_lines(
                session.terminal.as_ref(),
                "  terminal",
                "  location",
            ));
            lines.extend([
                format!("  focus: {}", format_selected_summary(session)),
                format!("  files: {}", session.file_count),
                format!("  comments: {}", session.snapshot.state.live_comment_count),
            ]);
            lines.join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("{entries}\n")
}

#[must_use]
pub fn format_session_output(session: &ListedSession) -> String {
    let mut lines = vec![
        format!("Session: {}", session.session_id),
        format!("Title: {}", format_session_path(&session.title)),
        format!("Source: {}", format_session_path(&session.source_label)),
        format!("Path: {}", format_session_path(&session.cwd)),
        format!(
            "Repo: {}",
            session
                .repo_root
                .as_deref()
                .map_or_else(|| "-".into(), format_session_path)
        ),
        format!("Input: {}", input_kind(session.input_kind)),
    ];
    if let Some(features) = session
        .experimental_features
        .as_deref()
        .filter(|features| !features.is_empty())
    {
        lines.push(format!(
            "Experimental features: {}",
            features
                .iter()
                .copied()
                .map(feature)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    lines.push(format!("Launched: {}", session.launched_at));
    lines.extend(format_terminal_lines(
        session.terminal.as_ref(),
        "Terminal",
        "Location",
    ));
    lines.extend([
        format!("Selected: {}", format_selected_summary(session)),
        format!(
            "Agent notes visible: {}",
            if session.snapshot.state.show_agent_notes {
                "yes"
            } else {
                "no"
            }
        ),
        format!(
            "Live comments: {}",
            session.snapshot.state.live_comment_count
        ),
        "Files:".into(),
    ]);
    lines.extend(session.files.iter().map(|file| {
        format!(
            "  - {} (+{} -{}, hunks: {})",
            format_session_path(&file.path),
            file.additions,
            file.deletions,
            file.hunk_count
        )
    }));
    lines.push(String::new());
    lines.join("\n")
}

#[must_use]
pub fn format_context_output(context: &SelectedSessionContext) -> String {
    let selected_file = context
        .selected_file
        .as_ref()
        .map_or_else(|| "(none)".into(), |file| format_session_path(&file.path));
    let mut lines = vec![
        format!("Session: {}", context.session_id),
        format!("Title: {}", format_session_path(&context.title)),
        format!(
            "Path: {}",
            context
                .cwd
                .as_deref()
                .map_or_else(|| "-".into(), format_session_path)
        ),
        format!(
            "Repo: {}",
            context
                .repo_root
                .as_deref()
                .map_or_else(|| "-".into(), format_session_path)
        ),
        format!("File: {selected_file}"),
        format!(
            "Hunk: {}",
            context
                .selected_hunk
                .as_ref()
                .map_or_else(|| "-".into(), |hunk| (hunk.index + 1).to_string())
        ),
        format!(
            "Old range: {}",
            context
                .selected_hunk
                .as_ref()
                .and_then(|hunk| hunk.old_range)
                .map_or_else(|| "-".into(), |range| format!("{}..{}", range[0], range[1]))
        ),
        format!(
            "New range: {}",
            context
                .selected_hunk
                .as_ref()
                .and_then(|hunk| hunk.new_range)
                .map_or_else(|| "-".into(), |range| format!("{}..{}", range[0], range[1]))
        ),
        format!(
            "Agent notes visible: {}",
            if context.show_agent_notes {
                "yes"
            } else {
                "no"
            }
        ),
    ];
    if context
        .experimental_features
        .as_deref()
        .is_some_and(|features| features.contains(&WorkdeckExperimentalFeature::Stml))
    {
        lines.push(format!(
            "Experimental features: {}",
            context
                .experimental_features
                .as_deref()
                .unwrap_or_default()
                .iter()
                .copied()
                .map(feature)
                .collect::<Vec<_>>()
                .join(", ")
        ));
        lines.push(format!(
            "Note markup width: {}",
            context
                .note_markup_width
                .map_or_else(|| "-".into(), |width| width.to_string())
        ));
    }
    lines.push(format!("Live comments: {}", context.live_comment_count));
    lines.push(String::new());
    lines.join("\n")
}

#[must_use]
pub fn format_review_output(review: &SessionReview) -> String {
    let mut lines = vec![
        format!("Session: {}", review.session_id),
        format!("Title: {}", format_session_path(&review.title)),
        format!("Source: {}", format_session_path(&review.source_label)),
        format!(
            "Path: {}",
            review
                .cwd
                .as_deref()
                .map_or_else(|| "-".into(), format_session_path)
        ),
        format!(
            "Repo: {}",
            review
                .repo_root
                .as_deref()
                .map_or_else(|| "-".into(), format_session_path)
        ),
        format!("Input: {}", input_kind(review.input_kind)),
    ];
    if let Some(features) = review
        .experimental_features
        .as_deref()
        .filter(|features| !features.is_empty())
    {
        lines.push(format!(
            "Experimental features: {}",
            features
                .iter()
                .copied()
                .map(feature)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    lines.extend([
        format!(
            "Selected file: {}",
            review.selected_file.as_ref().map_or_else(
                || "(none)".into(),
                |file| format_session_path(&file.summary.path)
            )
        ),
        format!(
            "Selected hunk: {}",
            review
                .selected_hunk
                .as_ref()
                .map_or_else(|| "-".into(), |hunk| (hunk.index + 1).to_string())
        ),
        format!(
            "Agent notes visible: {}",
            if review.show_agent_notes { "yes" } else { "no" }
        ),
        format!("Live comments: {}", review.live_comment_count),
        format!(
            "Review notes: {}",
            review.review_note_count.unwrap_or_else(|| {
                review
                    .review_notes
                    .as_ref()
                    .map_or(0, |notes| notes.len() as u64)
            })
        ),
    ]);
    if let Some(notes) = &review.review_notes {
        lines.push("Notes:".into());
        lines.extend(notes.iter().map(|note| {
            format!(
                "  - {} [{}] {}: {}",
                note.note_id,
                note_source(note.source),
                format_session_path(&note.file_path),
                note.body
            )
        }));
    }
    lines.push("Files:".into());
    for file in &review.files {
        lines.push(format!(
            "  - {} (+{} -{}, hunks: {})",
            format_session_path(&file.summary.path),
            file.summary.additions,
            file.summary.deletions,
            file.summary.hunk_count
        ));
        lines.extend(
            file.hunks
                .iter()
                .map(|hunk| format!("      hunk {}: {}", hunk.index + 1, hunk.header)),
        );
    }
    lines.push(String::new());
    lines.join("\n")
}

#[must_use]
pub fn format_navigation_output(
    selector: &SessionSelector,
    result: &NavigatedSelectionResult,
) -> String {
    if result.revealed == Some(RevealedTarget::Line)
        && let (Some(line), Some(result_side)) = (result.line, result.side)
    {
        return format!(
            "Revealed {}:{line} ({}) in hunk {} of {}.\n",
            format_session_path(&result.file_path),
            side(result_side),
            result.hunk_index + 1,
            format_session_selector(selector)
        );
    }
    format!(
        "Focused {} hunk {} in {}.\n",
        format_session_path(&result.file_path),
        result.hunk_index + 1,
        format_session_selector(selector)
    )
}

#[must_use]
pub fn format_reload_output(selector: &SessionSelector, result: &ReloadedSessionResult) -> String {
    let selected = result.selected_file_path.as_deref().map_or_else(
        || "(no files)".into(),
        |path| {
            format!(
                "{} hunk {}",
                format_session_path(path),
                result.selected_hunk_index + 1
            )
        },
    );
    format!(
        "Reloaded {} with {} ({} files). Selected: {selected}.\n",
        format_session_selector(selector),
        format_session_path(&result.title),
        result.file_count
    )
}

fn format_markup_notes(result: &AppliedCommentResult, indent: &str) -> Vec<String> {
    let width_hint = result.markup_width.map_or_else(
        || " (preview with `workdeck markup render`)".into(),
        |width| format!(" (preview with `workdeck markup render - --width {width}`)"),
    );
    result
        .markup_notes
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|note| format!("{indent}Markup note: {note}{width_hint}."))
        .collect()
}

#[must_use]
pub fn format_comment_output(selector: &SessionSelector, result: &AppliedCommentResult) -> String {
    let mut lines = vec![format!(
        "Added live comment {} on {}:{} ({}) in hunk {} for {}.",
        result.comment_id,
        format_session_path(&result.file_path),
        result.line,
        side(result.side),
        result.hunk_index + 1,
        format_session_selector(selector)
    )];
    lines.extend(format_markup_notes(result, ""));
    format!("{}\n", lines.join("\n"))
}

#[must_use]
pub fn format_comment_apply_output(
    selector: &SessionSelector,
    result: &AppliedCommentBatchResult,
) -> String {
    if result.applied.is_empty() {
        return format!(
            "Applied 0 live comments to {}.\n",
            format_session_selector(selector)
        );
    }
    let mut lines = vec![format!(
        "Applied {} live comments to {}:",
        result.applied.len(),
        format_session_selector(selector)
    )];
    for comment in &result.applied {
        lines.push(format!(
            "  - {} on {}:{} ({}) hunk {}",
            comment.comment_id,
            format_session_path(&comment.file_path),
            comment.line,
            side(comment.side),
            comment.hunk_index + 1
        ));
        lines.extend(format_markup_notes(comment, "    "));
    }
    format!("{}\n", lines.join("\n"))
}

#[must_use]
pub fn format_comment_list_output(
    selector: &SessionSelector,
    comments: &[SessionLiveCommentSummary],
) -> String {
    if comments.is_empty() {
        return format!(
            "No live comments for {}.\n",
            format_session_selector(selector)
        );
    }
    let entries = comments
        .iter()
        .map(|comment| {
            let mut lines = vec![
                format!(
                    "{}  {}:{} ({})",
                    comment.comment_id,
                    format_session_path(&comment.file_path),
                    comment.line,
                    side(comment.side)
                ),
                format!("  hunk: {}", comment.hunk_index + 1),
                format!("  summary: {}", comment.summary),
            ];
            if let Some(author) = &comment.author {
                lines.push(format!("  author: {author}"));
            }
            lines.join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("{entries}\n")
}

#[must_use]
pub fn format_remove_comment_output(
    selector: &SessionSelector,
    result: &RemovedCommentResult,
) -> String {
    let label = if result.source == Some(ReviewNoteSource::User) {
        "user note"
    } else {
        "live comment"
    };
    format!(
        "Removed {label} {} from {}. Remaining comments: {}.\n",
        result.comment_id,
        format_session_selector(selector),
        result.remaining_comment_count
    )
}

#[must_use]
pub fn format_note_list_output(
    selector: &SessionSelector,
    notes: &[SessionReviewNoteSummary],
) -> String {
    if notes.is_empty() {
        return format!(
            "No review notes for {}.\n",
            format_session_selector(selector)
        );
    }
    let entries = notes
        .iter()
        .map(|note| {
            let mut lines = vec![format!(
                "{}  {} [{}]",
                note.note_id,
                format_session_path(&note.file_path),
                note_source(note.source)
            )];
            if let Some(index) = note.hunk_index {
                lines.push(format!("  hunk: {}", index + 1));
            }
            lines.push(format!("  body: {}", note.body));
            if let Some(author) = &note.author {
                lines.push(format!("  author: {author}"));
            }
            lines.join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("{entries}\n")
}

#[must_use]
pub fn format_highlight_output(
    selector: &SessionSelector,
    result: &AppliedHighlightResult,
) -> String {
    let reveal = match result.revealed {
        Some(RevealedTarget::Line) => " and revealed its line",
        Some(RevealedTarget::Hunk) => " and revealed its hunk",
        None => "",
    };
    format!(
        "Marked {}:{} ({}) [{}, {}) as {} in {}{reveal}. File marks: {}.\n",
        format_session_path(&result.file_path),
        result.line,
        side(result.side),
        result.start,
        result.end,
        tone(result.tone),
        format_session_selector(selector),
        result.file_mark_count
    )
}

#[must_use]
pub fn format_clear_highlights_output(
    selector: &SessionSelector,
    result: &ClearedHighlightsResult,
) -> String {
    let scope = result.file_path.as_deref().map_or_else(
        || format_session_selector(selector),
        |path| {
            format!(
                "{} in {}",
                format_session_path(path),
                format_session_selector(selector)
            )
        },
    );
    format!(
        "Cleared {} attention marks from {scope}. Remaining marks: {}.\n",
        result.removed_count, result.remaining_count
    )
}

#[must_use]
pub fn format_clear_comments_output(
    selector: &SessionSelector,
    result: &ClearedCommentsResult,
) -> String {
    let scope = result.file_path.as_deref().map_or_else(
        || format_session_selector(selector),
        |path| {
            format!(
                "{} in {}",
                format_session_path(path),
                format_session_selector(selector)
            )
        },
    );
    let label = if result.include_user.unwrap_or(false) {
        "comments"
    } else {
        "live comments"
    };
    format!(
        "Cleared {} {label} from {scope}. Remaining comments: {}.\n",
        result.removed_count, result.remaining_comment_count
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{
        SelectedHunkSummary, SessionFileSummary, SessionReviewFile, SessionReviewHunk,
        SessionSnapshot, WorkdeckSessionState,
    };

    fn selector() -> SessionSelector {
        SessionSelector {
            session_id: Some("session-1".into()),
            ..SessionSelector::default()
        }
    }

    fn session_file(path: &str, additions: u64, deletions: u64) -> SessionFileSummary {
        SessionFileSummary {
            id: format!("file-{path}"),
            path: path.into(),
            previous_path: None,
            additions,
            deletions,
            hunk_count: 1,
        }
    }

    fn listed_session() -> ListedSession {
        ListedSession {
            session_id: "session-1".into(),
            pid: 42,
            cwd: "/repo".into(),
            repo_root: Some("/repo".into()),
            launched_at: "2026-01-01T00:00:00Z".into(),
            terminal: None,
            input_kind: WorkdeckSessionInputKind::Vcs,
            title: "repo working tree".into(),
            source_label: "/repo".into(),
            experimental_features: None,
            file_count: 0,
            files: Vec::new(),
            snapshot: SessionSnapshot {
                updated_at: "2026-01-01T00:00:00Z".into(),
                state: WorkdeckSessionState {
                    selected_file_id: None,
                    selected_file_path: None,
                    selected_hunk_index: 0,
                    selected_hunk_old_range: None,
                    selected_hunk_new_range: None,
                    show_agent_notes: false,
                    note_markup_width: None,
                    live_comment_count: 0,
                    live_comments: Vec::new(),
                    review_note_count: None,
                    review_notes: None,
                    review_publication: None,
                },
            },
        }
    }

    fn context() -> SelectedSessionContext {
        SelectedSessionContext {
            session_id: "session-1".into(),
            title: "repo diff".into(),
            source_label: "/repo".into(),
            cwd: None,
            repo_root: None,
            input_kind: WorkdeckSessionInputKind::Diff,
            experimental_features: None,
            selected_file: None,
            selected_hunk: None,
            show_agent_notes: true,
            note_markup_width: None,
            live_comment_count: 2,
        }
    }

    fn comment_result() -> AppliedCommentResult {
        AppliedCommentResult {
            comment_id: "comment-1".into(),
            file_id: "file-1".into(),
            file_path: "src/app.ts".into(),
            hunk_index: 0,
            side: ReviewSide::New,
            line: 12,
            markup_width: None,
            markup_notes: None,
        }
    }

    #[test]
    fn json_output_is_pretty_and_newline_terminated() {
        assert_eq!(
            stringify_json(&json!({"sessions": [{"id": 1}]})).unwrap(),
            "{\n  \"sessions\": [\n    {\n      \"id\": 1\n    }\n  ]\n}\n"
        );
    }

    #[test]
    fn list_and_get_preserve_terminal_metadata_and_selected_hunk_summaries() {
        let mut session = listed_session();
        session.files = vec![session_file("src/app.ts", 3, 1)];
        session.file_count = 1;
        session.snapshot.state.selected_file_path = Some("src/app.ts".into());
        session.snapshot.state.selected_hunk_index = 2;
        session.snapshot.state.show_agent_notes = true;
        session.snapshot.state.live_comment_count = 4;
        session.terminal = Some(SessionTerminalMetadata {
            program: Some("ghostty".into()),
            locations: vec![
                SessionTerminalLocation {
                    source: "tty".into(),
                    tty: Some("/dev/ttys005".into()),
                    ..SessionTerminalLocation::default()
                },
                SessionTerminalLocation {
                    source: "tmux".into(),
                    pane_id: Some("%7".into()),
                    session_id: Some("work".into()),
                    ..SessionTerminalLocation::default()
                },
                SessionTerminalLocation {
                    source: "iterm2".into(),
                    window_id: Some("1".into()),
                    tab_id: Some("2".into()),
                    pane_id: Some("3".into()),
                    terminal_id: Some("abc".into()),
                    ..SessionTerminalLocation::default()
                },
                SessionTerminalLocation {
                    source: "unknown".into(),
                    ..SessionTerminalLocation::default()
                },
            ],
        });
        assert_eq!(
            format_list_output(&[session.clone()]),
            [
                "session-1  repo working tree",
                "  path: /repo",
                "  repo: /repo",
                "  terminal: ghostty",
                "  location[tty]: /dev/ttys005",
                "  location[tmux]: pane %7, session work",
                "  location[iterm2]: window 1, tab 2, pane 3, terminal abc",
                "  location[unknown]: present",
                "  focus: src/app.ts hunk 3",
                "  files: 1",
                "  comments: 4",
                "",
            ]
            .join("\n")
        );
        let output = format_session_output(&session);
        assert!(output.contains("Selected: src/app.ts hunk 3\n"));
        assert!(output.contains("Agent notes visible: yes\n"));
        assert!(output.contains("Live comments: 4\n"));
        assert!(output.contains("  - src/app.ts (+3 -1, hunks: 1)"));
    }

    #[test]
    fn human_readable_session_paths_cannot_emit_terminal_controls() {
        let unsafe_path = "src/日本語\x1b[2J\tline\n🧪.ts";
        let mut session = listed_session();
        session.title = unsafe_path.into();
        session.source_label = unsafe_path.into();
        session.cwd = unsafe_path.into();
        session.repo_root = Some(unsafe_path.into());
        session.files = vec![session_file(unsafe_path, 0, 0)];
        session.snapshot.state.selected_file_path = Some(unsafe_path.into());
        let output = format_session_output(&session);
        assert!(!output.contains('\x1b'));
        assert!(!output.contains('\t'));
        assert!(output.contains("src/日本語line🧪.ts"));

        let selector = SessionSelector {
            session_path: Some(unsafe_path.into()),
            ..SessionSelector::default()
        };
        let reload = ReloadedSessionResult {
            session_id: "session-1".into(),
            input_kind: WorkdeckSessionInputKind::Vcs,
            title: "repo working tree".into(),
            source_label: "/repo".into(),
            file_count: 1,
            selected_file_path: Some(unsafe_path.into()),
            selected_hunk_index: 0,
        };
        let output = format_reload_output(&selector, &reload);
        assert!(!output.contains('\x1b'));
        assert!(!output.contains('\t'));
        assert!(output.contains("session path src/日本語line🧪.ts"));
        assert!(output.contains("Selected: src/日本語line🧪.ts hunk 1"));
    }

    #[test]
    fn empty_context_and_stml_summaries_stay_explicit() {
        let session = listed_session();
        let mut selected_context = context();
        assert_eq!(format_list_output(&[]), "No active Workdeck sessions.\n");
        assert!(format_list_output(&[session]).contains("  focus: (none)\n"));
        assert_eq!(
            format_context_output(&selected_context),
            [
                "Session: session-1",
                "Title: repo diff",
                "Path: -",
                "Repo: -",
                "File: (none)",
                "Hunk: -",
                "Old range: -",
                "New range: -",
                "Agent notes visible: yes",
                "Live comments: 2",
                "",
            ]
            .join("\n")
        );
        selected_context.experimental_features = Some(Vec::new());
        assert!(!format_context_output(&selected_context).contains("markup"));
        selected_context.experimental_features = Some(vec![WorkdeckExperimentalFeature::Stml]);
        selected_context.note_markup_width = Some(72);
        let output = format_context_output(&selected_context);
        assert!(output.contains("Experimental features: stml\n"));
        assert!(output.contains("Note markup width: 72\n"));
    }

    #[test]
    fn review_output_keeps_file_order_hunks_notes_and_no_selection() {
        let files = vec![
            SessionReviewFile {
                summary: SessionFileSummary {
                    hunk_count: 2,
                    ..session_file("src/first.ts", 2, 1)
                },
                patch: None,
                hunks: vec![
                    SessionReviewHunk {
                        index: 0,
                        header: "@@ -1,1 +1,2 @@".into(),
                        old_range: None,
                        new_range: None,
                    },
                    SessionReviewHunk {
                        index: 1,
                        header: "@@ -10,1 +11,1 @@".into(),
                        old_range: None,
                        new_range: None,
                    },
                ],
            },
            SessionReviewFile {
                summary: session_file("src/second.ts", 0, 1),
                patch: None,
                hunks: vec![SessionReviewHunk {
                    index: 0,
                    header: "@@ -1,1 +1,1 @@".into(),
                    old_range: None,
                    new_range: None,
                }],
            },
        ];
        let review = SessionReview {
            session_id: "session-1".into(),
            title: "repo diff".into(),
            source_label: "/repo".into(),
            cwd: None,
            repo_root: Some("/repo".into()),
            input_kind: WorkdeckSessionInputKind::Diff,
            experimental_features: None,
            selected_file: None,
            selected_hunk: None,
            show_agent_notes: false,
            live_comment_count: 1,
            review_note_count: None,
            review_notes: None,
            files,
        };
        assert_eq!(
            format_review_output(&review),
            [
                "Session: session-1",
                "Title: repo diff",
                "Source: /repo",
                "Path: -",
                "Repo: /repo",
                "Input: diff",
                "Selected file: (none)",
                "Selected hunk: -",
                "Agent notes visible: no",
                "Live comments: 1",
                "Review notes: 0",
                "Files:",
                "  - src/first.ts (+2 -1, hunks: 2)",
                "      hunk 1: @@ -1,1 +1,2 @@",
                "      hunk 2: @@ -10,1 +11,1 @@",
                "  - src/second.ts (+0 -1, hunks: 1)",
                "      hunk 1: @@ -1,1 +1,1 @@",
                "",
            ]
            .join("\n")
        );
    }

    #[test]
    fn command_result_formatters_describe_navigation_and_comment_side_effects() {
        let selector = selector();
        let navigation = NavigatedSelectionResult {
            file_id: "file-1".into(),
            file_path: "src/app.ts".into(),
            hunk_index: 1,
            selected_hunk: None,
            revealed: None,
            side: None,
            line: None,
        };
        assert_eq!(
            format_navigation_output(&selector, &navigation),
            "Focused src/app.ts hunk 2 in session session-1.\n"
        );
        let reload = ReloadedSessionResult {
            session_id: "session-1".into(),
            input_kind: WorkdeckSessionInputKind::Vcs,
            title: "repo working tree".into(),
            source_label: "/repo".into(),
            file_count: 0,
            selected_file_path: None,
            selected_hunk_index: 0,
        };
        assert_eq!(
            format_reload_output(&selector, &reload),
            "Reloaded session session-1 with repo working tree (0 files). Selected: (no files).\n"
        );
        assert_eq!(
            format_comment_output(&selector, &comment_result()),
            "Added live comment comment-1 on src/app.ts:12 (new) in hunk 1 for session session-1.\n"
        );
        let mut markup = comment_result();
        markup.markup_notes = Some(vec!["unknown tag <sparkline>".into()]);
        assert_eq!(
            format_comment_output(&selector, &markup),
            "Added live comment comment-1 on src/app.ts:12 (new) in hunk 1 for session session-1.\nMarkup note: unknown tag <sparkline> (preview with `workdeck markup render`).\n"
        );
        assert_eq!(
            format_comment_apply_output(
                &selector,
                &AppliedCommentBatchResult {
                    applied: Vec::new()
                }
            ),
            "Applied 0 live comments to session session-1.\n"
        );
        let mut applied = comment_result();
        applied.comment_id = "comment-2".into();
        applied.hunk_index = 2;
        applied.side = ReviewSide::Old;
        applied.line = 8;
        assert_eq!(
            format_comment_apply_output(
                &selector,
                &AppliedCommentBatchResult {
                    applied: vec![applied]
                }
            ),
            "Applied 1 live comments to session session-1:\n  - comment-2 on src/app.ts:8 (old) hunk 3\n"
        );
        assert_eq!(
            format_comment_list_output(&selector, &[]),
            "No live comments for session session-1.\n"
        );
        let comments = [SessionLiveCommentSummary {
            comment_id: "comment-3".into(),
            file_path: "src/app.ts".into(),
            hunk_index: 1,
            side: ReviewSide::New,
            line: 20,
            summary: "Check this branch".into(),
            rationale: None,
            author: Some("pi".into()),
            created_at: "now".into(),
        }];
        assert_eq!(
            format_comment_list_output(&selector, &comments),
            "comment-3  src/app.ts:20 (new)\n  hunk: 2\n  summary: Check this branch\n  author: pi\n"
        );
        assert_eq!(
            format_remove_comment_output(
                &selector,
                &RemovedCommentResult {
                    comment_id: "comment-3".into(),
                    removed: true,
                    remaining_comment_count: 1,
                    source: None,
                }
            ),
            "Removed live comment comment-3 from session session-1. Remaining comments: 1.\n"
        );
        assert_eq!(
            format_clear_comments_output(
                &selector,
                &ClearedCommentsResult {
                    removed_count: 2,
                    remaining_comment_count: 3,
                    file_path: Some("src/app.ts".into()),
                    include_user: None,
                    removed_live_comment_count: None,
                    removed_user_note_count: None,
                    remaining_live_comment_count: None,
                    remaining_user_note_count: None,
                }
            ),
            "Cleared 2 live comments from src/app.ts in session session-1. Remaining comments: 3.\n"
        );
    }

    #[test]
    fn note_and_highlight_formatters_preserve_scope_reveals_and_coordinates() {
        let selector = selector();
        let notes = [SessionReviewNoteSummary {
            note_id: "note-1".into(),
            parent_id: None,
            source: ReviewNoteSource::User,
            file_path: "src/app.ts".into(),
            hunk_index: Some(1),
            old_range: None,
            new_range: None,
            body: "Keep this".into(),
            title: None,
            author: Some("Ada".into()),
            created_at: "now".into(),
            updated_at: None,
            editable: true,
        }];
        assert_eq!(
            format_note_list_output(&selector, &notes),
            "note-1  src/app.ts [user]\n  hunk: 2\n  body: Keep this\n  author: Ada\n"
        );
        let navigation = NavigatedSelectionResult {
            file_id: "file-1".into(),
            file_path: "src/app.ts".into(),
            hunk_index: 1,
            selected_hunk: Some(SelectedHunkSummary {
                index: 1,
                old_range: None,
                new_range: None,
            }),
            revealed: Some(RevealedTarget::Line),
            side: Some(ReviewSide::New),
            line: Some(42),
        };
        assert_eq!(
            format_navigation_output(&selector, &navigation),
            "Revealed src/app.ts:42 (new) in hunk 2 of session session-1.\n"
        );
        let highlight = AppliedHighlightResult {
            file_id: "file-1".into(),
            file_path: "src/app.ts".into(),
            hunk_index: 0,
            side: ReviewSide::New,
            line: 12,
            start: 2,
            end: 9,
            tone: SessionLineHighlightTone::Warning,
            file_mark_count: 3,
            revealed: Some(RevealedTarget::Line),
        };
        assert_eq!(
            format_highlight_output(&selector, &highlight),
            "Marked src/app.ts:12 (new) [2, 9) as warning in session session-1 and revealed its line. File marks: 3.\n"
        );
        assert_eq!(
            format_clear_highlights_output(
                &selector,
                &ClearedHighlightsResult {
                    removed_count: 2,
                    remaining_count: 1,
                    file_path: Some("src/app.ts".into()),
                }
            ),
            "Cleared 2 attention marks from src/app.ts in session session-1. Remaining marks: 1.\n"
        );
    }
}
