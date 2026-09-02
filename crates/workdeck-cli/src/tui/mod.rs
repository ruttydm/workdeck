use crate::app::{App, LayoutMode, PreviewData, PreviewKind, PreviewTarget, RefreshData, Tab};
use crate::config;
use crate::git;
use crate::syntax::SyntaxHighlighter;
use crate::views;
use anyhow::Result;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
};
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
use workdeck_core::StartupNotice;
use workdeck_session::{ReviewSessionServer, default_discovery_directory};
use workdeck_tui::{
    DEFAULT_STARTUP_NOTICE_DELAY, DEFAULT_STARTUP_NOTICE_DURATION, DEFAULT_STARTUP_NOTICE_REPEAT,
    JobControlAction, JobControlPlatform, JobControlSupport, StartupNoticeQueue,
    open_review_editor_in_crossterm,
};
#[cfg(unix)]
use workdeck_tui::{JobControlRuntime, suspend_foreground_process_group};
use workdeck_vcs::{AnyProvider, DiffRequest, ProviderPreference, VcsProvider};

pub fn run(mut app: App) -> Result<()> {
    let _panic_hook = TerminalPanicHook::install();
    let mut terminal = TerminalSession::enter()?;
    let highlighter = SyntaxHighlighter::new(&app.config.ui.theme);
    let review_session = app
        .review
        .as_ref()
        .and_then(|review| {
            default_discovery_directory().map(|directory| {
                ReviewSessionServer::spawn(review.shared_state(), app.repo_root.clone(), directory)
            })
        })
        .transpose()?;
    run_loop(
        terminal.terminal_mut(),
        &mut app,
        &highlighter,
        review_session.as_ref(),
    )
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
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    let _ = disable_raw_mode();
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    Ok(())
}

fn force_restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    highlighter: &SyntaxHighlighter,
    review_session: Option<&ReviewSessionServer>,
) -> Result<()> {
    let (refresh_tx, refresh_rx) = mpsc::channel();
    let (preview_tx, preview_rx) = mpsc::channel();
    let (notice_tx, notice_rx) = mpsc::channel();
    spawn_refresh(app, refresh_tx.clone());
    let mut next_auto_refresh = next_auto_refresh_deadline(app);
    let mut next_review_reload = Instant::now() + Duration::from_millis(250);
    let mut startup_notices = StartupNoticeQueue::new(true, DEFAULT_STARTUP_NOTICE_DURATION);
    let mut notice_lookup_in_flight = false;
    let mut next_notice_check = Instant::now() + DEFAULT_STARTUP_NOTICE_DELAY;
    let job_control = JobControlSupport::default();

    loop {
        if review_session.is_some_and(|session| {
            session
                .stop_signal()
                .load(std::sync::atomic::Ordering::Relaxed)
        }) {
            return Ok(());
        }
        drain_refresh_results(app, &refresh_rx, &refresh_tx);
        maybe_spawn_auto_refresh(app, &refresh_tx, &mut next_auto_refresh);
        drain_preview_results(app, &preview_rx);
        spawn_preview_if_needed(app, preview_tx.clone());
        drain_startup_notice_results(
            app,
            &notice_rx,
            &mut startup_notices,
            &mut notice_lookup_in_flight,
        );
        maybe_spawn_startup_notice_lookup(
            &notice_tx,
            &mut notice_lookup_in_flight,
            &mut next_notice_check,
        );
        if startup_notices.tick(Instant::now()) {
            sync_startup_notice(app, &startup_notices);
        }
        let session_reload = review_session.is_some_and(|session| {
            session
                .reload_signal()
                .swap(false, std::sync::atomic::Ordering::Relaxed)
        });
        let watched_reload = app
            .review
            .as_ref()
            .is_some_and(|review| review.options().watch)
            && Instant::now() >= next_review_reload;
        if session_reload || watched_reload {
            reload_review(app);
            next_review_reload = Instant::now() + Duration::from_millis(250);
        }
        if let Some(review) = &mut app.review {
            review.tick_extension_notifications(Instant::now());
        }
        terminal.draw(|frame| views::render(app, highlighter, frame))?;
        if !event::poll(Duration::from_millis(50))? {
            continue;
        }
        match event::read()? {
            Event::Key(key) => {
                if handle_key(terminal, app, key, &refresh_tx, &job_control)? {
                    return Ok(());
                }
                process_review_extension_trust(app);
            }
            Event::Mouse(mouse) if app.active_tab == Tab::Review => {
                if let Some(review) = &mut app.review {
                    review.handle_mouse_event(mouse);
                }
                process_review_extension_trust(app);
            }
            Event::Mouse(_)
            | Event::Resize(_, _)
            | Event::FocusGained
            | Event::FocusLost
            | Event::Paste(_) => {}
        }
    }
}

fn process_review_extension_trust(app: &mut App) {
    let repo_root = app.repo_root.clone();
    let vcs = app.config.review.vcs.clone();
    let exclude_untracked = app.config.review.exclude_untracked;
    let color_moved = app.config.review.color_moved;
    let Some(review) = &mut app.review else {
        return;
    };
    let mut reload = || {
        let preference = ProviderPreference::parse(&vcs).map_err(anyhow::Error::from)?;
        let provider =
            AnyProvider::discover(&repo_root, preference).map_err(anyhow::Error::from)?;
        provider
            .working_tree(&DiffRequest {
                exclude_untracked,
                color_moved,
                ..DiffRequest::default()
            })
            .map_err(anyhow::Error::from)
    };
    let mut reload: Option<&mut dyn FnMut() -> anyhow::Result<workdeck_core::Changeset>> =
        Some(&mut reload);
    review.process_extension_trust_request(&mut reload);
}

fn maybe_spawn_startup_notice_lookup(
    sender: &Sender<Option<StartupNotice>>,
    in_flight: &mut bool,
    next_check: &mut Instant,
) {
    let now = Instant::now();
    if now < *next_check {
        return;
    }
    *next_check = now + DEFAULT_STARTUP_NOTICE_REPEAT;
    if *in_flight {
        return;
    }
    *in_flight = true;
    let sender = sender.clone();
    thread::spawn(move || {
        let notice = crate::update_notice::StartupUpdateNoticeContext::current()
            .ok()
            .and_then(|context| crate::update_notice::resolve_startup_update_notice(&context));
        let _ = sender.send(notice);
    });
}

fn drain_startup_notice_results(
    app: &mut App,
    receiver: &Receiver<Option<StartupNotice>>,
    queue: &mut StartupNoticeQueue,
    in_flight: &mut bool,
) {
    while let Ok(notice) = receiver.try_recv() {
        *in_flight = false;
        queue.enqueue(notice, Instant::now());
    }
    sync_startup_notice(app, queue);
}

fn sync_startup_notice(app: &mut App, queue: &StartupNoticeQueue) {
    let text = queue.text().map(str::to_owned);
    if app.startup_notice != text {
        app.startup_notice = text;
    }
}

fn reload_review(app: &mut App) {
    let result = (|| {
        let preference =
            ProviderPreference::parse(&app.config.review.vcs).map_err(anyhow::Error::from)?;
        let provider =
            AnyProvider::discover(&app.repo_root, preference).map_err(anyhow::Error::from)?;
        provider
            .working_tree(&DiffRequest {
                exclude_untracked: app.config.review.exclude_untracked,
                color_moved: app.config.review.color_moved,
                ..DiffRequest::default()
            })
            .map_err(anyhow::Error::from)
    })();
    if let Some(review) = &mut app.review {
        match result {
            Ok(changeset) => review.reload(changeset),
            Err(error) => review.set_status(format!("reload failed: {error:#}")),
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
    job_control: &JobControlSupport,
) -> Result<bool> {
    let layout = terminal
        .size()
        .map(|area| LayoutMode::for_width(area.width))
        .unwrap_or(LayoutMode::Wide);
    let narrow_files = app.active_tab == Tab::Files
        && layout == LayoutMode::Narrow
        && app.focus != crate::app::FocusPane::Preview;

    if app.active_tab == Tab::Review
        && let Some(review) = &mut app.review
        && review.extension_trust_prompt_root().is_some()
    {
        review.handle_key(key);
        return Ok(false);
    }

    match job_control.action(key, JobControlPlatform::current(), false) {
        Some(JobControlAction::Interrupt) => return Ok(true),
        Some(JobControlAction::Suspend) => {
            suspend_terminal_job(terminal)?;
            return Ok(false);
        }
        None => {}
    }

    if app.active_tab == Tab::Review
        && let Some(review) = &mut app.review
        && review.has_extension_dialog()
    {
        review.handle_key(key);
        if review.take_reload_requested() {
            reload_review(app);
        }
        return Ok(false);
    }

    if app.help_visible {
        if key.code == KeyCode::Esc || configured_key(key, &app.config.keys.help) {
            app.help_visible = false;
        }
        return Ok(false);
    }

    if app.active_tab == Tab::Search {
        match key.code {
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

    if app.active_tab == Tab::Review {
        if key.code == KeyCode::BackTab {
            app.active_tab = app.active_tab.previous();
            return Ok(false);
        }
        if let Some(review) = &mut app.review {
            review.handle_key(key);
            if review.take_quit_requested() {
                return Ok(true);
            }
            if let Some(request) = review.take_editor_request()
                && let Some(message) = open_review_editor_in_crossterm(terminal, &request)
            {
                review.set_status(message);
            }
            if review.take_reload_requested() {
                reload_review(app);
            }
        }
        return Ok(false);
    }

    if configured_key(key, &app.config.keys.quit) {
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

#[cfg(unix)]
struct CrosstermJobControlRuntime<'a> {
    terminal: &'a mut Terminal<CrosstermBackend<Stdout>>,
}

#[cfg(unix)]
impl JobControlRuntime for CrosstermJobControlRuntime<'_> {
    fn is_destroyed(&self) -> bool {
        false
    }

    fn suspend_renderer(&mut self) -> Result<(), String> {
        restore_terminal(self.terminal).map_err(|error| error.to_string())
    }

    fn signal_foreground_process_group(&mut self) -> Result<(), String> {
        // SAFETY: group zero and SIGTSTP are fixed libc values; no pointer crosses the FFI boundary.
        let result = unsafe { libc::kill(0, libc::SIGTSTP) };
        if result == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error().to_string())
        }
    }

    fn resume_renderer(&mut self) -> Result<(), String> {
        *self.terminal = setup_terminal().map_err(|error| error.to_string())?;
        Ok(())
    }
}

#[cfg(unix)]
fn suspend_terminal_job(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    let mut runtime = CrosstermJobControlRuntime { terminal };
    suspend_foreground_process_group(&mut runtime).map_err(anyhow::Error::msg)
}

#[cfg(not(unix))]
fn suspend_terminal_job(_terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    Ok(())
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
}
