//! Ratatui review canvas.

mod agent_annotations;
mod agent_note_geometry;
mod agent_popover;
mod color;
mod command_keys;
mod current_review_controller;
mod current_review_refresh;
mod cursor_highlight;
mod extension_notifications;
mod file_header;
mod file_view_geometry;
mod hunk_scroll;
mod ids;
mod job_control;
mod keyboard;
mod line_highlights;
mod list_geometry;
mod menu;
mod mouse_scroll;
mod shutdown;
mod spatial;
mod startup_notices;
mod synthetic_key_event;
mod terminal_runtime;
mod text;
mod theme_detection;
mod timed_notice;
mod ui_geometry;
mod watched_input;

pub use agent_annotations::*;
pub use agent_note_geometry::*;
pub use agent_popover::*;
pub use color::*;
pub use command_keys::*;
pub use current_review_controller::*;
pub use current_review_refresh::*;
pub use cursor_highlight::*;
pub use extension_notifications::*;
pub use file_header::*;
pub use file_view_geometry::*;
pub use hunk_scroll::*;
pub use ids::*;
pub use job_control::*;
pub use keyboard::*;
pub use line_highlights::*;
pub use list_geometry::*;
pub use menu::*;
pub use mouse_scroll::*;
pub use shutdown::*;
pub use spatial::*;
pub use startup_notices::*;
pub use synthetic_key_event::*;
pub use terminal_runtime::*;
pub use text::*;
pub use theme_detection::*;
pub use timed_notice::*;
pub use ui_geometry::*;
pub use watched_input::*;

use anyhow::Result;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
    MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Widget, Wrap};
use std::cell::Cell;
use std::collections::BTreeSet;
use std::io::{self, IsTerminal};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use workdeck_core::{Changeset, DiffFile, DiffLine, DiffLineKind, ReviewSelection, ReviewSide};
use workdeck_diff::{
    HighlightCache, SyntaxToken, TextSegment, clip_segments, plan_split_line_pairs,
    word_diff_ranges, wrap_segments,
};
use workdeck_extension_api::{
    ExtensionNotification, ExtensionNotificationHub, ExtensionNotificationSubscription,
    ExtensionPaneView, PanePlacement, ViewNode, ViewStyle, extension_pane_size,
};
use workdeck_review::{LayoutMode, ReviewComment, ReviewState, normalized_review_source_lines};
use workdeck_session::{ReviewSessionServer, default_discovery_directory};

#[derive(Debug, Clone)]
pub struct ReviewOptions {
    pub layout: LayoutMode,
    pub sidebar: bool,
    pub line_numbers: bool,
    pub tab_width: u16,
    pub cursor_line: CursorLineMode,
    pub hunk_headers: bool,
    pub wrap_lines: bool,
    pub file_gap: u16,
    pub hunk_gap: u16,
    pub transparent_background: bool,
    pub pager: bool,
    pub watch: bool,
    pub agent_notes: bool,
    pub syntax_theme: String,
    pub repo: Option<PathBuf>,
    pub extension_panes: Vec<ExtensionPaneView>,
    pub extension_notifications: Option<ExtensionNotificationHub>,
}

impl Default for ReviewOptions {
    fn default() -> Self {
        Self {
            layout: LayoutMode::Auto,
            sidebar: true,
            line_numbers: true,
            tab_width: 4,
            cursor_line: CursorLineMode::Row,
            hunk_headers: true,
            wrap_lines: false,
            file_gap: 1,
            hunk_gap: 0,
            transparent_background: false,
            pager: false,
            watch: false,
            agent_notes: false,
            syntax_theme: "base16-ocean.dark".into(),
            repo: None,
            extension_panes: Vec::new(),
            extension_notifications: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CursorLineMode {
    #[default]
    Row,
    Number,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Review,
    Sidebar,
}

#[derive(Debug)]
pub struct ReviewApp {
    state: Arc<Mutex<ReviewState>>,
    options: ReviewOptions,
    focus: Focus,
    scroll: usize,
    show_help: bool,
    should_quit: bool,
    reload_requested: bool,
    status: Option<String>,
    review_width: Cell<u16>,
    expanded_gaps: BTreeSet<(String, usize)>,
    highlights: Mutex<HighlightCache>,
    themes: ThemeController,
    extension_toasts: Arc<Mutex<ExtensionNotificationSurface>>,
    extension_notification_subscription: Option<ExtensionNotificationSubscription>,
    mouse_scroll_acceleration: ReviewMouseWheelScrollAcceleration,
    mouse_scroll_accumulator: f64,
}

impl ReviewApp {
    pub fn new(changeset: Changeset, options: ReviewOptions) -> Self {
        let mut state = ReviewState::new(changeset);
        state.set_layout(options.layout);
        let themes = ThemeController::new(options.syntax_theme.clone());
        let extension_toasts = Arc::new(Mutex::new(ExtensionNotificationSurface::default()));
        let extension_notification_subscription =
            options
                .extension_notifications
                .as_ref()
                .map(|notifications| {
                    let extension_toasts = Arc::clone(&extension_toasts);
                    notifications.subscribe(move |notification| {
                        extension_toasts
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .enqueue(notification);
                    })
                });
        Self {
            state: Arc::new(Mutex::new(state)),
            options,
            focus: Focus::Review,
            scroll: 0,
            show_help: false,
            should_quit: false,
            reload_requested: false,
            status: None,
            review_width: Cell::new(120),
            expanded_gaps: BTreeSet::new(),
            highlights: Mutex::new(HighlightCache::default()),
            themes,
            extension_toasts,
            extension_notification_subscription,
            mouse_scroll_acceleration: ReviewMouseWheelScrollAcceleration::default(),
            mouse_scroll_accumulator: 0.0,
        }
    }

    pub fn shared_state(&self) -> Arc<Mutex<ReviewState>> {
        Arc::clone(&self.state)
    }

    pub fn layout(&self) -> LayoutMode {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .layout()
    }

    pub fn options(&self) -> &ReviewOptions {
        &self.options
    }

    pub fn reload(&mut self, changeset: Changeset) {
        if self.with_state(|state| state.changeset() != &changeset) {
            self.with_state(|state| state.reload(changeset));
            self.status = Some("review reloaded".into());
            self.highlights
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clear();
        }
    }

    pub fn set_status(&mut self, status: impl Into<String>) {
        self.status = Some(status.into());
    }

    pub fn tick_extension_notifications(&mut self, now: Instant) {
        self.extension_toasts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .tick(now);
    }

    #[must_use]
    pub fn active_extension_notification(&self) -> Option<ExtensionNotification> {
        self.extension_toasts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active()
            .cloned()
    }

    #[must_use]
    pub fn has_extension_notification_subscription(&self) -> bool {
        self.extension_notification_subscription.is_some()
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }
        if self.show_help {
            if matches!(
                key.code,
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')
            ) {
                self.show_help = false;
            }
            return;
        }
        if matches!(
            key.code,
            KeyCode::Down
                | KeyCode::Up
                | KeyCode::PageDown
                | KeyCode::PageUp
                | KeyCode::Home
                | KeyCode::End
                | KeyCode::Char('j')
                | KeyCode::Char('k')
                | KeyCode::Char('g')
                | KeyCode::Char('G')
        ) {
            self.mouse_scroll_acceleration.reset();
            self.mouse_scroll_accumulator = 0.0;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Char('?') => self.show_help = true,
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::Review => Focus::Sidebar,
                    Focus::Sidebar => Focus::Review,
                };
            }
            KeyCode::Char('f') => self.options.sidebar = !self.options.sidebar,
            KeyCode::Char('w') => self.options.wrap_lines = !self.options.wrap_lines,
            KeyCode::Char('e') => self.toggle_source_gap(),
            KeyCode::Char('o') => self.options.agent_notes = !self.options.agent_notes,
            KeyCode::Char('l') => self.options.line_numbers = !self.options.line_numbers,
            KeyCode::Char('t') => {
                let theme = self.themes.cycle_preview();
                self.options.syntax_theme = theme.clone();
                self.highlights
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clear();
                self.status = Some(format!("theme {theme}"));
            }
            KeyCode::Char('r') => self.reload_requested = true,
            KeyCode::Char('s') => self.with_state(|state| state.set_layout(LayoutMode::Split)),
            KeyCode::Char('u') => self.with_state(|state| state.set_layout(LayoutMode::Stack)),
            KeyCode::Char('a') => self.with_state(|state| state.set_layout(LayoutMode::Auto)),
            KeyCode::Char(']') => self.navigate(ReviewState::next_file),
            KeyCode::Char('[') => self.navigate(ReviewState::previous_file),
            KeyCode::Char('n') => self.navigate(ReviewState::next_hunk),
            KeyCode::Char('p') => self.navigate(ReviewState::previous_hunk),
            KeyCode::Down | KeyCode::Char('j') => match self.focus {
                Focus::Review => self.scroll = self.scroll.saturating_add(1),
                Focus::Sidebar => {
                    self.navigate(ReviewState::next_file);
                }
            },
            KeyCode::Up | KeyCode::Char('k') => match self.focus {
                Focus::Review => self.scroll = self.scroll.saturating_sub(1),
                Focus::Sidebar => {
                    self.navigate(ReviewState::previous_file);
                }
            },
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(10),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::Home | KeyCode::Char('g') => self.scroll = 0,
            KeyCode::End | KeyCode::Char('G') => self.scroll = usize::MAX,
            KeyCode::Enter if self.focus == Focus::Sidebar => {
                self.focus = Focus::Review;
                self.scroll_to_selection();
            }
            _ => {}
        }
    }

    fn scroll_to_selection(&mut self) {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let selected = state.selection();
        let width = self.review_width.get();
        let layout = state.resolved_layout(width);
        let mut highlights = self
            .highlights
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let rows = build_review_rows(
            state.changeset(),
            state.comments(),
            selected,
            layout,
            &self.options,
            width,
            &mut highlights,
            &self.expanded_gaps,
        );
        self.scroll = selected
            .hunk_index
            .and_then(|hunk_index| {
                rows.hunk_tops
                    .get(&(selected.file_index, hunk_index))
                    .copied()
            })
            .or_else(|| rows.file_tops.get(selected.file_index).copied())
            .unwrap_or(0);
    }

    fn navigate(&mut self, action: impl FnOnce(&mut ReviewState) -> bool) {
        if self.with_state(action) {
            self.scroll_to_selection();
        }
    }

    fn toggle_source_gap(&mut self) {
        let target = self.with_state(|state| {
            let selection = state.selection();
            let file = state.selected_file()?;
            file.sources.new.as_ref().or(file.sources.old.as_ref())?;
            Some((file.key.clone(), selection.hunk_index.unwrap_or(0)))
        });
        let Some(key) = target else {
            self.status = Some("source expansion is unavailable for this file".into());
            return;
        };
        if !self.expanded_gaps.remove(&key) {
            self.expanded_gaps.insert(key);
            self.status = Some("source gap expanded".into());
        } else {
            self.status = Some("source gap collapsed".into());
        }
        self.scroll_to_selection();
    }

    fn with_state<T>(&self, action: impl FnOnce(&mut ReviewState) -> T) -> T {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        action(&mut state)
    }

    pub fn handle_mouse(&mut self, kind: MouseEventKind) {
        self.handle_mouse_at(kind, Instant::now());
    }

    fn handle_mouse_at(&mut self, kind: MouseEventKind, now: Instant) {
        let direction = match kind {
            MouseEventKind::ScrollDown => 1.0,
            MouseEventKind::ScrollUp => -1.0,
            _ => return,
        };
        self.mouse_scroll_accumulator += direction * self.mouse_scroll_acceleration.tick(now);
        let integer_scroll = self.mouse_scroll_accumulator.trunc() as isize;
        if integer_scroll > 0 {
            self.scroll = self.scroll.saturating_add(integer_scroll.unsigned_abs());
        } else if integer_scroll < 0 {
            self.scroll = self.scroll.saturating_sub(integer_scroll.unsigned_abs());
        }
        self.mouse_scroll_accumulator -= integer_scroll as f64;
    }
}

/// Generation-based theme preview state. Only the newest preview may commit, and cursor palette
/// writes are emitted solely when the requested value changes. These invariants cover the two
/// stable-only Hunk theme regressions without relying on a React lifecycle.
#[derive(Debug)]
pub struct ThemeController {
    active: String,
    requested: String,
    generation: u64,
    cursor_palette: Option<String>,
    cursor_updates: u64,
}

impl ThemeController {
    pub fn new(theme: String) -> Self {
        Self {
            active: theme.clone(),
            requested: theme,
            generation: 0,
            cursor_palette: None,
            cursor_updates: 0,
        }
    }

    pub fn request_preview(&mut self, theme: impl Into<String>) -> u64 {
        self.generation = self.generation.saturating_add(1);
        self.requested = theme.into();
        self.generation
    }

    pub fn commit_preview(&mut self, generation: u64) -> bool {
        if generation != self.generation {
            return false;
        }
        self.active.clone_from(&self.requested);
        true
    }

    pub fn set_cursor_palette(&mut self, palette: Option<String>) -> bool {
        if self.cursor_palette == palette {
            return false;
        }
        self.cursor_palette = palette;
        self.cursor_updates = self.cursor_updates.saturating_add(1);
        true
    }

    fn cycle_preview(&mut self) -> String {
        const THEMES: &[&str] = &[
            "base16-ocean.dark",
            "Solarized (dark)",
            "Solarized (light)",
            "InspiredGitHub",
        ];
        let next = THEMES
            .iter()
            .position(|theme| *theme == self.requested)
            .map_or(0, |index| (index + 1) % THEMES.len());
        let generation = self.request_preview(THEMES[next]);
        self.commit_preview(generation);
        self.set_cursor_palette(Some(THEMES[next].into()));
        self.active.clone()
    }
}

pub fn run_review(changeset: Changeset, options: ReviewOptions) -> Result<()> {
    run_review_inner(changeset, options, None)
}

pub fn run_review_with_reload<F>(
    changeset: Changeset,
    options: ReviewOptions,
    reload: &mut F,
) -> Result<()>
where
    F: FnMut() -> Result<Changeset>,
{
    run_review_inner(changeset, options, Some(reload))
}

fn run_review_inner(
    changeset: Changeset,
    options: ReviewOptions,
    mut reloader: Option<&mut dyn FnMut() -> Result<Changeset>>,
) -> Result<()> {
    if !io::stdout().is_terminal() {
        anyhow::bail!(
            "interactive review requires a terminal; use a Workdeck headless command for redirected output"
        );
    }
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let session_repo = options
        .repo
        .clone()
        .map(Ok)
        .unwrap_or_else(std::env::current_dir)?;
    let mut app = ReviewApp::new(changeset, options);
    let session = default_discovery_directory()
        .map(|directory| ReviewSessionServer::spawn(app.shared_state(), session_repo, directory))
        .transpose()?;
    let stop = session.as_ref().map(ReviewSessionServer::stop_signal);
    let reload = session.as_ref().map(ReviewSessionServer::reload_signal);
    let result = run_loop(
        &mut terminal,
        &mut app,
        stop.as_deref(),
        reload.as_deref(),
        &mut reloader,
    );
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    result
}

fn run_loop<B: Backend>(
    terminal: &mut Terminal<B>,
    app: &mut ReviewApp,
    session_stop: Option<&AtomicBool>,
    session_reload: Option<&AtomicBool>,
    reloader: &mut Option<&mut dyn FnMut() -> Result<Changeset>>,
) -> Result<()> {
    let mut next_reload = Instant::now() + Duration::from_millis(250);
    while !app.should_quit && !session_stop.is_some_and(|stop| stop.load(Ordering::Relaxed)) {
        app.tick_extension_notifications(Instant::now());
        terminal.draw(|frame| render(frame.area(), frame.buffer_mut(), app))?;
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => app.handle_key(key),
                Event::Mouse(mouse) => app.handle_mouse(mouse.kind),
                Event::Resize(_, _) | Event::FocusGained | Event::FocusLost | Event::Paste(_) => {}
            }
        }
        let session_requested =
            session_reload.is_some_and(|reload| reload.swap(false, Ordering::Relaxed));
        let timer_requested = app.options.watch && Instant::now() >= next_reload;
        let manual_requested = app.reload_requested;
        let reload_requested = manual_requested || session_requested || timer_requested;
        if reload_requested {
            app.reload_requested = false;
            next_reload = Instant::now() + Duration::from_millis(250);
            match reloader.as_deref_mut() {
                Some(reload) => match reload() {
                    Ok(changeset) => {
                        let mut state = app
                            .state
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        if state.changeset() != &changeset {
                            state.reload(changeset);
                            app.status = Some("review reloaded".into());
                        }
                    }
                    Err(error) => app.status = Some(format!("reload failed: {error:#}")),
                },
                None if manual_requested || session_requested => {
                    app.status = Some("this review input cannot be reloaded".into());
                }
                None => {}
            }
        }
    }
    Ok(())
}

pub fn render(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    let background = if app.options.transparent_background {
        Color::Reset
    } else {
        Color::Rgb(13, 17, 23)
    };
    Block::default()
        .style(Style::default().bg(background))
        .render(area, buffer);
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);
    render_header(outer[0], buffer, app);
    render_body(outer[1], buffer, app);
    render_footer(outer[2], buffer, app);
    if app.show_help {
        render_help(area, buffer);
    }
}

/// Render the review surface inside Workdeck's unified tab shell.
pub fn render_embedded(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    render_body(area, buffer, app);
    if app.active_extension_notification().is_some() {
        let toast_area = Rect {
            x: area.x,
            y: area.bottom().saturating_sub(1),
            width: area.width,
            height: area.height.min(1),
        };
        Clear.render(toast_area, buffer);
        render_extension_toast(toast_area, buffer, app);
    }
}

fn render_header(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    let state = app
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let stats = state.changeset().stats();
    let layout = state.resolved_layout(area.width);
    let title = Line::from(vec![
        Span::styled(
            " Workdeck ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {} ", state.changeset().title),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{} files", state.changeset().files.len()),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            format!("  +{}", stats.additions),
            Style::default().fg(Color::Green),
        ),
        Span::styled(
            format!(" -{}", stats.deletions),
            Style::default().fg(Color::Red),
        ),
        Span::styled(
            format!("  {:?}", layout).to_lowercase(),
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    Paragraph::new(title).render(area, buffer);
}

fn render_body(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    let mut review_area = area;
    let mut panes = Vec::new();
    for pane in &app.options.extension_panes {
        if review_area.width < 20 || review_area.height < 6 {
            break;
        }
        let requested = extension_pane_size(&pane.pane, None).preferred;
        let (direction, constraints, pane_index, review_index) = match pane.pane.placement {
            PanePlacement::Left => (
                Direction::Horizontal,
                [Constraint::Length(requested), Constraint::Min(20)],
                0,
                1,
            ),
            PanePlacement::Right => (
                Direction::Horizontal,
                [Constraint::Min(20), Constraint::Length(requested)],
                1,
                0,
            ),
            PanePlacement::Top => (
                Direction::Vertical,
                [Constraint::Length(requested), Constraint::Min(4)],
                0,
                1,
            ),
            PanePlacement::Bottom => (
                Direction::Vertical,
                [Constraint::Min(4), Constraint::Length(requested)],
                1,
                0,
            ),
        };
        let split = Layout::default()
            .direction(direction)
            .constraints(constraints)
            .split(review_area);
        panes.push((pane, split[pane_index]));
        review_area = split[review_index];
    }
    render_builtin_body(review_area, buffer, app);
    for (pane, pane_area) in panes {
        render_extension_pane(pane_area, buffer, pane);
    }
}

fn render_builtin_body(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    let state = app
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if state.changeset().is_empty() {
        Paragraph::new("No changes to review")
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::TOP))
            .render(area, buffer);
        return;
    }
    let responsive = state.responsive_layout(area.width);
    drop(state);
    let chunks = if app.options.sidebar && responsive.show_sidebar {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(30), Constraint::Min(30)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(0), Constraint::Min(1)])
            .split(area)
    };
    if chunks[0].width > 0 {
        render_sidebar(chunks[0], buffer, app);
    }
    render_review(chunks[1], buffer, app);
}

fn render_extension_pane(area: Rect, buffer: &mut Buffer, pane: &ExtensionPaneView) {
    let mut lines = Vec::new();
    flatten_view(&pane.content, 0, &mut lines);
    Paragraph::new(lines)
        .block(
            Block::default()
                .title(format!(" {} ", pane.pane.title))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Magenta)),
        )
        .wrap(Wrap { trim: false })
        .render(area, buffer);
}

fn flatten_view(node: &ViewNode, indent: usize, lines: &mut Vec<Line<'static>>) {
    match node {
        ViewNode::Text { text, style } => lines.push(Line::from(vec![
            Span::raw(" ".repeat(indent)),
            Span::styled(text.clone(), extension_style(style)),
        ])),
        ViewNode::Row { children, gap } => {
            let mut spans = vec![Span::raw(" ".repeat(indent))];
            for (index, child) in children.iter().enumerate() {
                if index > 0 {
                    spans.push(Span::raw(" ".repeat(usize::from(*gap))));
                }
                match child {
                    ViewNode::Text { text, style } => {
                        spans.push(Span::styled(text.clone(), extension_style(style)));
                    }
                    _ => spans.push(Span::raw("…")),
                }
            }
            lines.push(Line::from(spans));
        }
        ViewNode::Column { children, gap } => {
            for (index, child) in children.iter().enumerate() {
                if index > 0 {
                    lines.extend((0..*gap).map(|_| Line::default()));
                }
                flatten_view(child, indent, lines);
            }
        }
        ViewNode::List { items, selected } => {
            for (index, item) in items.iter().enumerate() {
                let marker = if *selected == Some(index) {
                    "› "
                } else {
                    "  "
                };
                lines.push(Line::styled(
                    format!("{}{}", " ".repeat(indent), marker),
                    Style::default().fg(Color::Cyan),
                ));
                flatten_view(item, indent + 2, lines);
            }
        }
        ViewNode::Divider => lines.push(Line::styled(
            format!("{}────────", " ".repeat(indent)),
            Style::default().fg(Color::DarkGray),
        )),
        ViewNode::Empty => {}
    }
}

fn extension_style(style: &ViewStyle) -> Style {
    let mut result = Style::default();
    if let Some(color) = style.foreground.as_deref().and_then(parse_extension_color) {
        result = result.fg(color);
    }
    if let Some(color) = style.background.as_deref().and_then(parse_extension_color) {
        result = result.bg(color);
    }
    if style.bold {
        result = result.add_modifier(Modifier::BOLD);
    }
    if style.italic {
        result = result.add_modifier(Modifier::ITALIC);
    }
    if style.underline {
        result = result.add_modifier(Modifier::UNDERLINED);
    }
    if style.dim {
        result = result.add_modifier(Modifier::DIM);
    }
    result
}

fn parse_extension_color(value: &str) -> Option<Color> {
    match value {
        "accent" | "info" => Some(Color::Cyan),
        "success" => Some(Color::Green),
        "warning" => Some(Color::Yellow),
        "danger" | "error" => Some(Color::Red),
        "muted" | "subtle" => Some(Color::DarkGray),
        "heading" => Some(Color::White),
        value if value.len() == 7 && value.starts_with('#') => {
            let red = u8::from_str_radix(&value[1..3], 16).ok()?;
            let green = u8::from_str_radix(&value[3..5], 16).ok()?;
            let blue = u8::from_str_radix(&value[5..7], 16).ok()?;
            Some(Color::Rgb(red, green, blue))
        }
        _ => None,
    }
}

fn render_sidebar(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    let state = app
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let selected = state.selection().file_index;
    let visible_rows = usize::from(area.height.saturating_sub(1).max(1));
    let start = list_window_start(selected, state.changeset().files.len(), visible_rows);
    let items = state
        .changeset()
        .files
        .iter()
        .enumerate()
        .skip(start)
        .take(visible_rows)
        .map(|(index, file)| {
            let marker = if index == selected { "›" } else { " " };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{marker} "), Style::default().fg(Color::Cyan)),
                Span::raw(truncate_start(
                    &file.path,
                    area.width.saturating_sub(10) as usize,
                )),
                Span::styled(
                    format!(" +{}", file.stats.additions),
                    Style::default().fg(Color::Green),
                ),
                Span::styled(
                    format!(" -{}", file.stats.deletions),
                    Style::default().fg(Color::Red),
                ),
            ]))
        })
        .collect::<Vec<_>>();
    let border_style = if app.focus == Focus::Sidebar {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    List::new(items)
        .block(
            Block::default()
                .title(" Files ")
                .borders(Borders::TOP | Borders::RIGHT)
                .border_style(border_style),
        )
        .render(area, buffer);
}

fn render_review(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    app.review_width.set(area.width);
    let state = app
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let layout = state.resolved_layout(area.width);
    let mut highlights = app
        .highlights
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let rows = build_review_rows(
        state.changeset(),
        state.comments(),
        state.selection(),
        layout,
        &app.options,
        area.width,
        &mut highlights,
        &app.expanded_gaps,
    );
    drop(state);
    let viewport = area.height.saturating_sub(1) as usize;
    let max_scroll = rows.lines.len().saturating_sub(viewport);
    let scroll = if app.scroll == usize::MAX {
        max_scroll
    } else {
        app.scroll.min(max_scroll)
    };
    let visible = rows
        .lines
        .into_iter()
        .skip(scroll)
        .take(viewport)
        .collect::<Vec<_>>();
    let border_style = if app.focus == Focus::Review {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    Paragraph::new(visible)
        .block(
            Block::default()
                .title(" Review ")
                .borders(Borders::TOP)
                .border_style(border_style),
        )
        .render(area, buffer);
}

#[derive(Debug)]
struct ReviewRows {
    lines: Vec<Line<'static>>,
    file_tops: Vec<usize>,
    hunk_tops: std::collections::HashMap<(usize, usize), usize>,
}

#[allow(clippy::too_many_arguments)]
fn build_review_rows(
    changeset: &Changeset,
    comments: &[ReviewComment],
    selection: ReviewSelection,
    layout: LayoutMode,
    options: &ReviewOptions,
    width: u16,
    highlight_cache: &mut HighlightCache,
    expanded_gaps: &BTreeSet<(String, usize)>,
) -> ReviewRows {
    let mut rows = Vec::new();
    let mut file_tops = Vec::with_capacity(changeset.files.len());
    let mut hunk_tops = std::collections::HashMap::new();
    let header_stats_width = max_file_header_stats_width(&changeset.files);
    for (file_index, file) in changeset.files.iter().enumerate() {
        if file_index > 0 {
            rows.extend((0..options.file_gap).map(|_| Line::default()));
        }
        file_tops.push(rows.len());
        rows.push(file_header(
            file,
            selection.file_index == file_index,
            usize::from(width),
            header_stats_width,
        ));
        let file_selection = if selection.file_index == file_index {
            selection
        } else {
            ReviewSelection::default()
        };
        let highlighted = highlight_cache.highlight(file, &options.syntax_theme);
        if options.agent_notes {
            rows.extend(agent_rows(file, layout, usize::from(width)));
        }
        if file.flags.too_large {
            let qualifier = if file.stats.truncated {
                "at least "
            } else {
                ""
            };
            rows.push(Line::styled(
                format!(
                    "  File exceeds review limits ({qualifier}{} lines)",
                    file.stats.additions.saturating_add(file.stats.deletions)
                ),
                Style::default().fg(Color::Yellow),
            ));
            continue;
        }
        if file.flags.binary {
            rows.push(Line::styled(
                "  Binary file",
                Style::default().fg(Color::Yellow),
            ));
            continue;
        }
        let mut previous_old_end = 1;
        let mut previous_new_end = 1;
        for (hunk_index, hunk) in file.hunks.iter().enumerate() {
            rows.extend(source_gap_rows(
                file,
                hunk_index,
                previous_old_end,
                previous_new_end,
                hunk.old_start,
                hunk.new_start,
                layout,
                options,
                width,
                expanded_gaps,
            ));
            if hunk_index > 0 {
                rows.extend((0..options.hunk_gap).map(|_| Line::default()));
            }
            hunk_tops.insert((file_index, hunk_index), rows.len());
            if options.hunk_headers {
                let selected_hunk = selection.file_index == file_index
                    && selection.hunk_index == Some(hunk_index)
                    && selection.line.is_none();
                rows.push(Line::styled(
                    format!("  {}", hunk.formatted_header()),
                    if selected_hunk {
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Rgb(139, 148, 158))
                    },
                ));
            }
            match layout {
                LayoutMode::Split => rows.extend(split_hunk_rows(
                    file,
                    hunk,
                    options,
                    width,
                    highlighted.get(hunk_index),
                    comments,
                    file_selection,
                )),
                LayoutMode::Stack | LayoutMode::Auto => rows.extend(stack_hunk_rows(
                    file,
                    hunk,
                    options,
                    highlighted.get(hunk_index),
                    comments,
                    width,
                    file_selection,
                )),
            }
            previous_old_end = hunk.old_start.saturating_add(hunk.old_count);
            previous_new_end = hunk.new_start.saturating_add(hunk.new_count);
        }
        if file.sources.old.is_some() || file.sources.new.is_some() {
            let old_end = file
                .sources
                .old
                .as_ref()
                .map_or(previous_old_end, |source| {
                    u32::try_from(normalized_review_source_lines(&source.content).len())
                        .unwrap_or(u32::MAX)
                        .saturating_add(1)
                });
            let new_end = file
                .sources
                .new
                .as_ref()
                .map_or(previous_new_end, |source| {
                    u32::try_from(normalized_review_source_lines(&source.content).len())
                        .unwrap_or(u32::MAX)
                        .saturating_add(1)
                });
            rows.extend(source_gap_rows(
                file,
                file.hunks.len(),
                previous_old_end,
                previous_new_end,
                old_end,
                new_end,
                layout,
                options,
                width,
                expanded_gaps,
            ));
        }
    }
    ReviewRows {
        lines: rows,
        file_tops,
        hunk_tops,
    }
}

const MAX_EXPANDED_GAP_LINES: usize = 200;

#[allow(clippy::too_many_arguments)]
fn source_gap_rows(
    file: &DiffFile,
    gap_index: usize,
    old_start: u32,
    new_start: u32,
    old_end: u32,
    new_end: u32,
    layout: LayoutMode,
    options: &ReviewOptions,
    width: u16,
    expanded_gaps: &BTreeSet<(String, usize)>,
) -> Vec<Line<'static>> {
    let old_source = file.sources.old.as_ref();
    let new_source = file.sources.new.as_ref();
    if old_source.is_none() && new_source.is_none() {
        return Vec::new();
    }

    let old_count = usize::try_from(old_end.saturating_sub(old_start)).unwrap_or(usize::MAX);
    let new_count = usize::try_from(new_end.saturating_sub(new_start)).unwrap_or(usize::MAX);
    let count = match (old_source.is_some(), new_source.is_some()) {
        (true, true) => old_count.min(new_count),
        (true, false) => old_count,
        (false, true) => new_count,
        (false, false) => 0,
    };
    if count == 0 {
        return Vec::new();
    }

    let key = (file.key.clone(), gap_index);
    if !expanded_gaps.contains(&key) {
        return vec![source_gap_label(count, width, "e to expand")];
    }

    let old_lines = old_source.map(|source| normalized_review_source_lines(&source.content));
    let new_lines = new_source.map(|source| normalized_review_source_lines(&source.content));
    let visible_count = count.min(MAX_EXPANDED_GAP_LINES);
    let mut rows = Vec::with_capacity(visible_count.saturating_add(1));
    for offset in 0..visible_count {
        let old_number =
            old_source.map(|_| old_start.saturating_add(u32::try_from(offset).unwrap_or(u32::MAX)));
        let new_number =
            new_source.map(|_| new_start.saturating_add(u32::try_from(offset).unwrap_or(u32::MAX)));
        let old_content = old_lines.as_ref().and_then(|lines| {
            old_number
                .and_then(|line| usize::try_from(line.saturating_sub(1)).ok())
                .and_then(|index| lines.get(index))
                .map(String::as_str)
        });
        let new_content = new_lines.as_ref().and_then(|lines| {
            new_number
                .and_then(|line| usize::try_from(line.saturating_sub(1)).ok())
                .and_then(|index| lines.get(index))
                .map(String::as_str)
        });
        let content = new_content.or(old_content).unwrap_or_default().to_owned();
        let kind = match (old_content.is_some(), new_content.is_some()) {
            (true, false) => DiffLineKind::Deletion,
            (false, true) => DiffLineKind::Addition,
            _ => DiffLineKind::Context,
        };
        let line = DiffLine {
            kind,
            content,
            old_line: old_number.filter(|_| old_content.is_some()),
            new_line: new_number.filter(|_| new_content.is_some()),
            moved: false,
            no_newline_at_eof: false,
        };
        match layout {
            LayoutMode::Stack | LayoutMode::Auto => {
                rows.extend(stack_line_rows(&line, options, None, &[], false, width))
            }
            LayoutMode::Split => {
                let available = usize::from(width.saturating_sub(1));
                let left_width = available / 2;
                let right_width = available.saturating_sub(left_width);
                rows.extend(split_pair_rows(
                    SplitCellInput {
                        line: old_content.map(|_| &line),
                        highlighted: None,
                        emphasis: &[],
                    },
                    SplitCellInput {
                        line: new_content.map(|_| &line),
                        highlighted: None,
                        emphasis: &[],
                    },
                    options,
                    left_width,
                    right_width,
                    false,
                ));
            }
        }
    }
    if count > visible_count {
        rows.push(source_gap_label(
            count - visible_count,
            width,
            "expansion limit reached",
        ));
    }
    rows
}

fn source_gap_label(count: usize, width: u16, action: &str) -> Line<'static> {
    let spans = clip_styled_spans(
        vec![Span::styled(
            format!("  … {count} unchanged lines  ({action})"),
            Style::default().fg(Color::DarkGray),
        )],
        usize::from(width),
    );
    Line::from(spans)
}

fn agent_rows(file: &DiffFile, layout: LayoutMode, width: usize) -> Vec<Line<'static>> {
    let Some(context) = &file.agent else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    if let Some(summary) = &context.summary {
        let geometry = agent_note_box_layout(None, layout, width, 0);
        let summary = clip_styled_spans(
            vec![Span::styled(
                summary.clone(),
                Style::default().fg(Color::LightMagenta),
            )],
            geometry.content_width,
        );
        let mut spans = vec![Span::styled(
            format!("{}agent  ", " ".repeat(geometry.box_left)),
            Style::default().fg(Color::Magenta),
        )];
        spans.extend(summary);
        rows.push(Line::from(spans));
    }
    for annotation in &context.annotations {
        let anchor_side = if annotation.new_range.is_some() {
            Some(ReviewSide::New)
        } else if annotation.old_range.is_some() {
            Some(ReviewSide::Old)
        } else {
            None
        };
        let geometry = agent_note_box_layout(anchor_side, layout, width, 0);
        let location = annotation
            .new_range
            .map(|range| format!("+{}..{}", range.start, range.end))
            .or_else(|| {
                annotation
                    .old_range
                    .map(|range| format!("-{}..{}", range.start, range.end))
            })
            .unwrap_or_else(|| "file".into());
        let summary = clip_styled_spans(
            vec![Span::styled(
                annotation.summary.clone(),
                Style::default().fg(Color::LightMagenta),
            )],
            geometry.content_width,
        );
        let mut spans = vec![Span::styled(
            format!("{}note {location}  ", " ".repeat(geometry.box_left)),
            Style::default().fg(Color::Magenta),
        )];
        spans.extend(summary);
        rows.push(Line::from(spans));
        if let Some(rationale) = &annotation.rationale {
            let rationale = clip_styled_spans(
                vec![Span::styled(
                    rationale.clone(),
                    Style::default().fg(Color::DarkGray),
                )],
                geometry.content_width,
            );
            let mut spans = vec![Span::raw(" ".repeat(geometry.box_left + 2))];
            spans.extend(rationale);
            rows.push(Line::from(spans));
        }
    }
    rows
}

fn file_header(
    file: &DiffFile,
    selected: bool,
    width: usize,
    header_stats_width: usize,
) -> Line<'static> {
    let label_width = width.saturating_sub(2 + header_stats_width + 1);
    let label = fit_file_header_label(file, label_width);
    let label_cell_width = label.filename.width()
        + label
            .state_label
            .map_or(0, unicode_width::UnicodeWidthStr::width);
    let gap = width
        .saturating_sub(1 + label_cell_width + header_stats_width + 1)
        .max(1);
    let stats = file_header_stats(file);
    Line::from(vec![
        Span::raw(" "),
        Span::styled(
            label.filename,
            Style::default()
                .fg(if selected { Color::Cyan } else { Color::White })
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            label.state_label.unwrap_or_default(),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw(" ".repeat(gap.saturating_add(header_stats_width.saturating_sub(stats.width)))),
        Span::styled(stats.additions_text, Style::default().fg(Color::Green)),
        Span::raw(" "),
        Span::styled(stats.deletions_text, Style::default().fg(Color::Red)),
        Span::raw("  "),
    ])
}

fn stack_hunk_rows(
    file: &DiffFile,
    hunk: &workdeck_core::DiffHunk,
    options: &ReviewOptions,
    highlighted: Option<&Vec<Vec<SyntaxToken>>>,
    comments: &[ReviewComment],
    width: u16,
    selection: ReviewSelection,
) -> Vec<Line<'static>> {
    let mut rows = Vec::new();
    let mut emphasis = vec![Vec::new(); hunk.lines.len()];
    for pair in plan_split_line_pairs(&hunk.lines) {
        let (Some(old_index), Some(new_index)) = (pair.old_index, pair.new_index) else {
            continue;
        };
        if old_index == new_index {
            continue;
        }
        let ranges = word_diff_ranges(
            &expanded_line_content(&hunk.lines[old_index], options.tab_width),
            &expanded_line_content(&hunk.lines[new_index], options.tab_width),
        );
        emphasis[old_index] = ranges.old;
        emphasis[new_index] = ranges.new;
    }
    for (index, line) in hunk.lines.iter().enumerate() {
        rows.extend(stack_line_rows(
            line,
            options,
            highlighted.and_then(|lines| lines.get(index)),
            &emphasis[index],
            line_is_selected(line, selection),
            width,
        ));
        rows.extend(comment_rows(file, line, comments, width));
    }
    rows
}

fn stack_line_rows(
    line: &DiffLine,
    options: &ReviewOptions,
    highlighted: Option<&Vec<SyntaxToken>>,
    emphasis: &[Range<usize>],
    selected: bool,
    width: u16,
) -> Vec<Line<'static>> {
    let (marker, fg, default_bg) = line_style(line.kind);
    let bg = moved_line_background(line).unwrap_or(default_bg);
    let row_bg = if selected && options.cursor_line == CursorLineMode::Row {
        Color::Rgb(45, 55, 72)
    } else {
        bg
    };
    let number = if options.line_numbers {
        format!(
            "{:>4} {:>4} ",
            line.old_line.map_or(String::new(), |line| line.to_string()),
            line.new_line.map_or(String::new(), |line| line.to_string())
        )
    } else {
        String::new()
    };
    let number_style = Style::default()
        .fg(
            if selected && options.cursor_line == CursorLineMode::Number {
                Color::Cyan
            } else {
                Color::DarkGray
            },
        )
        .bg(row_bg);
    let marker_style = Style::default().fg(fg).bg(row_bg);
    let mut code = Vec::new();
    if let Some(tokens) = highlighted.filter(|tokens| !tokens.is_empty()) {
        let mut code_column = 0;
        code.extend(tokens.iter().map(|token| {
            let mut style = Style::default()
                .fg(Color::Rgb(
                    token.foreground.red,
                    token.foreground.green,
                    token.foreground.blue,
                ))
                .bg(row_bg);
            if token.bold {
                style = style.add_modifier(Modifier::BOLD);
            }
            if token.italic {
                style = style.add_modifier(Modifier::ITALIC);
            }
            if token.underline {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            Span::styled(
                expand_tabs(&token.text, options.tab_width, &mut code_column),
                style,
            )
        }));
    } else {
        code.push(Span::styled(
            expand_tabs(&line.content, options.tab_width, &mut 0),
            Style::default().fg(fg).bg(row_bg),
        ));
    }
    code = emphasize_spans(code, emphasis, emphasis_background(line.kind));
    let prefix_width = number.width() + 1;
    let content_width = usize::from(width).saturating_sub(prefix_width);
    let wrapped = if options.wrap_lines {
        wrap_styled_spans(code, content_width)
    } else {
        vec![clip_styled_spans(code, content_width)]
    };
    wrapped
        .into_iter()
        .enumerate()
        .map(|(index, mut code)| {
            let mut spans = if index == 0 {
                vec![
                    Span::styled(number.clone(), number_style),
                    Span::styled(marker.to_string(), marker_style),
                ]
            } else {
                vec![Span::styled(
                    " ".repeat(prefix_width),
                    Style::default().bg(row_bg),
                )]
            };
            spans.append(&mut code);
            spans = clip_styled_spans(spans, usize::from(width));
            pad_spans(&mut spans, usize::from(width), Style::default().bg(row_bg));
            Line::from(spans)
        })
        .collect()
}

fn split_hunk_rows(
    file: &DiffFile,
    hunk: &workdeck_core::DiffHunk,
    options: &ReviewOptions,
    width: u16,
    highlighted: Option<&Vec<Vec<SyntaxToken>>>,
    comments: &[ReviewComment],
    selection: ReviewSelection,
) -> Vec<Line<'static>> {
    let mut rows = Vec::new();
    let available = usize::from(width.saturating_sub(1));
    let left_width = available / 2;
    let right_width = available.saturating_sub(left_width);
    for pair in plan_split_line_pairs(&hunk.lines) {
        let old = pair.old_index.and_then(|index| hunk.lines.get(index));
        let new = pair.new_index.and_then(|index| hunk.lines.get(index));
        let emphasis = old
            .zip(new)
            .filter(|(old, new)| !std::ptr::eq(*old, *new))
            .map(|(old, new)| {
                word_diff_ranges(
                    &expanded_line_content(old, options.tab_width),
                    &expanded_line_content(new, options.tab_width),
                )
            });
        rows.extend(split_pair_rows(
            SplitCellInput {
                line: old,
                highlighted: pair
                    .old_index
                    .and_then(|index| highlighted.and_then(|lines| lines.get(index))),
                emphasis: emphasis.as_ref().map_or(&[], |ranges| &ranges.old),
            },
            SplitCellInput {
                line: new,
                highlighted: pair
                    .new_index
                    .and_then(|index| highlighted.and_then(|lines| lines.get(index))),
                emphasis: emphasis.as_ref().map_or(&[], |ranges| &ranges.new),
            },
            options,
            left_width,
            right_width,
            old.is_some_and(|line| line_is_selected(line, selection))
                || new.is_some_and(|line| line_is_selected(line, selection)),
        ));
        if let Some(line) = old {
            rows.extend(comment_rows(file, line, comments, width));
        }
        if pair.new_index != pair.old_index
            && let Some(line) = new
        {
            rows.extend(comment_rows(file, line, comments, width));
        }
    }
    rows
}

fn comment_rows(
    file: &DiffFile,
    line: &DiffLine,
    comments: &[ReviewComment],
    width: u16,
) -> Vec<Line<'static>> {
    let mut rows = Vec::new();
    for comment in comments
        .iter()
        .filter(|comment| comment.anchor.file_key == file.key)
        .filter(|comment| comment_matches_line(comment, line))
    {
        let author = comment.author.as_deref().unwrap_or(&comment.source);
        rows.push(Line::from(vec![
            Span::styled("  │ note ", Style::default().fg(Color::Magenta)),
            Span::styled(
                author.to_owned(),
                Style::default()
                    .fg(Color::LightMagenta)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        let body = comment.markup.as_deref().unwrap_or(&comment.summary);
        let rendered = workdeck_markup::render(body, usize::from(width.saturating_sub(8).max(1)));
        for text in rendered.lines {
            rows.push(Line::styled(
                format!("  │   {text}"),
                Style::default().fg(Color::LightMagenta),
            ));
        }
        if let Some(rationale) = &comment.rationale {
            rows.push(Line::styled(
                format!("  │   {rationale}"),
                Style::default().fg(Color::DarkGray),
            ));
        }
    }
    rows
}

fn line_is_selected(line: &DiffLine, selection: ReviewSelection) -> bool {
    let Some(target) = selection.line else {
        return false;
    };
    match selection.side {
        Some(ReviewSide::Old) => line.old_line == Some(target),
        Some(ReviewSide::New) => line.new_line == Some(target),
        None => line.old_line == Some(target) || line.new_line == Some(target),
    }
}

fn comment_matches_line(comment: &ReviewComment, line: &DiffLine) -> bool {
    let old_match = comment
        .anchor
        .old_range
        .zip(line.old_line)
        .is_some_and(|(range, line)| range.start <= line && line <= range.end);
    let new_match = comment
        .anchor
        .new_range
        .zip(line.new_line)
        .is_some_and(|(range, line)| range.start <= line && line <= range.end);
    old_match
        || new_match
        || comment.anchor.preferred_line.is_some_and(|target| {
            comment.anchor.preferred_side.map_or(
                line.old_line == Some(target) || line.new_line == Some(target),
                |side| match side {
                    workdeck_core::ReviewSide::Old => line.old_line == Some(target),
                    workdeck_core::ReviewSide::New => line.new_line == Some(target),
                },
            )
        })
}

#[derive(Debug, Clone, Copy)]
struct SplitCellInput<'a> {
    line: Option<&'a DiffLine>,
    highlighted: Option<&'a Vec<SyntaxToken>>,
    emphasis: &'a [Range<usize>],
}

fn split_pair_rows(
    old: SplitCellInput<'_>,
    new: SplitCellInput<'_>,
    options: &ReviewOptions,
    left_width: usize,
    right_width: usize,
    selected: bool,
) -> Vec<Line<'static>> {
    let mut old_lines = split_cell_lines(
        old.line,
        true,
        options,
        left_width,
        old.highlighted,
        old.emphasis,
        selected,
    );
    let mut new_lines = split_cell_lines(
        new.line,
        false,
        options,
        right_width,
        new.highlighted,
        new.emphasis,
        selected,
    );
    let height = old_lines.len().max(new_lines.len()).max(1);
    old_lines.resize_with(height, || vec![Span::raw(" ".repeat(left_width))]);
    new_lines.resize_with(height, || vec![Span::raw(" ".repeat(right_width))]);
    old_lines
        .into_iter()
        .zip(new_lines)
        .map(|(mut old, mut new)| {
            old.push(Span::styled("│", Style::default().fg(Color::DarkGray)));
            old.append(&mut new);
            Line::from(old)
        })
        .collect()
}

fn split_cell_lines(
    line: Option<&DiffLine>,
    old: bool,
    options: &ReviewOptions,
    width: usize,
    highlighted: Option<&Vec<SyntaxToken>>,
    emphasis: &[Range<usize>],
    selected: bool,
) -> Vec<Vec<Span<'static>>> {
    let Some(line) = line else {
        return vec![vec![Span::raw(" ".repeat(width))]];
    };
    let (marker, fg, default_bg) = line_style(line.kind);
    let bg = moved_line_background(line).unwrap_or(default_bg);
    let row_bg = if selected && options.cursor_line == CursorLineMode::Row {
        Color::Rgb(45, 55, 72)
    } else {
        bg
    };
    let number = if options.line_numbers {
        let number = if old { line.old_line } else { line.new_line };
        format!(
            "{:>4} ",
            number.map_or(String::new(), |line| line.to_string())
        )
    } else {
        String::new()
    };
    let number_style = Style::default()
        .fg(
            if selected && options.cursor_line == CursorLineMode::Number {
                Color::Cyan
            } else {
                Color::DarkGray
            },
        )
        .bg(row_bg);
    let marker_style = Style::default().fg(fg).bg(row_bg);
    let mut code = Vec::new();
    if let Some(tokens) = highlighted.filter(|tokens| !tokens.is_empty()) {
        let mut code_column = 0;
        code.extend(tokens.iter().map(|token| {
            let mut style = Style::default()
                .fg(Color::Rgb(
                    token.foreground.red,
                    token.foreground.green,
                    token.foreground.blue,
                ))
                .bg(row_bg);
            if token.bold {
                style = style.add_modifier(Modifier::BOLD);
            }
            if token.italic {
                style = style.add_modifier(Modifier::ITALIC);
            }
            if token.underline {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            Span::styled(
                expand_tabs(&token.text, options.tab_width, &mut code_column),
                style,
            )
        }));
    } else {
        code.push(Span::styled(
            expand_tabs(&line.content, options.tab_width, &mut 0),
            Style::default().fg(fg).bg(row_bg),
        ));
    }
    code = emphasize_spans(code, emphasis, emphasis_background(line.kind));
    let prefix_width = number.width() + 1;
    let content_width = width.saturating_sub(prefix_width);
    let wrapped = if options.wrap_lines {
        wrap_styled_spans(code, content_width)
    } else {
        vec![clip_styled_spans(code, content_width)]
    };
    wrapped
        .into_iter()
        .enumerate()
        .map(|(index, mut code)| {
            let mut spans = if index == 0 {
                vec![
                    Span::styled(number.clone(), number_style),
                    Span::styled(marker.to_string(), marker_style),
                ]
            } else {
                vec![Span::styled(
                    " ".repeat(prefix_width),
                    Style::default().bg(row_bg),
                )]
            };
            spans.append(&mut code);
            spans = clip_styled_spans(spans, width);
            pad_spans(&mut spans, width, Style::default().bg(row_bg));
            spans
        })
        .collect()
}

fn line_style(kind: DiffLineKind) -> (char, Color, Color) {
    match kind {
        DiffLineKind::Context => (' ', Color::Rgb(201, 209, 217), Color::Reset),
        DiffLineKind::Addition => ('+', Color::Rgb(126, 231, 135), Color::Rgb(14, 54, 30)),
        DiffLineKind::Deletion => ('-', Color::Rgb(255, 123, 114), Color::Rgb(67, 24, 29)),
    }
}

fn moved_line_background(line: &DiffLine) -> Option<Color> {
    if !line.moved {
        return None;
    }
    match line.kind {
        DiffLineKind::Addition => Some(Color::Rgb(17, 66, 64)),
        DiffLineKind::Deletion => Some(Color::Rgb(73, 35, 75)),
        DiffLineKind::Context => None,
    }
}

fn expanded_line_content(line: &DiffLine, tab_width: u16) -> String {
    expand_tabs(&line.content, tab_width, &mut 0)
}

fn emphasis_background(kind: DiffLineKind) -> Color {
    match kind {
        DiffLineKind::Deletion => Color::Rgb(126, 42, 49),
        DiffLineKind::Addition => Color::Rgb(34, 104, 58),
        DiffLineKind::Context => Color::Reset,
    }
}

fn emphasize_spans(
    spans: Vec<Span<'static>>,
    ranges: &[Range<usize>],
    background: Color,
) -> Vec<Span<'static>> {
    if ranges.is_empty() {
        return spans;
    }
    let mut result: Vec<Span<'static>> = Vec::new();
    let mut offset = 0;
    for span in spans {
        for character in span.content.chars() {
            let emphasized = ranges
                .iter()
                .any(|range| range.start <= offset && offset < range.end);
            let style = if emphasized {
                span.style.bg(background).add_modifier(Modifier::BOLD)
            } else {
                span.style
            };
            if let Some(last) = result.last_mut().filter(|last| last.style == style) {
                last.content.to_mut().push(character);
            } else {
                result.push(Span::styled(character.to_string(), style));
            }
            offset += character.len_utf8();
        }
    }
    result
}

fn wrap_styled_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Vec<Span<'static>>> {
    wrap_segments(spans_to_segments(spans), width)
        .into_iter()
        .map(segments_to_spans)
        .collect()
}

fn clip_styled_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    segments_to_spans(clip_segments(spans_to_segments(spans), width))
}

fn spans_to_segments(spans: Vec<Span<'static>>) -> Vec<TextSegment<Style>> {
    spans
        .into_iter()
        .map(|span| TextSegment {
            text: span.content.into_owned(),
            style: span.style,
        })
        .collect()
}

fn segments_to_spans(segments: Vec<TextSegment<Style>>) -> Vec<Span<'static>> {
    segments
        .into_iter()
        .map(|segment| Span::styled(segment.text, segment.style))
        .collect()
}

fn pad_spans(spans: &mut Vec<Span<'static>>, width: usize, style: Style) {
    let used = spans.iter().map(|span| span.content.width()).sum::<usize>();
    if used < width {
        spans.push(Span::styled(" ".repeat(width - used), style));
    }
}

fn expand_tabs(value: &str, tab_width: u16, column: &mut usize) -> String {
    let tab_width = usize::from(tab_width.max(1));
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        if character == '\t' {
            let spaces = tab_width - (*column % tab_width);
            output.push_str(&" ".repeat(spaces));
            *column += spaces;
        } else {
            output.push(character);
            *column += character.width().unwrap_or(0);
        }
    }
    output
}

fn truncate_start(value: &str, width: usize) -> String {
    if value.width() <= width {
        return value.to_owned();
    }
    if width <= 1 {
        return "…".to_owned();
    }
    let mut result = String::new();
    let mut used = 1;
    for character in value.chars().rev() {
        let character_width = character.width().unwrap_or(0);
        if used + character_width > width {
            break;
        }
        result.insert(0, character);
        used += character_width;
    }
    result.insert(0, '…');
    result
}

fn render_footer(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    if app.active_extension_notification().is_some() {
        render_extension_toast(area, buffer, app);
        return;
    }
    let focus = match app.focus {
        Focus::Review => "review",
        Focus::Sidebar => "files",
    };
    let mut spans = vec![
        Span::styled(format!(" {focus} "), Style::default().fg(Color::Cyan)),
        Span::styled(
            "j/k scroll  n/p hunk  [/ ] file  e source  r reload  s/u/a layout  ? help  q quit",
            Style::default().fg(Color::DarkGray),
        ),
    ];
    if let Some(status) = &app.status {
        spans.push(Span::styled(
            format!("  {status}"),
            Style::default().fg(Color::Yellow),
        ));
    }
    Paragraph::new(Line::from(spans)).render(area, buffer);
}

fn render_extension_toast(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    let Some(notification) = app.active_extension_notification() else {
        return;
    };
    let theme = ExtensionToastTheme {
        badge_removed: Color::Red,
        file_modified: Color::Yellow,
        badge_neutral: Color::Cyan,
    };
    let color = extension_toast_color(notification.notification_type, theme);
    Paragraph::new(Line::from(vec![
        Span::styled(
            format!(" {} ", extension_toast_prefix()),
            Style::default()
                .fg(Color::Black)
                .bg(color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                " {}",
                extension_toast_message(&notification.message, area.width)
            ),
            Style::default().fg(color),
        ),
    ]))
    .render(area, buffer);
}

fn render_help(area: Rect, buffer: &mut Buffer) {
    let geometry = resolve_modal_geometry(72, 20, area.width, area.height);
    let popup = Rect {
        x: area.x.saturating_add(geometry.left),
        y: area.y.saturating_add(geometry.top),
        width: geometry.width,
        height: geometry.height,
    };
    Clear.render(popup, buffer);
    Paragraph::new(vec![
        Line::from("Navigation"),
        Line::from("  j/k, arrows       scroll or choose file"),
        Line::from("  n/p               next/previous hunk"),
        Line::from("  ]/[               next/previous file"),
        Line::from("  PageUp/PageDown   scroll by page"),
        Line::from(""),
        Line::from("Appearance"),
        Line::from("  s/u/a             split/stack/auto"),
        Line::from("  f                 toggle files sidebar"),
        Line::from("  l                 toggle line numbers"),
        Line::from("  w                 toggle wrapping"),
        Line::from("  e                 expand/collapse source gap"),
        Line::from("  o                 toggle agent notes"),
        Line::from("  r                 reload input"),
        Line::from(""),
        Line::from("  Tab               switch focus"),
        Line::from("  q/Esc             quit"),
    ])
    .block(
        Block::default()
            .title(" Workdeck help ")
            .borders(Borders::ALL),
    )
    .render(popup, buffer);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use workdeck_core::{
        ChangesetSource, FileSourceSnapshots, LineRange, SourceOrigin, SourceSnapshot,
    };
    use workdeck_diff::parse_patch;
    use workdeck_review::{CommentAnchor, ReviewComment};

    fn changeset() -> Changeset {
        parse_patch(
            "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n",
            "test",
            "Working tree",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap()
    }

    fn long_changeset() -> Changeset {
        parse_patch(
            "diff --git a/long.rs b/long.rs\n--- a/long.rs\n+++ b/long.rs\n@@ -1 +1 @@\n-abcdefghijabcdefghij\n+界界界界界界界界界界\n",
            "long",
            "Long rows",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap()
    }

    #[test]
    fn renders_split_review_into_terminal_cells() {
        let backend = TestBackend::new(100, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                layout: LayoutMode::Split,
                ..ReviewOptions::default()
            },
        );
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Workdeck"));
        assert!(rendered.contains("a.rs"));
        assert!(rendered.contains("old"));
        assert!(rendered.contains("new"));
    }

    #[test]
    fn explicit_row_plan_wraps_unicode_and_keeps_nowrap_to_one_physical_row() {
        let changeset = long_changeset();
        let mut highlights = HighlightCache::default();
        let nowrap = build_review_rows(
            &changeset,
            &[],
            ReviewSelection::default(),
            LayoutMode::Stack,
            &ReviewOptions {
                layout: LayoutMode::Stack,
                sidebar: false,
                line_numbers: false,
                wrap_lines: false,
                ..ReviewOptions::default()
            },
            12,
            &mut highlights,
            &BTreeSet::new(),
        );
        let wrapped = build_review_rows(
            &changeset,
            &[],
            ReviewSelection::default(),
            LayoutMode::Stack,
            &ReviewOptions {
                layout: LayoutMode::Stack,
                sidebar: false,
                line_numbers: false,
                wrap_lines: true,
                ..ReviewOptions::default()
            },
            12,
            &mut highlights,
            &BTreeSet::new(),
        );

        assert_eq!(nowrap.lines.len(), 4);
        assert_eq!(wrapped.lines.len(), 6);
        assert_eq!(wrapped.hunk_tops.get(&(0, 0)), Some(&1));
        assert!(wrapped.lines[2..].iter().all(|line| line.width() <= 12));
    }

    #[test]
    fn split_row_plan_wraps_each_pane_and_preserves_the_divider_geometry() {
        let changeset = long_changeset();
        let mut highlights = HighlightCache::default();
        let rows = build_review_rows(
            &changeset,
            &[],
            ReviewSelection::default(),
            LayoutMode::Split,
            &ReviewOptions {
                layout: LayoutMode::Split,
                sidebar: false,
                line_numbers: false,
                wrap_lines: true,
                ..ReviewOptions::default()
            },
            21,
            &mut highlights,
            &BTreeSet::new(),
        );

        assert_eq!(rows.lines.len(), 5);
        for row in &rows.lines[2..] {
            assert_eq!(row.width(), 21);
            assert_eq!(
                row.spans
                    .iter()
                    .filter(|span| span.content.as_ref() == "│")
                    .count(),
                1
            );
        }
    }

    #[test]
    fn styled_wrapping_retains_span_styles_across_boundaries() {
        let red = Style::default().fg(Color::Red);
        let blue = Style::default().fg(Color::Blue);
        let rows = wrap_styled_spans(vec![Span::styled("ab界", red), Span::styled("cd", blue)], 4);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0].content, "ab界");
        assert_eq!(rows[0][0].style, red);
        assert_eq!(rows[1][0].content, "cd");
        assert_eq!(rows[1][0].style, blue);
    }

    #[test]
    fn source_gaps_are_collapsed_by_default_and_expand_from_snapshots() {
        let mut changeset = parse_patch(
            "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -3 +3 @@\n-old\n+new\n",
            "source-gaps",
            "Source gaps",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap();
        let file_key = changeset.files[0].key.clone();
        changeset.files[0].set_sources(FileSourceSnapshots {
            old: Some(SourceSnapshot::new(
                "one\ntwo\nold\nfour\n".into(),
                SourceOrigin::Revision {
                    revision: "HEAD".into(),
                },
                true,
            )),
            new: Some(SourceSnapshot::new(
                "one\ntwo\nnew\nfour\n".into(),
                SourceOrigin::WorkingTree,
                false,
            )),
        });
        let options = ReviewOptions {
            layout: LayoutMode::Stack,
            sidebar: false,
            line_numbers: false,
            ..ReviewOptions::default()
        };
        let mut highlights = HighlightCache::default();

        let collapsed = build_review_rows(
            &changeset,
            &[],
            ReviewSelection::default(),
            LayoutMode::Stack,
            &options,
            80,
            &mut highlights,
            &BTreeSet::new(),
        );
        let collapsed_text = collapsed
            .lines
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(collapsed_text.contains("2 unchanged lines"));
        assert!(!collapsed_text.contains("one"));

        let expanded = build_review_rows(
            &changeset,
            &[],
            ReviewSelection::default(),
            LayoutMode::Stack,
            &options,
            80,
            &mut highlights,
            &BTreeSet::from([(file_key, 0)]),
        );
        let expanded_text = expanded
            .lines
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(expanded_text.contains("one"));
        assert!(expanded_text.contains("two"));
        assert!(expanded_text.contains("1 unchanged lines"));
    }

    #[test]
    fn stack_and_split_rows_apply_inline_word_emphasis() {
        let changeset = parse_patch(
            "diff --git a/call.rs b/call.rs\n--- a/call.rs\n+++ b/call.rs\n@@ -1 +1 @@\n-return total(items);\n+return total(items, tax);\n",
            "words",
            "Word emphasis",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap();
        let mut highlights = HighlightCache::default();
        for layout in [LayoutMode::Stack, LayoutMode::Split] {
            let rows = build_review_rows(
                &changeset,
                &[],
                ReviewSelection::default(),
                layout,
                &ReviewOptions {
                    layout,
                    sidebar: false,
                    line_numbers: false,
                    ..ReviewOptions::default()
                },
                80,
                &mut highlights,
                &BTreeSet::new(),
            );
            assert!(rows.lines.iter().flat_map(|line| &line.spans).any(|span| {
                span.content.contains(", tax")
                    && span.style.add_modifier.contains(Modifier::BOLD)
                    && span.style.bg == Some(emphasis_background(DiffLineKind::Addition))
            }));
        }
    }

    #[test]
    fn renders_live_stml_comments_inline_and_expands_tabs() {
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                layout: LayoutMode::Stack,
                tab_width: 4,
                ..ReviewOptions::default()
            },
        );
        let state = app.shared_state();
        let file_key = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .changeset()
            .files[0]
            .key
            .clone();
        state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .add_comment(ReviewComment {
                id: "comment-1".into(),
                parent_id: None,
                source: "agent".into(),
                author: Some("Pi".into()),
                created_at: None,
                file_path: None,
                hunk_index: None,
                side: None,
                line: None,
                summary: "fallback".into(),
                rationale: Some("because it matters".into()),
                markup: Some("<box title=\"flow\">shape</box>".into()),
                tags: Vec::new(),
                confidence: None,
                anchor: CommentAnchor {
                    file_key,
                    old_range: None,
                    new_range: Some(LineRange { start: 1, end: 1 }),
                    preferred_side: Some(ReviewSide::New),
                    preferred_line: Some(1),
                    intersecting_hunk_indices: vec![0],
                    owner_hunk_index: Some(0),
                },
                editable: false,
            })
            .unwrap();
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("note Pi"));
        assert!(rendered.contains("flow"));
        assert!(rendered.contains("because it matters"));

        let mut column = 0;
        assert_eq!(expand_tabs("a\tb", 4, &mut column), "a   b");
        assert_eq!(column, 5);
    }

    #[test]
    fn renders_declarative_native_extension_panes() {
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                extension_panes: vec![ExtensionPaneView {
                    extension_id: "demo".into(),
                    pane: workdeck_extension_api::PaneRegistration {
                        id: "summary".into(),
                        title: "Extension summary".into(),
                        placement: PanePlacement::Right,
                        preferred_size: Some(28),
                        width: None,
                        height: None,
                    },
                    content: ViewNode::Column {
                        children: vec![ViewNode::Text {
                            text: "native pane content".into(),
                            style: ViewStyle {
                                foreground: Some("accent".into()),
                                bold: true,
                                ..ViewStyle::default()
                            },
                        }],
                        gap: 0,
                    },
                }],
                ..ReviewOptions::default()
            },
        );
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Extension summary"));
        assert!(rendered.contains("native pane content"));
    }

    #[test]
    fn review_shortcuts_change_layout_and_navigation() {
        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert_eq!(app.layout(), LayoutMode::Split);
        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        assert!(!app.options.sidebar);
        app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
        assert!(app.show_help);
    }

    #[test]
    fn review_mouse_wheel_starts_precise_and_accumulates_burst_acceleration() {
        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        let start = Instant::now();
        app.handle_mouse_at(MouseEventKind::ScrollDown, start);
        assert_eq!(app.scroll, 1);
        app.handle_mouse_at(
            MouseEventKind::ScrollDown,
            start + Duration::from_millis(50),
        );
        assert_eq!(app.scroll, 2);
        for offset in [56, 62, 68, 74] {
            app.handle_mouse_at(
                MouseEventKind::ScrollDown,
                start + Duration::from_millis(offset),
            );
        }
        assert!(app.scroll > 6);
    }

    #[test]
    fn theme_cursor_updates_are_idempotent() {
        let mut themes = ThemeController::new("dark".into());
        assert!(themes.set_cursor_palette(Some("dark".into())));
        assert!(!themes.set_cursor_palette(Some("dark".into())));
        assert_eq!(themes.cursor_updates, 1);
    }

    #[test]
    fn stale_rapid_theme_previews_cannot_replace_the_latest() {
        let mut themes = ThemeController::new("dark".into());
        let first = themes.request_preview("light");
        let latest = themes.request_preview("solarized");
        assert!(!themes.commit_preview(first));
        assert!(themes.commit_preview(latest));
        assert_eq!(themes.active, "solarized");
    }

    #[test]
    fn review_notification_surface_flushes_buffered_toasts_one_at_a_time() {
        let hub = ExtensionNotificationHub::new();
        hub.notify_info("first");
        hub.notify_info("second");
        let mut app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                extension_notifications: Some(hub),
                ..ReviewOptions::default()
            },
        );
        assert!(app.has_extension_notification_subscription());

        let start = Instant::now();
        app.tick_extension_notifications(start);
        assert_eq!(
            app.active_extension_notification()
                .map(|notification| notification.message),
            Some("first".into())
        );
        app.tick_extension_notifications(start + Duration::from_millis(4_001));
        assert_eq!(
            app.active_extension_notification()
                .map(|notification| notification.message),
            Some("second".into())
        );
        app.tick_extension_notifications(start + Duration::from_millis(8_002));
        assert!(app.active_extension_notification().is_none());
    }

    #[test]
    fn live_notification_does_not_lose_its_window_to_the_active_toast_timer() {
        let hub = ExtensionNotificationHub::new();
        hub.notify_info("first");
        let mut app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                extension_notifications: Some(hub.clone()),
                ..ReviewOptions::default()
            },
        );
        let start = Instant::now();
        app.tick_extension_notifications(start);
        hub.notify_info("second");
        app.tick_extension_notifications(start + Duration::from_millis(4_001));
        assert_eq!(
            app.active_extension_notification()
                .map(|notification| notification.message),
            Some("second".into())
        );
        app.tick_extension_notifications(start + Duration::from_millis(6_000));
        assert_eq!(
            app.active_extension_notification()
                .map(|notification| notification.message),
            Some("second".into())
        );
    }
}
