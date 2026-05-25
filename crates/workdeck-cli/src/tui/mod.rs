use crate::app::{App, LayoutMode, PreviewData, PreviewKind, PreviewTarget, RefreshData, Tab};
use crate::config;
use crate::git;
use crate::syntax::SyntaxHighlighter;
use crate::views;
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io::{self, Stdout};
use std::panic;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

pub fn run(mut app: App) -> Result<()> {
    let _panic_hook = TerminalPanicHook::install();
    let mut terminal = TerminalSession::enter()?;
    let highlighter = SyntaxHighlighter::new(&app.config.ui.theme);
    run_loop(terminal.terminal_mut(), &mut app, &highlighter)
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalSession {
    fn enter() -> Result<Self> {
        Ok(Self {
            terminal: setup_terminal()?,
        })
    }

    fn terminal_mut(&mut self) -> &mut Terminal<CrosstermBackend<Stdout>> {
        &mut self.terminal
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = restore_terminal(&mut self.terminal);
    }
}

type PanicHook = Box<dyn Fn(&panic::PanicHookInfo<'_>) + Sync + Send + 'static>;

struct TerminalPanicHook {
    previous: Option<PanicHook>,
}

impl TerminalPanicHook {
    fn install() -> Self {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(|info| {
            force_restore_terminal();
            eprintln!("{info}");
        }));
        Self {
            previous: Some(previous),
        }
    }
}

impl Drop for TerminalPanicHook {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            panic::set_hook(previous);
        }
    }
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    let _ = disable_raw_mode();
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn force_restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen);
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    highlighter: &SyntaxHighlighter,
) -> Result<()> {
    let (refresh_tx, refresh_rx) = mpsc::channel();
    let (preview_tx, preview_rx) = mpsc::channel();
    spawn_refresh(app, refresh_tx.clone());
    let mut next_auto_refresh = next_auto_refresh_deadline(app);

    loop {
        drain_refresh_results(app, &refresh_rx, &refresh_tx);
        maybe_spawn_auto_refresh(app, &refresh_tx, &mut next_auto_refresh);
        drain_preview_results(app, &preview_rx);
        spawn_preview_if_needed(app, preview_tx.clone());
        terminal.draw(|frame| views::render(app, highlighter, frame))?;
        if !event::poll(Duration::from_millis(50))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if handle_key(terminal, app, key, &refresh_tx)? {
            return Ok(());
        }
    }
}

fn spawn_refresh(app: &mut App, sender: Sender<Result<RefreshData, RefreshError>>) -> bool {
    let Some(generation) = app.request_refresh() else {
        return false;
    };
    let repo_root = app.repo_root.clone();
    let store = app.store.clone();
    let base_branch = app.config.git.base_branch.clone();
    let recent_commits = app.config.git.recent_commits;
    thread::spawn(move || {
        let result = (|| {
            let snapshot = git::scan_repo(&repo_root)?;
            let git_overview =
                git::scan_git_overview(&repo_root, Some(&base_branch), recent_commits)?;
            let files = git::list_repo_files(&repo_root, 20_000)?;
            let issues = store.load_issues()?;
            let sessions = store.load_agent_sessions()?;
            let reference_data = store.load_reference_data()?;
            let symbols = crate::search::extract_symbols(&repo_root, &files);
            Ok(RefreshData {
                generation,
                snapshot,
                git_overview,
                files,
                issues,
                sessions,
                reference_data,
                symbols,
            })
        })()
        .map_err(|error: anyhow::Error| RefreshError {
            generation,
            message: error.to_string(),
        });
        let _ = sender.send(result);
    });
    true
}

fn maybe_spawn_auto_refresh(
    app: &mut App,
    sender: &Sender<Result<RefreshData, RefreshError>>,
    next_auto_refresh: &mut Instant,
) {
    if !app.config.refresh.auto || Instant::now() < *next_auto_refresh {
        return;
    }
    spawn_refresh(app, sender.clone());
    *next_auto_refresh = next_auto_refresh_deadline(app);
}

fn next_auto_refresh_deadline(app: &App) -> Instant {
    Instant::now() + Duration::from_millis(app.config.refresh.interval_ms.max(1))
}

#[derive(Debug)]
struct RefreshError {
    generation: u64,
    message: String,
}

fn drain_refresh_results(
    app: &mut App,
    receiver: &Receiver<Result<RefreshData, RefreshError>>,
    sender: &Sender<Result<RefreshData, RefreshError>>,
) {
    let mut completed_current_refresh = false;
    while let Ok(result) = receiver.try_recv() {
        match result {
            Ok(data) => {
                completed_current_refresh |= app.apply_refresh_data(data);
            }
            Err(error) => {
                completed_current_refresh |=
                    app.apply_refresh_error(error.generation, error.message);
            }
        }
    }
    if completed_current_refresh && app.refresh_pending && !app.loading {
        spawn_refresh(app, sender.clone());
    }
}

fn spawn_preview_if_needed(app: &mut App, sender: Sender<Result<PreviewData, PreviewError>>) {
    let Some(target) = app.missing_preview_target() else {
        return;
    };
    app.mark_preview_loading(target.clone());
    let repo_root = app.repo_root.clone();
    thread::spawn(move || {
        let result = load_preview(&repo_root, target).map_err(|error| PreviewError {
            target: error.0,
            message: error.1,
        });
        let _ = sender.send(result);
    });
}

fn load_preview(
    repo_root: &std::path::Path,
    target: PreviewTarget,
) -> Result<PreviewData, (PreviewTarget, String)> {
    let preview = match target.kind {
        PreviewKind::Diff => git::diff_for_path(repo_root, &target.path),
        PreviewKind::File => git::read_file_preview(repo_root, &target.path, 80_000),
        PreviewKind::GitCommit => {
            git::git_commit_preview(repo_root, &target.path.to_string_lossy())
        }
        PreviewKind::GitStash => git::git_stash_preview(repo_root, &target.path.to_string_lossy()),
        PreviewKind::GitBranch => {
            git::git_branch_preview(repo_root, &target.path.to_string_lossy(), 30)
        }
        PreviewKind::GitSummary => {
            let base = target
                .path
                .to_str()
                .filter(|value| !value.trim().is_empty());
            git::git_summary_preview(repo_root, base)
        }
        PreviewKind::Issue | PreviewKind::Agent | PreviewKind::GitTag | PreviewKind::GitRemote => {
            unreachable!("work-state previews are generated from app state")
        }
    }
    .map_err(|error| (target.clone(), error.to_string()))?;
    Ok(PreviewData { target, preview })
}

#[derive(Debug)]
struct PreviewError {
    target: PreviewTarget,
    message: String,
}

fn drain_preview_results(app: &mut App, receiver: &Receiver<Result<PreviewData, PreviewError>>) {
    while let Ok(result) = receiver.try_recv() {
        match result {
            Ok(data) => app.apply_preview_data(data),
            Err(error) => app.apply_preview_error(error.target, error.message),
        }
    }
}

fn handle_key(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    key: KeyEvent,
    refresh_tx: &Sender<Result<RefreshData, RefreshError>>,
) -> Result<bool> {
    let layout = terminal
        .size()
        .map(|area| LayoutMode::for_width(area.width))
        .unwrap_or(LayoutMode::Wide);
    let narrow_files = app.active_tab == Tab::Files
        && layout == LayoutMode::Narrow
        && app.focus != crate::app::FocusPane::Preview;

    if app.help_visible {
        if key.code == KeyCode::Esc || configured_key(key, &app.config.keys.help) {
            app.help_visible = false;
        }
        return Ok(false);
    }

    if app.active_tab == Tab::Search {
        match key.code {
            _ if is_ctrl_c(key) => return Ok(true),
            KeyCode::Esc => {
                app.active_tab = Tab::Changes;
                app.search_query.clear();
                app.rebuild_search();
            }
            KeyCode::Enter => app.accept_search_result(),
            KeyCode::Backspace => {
                app.search_query.pop();
                app.rebuild_search();
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                app.search_query.push(ch);
                app.rebuild_search();
            }
            KeyCode::Up | KeyCode::Char('k') => app.move_up(),
            KeyCode::Down | KeyCode::Char('j') => app.move_down(),
            KeyCode::Tab => app.active_tab = app.active_tab.next(),
            KeyCode::BackTab => app.active_tab = app.active_tab.previous(),
            _ if configured_key(key, &app.config.keys.quit) => return Ok(true),
            _ => {}
        }
        return Ok(false);
    }

    if is_ctrl_c(key) || configured_key(key, &app.config.keys.quit) {
        return Ok(true);
    } else if app.focus == crate::app::FocusPane::Preview {
        match key.code {
            KeyCode::Char('h') | KeyCode::Left | KeyCode::Esc => app.focus_tree(),
            KeyCode::Char('j') | KeyCode::Down => app.scroll_preview_down(1, usize::MAX),
            KeyCode::Char('k') | KeyCode::Up => app.scroll_preview_up(1),
            KeyCode::Char('g') => app.preview_top(),
            KeyCode::Char('G') => app.preview_bottom(usize::MAX),
            KeyCode::Tab => app.active_tab = app.active_tab.next(),
            KeyCode::BackTab => app.active_tab = app.active_tab.previous(),
            _ => {}
        }
    } else if configured_key(key, &app.config.keys.changes) {
        app.active_tab = Tab::Changes;
    } else if configured_key(key, &app.config.keys.git) {
        app.active_tab = Tab::Git;
    } else if configured_key(key, &app.config.keys.files) {
        app.active_tab = Tab::Files;
    } else if configured_key(key, &app.config.keys.issues) {
        app.active_tab = Tab::Issues;
    } else if configured_key(key, &app.config.keys.agents) {
        app.active_tab = Tab::Agents;
    } else if configured_key(key, &app.config.keys.search) {
        app.active_tab = Tab::Search;
        app.search_query.clear();
        app.rebuild_search();
    } else if configured_key(key, &app.config.keys.help) {
        app.help_visible = true;
    } else if configured_key(key, &app.config.keys.toggle_preview) {
        app.preview_visible = !app.preview_visible;
    } else if app.active_tab == Tab::Changes && configured_key(key, &app.config.keys.group_changes)
    {
        app.cycle_change_grouping();
    } else if app.active_tab == Tab::Changes && configured_key(key, &app.config.keys.toggle_dirstat)
    {
        app.toggle_dirstat();
    } else if app.active_tab == Tab::Git && configured_key(key, &app.config.keys.base) {
        app.status_message = "base branch selection not implemented yet".to_string();
    } else if app.active_tab == Tab::Git && configured_key(key, &app.config.keys.pull_requests) {
        app.status_message = "PR refresh not implemented yet".to_string();
    } else if configured_key(key, &app.config.keys.refresh) {
        spawn_refresh(app, refresh_tx.clone());
    } else if configured_key(key, &app.config.keys.new_issue) {
        if let Err(error) = app.create_issue_from_selection() {
            app.status_message = error.to_string();
        }
    } else if key.code == KeyCode::Enter {
        if app.active_tab == Tab::Issues {
            restore_terminal(terminal)?;
            let result = app.open_selected_issue_in_editor();
            *terminal = setup_terminal()?;
            if let Err(error) = result {
                app.status_message = error.to_string();
            }
        } else if narrow_files {
            app.activate_selected_file_browser_entry();
        } else {
            app.reveal_selected_context();
        }
    } else if app.active_tab == Tab::Issues && configured_key(key, &app.config.keys.status) {
        if let Err(error) = app.cycle_selected_issue_status() {
            app.status_message = error.to_string();
        }
    } else if app.active_tab == Tab::Issues && configured_key(key, &app.config.keys.priority) {
        if let Err(error) = app.cycle_selected_issue_priority() {
            app.status_message = error.to_string();
        }
    } else if app.active_tab == Tab::Issues && configured_key(key, &app.config.keys.labels) {
        if let Err(error) = app.toggle_selected_issue_label() {
            app.status_message = error.to_string();
        }
    } else if app.active_tab == Tab::Issues && configured_key(key, &app.config.keys.assign) {
        if let Err(error) = app.toggle_selected_issue_assignee() {
            app.status_message = error.to_string();
        }
    } else if configured_key(key, &app.config.keys.jump) {
        app.jump_between_issue_and_file();
    } else if configured_key(key, &app.config.keys.link_file) {
        if let Err(error) = app.link_selected_file_to_issue() {
            app.status_message = error.to_string();
        }
    } else if app.active_tab == Tab::Issues && configured_key(key, &app.config.keys.edit_issue) {
        restore_terminal(terminal)?;
        let result = app.open_selected_issue_in_editor();
        *terminal = setup_terminal()?;
        if let Err(error) = result {
            app.status_message = error.to_string();
        }
    } else if configured_key(key, &app.config.keys.open_editor) {
        restore_terminal(terminal)?;
        let result = app.open_selected_in_editor();
        *terminal = setup_terminal()?;
        if let Err(error) = result {
            app.status_message = error.to_string();
        }
    } else if configured_key(key, &app.config.keys.copy) {
        app.copy_selected_reference();
    } else {
        match key.code {
            KeyCode::Tab => app.active_tab = app.active_tab.next(),
            KeyCode::BackTab => app.active_tab = app.active_tab.previous(),
            KeyCode::Down | KeyCode::Char('j') => {
                if narrow_files {
                    app.move_file_browser_down();
                } else {
                    app.move_down();
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if narrow_files {
                    app.move_file_browser_up();
                } else {
                    app.move_up();
                }
            }
            KeyCode::Char('g') if narrow_files => app.file_browser_top(),
            KeyCode::Char('G') if narrow_files => app.file_browser_bottom(),
            KeyCode::Char('.') if narrow_files => app.file_browser_root(),
            KeyCode::Esc | KeyCode::Backspace if narrow_files => {
                app.move_file_browser_parent();
            }
            KeyCode::Left | KeyCode::Char('h') => {
                if narrow_files {
                    app.move_file_browser_parent();
                } else if !app.collapse_selected_tree_row() {
                    app.focus_tree();
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if narrow_files {
                    app.activate_selected_file_browser_entry();
                } else if !app.expand_selected_tree_row() {
                    app.focus_preview();
                }
            }
            _ => {}
        }
    }
    Ok(false)
}

fn is_ctrl_c(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn configured_key(key: KeyEvent, binding: &str) -> bool {
    let Ok(binding) = config::normalize_key(binding) else {
        return false;
    };
    match binding.as_str() {
        "tab" => key.code == KeyCode::Tab,
        "shift-tab" => key.code == KeyCode::BackTab,
        "enter" => key.code == KeyCode::Enter,
        "esc" => key.code == KeyCode::Esc,
        "space" => key.code == KeyCode::Char(' '),
        _ => {
            let mut chars = binding.chars();
            let Some(ch) = chars.next() else {
                return false;
            };
            chars.next().is_none() && key.code == KeyCode::Char(ch)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_key_matches_char_and_named_keys() {
        assert!(configured_key(
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
            "q"
        ));
        assert!(configured_key(
            KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
            "tab"
        ));
        assert!(configured_key(
            KeyEvent::new(KeyCode::Char('L'), KeyModifiers::SHIFT),
            "L"
        ));
        assert!(!configured_key(
            KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE),
            "L"
        ));
    }

    #[test]
    fn ctrl_c_is_quit_key() {
        assert!(is_ctrl_c(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL
        )));
        assert!(!is_ctrl_c(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::NONE
        )));
    }
}
