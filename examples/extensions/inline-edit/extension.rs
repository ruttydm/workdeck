//! Native Rust port of Hunk's interactive inline-edit file-view example.

use std::collections::BTreeSet;
use std::io::{self, BufRead, Write};

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use workdeck_core::{ChangesetSource, DiffFile, SourceOrigin};
use workdeck_extension_api::{
    API_VERSION, Capability, CommandExecution, CommandInvocation, CommandRegistration,
    ExtensionDiffFile, ExtensionFileChangeKind, ExtensionFileChangeRange,
    ExtensionFileViewHunkRows, ExtensionFileViewLayout, ExtensionFileViewRow,
    ExtensionFileViewSourceRange, ExtensionFileViewSpan, ExtensionFileViewTone,
    ExtensionHostAction, ExtensionKeyEvent, ExtensionNotifyType, ExtensionTextAttribute,
    ExtensionWorkspaceWriteCompletion, ExtensionWorkspaceWriteResult, FileViewLayoutRequest,
    FileViewMatchRequest, FileViewModeKeyRequest, FileViewModeLifecycleRequest, HandshakeResponse,
    JsonRpcError, JsonRpcRequest, JsonRpcResponse, KeyRoutingResult, KeyboardModeExecution,
    Registration, matches_key,
};

pub const VIEW_ID: &str = "inline-edit";
pub const COMMAND_ID: &str = "edit";
pub const REWRITE_COMMAND_ID: &str = "rewrite-selected";
pub const DELAYED_COMMAND_ID: &str = "delayed-next-hunk";
pub const FAIL_SYNC_COMMAND_ID: &str = "fail-sync";
pub const FAIL_ASYNC_COMMAND_ID: &str = "fail-async";
const HEADER_LABEL: &str = "EDITING — Esc exits · ctrl+s writes";
const MODIFIED_MARKER: &str = "  MODIFIED";
const CURSOR_MARK: &str = "▎";

#[derive(Debug, Clone)]
struct EditSession {
    file_id: String,
    path: String,
    newline: String,
    ends_with_newline: bool,
    lines: Vec<String>,
    hunk_ranges: Vec<[usize; 2]>,
    provenance: Vec<Vec<usize>>,
    saved_text: String,
    cursor_line: usize,
    cursor_column: usize,
    next_write_id: u64,
    pending_write: Option<String>,
}

impl EditSession {
    fn new(file: &DiffFile, text: &str, cursor_line: usize) -> Self {
        let newline = detect_newline(text).to_owned();
        let ends_with_newline = text.ends_with('\r') || text.ends_with('\n');
        let lines = split_document_lines(text);
        let saved_text = join_document(&lines, &newline, ends_with_newline);
        let last_line = lines.len().saturating_sub(1);
        Self {
            file_id: file.runtime_id.clone(),
            path: file.path.clone(),
            newline,
            ends_with_newline,
            provenance: (1..=lines.len()).map(|line| vec![line]).collect(),
            hunk_ranges: file
                .hunks
                .iter()
                .map(|hunk| {
                    let start = usize::try_from(hunk.new_start).unwrap_or(usize::MAX);
                    let count = usize::try_from(hunk.new_count).unwrap_or(usize::MAX);
                    [start, start.saturating_add(count).saturating_sub(1)]
                })
                .collect(),
            lines,
            saved_text,
            cursor_line: cursor_line.min(last_line),
            cursor_column: 0,
            next_write_id: 1,
            pending_write: None,
        }
    }

    fn text(&self) -> String {
        join_document(&self.lines, &self.newline, self.ends_with_newline)
    }

    fn modified(&self) -> bool {
        self.text() != self.saved_text
    }

    fn clamp_cursor(&mut self) {
        self.cursor_line = self.cursor_line.min(self.lines.len().saturating_sub(1));
        let line = &self.lines[self.cursor_line];
        self.cursor_column = snap_grapheme_boundary(line, self.cursor_column.min(line.len()));
    }

    fn insert(&mut self, text: &str) {
        let line = &mut self.lines[self.cursor_line];
        line.insert_str(self.cursor_column, text);
        self.cursor_column += text.len();
        self.clamp_cursor();
    }

    fn delete_backwards(&mut self) {
        let line = self.lines[self.cursor_line].clone();
        if self.cursor_column > 0 {
            let previous = previous_grapheme_boundary(&line, self.cursor_column);
            self.lines[self.cursor_line].replace_range(previous..self.cursor_column, "");
            self.cursor_column = previous;
            self.clamp_cursor();
            return;
        }
        if self.cursor_line == 0 {
            return;
        }
        let mut sources = self.provenance[self.cursor_line - 1].clone();
        sources.extend(self.provenance[self.cursor_line].iter().copied());
        let owners = sources
            .iter()
            .flat_map(|source| {
                self.hunk_ranges
                    .iter()
                    .enumerate()
                    .filter_map(move |(index, range)| {
                        (*source >= range[0] && *source <= range[1]).then_some(index)
                    })
            })
            .collect::<BTreeSet<_>>();
        if owners.len() > 1 {
            return;
        }
        let previous = self.lines[self.cursor_line - 1].clone();
        let joined = format!("{previous}{line}");
        self.lines
            .splice(self.cursor_line - 1..=self.cursor_line, [joined]);
        self.provenance
            .splice(self.cursor_line - 1..=self.cursor_line, [sources]);
        self.cursor_line -= 1;
        self.cursor_column = previous.len();
        self.clamp_cursor();
    }

    fn split_line(&mut self) {
        let line = self.lines[self.cursor_line].clone();
        let head = line[..self.cursor_column].to_owned();
        let tail = line[self.cursor_column..].to_owned();
        self.lines
            .splice(self.cursor_line..=self.cursor_line, [head, tail]);
        let sources = self.provenance[self.cursor_line].clone();
        self.provenance
            .splice(self.cursor_line..=self.cursor_line, [sources, Vec::new()]);
        self.cursor_line += 1;
        self.cursor_column = 0;
        self.clamp_cursor();
    }

    fn move_cursor(&mut self, name: &str) {
        if matches!(name, "up" | "down") {
            let column = grapheme_column(&self.lines[self.cursor_line], self.cursor_column);
            if name == "down" {
                self.cursor_line = (self.cursor_line + 1).min(self.lines.len() - 1);
            } else {
                self.cursor_line = self.cursor_line.saturating_sub(1);
            }
            self.cursor_column = boundary_at_grapheme_column(&self.lines[self.cursor_line], column);
            return;
        }
        if name == "left" {
            if self.cursor_column == 0 && self.cursor_line > 0 {
                self.cursor_line -= 1;
                self.cursor_column = self.lines[self.cursor_line].len();
            } else {
                self.cursor_column =
                    previous_grapheme_boundary(&self.lines[self.cursor_line], self.cursor_column);
            }
            self.clamp_cursor();
            return;
        }
        if self.cursor_column >= self.lines[self.cursor_line].len()
            && self.cursor_line < self.lines.len() - 1
        {
            self.cursor_line += 1;
            self.cursor_column = 0;
        } else {
            self.cursor_column =
                next_grapheme_boundary(&self.lines[self.cursor_line], self.cursor_column);
        }
        self.clamp_cursor();
    }
}

#[derive(Debug, Default)]
pub struct InlineEditExtension {
    session: Option<EditSession>,
}

impl InlineEditExtension {
    #[must_use]
    pub fn registrations() -> Vec<Registration> {
        vec![
            Registration::FileView {
                id: VIEW_ID.into(),
                title: "Inline edit".into(),
                priority: 0,
                interactive_mode: true,
            },
            Registration::Command(CommandRegistration {
                id: COMMAND_ID.into(),
                title: "Edit the selected file inline".into(),
                description: None,
                default_keys: vec!["ctrl+e".into()],
            }),
            Registration::Command(CommandRegistration {
                id: REWRITE_COMMAND_ID.into(),
                title: "Uppercase the selected file".into(),
                description: Some("Demonstrates command-context workspace reads and writes".into()),
                default_keys: vec!["f5".into()],
            }),
            Registration::Command(CommandRegistration {
                id: DELAYED_COMMAND_ID.into(),
                title: "Delayed next hunk".into(),
                description: Some("Demonstrates nonblocking async command context".into()),
                default_keys: vec!["f12".into()],
            }),
            Registration::Command(CommandRegistration {
                id: FAIL_SYNC_COMMAND_ID.into(),
                title: "Fail synchronously".into(),
                description: Some("Demonstrates contained command failures".into()),
                default_keys: vec!["f11".into()],
            }),
            Registration::Command(CommandRegistration {
                id: FAIL_ASYNC_COMMAND_ID.into(),
                title: "Fail after awaiting".into(),
                description: Some("Demonstrates contained async command failures".into()),
                default_keys: vec!["f7".into()],
            }),
        ]
    }

    #[must_use]
    pub fn required_capabilities() -> Vec<Capability> {
        vec![
            Capability::Commands,
            Capability::FileViews,
            Capability::Notifications,
            Capability::ReviewNavigation,
            Capability::WorkspaceRead,
            Capability::WorkspaceWrite,
        ]
    }

    pub fn invoke_command(
        &mut self,
        invocation: &CommandInvocation,
    ) -> Result<CommandExecution, String> {
        if invocation.command_id == FAIL_SYNC_COMMAND_ID {
            return Err("sync boom".into());
        }
        if invocation.command_id == FAIL_ASYNC_COMMAND_ID {
            std::thread::sleep(std::time::Duration::from_millis(25));
            return Err("async boom".into());
        }
        if invocation.command_id == DELAYED_COMMAND_ID {
            let captured_file_index = invocation.snapshot.selection.file_index;
            std::thread::sleep(std::time::Duration::from_millis(75));
            return Ok(CommandExecution {
                actions: vec![
                    ExtensionHostAction::Notify {
                        message: format!("Captured file index {captured_file_index}"),
                        notification_type: ExtensionNotifyType::Info,
                    },
                    ExtensionHostAction::ExecuteReviewCommand {
                        id: "workdeck.review.nextHunk".into(),
                        count: None,
                    },
                ],
            });
        }
        if invocation.command_id == REWRITE_COMMAND_ID {
            let Some(file) = invocation
                .snapshot
                .changeset
                .files
                .get(invocation.snapshot.selection.file_index)
            else {
                return Ok(warn("Select a file to rewrite"));
            };
            let Some(workspace) = invocation.workspace.as_ref() else {
                return Ok(warn("Workspace controls are unavailable for this command"));
            };
            if !workspace.can_write_document(&file.runtime_id) {
                return Ok(warn(format!(
                    "{} cannot be written from this review",
                    file.path
                )));
            }
            let Some(document) = workspace.read_document(
                &file.runtime_id,
                workdeck_extension_api::ExtensionFileSide::New,
            ) else {
                return Ok(warn(format!("{} could not be read", file.path)));
            };
            return Ok(CommandExecution {
                actions: vec![ExtensionHostAction::RequestWorkspaceWrite {
                    request_id: format!("rewrite-selected:{}", invocation.snapshot.generation),
                    file_id: file.runtime_id.clone(),
                    text: document.to_uppercase(),
                }],
            });
        }
        if invocation.command_id != COMMAND_ID {
            return Err(format!("Unknown command: {}", invocation.command_id));
        }
        if let Some(session) = &self.session {
            return Ok(notify(format!(
                "Already editing {} — Esc exits, ctrl+s writes",
                session.path
            )));
        }
        let Some(file) = invocation
            .snapshot
            .changeset
            .files
            .get(invocation.snapshot.selection.file_index)
        else {
            return Ok(warn("Select a file to edit"));
        };
        let can_write = invocation.workspace.as_ref().map_or_else(
            || can_write_document(&invocation.snapshot.changeset.source, file),
            |workspace| workspace.can_write_document(&file.runtime_id),
        );
        if !can_write {
            return Ok(warn(format!(
                "{} cannot be written from this review — inline edit needs a working-tree diff",
                file.path
            )));
        }
        let document = invocation
            .workspace
            .as_ref()
            .and_then(|workspace| {
                workspace.read_document(
                    &file.runtime_id,
                    workdeck_extension_api::ExtensionFileSide::New,
                )
            })
            .or_else(|| {
                file.sources
                    .new
                    .as_ref()
                    .map(|source| source.content.as_str())
            });
        let Some(document) = document else {
            return Ok(warn(format!("No readable document for {}", file.path)));
        };
        let selected_hunk = invocation
            .snapshot
            .selection
            .hunk_index
            .and_then(|index| file.hunks.get(index))
            .or_else(|| file.hunks.first());
        let cursor_line = selected_hunk.map_or(0, |hunk| {
            usize::try_from(hunk.new_start.saturating_sub(1)).unwrap_or(0)
        });
        self.session = Some(EditSession::new(file, document, cursor_line));
        Ok(CommandExecution {
            actions: vec![
                ExtensionHostAction::EnterFileViewMode { id: VIEW_ID.into() },
                ExtensionHostAction::RefreshFileView {
                    id: VIEW_ID.into(),
                    file_id: Some(file.runtime_id.clone()),
                },
            ],
        })
    }

    #[must_use]
    pub fn matches(file: &ExtensionDiffFile) -> bool {
        !file.is_binary && !file.is_too_large
    }

    #[must_use]
    pub fn layout(&self, input: &FileViewLayoutRequest) -> Option<ExtensionFileViewLayout> {
        if input.aborted || !Self::matches(&input.file) {
            return None;
        }
        let session = self
            .session
            .as_ref()
            .filter(|session| session.file_id == input.file.id);
        let document;
        let lines = if let Some(session) = session {
            &session.lines
        } else {
            document = split_document_lines(
                input
                    .documents
                    .get(&workdeck_extension_api::ExtensionFileSide::New)?
                    .as_deref()?,
            );
            &document
        };
        let number_width = lines.len().max(1).to_string().len();
        let text_width = input.width.saturating_sub(number_width + 2).max(1);
        let added = if session.is_none() {
            added_line_numbers(&input.changes)
        } else {
            BTreeSet::new()
        };
        let header_offset = usize::from(session.is_some());
        let provenance = session.map_or_else(
            || (1..=lines.len()).map(|line| vec![line]).collect::<Vec<_>>(),
            |session| session.provenance.clone(),
        );
        let hunk_ranges = input
            .file
            .hunks
            .iter()
            .map(|hunk| hunk.new_range.unwrap_or([1, 1]))
            .map(|range| [range[0] as usize, range[1] as usize])
            .collect::<Vec<_>>();
        let mut rows = Vec::with_capacity(lines.len() + header_offset);
        if let Some(session) = session {
            rows.push(header_row(session, input.width));
        }
        for (index, line) in lines.iter().enumerate() {
            let line_number = index + 1;
            let on_cursor = session.is_some_and(|session| index == session.cursor_line);
            let visible = truncate_cells(line, text_width);
            let mut spans = vec![ExtensionFileViewSpan {
                text: format!(
                    "{}{:>width$} ",
                    if on_cursor { CURSOR_MARK } else { " " },
                    line_number,
                    width = number_width
                ),
                tone: Some(if on_cursor {
                    ExtensionFileViewTone::Accent
                } else {
                    ExtensionFileViewTone::Muted
                }),
                attributes: Vec::new(),
            }];
            if let Some(session) = session.filter(|_| on_cursor) {
                let caret =
                    snap_grapheme_boundary(&visible, session.cursor_column.min(visible.len()));
                let caret_end = next_grapheme_boundary(&visible, caret);
                if caret > 0 {
                    spans.push(plain_span(&visible[..caret]));
                }
                spans.push(ExtensionFileViewSpan {
                    text: visible
                        .get(caret..caret_end)
                        .unwrap_or("")
                        .to_owned()
                        .or_space(),
                    tone: Some(ExtensionFileViewTone::Accent),
                    attributes: vec![
                        ExtensionTextAttribute::Underline,
                        ExtensionTextAttribute::Bold,
                    ],
                });
                if caret_end < visible.len() {
                    spans.push(plain_span(&visible[caret_end..]));
                }
            } else {
                let mut span = plain_span(&visible);
                if added.contains(&line_number) {
                    span.tone = Some(ExtensionFileViewTone::Added);
                }
                spans.push(span);
            }
            rows.push(ExtensionFileViewRow {
                id: format!("line:{line_number}"),
                spans,
                source_ranges: provenance[index]
                    .iter()
                    .copied()
                    .map(|source| ExtensionFileViewSourceRange {
                        side: workdeck_extension_api::ExtensionFileSide::New,
                        range: [source, source],
                    })
                    .collect(),
                component: None,
            });
        }
        let hunk_rows = hunk_ranges
            .iter()
            .map(|range| {
                let mut owned = provenance
                    .iter()
                    .enumerate()
                    .filter_map(|(index, sources)| {
                        sources
                            .iter()
                            .any(|line| *line >= range[0] && *line <= range[1])
                            .then_some(header_offset + index)
                    });
                let first = owned.next().unwrap_or(0);
                ExtensionFileViewHunkRows {
                    start_row: first,
                    end_row: owned.next_back().unwrap_or(first),
                }
            })
            .collect::<Vec<_>>();
        for (row_index, row) in rows.iter_mut().enumerate() {
            let owners = hunk_rows
                .iter()
                .filter(|extent| row_index >= extent.start_row && row_index <= extent.end_row)
                .count();
            if owners != 1 {
                row.source_ranges.clear();
            }
        }
        Some(ExtensionFileViewLayout { rows, hunk_rows })
    }

    pub fn enter_mode(&mut self, request: &FileViewModeLifecycleRequest) -> CommandExecution {
        if self
            .session
            .as_ref()
            .is_some_and(|session| session.file_id == request.file.id)
        {
            CommandExecution::default()
        } else {
            CommandExecution {
                actions: vec![
                    ExtensionHostAction::Notify {
                        message: "Inline edit mode must be opened with Ctrl-E".into(),
                        notification_type: workdeck_extension_api::ExtensionNotifyType::Warning,
                    },
                    ExtensionHostAction::ExitFileViewMode,
                ],
            }
        }
    }

    pub fn exit_mode(&mut self, _request: &FileViewModeLifecycleRequest) -> CommandExecution {
        let Some(session) = self.session.take() else {
            return CommandExecution::default();
        };
        let mut actions = vec![ExtensionHostAction::RefreshFileView {
            id: VIEW_ID.into(),
            file_id: Some(session.file_id.clone()),
        }];
        if session.modified() {
            actions.push(ExtensionHostAction::Notify {
                message: format!("Discarded unsaved edits to {}", session.path),
                notification_type: workdeck_extension_api::ExtensionNotifyType::Info,
            });
        }
        CommandExecution { actions }
    }

    pub fn key(&mut self, request: &FileViewModeKeyRequest) -> KeyboardModeExecution {
        let Some(session) = self
            .session
            .as_mut()
            .filter(|session| session.file_id == request.file.id)
        else {
            return key_result(KeyRoutingResult::Pass, Vec::new());
        };
        if matches_key("ctrl+s", &request.key) {
            if !session.modified() {
                return key_result(
                    KeyRoutingResult::Handled,
                    vec![ExtensionHostAction::Notify {
                        message: "No unsaved edits".into(),
                        notification_type: workdeck_extension_api::ExtensionNotifyType::Info,
                    }],
                );
            }
            let request_id = format!("inline-edit-write-{}", session.next_write_id);
            session.next_write_id = session.next_write_id.saturating_add(1);
            session.pending_write = Some(request_id.clone());
            return key_result(
                KeyRoutingResult::Handled,
                vec![ExtensionHostAction::RequestWorkspaceWrite {
                    request_id,
                    file_id: session.file_id.clone(),
                    text: session.text(),
                }],
            );
        }
        if matches!(request.key.sequence.as_str(), "]" | "?" | "q") {
            return key_result(KeyRoutingResult::Pass, Vec::new());
        }
        match request.key.name.as_str() {
            "up" | "down" | "left" | "right" => session.move_cursor(&request.key.name),
            "backspace" => session.delete_backwards(),
            _ if matches_key("enter", &request.key) => session.split_line(),
            _ => {
                let Some(character) = printable_character(&request.key) else {
                    return key_result(KeyRoutingResult::Pass, Vec::new());
                };
                session.insert(character);
            }
        }
        key_result(
            KeyRoutingResult::Handled,
            vec![ExtensionHostAction::RefreshFileView {
                id: VIEW_ID.into(),
                file_id: Some(session.file_id.clone()),
            }],
        )
    }

    pub fn complete_write(
        &mut self,
        completion: &ExtensionWorkspaceWriteCompletion,
    ) -> CommandExecution {
        let Some(session) = self.session.as_mut() else {
            return CommandExecution::default();
        };
        if session.pending_write.as_deref() != Some(completion.request_id.as_str()) {
            return warn(format!(
                "Unknown inline-edit write request {}",
                completion.request_id
            ));
        }
        session.pending_write = None;
        match &completion.result {
            ExtensionWorkspaceWriteResult::Written => {
                session.saved_text = session.text();
                notify(format!("Wrote {}", session.path))
            }
            ExtensionWorkspaceWriteResult::Cancelled { .. } => CommandExecution::default(),
            ExtensionWorkspaceWriteResult::Unavailable { detail }
            | ExtensionWorkspaceWriteResult::Failed { detail } => warn(detail),
        }
    }
}

trait OrSpace {
    fn or_space(self) -> String;
}

impl OrSpace for String {
    fn or_space(self) -> String {
        if self.is_empty() { " ".into() } else { self }
    }
}

fn can_write_document(source: &ChangesetSource, file: &DiffFile) -> bool {
    matches!(source, ChangesetSource::WorkingTree { staged: false })
        && file.sources.new.as_ref().is_some_and(|snapshot| {
            snapshot.attested && matches!(snapshot.origin, SourceOrigin::WorkingTree)
        })
        && !file.flags.binary
        && !file.flags.too_large
}

fn split_document_lines(text: &str) -> Vec<String> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = normalized
        .split('\n')
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if lines.len() > 1 && lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

fn detect_newline(text: &str) -> &str {
    if text.contains("\r\n") {
        "\r\n"
    } else if text.contains('\r') {
        "\r"
    } else {
        "\n"
    }
}

fn join_document(lines: &[String], newline: &str, ends_with_newline: bool) -> String {
    let mut text = lines.join(newline);
    if ends_with_newline {
        text.push_str(newline);
    }
    text
}

fn previous_grapheme_boundary(value: &str, column: usize) -> usize {
    value
        .grapheme_indices(true)
        .take_while(|(index, _)| *index < column)
        .map(|(index, _)| index)
        .last()
        .unwrap_or(0)
}

fn snap_grapheme_boundary(value: &str, column: usize) -> usize {
    if column >= value.len() {
        return value.len();
    }
    value
        .grapheme_indices(true)
        .take_while(|(index, _)| *index <= column)
        .map(|(index, _)| index)
        .last()
        .unwrap_or(0)
}

fn next_grapheme_boundary(value: &str, column: usize) -> usize {
    value
        .grapheme_indices(true)
        .find(|(index, _)| *index >= column)
        .map_or(value.len(), |(index, grapheme)| index + grapheme.len())
}

fn grapheme_column(value: &str, boundary: usize) -> usize {
    value
        .grapheme_indices(true)
        .take_while(|(index, _)| *index < boundary)
        .count()
}

fn boundary_at_grapheme_column(value: &str, column: usize) -> usize {
    value
        .grapheme_indices(true)
        .nth(column)
        .map_or(value.len(), |(index, _)| index)
}

fn truncate_cells(value: &str, width: usize) -> String {
    if UnicodeWidthStr::width(value) <= width {
        return value.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let budget = width.saturating_sub(UnicodeWidthStr::width("…"));
    let mut used = 0;
    let mut visible = String::new();
    for grapheme in value.graphemes(true) {
        let cells = UnicodeWidthStr::width(grapheme);
        if used + cells > budget {
            break;
        }
        visible.push_str(grapheme);
        used += cells;
    }
    visible.push('…');
    visible
}

fn printable_character(key: &ExtensionKeyEvent) -> Option<&str> {
    if key.ctrl || key.meta || key.option || key.sequence.graphemes(true).count() != 1 {
        return None;
    }
    let character = key.sequence.chars().next()?;
    (!character.is_control() && character != '\u{7f}').then_some(key.sequence.as_str())
}

fn added_line_numbers(changes: &[ExtensionFileChangeRange]) -> BTreeSet<usize> {
    changes
        .iter()
        .filter(|change| change.kind == ExtensionFileChangeKind::Added)
        .flat_map(|change| change.range[0]..=change.range[1])
        .collect()
}

fn plain_span(text: &str) -> ExtensionFileViewSpan {
    ExtensionFileViewSpan {
        text: text.into(),
        tone: None,
        attributes: Vec::new(),
    }
}

fn header_row(session: &EditSession, width: usize) -> ExtensionFileViewRow {
    let marker = if session.modified() {
        MODIFIED_MARKER
    } else {
        ""
    };
    let label = truncate_cells(HEADER_LABEL, width.saturating_sub(marker.width()));
    let mut spans = Vec::new();
    if !label.is_empty() {
        spans.push(ExtensionFileViewSpan {
            text: label,
            tone: Some(ExtensionFileViewTone::Accent),
            attributes: vec![ExtensionTextAttribute::Bold],
        });
    }
    if !marker.is_empty() {
        spans.push(ExtensionFileViewSpan {
            text: truncate_cells(marker, width),
            tone: Some(ExtensionFileViewTone::Added),
            attributes: vec![ExtensionTextAttribute::Bold],
        });
    }
    if spans.is_empty() {
        spans.push(plain_span(" "));
    }
    ExtensionFileViewRow {
        id: "editing".into(),
        spans,
        source_ranges: Vec::new(),
        component: None,
    }
}

fn notify(message: impl Into<String>) -> CommandExecution {
    CommandExecution {
        actions: vec![ExtensionHostAction::Notify {
            message: message.into(),
            notification_type: workdeck_extension_api::ExtensionNotifyType::Info,
        }],
    }
}

fn warn(message: impl Into<String>) -> CommandExecution {
    CommandExecution {
        actions: vec![ExtensionHostAction::Notify {
            message: message.into(),
            notification_type: workdeck_extension_api::ExtensionNotifyType::Warning,
        }],
    }
}

fn key_result(
    result: KeyRoutingResult,
    actions: Vec<ExtensionHostAction>,
) -> KeyboardModeExecution {
    KeyboardModeExecution { result, actions }
}

pub fn serve<R: BufRead, W: Write>(mut input: R, mut output: W) -> io::Result<()> {
    let mut extension = InlineEditExtension::default();
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let request: JsonRpcRequest = serde_json::from_str(&line).map_err(io::Error::other)?;
        let result = match request.method.as_str() {
            "workdeck/handshake" => serde_json::to_value(HandshakeResponse {
                extension_api_version: API_VERSION,
                extension_version: env!("CARGO_PKG_VERSION").into(),
                registrations: InlineEditExtension::registrations(),
            })
            .map_err(io::Error::other),
            "workdeck/command/invoke" => parse(&request)
                .and_then(|value| extension.invoke_command(&value).map_err(io::Error::other))
                .and_then(to_value),
            "workdeck/file-view/matches" => parse::<FileViewMatchRequest>(&request)
                .and_then(|value| {
                    checked_view(&value.view_id).map(|()| InlineEditExtension::matches(&value.file))
                })
                .and_then(to_value),
            "workdeck/file-view/layout" => parse::<FileViewLayoutRequest>(&request)
                .and_then(|value| checked_view(&value.view_id).map(|()| extension.layout(&value)))
                .and_then(to_value),
            "workdeck/file-view-mode/enter" => parse::<FileViewModeLifecycleRequest>(&request)
                .and_then(|value| {
                    checked_view(&value.view_id).map(|()| extension.enter_mode(&value))
                })
                .and_then(to_value),
            "workdeck/file-view-mode/exit" => parse::<FileViewModeLifecycleRequest>(&request)
                .and_then(|value| {
                    checked_view(&value.view_id).map(|()| extension.exit_mode(&value))
                })
                .and_then(to_value),
            "workdeck/file-view-mode/key" => parse::<FileViewModeKeyRequest>(&request)
                .and_then(|value| checked_view(&value.view_id).map(|()| extension.key(&value)))
                .and_then(to_value),
            "workdeck/workspace/write-complete" => {
                parse::<ExtensionWorkspaceWriteCompletion>(&request)
                    .map(|value| extension.complete_write(&value))
                    .and_then(to_value)
            }
            method => Err(io::Error::other(format!("Unknown method: {method}"))),
        };
        let response = match result {
            Ok(result) => JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: request.id,
                result: Some(result),
                error: None,
            },
            Err(error) => JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: request.id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: error.to_string(),
                    data: None,
                }),
            },
        };
        serde_json::to_writer(&mut output, &response).map_err(io::Error::other)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
}

fn parse<T: serde::de::DeserializeOwned>(request: &JsonRpcRequest) -> io::Result<T> {
    serde_json::from_value(request.params.clone()).map_err(io::Error::other)
}

fn to_value<T: serde::Serialize>(value: T) -> io::Result<serde_json::Value> {
    serde_json::to_value(value).map_err(io::Error::other)
}

fn checked_view(view_id: &str) -> io::Result<()> {
    (view_id == VIEW_ID)
        .then_some(())
        .ok_or_else(|| io::Error::other(format!("Unknown file view: {view_id}")))
}
