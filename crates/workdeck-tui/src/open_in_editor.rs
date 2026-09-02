//! Cross-platform editor command planning and terminal-safe process execution.

use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use workdeck_core::{DiffFile, DiffHunk, DiffLineKind, ReviewSide};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorCommand {
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorLineTarget {
    pub side: ReviewSide,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorLineCursor {
    pub file_id: String,
    pub hunk_index: usize,
    pub target: EditorLineTarget,
}

#[derive(Debug, Clone)]
pub struct ReviewEditorRequest {
    pub base_path: PathBuf,
    pub file: Option<DiffFile>,
    pub line_cursor: Option<EditorLineCursor>,
    pub selected_hunk: Option<DiffHunk>,
}

pub trait EditorTerminal {
    fn suspend(&mut self);
    fn resume(&mut self);
    fn is_destroyed(&self) -> bool;
}

pub trait EditorSpawner {
    fn spawn(&mut self, command: &EditorCommand) -> Result<i32, String>;
}

#[derive(Debug, Default)]
pub struct SystemEditorSpawner;

impl EditorSpawner for SystemEditorSpawner {
    fn spawn(&mut self, command: &EditorCommand) -> Result<i32, String> {
        let status = Command::new(&command.command)
            .args(&command.args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|error| error.to_string())?;
        Ok(status.code().unwrap_or(1))
    }
}

struct CrosstermEditorTerminal<'a> {
    terminal: &'a mut Terminal<CrosstermBackend<std::io::Stdout>>,
}

impl EditorTerminal for CrosstermEditorTerminal<'_> {
    fn suspend(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            DisableMouseCapture,
            LeaveAlternateScreen
        );
        let _ = self.terminal.show_cursor();
    }

    fn resume(&mut self) {
        let _ = enable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            EnterAlternateScreen,
            EnableMouseCapture
        );
        let _ = self.terminal.clear();
    }

    fn is_destroyed(&self) -> bool {
        false
    }
}

/// Execute a prepared request from either the standalone reviewer or unified shell.
pub fn open_review_editor_in_crossterm(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    request: &ReviewEditorRequest,
) -> Option<String> {
    let editor = std::env::var("EDITOR").ok();
    let mut terminal = CrosstermEditorTerminal { terminal };
    open_selected_file_in_editor(
        editor.as_deref(),
        &request.base_path,
        request.file.as_ref(),
        request.line_cursor.as_ref(),
        request.selected_hunk.as_ref(),
        &mut terminal,
        &mut SystemEditorSpawner,
    )
}

fn grouped_hunk_content(hunk: &DiffHunk) -> Vec<(DiffLineKind, usize, usize, usize)> {
    let mut groups = Vec::new();
    let mut index = 0;
    while index < hunk.lines.len() {
        if hunk.lines[index].kind == DiffLineKind::Context {
            let start = index;
            while index < hunk.lines.len() && hunk.lines[index].kind == DiffLineKind::Context {
                index += 1;
            }
            groups.push((DiffLineKind::Context, index - start, 0, 0));
            continue;
        }
        let start = index;
        while index < hunk.lines.len() && hunk.lines[index].kind != DiffLineKind::Context {
            index += 1;
        }
        let change = &hunk.lines[start..index];
        groups.push((
            DiffLineKind::Deletion,
            0,
            change
                .iter()
                .filter(|line| line.kind == DiffLineKind::Deletion)
                .count(),
            change
                .iter()
                .filter(|line| line.kind == DiffLineKind::Addition)
                .count(),
        ));
    }
    groups
}

fn deletion_line_to_file_line(hunk: &DiffHunk, deletion_line: u32) -> u32 {
    let mut deletion_cursor = hunk.old_start;
    let mut addition_cursor = if hunk.new_count == 0 {
        hunk.new_start.saturating_add(1)
    } else {
        hunk.new_start
    };
    for (kind, context_lines, deletions, additions) in grouped_hunk_content(hunk) {
        if kind == DiffLineKind::Context {
            let context_lines = u32::try_from(context_lines).unwrap_or(u32::MAX);
            if deletion_line < deletion_cursor.saturating_add(context_lines) {
                return addition_cursor
                    .saturating_add(deletion_line.saturating_sub(deletion_cursor));
            }
            deletion_cursor = deletion_cursor.saturating_add(context_lines);
            addition_cursor = addition_cursor.saturating_add(context_lines);
            continue;
        }
        let deletions = u32::try_from(deletions).unwrap_or(u32::MAX);
        let additions = u32::try_from(additions).unwrap_or(u32::MAX);
        if deletion_line < deletion_cursor.saturating_add(deletions) {
            let offset = deletion_line
                .saturating_sub(deletion_cursor)
                .min(additions.saturating_sub(1));
            return addition_cursor.saturating_add(offset);
        }
        deletion_cursor = deletion_cursor.saturating_add(deletions);
        addition_cursor = addition_cursor.saturating_add(additions);
    }
    addition_cursor
}

fn selected_line(
    file: &DiffFile,
    selected_hunk: Option<&DiffHunk>,
    line_cursor: Option<&EditorLineCursor>,
) -> u32 {
    let deleted = file.change_kind == workdeck_core::FileChangeKind::Deleted;
    let disk_side = if deleted {
        ReviewSide::Old
    } else {
        ReviewSide::New
    };
    let cursor = line_cursor.filter(|cursor| cursor.file_id == file_id(file));
    if let Some(cursor) = cursor {
        if cursor.target.side == disk_side {
            return cursor.target.line;
        }
        if !deleted && let Some(hunk) = file.hunks.get(cursor.hunk_index) {
            return deletion_line_to_file_line(hunk, cursor.target.line);
        }
    }
    selected_hunk.map_or(1, |hunk| {
        if deleted {
            hunk.old_start
        } else {
            hunk.new_start
        }
    })
}

fn file_id(file: &DiffFile) -> &str {
    if file.runtime_id.is_empty() {
        &file.key
    } else {
        &file.runtime_id
    }
}

fn split_editor_command(editor: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut quote = None;
    let mut escaped_in_quote = false;
    for character in editor.chars() {
        if escaped_in_quote {
            token.push(character);
            escaped_in_quote = false;
            continue;
        }
        if quote.is_some() && character == '\\' {
            token.push(character);
            escaped_in_quote = true;
            continue;
        }
        if matches!(character, '\'' | '"') {
            match quote {
                Some(active) if active == character => quote = None,
                None => quote = Some(character),
                Some(_) => {}
            }
            token.push(character);
            continue;
        }
        if character.is_whitespace() && quote.is_none() {
            if !token.is_empty() {
                tokens.push(strip_matching_quotes(token));
                token = String::new();
            }
        } else {
            token.push(character);
        }
    }
    if !token.is_empty() {
        tokens.push(strip_matching_quotes(token));
    }
    tokens
}

fn strip_matching_quotes(token: String) -> String {
    let bytes = token.as_bytes();
    if bytes.len() >= 2
        && matches!(bytes[0], b'\'' | b'"')
        && bytes.last().copied() == Some(bytes[0])
    {
        token[1..token.len() - 1].to_owned()
    } else {
        token
    }
}

fn editor_program(editor: &str) -> String {
    let first = split_editor_command(editor)
        .into_iter()
        .next()
        .unwrap_or_default();
    let basename = first
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_lowercase();
    basename
        .strip_suffix(".cmd")
        .or_else(|| basename.strip_suffix(".exe"))
        .unwrap_or(&basename)
        .to_owned()
}

#[must_use]
pub fn should_suspend_for_editor(editor: &str) -> bool {
    !matches!(
        editor_program(editor).as_str(),
        "code" | "code-insiders" | "cursor"
    )
}

#[must_use]
pub fn build_editor_command(editor: &str, file_path: &str, line: u32) -> EditorCommand {
    let mut tokens = split_editor_command(editor).into_iter();
    let command = tokens.next().unwrap_or_default();
    let mut args = tokens.collect::<Vec<_>>();
    match editor_program(editor).as_str() {
        "vim" | "nvim" | "vi" => {
            args.push(format!("+{line}"));
            args.push(file_path.into());
        }
        "code" | "code-insiders" | "cursor" => {
            args.push("--goto".into());
            args.push(format!("{file_path}:{line}"));
        }
        "hx" => args.push(format!("{file_path}:{line}")),
        _ => args.push(file_path.into()),
    }
    EditorCommand { command, args }
}

#[must_use]
pub fn resolve_editable_file_path(file_path: &str, base_path: &Path) -> PathBuf {
    let path = Path::new(file_path);
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_path.join(path)
    };
    let mut resolved = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            other => resolved.push(other.as_os_str()),
        }
    }
    resolved
}

/// Open one selected file while keeping terminal suspension and process execution testable.
pub fn open_selected_file_in_editor(
    editor: Option<&str>,
    base_path: &Path,
    file: Option<&DiffFile>,
    line_cursor: Option<&EditorLineCursor>,
    selected_hunk: Option<&DiffHunk>,
    terminal: &mut impl EditorTerminal,
    spawner: &mut impl EditorSpawner,
) -> Option<String> {
    let Some(file) = file else {
        return Some("No file selected.".into());
    };
    let Some(editor) = editor.map(str::trim).filter(|editor| !editor.is_empty()) else {
        return Some("$EDITOR is not set.".into());
    };
    let absolute_path = resolve_editable_file_path(&file.path, base_path);
    if !absolute_path.exists() {
        return Some(format!(
            "Cannot edit {}: file does not exist on disk.",
            file.path
        ));
    }
    let line = selected_line(file, selected_hunk, line_cursor).max(1);
    let command = build_editor_command(editor, &absolute_path.to_string_lossy(), line);
    let suspend = should_suspend_for_editor(editor);
    if suspend {
        terminal.suspend();
    }
    let result = spawner.spawn(&command);
    if suspend && !terminal.is_destroyed() {
        terminal.resume();
    }
    match result {
        Err(error) => Some(format!("Failed to launch editor: {error}")),
        Ok(0) => None,
        Ok(status) => Some(format!("Editor exited with status {status}.")),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;
    use workdeck_core::{ChangesetSource, FileChangeKind};
    use workdeck_diff::parse_patch;

    use super::*;

    #[derive(Debug, Default)]
    struct TestTerminal {
        destroyed: bool,
        suspend_calls: usize,
        resume_calls: usize,
    }

    impl EditorTerminal for TestTerminal {
        fn suspend(&mut self) {
            self.suspend_calls += 1;
        }

        fn resume(&mut self) {
            self.resume_calls += 1;
        }

        fn is_destroyed(&self) -> bool {
            self.destroyed
        }
    }

    #[derive(Debug)]
    struct TestSpawner {
        calls: Vec<EditorCommand>,
        result: Result<i32, String>,
    }

    impl Default for TestSpawner {
        fn default() -> Self {
            Self {
                calls: Vec::new(),
                result: Ok(0),
            }
        }
    }

    impl EditorSpawner for TestSpawner {
        fn spawn(&mut self, command: &EditorCommand) -> Result<i32, String> {
            self.calls.push(command.clone());
            self.result.clone()
        }
    }

    fn file_from_patch(id: &str, path: &str, patch: &str) -> DiffFile {
        let mut file = parse_patch(
            patch,
            "editor",
            "Editor",
            ChangesetSource::Patch {
                label: "editor".into(),
            },
        )
        .unwrap()
        .files
        .remove(0);
        file.runtime_id = id.into();
        file.path = path.into();
        file
    }

    fn changed_file(id: &str, path: &str) -> DiffFile {
        file_from_patch(
            id,
            path,
            "diff --git a/example.ts b/example.ts\n--- a/example.ts\n+++ b/example.ts\n@@ -1 +1 @@\n-old\n+new\n",
        )
    }

    fn existing_file(directory: &TempDir, path: &str, contents: &str) {
        let target = directory.path().join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(target, contents).unwrap();
    }

    #[test]
    fn builds_vi_code_windows_hx_and_unknown_editor_commands_without_a_shell() {
        assert_eq!(
            build_editor_command("nvim", "/tmp/project/file with spaces's.ts", 12),
            EditorCommand {
                command: "nvim".into(),
                args: vec!["+12".into(), "/tmp/project/file with spaces's.ts".into()]
            }
        );
        assert_eq!(
            build_editor_command("code --reuse-window", "/tmp/project/example.ts", 4),
            EditorCommand {
                command: "code".into(),
                args: vec![
                    "--reuse-window".into(),
                    "--goto".into(),
                    "/tmp/project/example.ts:4".into()
                ]
            }
        );
        assert_eq!(
            build_editor_command(
                "\"C:\\Program Files\\Microsoft VS Code\\bin\\code.cmd\" --wait",
                "C:\\Users\\Duarte\\repo\\file with spaces.ts",
                7,
            ),
            EditorCommand {
                command: "C:\\Program Files\\Microsoft VS Code\\bin\\code.cmd".into(),
                args: vec![
                    "--wait".into(),
                    "--goto".into(),
                    "C:\\Users\\Duarte\\repo\\file with spaces.ts:7".into()
                ]
            }
        );
        assert_eq!(
            build_editor_command("hx", "/tmp/project/example.ts", 4).args,
            ["/tmp/project/example.ts:4"]
        );
        assert_eq!(
            build_editor_command("zed --new-window", "/tmp/project/example.ts", 4),
            EditorCommand {
                command: "zed".into(),
                args: vec!["--new-window".into(), "/tmp/project/example.ts".into()]
            }
        );
    }

    #[test]
    fn suspends_every_editor_except_known_code_style_gui_programs() {
        assert!(!should_suspend_for_editor("code --reuse-window"));
        assert!(!should_suspend_for_editor(
            "\"C:\\Program Files\\Cursor\\cursor.exe\""
        ));
        assert!(!should_suspend_for_editor("code-insiders"));
        assert!(should_suspend_for_editor("nvim"));
        assert!(should_suspend_for_editor("zed"));
    }

    #[test]
    fn resolves_repo_relative_paths_and_normalizes_dot_segments() {
        assert_eq!(
            resolve_editable_file_path("src/../src/main.tsx", Path::new("/tmp/project")),
            PathBuf::from("/tmp/project/src/main.tsx")
        );
        assert_eq!(
            resolve_editable_file_path("/tmp/other.ts", Path::new("/tmp/project")),
            PathBuf::from("/tmp/other.ts")
        );
    }

    #[test]
    fn rejects_missing_selection_editor_and_disk_file_before_process_or_terminal_actions() {
        let directory = TempDir::new().unwrap();
        let mut terminal = TestTerminal::default();
        let mut spawner = TestSpawner::default();
        assert_eq!(
            open_selected_file_in_editor(
                Some("nvim"),
                directory.path(),
                None,
                None,
                None,
                &mut terminal,
                &mut spawner,
            ),
            Some("No file selected.".into())
        );
        let file = changed_file("example", "missing.ts");
        assert_eq!(
            open_selected_file_in_editor(
                None,
                directory.path(),
                Some(&file),
                None,
                None,
                &mut terminal,
                &mut spawner,
            ),
            Some("$EDITOR is not set.".into())
        );
        assert_eq!(
            open_selected_file_in_editor(
                Some("   "),
                directory.path(),
                Some(&file),
                None,
                None,
                &mut terminal,
                &mut spawner,
            ),
            Some("$EDITOR is not set.".into())
        );
        assert_eq!(
            open_selected_file_in_editor(
                Some("nvim"),
                directory.path(),
                Some(&file),
                None,
                None,
                &mut terminal,
                &mut spawner,
            ),
            Some("Cannot edit missing.ts: file does not exist on disk.".into())
        );
        assert!(spawner.calls.is_empty());
        assert_eq!((terminal.suspend_calls, terminal.resume_calls), (0, 0));
    }

    #[test]
    fn terminal_editor_suspends_spawns_and_resumes_around_success() {
        let directory = TempDir::new().unwrap();
        existing_file(&directory, "example.ts", "const value = 1;\n");
        let file = changed_file("example", "example.ts");
        let mut terminal = TestTerminal::default();
        let mut spawner = TestSpawner::default();
        assert_eq!(
            open_selected_file_in_editor(
                Some("nvim --clean"),
                directory.path(),
                Some(&file),
                None,
                None,
                &mut terminal,
                &mut spawner,
            ),
            None
        );
        assert_eq!((terminal.suspend_calls, terminal.resume_calls), (1, 1));
        assert_eq!(spawner.calls.len(), 1);
        assert_eq!(spawner.calls[0].command, "nvim");
        assert_eq!(spawner.calls[0].args[0..2], ["--clean", "+1"]);
        assert_eq!(
            Path::new(spawner.calls[0].args.last().unwrap()),
            directory.path().join("example.ts")
        );
    }

    #[test]
    fn current_new_line_wins_over_selected_hunk_and_other_file_cursor_falls_back() {
        let directory = TempDir::new().unwrap();
        existing_file(&directory, "example.ts", "new\n");
        let file = changed_file("example", "example.ts");
        let mut terminal = TestTerminal::default();
        let mut spawner = TestSpawner::default();
        open_selected_file_in_editor(
            Some("vim"),
            directory.path(),
            Some(&file),
            Some(&EditorLineCursor {
                file_id: "example".into(),
                hunk_index: 0,
                target: EditorLineTarget {
                    side: ReviewSide::New,
                    line: 3,
                },
            }),
            file.hunks.first(),
            &mut terminal,
            &mut spawner,
        );
        assert_eq!(spawner.calls[0].args[0], "+3");

        open_selected_file_in_editor(
            Some("vim"),
            directory.path(),
            Some(&file),
            Some(&EditorLineCursor {
                file_id: "other-file".into(),
                hunk_index: 0,
                target: EditorLineTarget {
                    side: ReviewSide::New,
                    line: 42,
                },
            }),
            file.hunks.first(),
            &mut terminal,
            &mut spawner,
        );
        assert_eq!(spawner.calls[1].args[0], "+1");
    }

    #[test]
    fn maps_removed_lines_to_their_on_disk_replacement_positions() {
        let directory = TempDir::new().unwrap();
        existing_file(&directory, "example.ts", "one\nfour\n");
        let deletion = file_from_patch(
            "example",
            "example.ts",
            "diff --git a/example.ts b/example.ts\n--- a/example.ts\n+++ b/example.ts\n@@ -1,4 +1,2 @@\n one\n-two\n-three\n four\n",
        );
        let mut terminal = TestTerminal::default();
        let mut spawner = TestSpawner::default();
        open_selected_file_in_editor(
            Some("vim"),
            directory.path(),
            Some(&deletion),
            Some(&EditorLineCursor {
                file_id: "example".into(),
                hunk_index: 0,
                target: EditorLineTarget {
                    side: ReviewSide::Old,
                    line: 3,
                },
            }),
            deletion.hunks.first(),
            &mut terminal,
            &mut spawner,
        );
        assert_eq!(spawner.calls[0].args[0], "+2");

        existing_file(&directory, "replacement.ts", "one\nTWO\nTHREE\nfour\n");
        let replacement = file_from_patch(
            "replacement",
            "replacement.ts",
            "diff --git a/replacement.ts b/replacement.ts\n--- a/replacement.ts\n+++ b/replacement.ts\n@@ -1,4 +1,4 @@\n one\n-two\n-three\n+TWO\n+THREE\n four\n",
        );
        open_selected_file_in_editor(
            Some("vim"),
            directory.path(),
            Some(&replacement),
            Some(&EditorLineCursor {
                file_id: "replacement".into(),
                hunk_index: 0,
                target: EditorLineTarget {
                    side: ReviewSide::Old,
                    line: 3,
                },
            }),
            replacement.hunks.first(),
            &mut terminal,
            &mut spawner,
        );
        assert_eq!(spawner.calls[1].args[0], "+3");
    }

    #[test]
    fn deleted_files_use_old_side_line_numbers() {
        let directory = TempDir::new().unwrap();
        existing_file(&directory, "deleted.ts", "old\n");
        let mut file = changed_file("deleted", "deleted.ts");
        file.change_kind = FileChangeKind::Deleted;
        let mut hunk = file.hunks[0].clone();
        hunk.old_start = 9;
        hunk.new_start = 2;
        let mut terminal = TestTerminal::default();
        let mut spawner = TestSpawner::default();
        open_selected_file_in_editor(
            Some("vim"),
            directory.path(),
            Some(&file),
            None,
            Some(&hunk),
            &mut terminal,
            &mut spawner,
        );
        assert_eq!(spawner.calls[0].args[0], "+9");
    }

    #[test]
    fn gui_exit_and_terminal_launch_failure_report_exact_errors_and_resume_policy() {
        let directory = TempDir::new().unwrap();
        existing_file(&directory, "example.ts", "new\n");
        let file = changed_file("example", "example.ts");
        let mut terminal = TestTerminal::default();
        let mut gui = TestSpawner {
            calls: Vec::new(),
            result: Ok(2),
        };
        assert_eq!(
            open_selected_file_in_editor(
                Some("code --wait"),
                directory.path(),
                Some(&file),
                None,
                file.hunks.first(),
                &mut terminal,
                &mut gui,
            ),
            Some("Editor exited with status 2.".into())
        );
        assert_eq!((terminal.suspend_calls, terminal.resume_calls), (0, 0));

        let mut failing = TestSpawner {
            calls: Vec::new(),
            result: Err("boom".into()),
        };
        assert_eq!(
            open_selected_file_in_editor(
                Some("vi"),
                directory.path(),
                Some(&file),
                None,
                file.hunks.first(),
                &mut terminal,
                &mut failing,
            ),
            Some("Failed to launch editor: boom".into())
        );
        assert_eq!((terminal.suspend_calls, terminal.resume_calls), (1, 1));

        terminal.destroyed = true;
        open_selected_file_in_editor(
            Some("vi"),
            directory.path(),
            Some(&file),
            None,
            file.hunks.first(),
            &mut terminal,
            &mut failing,
        );
        assert_eq!((terminal.suspend_calls, terminal.resume_calls), (2, 1));
    }
}
