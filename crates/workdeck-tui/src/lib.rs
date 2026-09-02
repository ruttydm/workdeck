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
mod extension_panes;
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
mod public_review;
mod shutdown;
mod spatial;
mod startup_notices;
mod synthetic_key_event;
mod terminal_runtime;
mod text;
mod theme;
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
pub use extension_panes::*;
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
pub use public_review::*;
pub use shutdown::*;
pub use spatial::*;
pub use startup_notices::*;
pub use synthetic_key_event::*;
pub use terminal_runtime::*;
pub use text::*;
pub use theme::*;
pub use theme_detection::*;
pub use timed_notice::*;
pub use ui_geometry::*;
pub use watched_input::*;

use anyhow::Result;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
    MouseButton, MouseEvent, MouseEventKind,
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
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, IsTerminal};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use workdeck_core::{Changeset, DiffFile, DiffLine, DiffLineKind, ReviewSelection, ReviewSide};
use workdeck_diff::{
    DIFF_RAIL_PREFIX_WIDTH, HighlightCache, HighlightedDiffLine, SyntaxToken, TextSegment,
    clip_segments, expand_diff_tabs, plan_split_line_pairs, resolve_split_cell_geometry,
    resolve_split_pane_widths as resolve_diff_split_pane_widths, resolve_stack_cell_geometry,
    sanitize_terminal_line, slice_segments_window, word_diff_ranges, wrap_segments,
};
use workdeck_extension_api::{
    CommandRegistration, ExtensionHostAction, ExtensionKeyEvent, ExtensionNotification,
    ExtensionNotificationHub, ExtensionNotificationSubscription, ExtensionNotifyType,
    ExtensionPaintTheme, ExtensionPaneView, KeyRoutingResult, KeyboardModeRegistration,
    PanePlacement, PaneRegistration, PaneRenderRequest, Registration, ViewNode, ViewStyle,
    extension_pane_size,
};
use workdeck_extension_host::LoadedExtension;
use workdeck_review::{
    ExpandedSourceError, ExpandedSourceStatus, LayoutMode, ReviewComment, ReviewGapAddress,
    ReviewState, plan_expanded_gap, review_expansion_side, review_gap_source_for_file,
    review_leading_gap, review_trailing_gap,
};
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
    /// Horizontal code-column offset. Wrapped rows deliberately ignore it.
    pub horizontal_offset: usize,
    /// Optional explicit line-number width for embedded/public review surfaces.
    pub line_number_digits: Option<usize>,
    pub highlight: bool,
    pub file_gap: u16,
    pub hunk_gap: u16,
    pub transparent_background: bool,
    pub pager: bool,
    pub watch: bool,
    pub agent_notes: bool,
    pub theme: AppTheme,
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
            horizontal_offset: 0,
            line_number_digits: None,
            highlight: true,
            file_gap: 1,
            hunk_gap: 0,
            transparent_background: false,
            pager: false,
            watch: false,
            agent_notes: false,
            theme: resolve_theme(Some(DEFAULT_DARK_THEME_ID), None, &[]),
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

#[derive(Debug, Clone)]
struct LivePaneRegistration {
    key: String,
    extension_index: usize,
    extension_id: String,
    pane: PaneRegistration,
}

#[derive(Debug, Clone)]
struct LiveCommandRegistration {
    extension_index: usize,
    extension_id: String,
    command: CommandRegistration,
}

#[derive(Debug, Clone)]
struct LiveKeyboardModeRegistration {
    extension_index: usize,
    extension_id: String,
    mode: KeyboardModeRegistration,
}

#[derive(Debug, Clone)]
struct ActiveKeyboardMode {
    extension_index: usize,
    extension_id: String,
    mode: KeyboardModeRegistration,
}

#[derive(Debug, Clone)]
struct ExtensionInputDialog {
    extension_index: usize,
    extension_id: String,
    action_id: String,
    title: String,
    placeholder: String,
    value: String,
}

#[derive(Debug, Clone)]
struct CachedPaneRender {
    signature: PaneRenderSignature,
    view: ExtensionPaneView,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PaneRenderSignature {
    generation: u64,
    selection: ReviewSelection,
    placement: PanePlacement,
    width: u16,
    height: u16,
    theme: ExtensionPaintTheme,
}

#[derive(Debug, Clone, Copy)]
struct PaneResizeState {
    placement: PanePlacement,
    origin: u16,
    start_size: u16,
    min_size: u16,
    max_size: u16,
}

#[derive(Debug, Default)]
struct ExtensionPaneRuntime {
    extensions: Vec<LoadedExtension>,
    panes: Vec<LivePaneRegistration>,
    commands: Vec<LiveCommandRegistration>,
    keyboard_modes: Vec<LiveKeyboardModeRegistration>,
    active_keyboard_mode: Option<ActiveKeyboardMode>,
    input_dialog: Option<ExtensionInputDialog>,
    open: BTreeSet<String>,
    size_overrides: BTreeMap<String, u16>,
    cached_renders: BTreeMap<String, CachedPaneRender>,
    layout: ExtensionPaneLayoutPlan,
    resize: Option<(String, PaneResizeState)>,
    menu_open: bool,
    menu_selected: usize,
    menu_trigger: Option<Rect>,
    menu_bounds: Option<Rect>,
    mode_badge_bounds: Option<Rect>,
}

impl ExtensionPaneRuntime {
    fn new(extensions: Vec<LoadedExtension>) -> Self {
        let mut panes = Vec::new();
        let mut commands = Vec::new();
        let mut keyboard_modes = Vec::new();
        let mut open = BTreeSet::new();
        for (extension_index, extension) in extensions.iter().enumerate() {
            for registration in &extension.handshake.registrations {
                match registration {
                    Registration::Pane(pane) => {
                        let key = format!("{}:{}", extension.manifest.id, pane.id);
                        if pane.default_open {
                            open.insert(key.clone());
                        }
                        panes.push(LivePaneRegistration {
                            key,
                            extension_index,
                            extension_id: extension.manifest.id.clone(),
                            pane: pane.clone(),
                        });
                    }
                    Registration::Command(command) => commands.push(LiveCommandRegistration {
                        extension_index,
                        extension_id: extension.manifest.id.clone(),
                        command: command.clone(),
                    }),
                    Registration::KeyboardMode(mode) => {
                        let mut mode = mode.clone();
                        mode.title = sanitize_terminal_line(&mode.title).trim().to_owned();
                        if mode.title.is_empty() {
                            mode.title = format!("{}:{}", extension.manifest.id, mode.id);
                        }
                        keyboard_modes.push(LiveKeyboardModeRegistration {
                            extension_index,
                            extension_id: extension.manifest.id.clone(),
                            mode,
                        });
                    }
                    _ => {}
                }
            }
        }
        Self {
            extensions,
            panes,
            commands,
            keyboard_modes,
            open,
            ..Self::default()
        }
    }
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
    review_height: Cell<u16>,
    current_line_row: usize,
    expanded_gaps: BTreeSet<(String, usize)>,
    highlights: Mutex<HighlightCache>,
    themes: ThemeController,
    extension_toasts: Arc<Mutex<ExtensionNotificationSurface>>,
    extension_notification_subscription: Option<ExtensionNotificationSubscription>,
    mouse_scroll_acceleration: ReviewMouseWheelScrollAcceleration,
    mouse_scroll_accumulator: f64,
    extension_pane_runtime: Mutex<ExtensionPaneRuntime>,
}

impl ReviewApp {
    pub fn new(changeset: Changeset, options: ReviewOptions) -> Self {
        Self::new_with_extensions(changeset, options, Vec::new())
    }

    pub fn new_with_extensions(
        changeset: Changeset,
        options: ReviewOptions,
        extensions: Vec<LoadedExtension>,
    ) -> Self {
        let mut state = ReviewState::new(changeset);
        state.set_layout(options.layout);
        let themes = ThemeController::new(options.theme.id.clone());
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
            review_height: Cell::new(20),
            current_line_row: 0,
            expanded_gaps: BTreeSet::new(),
            highlights: Mutex::new(HighlightCache::default()),
            themes,
            extension_toasts,
            extension_notification_subscription,
            mouse_scroll_acceleration: ReviewMouseWheelScrollAcceleration::default(),
            mouse_scroll_accumulator: 0.0,
            extension_pane_runtime: Mutex::new(ExtensionPaneRuntime::new(extensions)),
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
        self.exit_active_keyboard_mode();
        let changeset = match self.apply_extension_transforms(changeset) {
            Ok(changeset) => changeset,
            Err(error) => {
                self.status = Some(format!("extension reload failed: {error}"));
                return;
            }
        };
        if self.with_state(|state| state.changeset() != &changeset) {
            self.with_state(|state| state.reload(changeset));
            self.status = Some("review reloaded".into());
            self.highlights
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clear();
            self.extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .cached_renders
                .clear();
        }
    }

    fn apply_extension_transforms(&self, mut changeset: Changeset) -> Result<Changeset> {
        let mut runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for extension in &mut runtime.extensions {
            changeset = extension.apply_changeset_transforms(changeset)?;
        }
        changeset.refresh_review_identities();
        Ok(changeset)
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

    #[must_use]
    pub fn active_keyboard_mode_title(&self) -> Option<String> {
        self.extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_keyboard_mode
            .as_ref()
            .map(|active| active.mode.title.clone())
    }

    #[must_use]
    pub fn active_keyboard_mode_status_hint(&self) -> Option<String> {
        self.extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_keyboard_mode
            .as_ref()
            .map(|active| {
                format!(
                    "{} — ext {}:{} — Esc exits",
                    active.mode.title, active.extension_id, active.mode.id
                )
            })
    }

    #[must_use]
    pub const fn current_line_row(&self) -> usize {
        self.current_line_row
    }

    #[must_use]
    pub const fn review_scroll(&self) -> usize {
        self.scroll
    }

    #[must_use]
    pub fn has_extension_input_dialog(&self) -> bool {
        self.extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .input_dialog
            .is_some()
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }
        if self.handle_extension_input_key(&key) {
            return;
        }
        if self.handle_extension_menu_key(&key) {
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
        if self.route_active_keyboard_mode(&key) {
            return;
        }
        if self.invoke_extension_command(&key) {
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
                let resolved = resolve_theme(Some(&theme), None, &[]);
                self.options.theme = if self.options.transparent_background {
                    with_transparent_surfaces(&resolved)
                } else {
                    resolved
                };
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

    fn invoke_extension_command(&mut self, key: &KeyEvent) -> bool {
        let key = to_live_extension_key_event(key);
        let command = {
            let runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            runtime
                .commands
                .iter()
                .find(|registration| {
                    matches_any_key_chord(&registration.command.default_keys).matches(&key)
                })
                .cloned()
        };
        let Some(command) = command else {
            return false;
        };
        self.invoke_registered_extension_command(command);
        true
    }

    fn invoke_registered_extension_command(&mut self, command: LiveCommandRegistration) {
        let snapshot = self.with_state(|state| state.snapshot());
        let execution = {
            let mut runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let open_panes = runtime.open.iter().cloned().collect();
            let active_keyboard_mode = runtime
                .active_keyboard_mode
                .as_ref()
                .map(|active| format!("{}:{}", active.extension_id, active.mode.id));
            runtime.extensions[command.extension_index].invoke_command_with_context(
                &command.command.id,
                snapshot,
                open_panes,
                active_keyboard_mode,
            )
        };
        match execution {
            Ok(execution) => {
                self.status = Some(command.command.title);
                self.apply_extension_actions(
                    command.extension_index,
                    &command.extension_id,
                    execution.actions,
                );
            }
            Err(error) => {
                self.status = Some(format!(
                    "extension {} command failed: {error}",
                    command.extension_id
                ))
            }
        }
    }

    fn route_active_keyboard_mode(&mut self, key: &KeyEvent) -> bool {
        let active = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_keyboard_mode
            .clone();
        let Some(active) = active else {
            return false;
        };
        if key.code == KeyCode::Esc {
            self.exit_active_keyboard_mode();
            return true;
        }
        let snapshot = self.with_state(|state| state.snapshot());
        let result = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extensions[active.extension_index]
            .route_keyboard_mode_key(&active.mode.id, to_live_extension_key_event(key), snapshot);
        match result {
            Ok(execution) => {
                let result = execution.result;
                self.apply_extension_actions(
                    active.extension_index,
                    &active.extension_id,
                    execution.actions,
                );
                if result == KeyRoutingResult::Exit {
                    let activation_unchanged = self
                        .extension_pane_runtime
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .active_keyboard_mode
                        .as_ref()
                        .is_some_and(|current| {
                            current.extension_index == active.extension_index
                                && current.mode.id == active.mode.id
                        });
                    if activation_unchanged {
                        self.exit_active_keyboard_mode();
                    }
                }
                result != KeyRoutingResult::Pass
            }
            Err(error) => {
                self.exit_active_keyboard_mode();
                self.status = Some(format!(
                    "extension {} keyboard mode failed: {error}",
                    active.extension_id
                ));
                true
            }
        }
    }

    fn apply_extension_actions(
        &mut self,
        extension_index: usize,
        extension_id: &str,
        actions: Vec<ExtensionHostAction>,
    ) {
        for action in actions {
            match action {
                ExtensionHostAction::OpenPane { id } => {
                    let pane_key = if id.contains(':') {
                        id
                    } else {
                        format!("{extension_id}:{id}")
                    };
                    let mut runtime = self
                        .extension_pane_runtime
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    runtime.open.insert(pane_key.clone());
                    runtime.cached_renders.remove(&pane_key);
                }
                ExtensionHostAction::ClosePane { id } => {
                    let pane_key = if id.contains(':') {
                        id
                    } else {
                        format!("{extension_id}:{id}")
                    };
                    let mut runtime = self
                        .extension_pane_runtime
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    runtime.open.remove(&pane_key);
                    runtime.cached_renders.remove(&pane_key);
                }
                ExtensionHostAction::EnterKeyboardMode { id } => {
                    self.enter_keyboard_mode(extension_index, extension_id, &id);
                }
                ExtensionHostAction::ExitKeyboardMode => {
                    self.exit_keyboard_mode_for_extension(extension_index);
                }
                ExtensionHostAction::ExecuteReviewCommand { id, count } => {
                    self.execute_extension_review_command(&id, count.unwrap_or(1));
                }
                ExtensionHostAction::OpenInputDialog {
                    id,
                    title,
                    placeholder,
                } => {
                    let mut runtime = self
                        .extension_pane_runtime
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    runtime.menu_open = false;
                    runtime.input_dialog = Some(ExtensionInputDialog {
                        extension_index,
                        extension_id: extension_id.into(),
                        action_id: id,
                        title: sanitize_terminal_line(&title),
                        placeholder: sanitize_terminal_line(&placeholder),
                        value: String::new(),
                    });
                }
                ExtensionHostAction::Notify {
                    message,
                    notification_type,
                } => {
                    let prefix = match notification_type {
                        ExtensionNotifyType::Info => "",
                        ExtensionNotifyType::Warning => "warning: ",
                        ExtensionNotifyType::Error => "error: ",
                    };
                    self.status = Some(format!(
                        "{extension_id}: {prefix}{}",
                        sanitize_terminal_line(&message)
                    ));
                }
            }
        }
    }

    fn enter_keyboard_mode(&mut self, extension_index: usize, extension_id: &str, id: &str) {
        let local_id = id.strip_prefix(&format!("{extension_id}:")).unwrap_or(id);
        let registration = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .keyboard_modes
            .iter()
            .find(|mode| {
                mode.extension_index == extension_index
                    && mode.extension_id == extension_id
                    && mode.mode.id == local_id
            })
            .cloned();
        let Some(registration) = registration else {
            self.status = Some(format!(
                "extension {extension_id} requested an unknown mode"
            ));
            return;
        };
        if self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_keyboard_mode
            .as_ref()
            .is_some_and(|active| {
                active.extension_index == extension_index && active.mode.id == local_id
            })
        {
            return;
        }
        self.exit_active_keyboard_mode();
        let active = ActiveKeyboardMode {
            extension_index,
            extension_id: extension_id.into(),
            mode: registration.mode,
        };
        self.extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_keyboard_mode = Some(active.clone());
        let snapshot = self.with_state(|state| state.snapshot());
        let execution = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extensions[extension_index]
            .enter_keyboard_mode(&active.mode.id, snapshot);
        match execution {
            Ok(execution) => {
                self.status = Some(format!("{} active · Esc exits", active.mode.title));
                self.apply_extension_actions(extension_index, extension_id, execution.actions);
            }
            Err(error) => {
                self.extension_pane_runtime
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .active_keyboard_mode = None;
                self.status = Some(format!(
                    "extension {extension_id} could not enter mode: {error}"
                ));
            }
        }
    }

    fn exit_active_keyboard_mode(&mut self) {
        let active = {
            let mut runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let active = runtime.active_keyboard_mode.take();
            if let Some(active) = &active
                && runtime
                    .input_dialog
                    .as_ref()
                    .is_some_and(|dialog| dialog.extension_index == active.extension_index)
            {
                runtime.input_dialog = None;
            }
            active
        };
        let Some(active) = active else {
            return;
        };
        let snapshot = self.with_state(|state| state.snapshot());
        let execution = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extensions[active.extension_index]
            .exit_keyboard_mode(&active.mode.id, snapshot);
        match execution {
            Ok(execution) => {
                self.status = Some(format!("{} exited", active.mode.title));
                self.apply_extension_actions(
                    active.extension_index,
                    &active.extension_id,
                    execution.actions,
                );
            }
            Err(error) => {
                self.status = Some(format!(
                    "extension {} mode exit failed: {error}",
                    active.extension_id
                ));
            }
        }
    }

    fn exit_keyboard_mode_for_extension(&mut self, extension_index: usize) {
        let owns_active_mode = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_keyboard_mode
            .as_ref()
            .is_some_and(|active| active.extension_index == extension_index);
        if owns_active_mode {
            self.exit_active_keyboard_mode();
        }
    }

    fn handle_extension_input_key(&mut self, key: &KeyEvent) -> bool {
        let has_dialog = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .input_dialog
            .is_some();
        if !has_dialog {
            return false;
        }
        match key.code {
            KeyCode::Enter | KeyCode::Esc => {
                let dialog = self
                    .extension_pane_runtime
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .input_dialog
                    .take();
                if let Some(dialog) = dialog {
                    let value = (key.code == KeyCode::Enter).then_some(dialog.value.clone());
                    self.submit_extension_input(dialog, value);
                }
            }
            KeyCode::Backspace => {
                let mut runtime = self
                    .extension_pane_runtime
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if let Some(dialog) = &mut runtime.input_dialog
                    && let Some(grapheme) = dialog.value.graphemes(true).next_back()
                {
                    dialog.value.truncate(dialog.value.len() - grapheme.len());
                }
            }
            KeyCode::Char(character)
                if !key.modifiers.intersects(
                    KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                ) =>
            {
                if let Some(dialog) = &mut self
                    .extension_pane_runtime
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .input_dialog
                {
                    dialog.value.push(character);
                }
            }
            _ => {}
        }
        true
    }

    fn submit_extension_input(&mut self, dialog: ExtensionInputDialog, value: Option<String>) {
        let snapshot = self.with_state(|state| state.snapshot());
        let execution = {
            let mut runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let active_keyboard_mode = runtime
                .active_keyboard_mode
                .as_ref()
                .map(|active| format!("{}:{}", active.extension_id, active.mode.id));
            runtime.extensions[dialog.extension_index].submit_input_dialog(
                &dialog.action_id,
                value,
                snapshot,
                active_keyboard_mode,
            )
        };
        match execution {
            Ok(execution) => self.apply_extension_actions(
                dialog.extension_index,
                &dialog.extension_id,
                execution.actions,
            ),
            Err(error) => {
                self.status = Some(format!(
                    "extension {} input failed: {error}",
                    dialog.extension_id
                ));
            }
        }
    }

    fn execute_extension_review_command(&mut self, id: &str, count: u16) {
        let rows = self.current_review_rows();
        let last = rows.lines.len().saturating_sub(1);
        let viewport = usize::from(self.review_height.get().saturating_sub(1).max(1));
        let magnitude = usize::from(count);
        match id {
            "workdeck.view.cursor-line-row" => {
                self.options.cursor_line = CursorLineMode::Row;
                self.scroll_to_selection();
                self.current_line_row = self.scroll.min(last);
            }
            "workdeck.review.step-down" => {
                self.current_line_row = self.current_line_row.saturating_add(magnitude).min(last);
                self.keep_current_line_visible(viewport, last);
            }
            "workdeck.review.step-up" => {
                self.current_line_row = self.current_line_row.saturating_sub(magnitude);
                self.keep_current_line_visible(viewport, last);
            }
            "workdeck.review.previous-hunk" => {
                for _ in 0..magnitude {
                    self.navigate(ReviewState::previous_hunk);
                }
                self.current_line_row = self.scroll.min(last);
            }
            "workdeck.review.next-hunk" => {
                for _ in 0..magnitude {
                    self.navigate(ReviewState::next_hunk);
                }
                self.current_line_row = self.scroll.min(last);
            }
            "workdeck.review.half-page-down" => {
                self.current_line_row = self
                    .current_line_row
                    .saturating_add((viewport / 2).max(1).saturating_mul(magnitude))
                    .min(last);
                self.keep_current_line_visible(viewport, last);
            }
            "workdeck.review.half-page-up" => {
                self.current_line_row = self
                    .current_line_row
                    .saturating_sub((viewport / 2).max(1).saturating_mul(magnitude));
                self.keep_current_line_visible(viewport, last);
            }
            "workdeck.review.jump-to-top" => {
                self.current_line_row = 0;
                self.scroll = 0;
            }
            "workdeck.review.jump-to-bottom" => {
                self.current_line_row = last;
                self.scroll = last.saturating_sub(viewport.saturating_sub(1));
            }
            "workdeck.review.align-current-line-top" => self.scroll = self.current_line_row,
            "workdeck.review.align-current-line-center" => {
                self.scroll = self.current_line_row.saturating_sub(viewport / 2);
            }
            "workdeck.review.align-current-line-bottom" => {
                self.scroll = self
                    .current_line_row
                    .saturating_sub(viewport.saturating_sub(1));
            }
            _ => {}
        }
    }

    fn keep_current_line_visible(&mut self, viewport: usize, last: usize) {
        let max_scroll = last.saturating_add(1).saturating_sub(viewport);
        let scroll = if self.scroll == usize::MAX {
            max_scroll
        } else {
            self.scroll.min(max_scroll)
        };
        self.scroll = if self.current_line_row < scroll {
            self.current_line_row
        } else if self.current_line_row >= scroll.saturating_add(viewport) {
            self.current_line_row
                .saturating_sub(viewport.saturating_sub(1))
        } else {
            scroll
        };
    }

    fn current_review_rows(&self) -> ReviewRows {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let width = self.review_width.get();
        let layout = state.resolved_layout(width);
        let mut highlights = self
            .highlights
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        build_live_review_rows(
            state.changeset(),
            state.comments(),
            state.selection(),
            layout,
            &self.options,
            width,
            &mut highlights,
            &self.expanded_gaps,
        )
    }

    fn handle_extension_menu_key(&mut self, key: &KeyEvent) -> bool {
        let mut runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let menu_shortcut = key.code == KeyCode::Menu
            || (key.code == KeyCode::Char('e') && key.modifiers == KeyModifiers::ALT);
        let entry_count =
            runtime.commands.len() + usize::from(runtime.active_keyboard_mode.is_some());
        if menu_shortcut && entry_count > 0 {
            runtime.menu_open = !runtime.menu_open;
            runtime.menu_selected = 0;
            return true;
        }
        if !runtime.menu_open {
            return false;
        }
        match key.code {
            KeyCode::Esc => {
                runtime.menu_open = false;
                true
            }
            KeyCode::Down | KeyCode::Char('j') => {
                runtime.menu_selected = (runtime.menu_selected + 1) % entry_count.max(1);
                true
            }
            KeyCode::Up | KeyCode::Char('k') => {
                runtime.menu_selected = runtime
                    .menu_selected
                    .checked_sub(1)
                    .unwrap_or_else(|| entry_count.saturating_sub(1));
                true
            }
            KeyCode::Enter => {
                let has_exit = runtime.active_keyboard_mode.is_some();
                let exit_mode = has_exit && runtime.menu_selected == 0;
                let command = (!exit_mode)
                    .then(|| {
                        runtime
                            .commands
                            .get(runtime.menu_selected.saturating_sub(usize::from(has_exit)))
                    })
                    .flatten()
                    .cloned();
                runtime.menu_open = false;
                drop(runtime);
                if exit_mode {
                    self.exit_active_keyboard_mode();
                } else if let Some(command) = command {
                    self.invoke_registered_extension_command(command);
                }
                true
            }
            _ => true,
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
        let rows = build_live_review_rows(
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
            let source = review_gap_source_for_file(file);
            let hunk_index = selection.hunk_index.unwrap_or(0);
            let gap_slot = review_leading_gap(&source, hunk_index)
                .map(|_| hunk_index)
                .or_else(|| {
                    review_trailing_gap(&source)
                        .filter(|gap| gap.hunk_index == hunk_index)
                        .map(|_| file.hunks.len())
                })?;
            Some((file.key.clone(), gap_slot))
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

    pub fn handle_mouse_event(&mut self, event: MouseEvent) {
        if self.has_extension_input_dialog() {
            return;
        }
        if self.handle_extension_mode_badge_mouse(&event)
            || self.handle_extension_menu_mouse(&event)
            || self.handle_extension_pane_mouse(&event)
        {
            return;
        }
        self.handle_mouse_at(event.kind, Instant::now());
    }

    fn handle_extension_mode_badge_mouse(&mut self, event: &MouseEvent) -> bool {
        if event.kind != MouseEventKind::Up(MouseButton::Left) {
            return false;
        }
        let hit = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .mode_badge_bounds
            .is_some_and(|area| rect_contains(area, event.column, event.row));
        if hit {
            self.exit_active_keyboard_mode();
        }
        hit
    }

    fn handle_extension_menu_mouse(&mut self, event: &MouseEvent) -> bool {
        if event.kind != MouseEventKind::Down(MouseButton::Left) {
            return false;
        }
        let mut runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if runtime
            .menu_trigger
            .is_some_and(|area| rect_contains(area, event.column, event.row))
        {
            runtime.menu_open = !runtime.menu_open;
            runtime.menu_selected = 0;
            return true;
        }
        if !runtime.menu_open {
            return false;
        }
        let selection = runtime.menu_bounds.and_then(|area| {
            if !rect_contains(area, event.column, event.row)
                || event.row == area.y
                || event.row + 1 == area.bottom()
            {
                return None;
            }
            Some(usize::from(event.row.saturating_sub(area.y + 1)))
        });
        let has_exit = runtime.active_keyboard_mode.is_some();
        let exit_mode = selection.is_some_and(|selection| has_exit && selection == 0);
        let command = selection
            .filter(|_| !exit_mode)
            .and_then(|selection| {
                runtime
                    .commands
                    .get(selection.saturating_sub(usize::from(has_exit)))
            })
            .cloned();
        runtime.menu_open = false;
        drop(runtime);
        if exit_mode {
            self.exit_active_keyboard_mode();
        } else if let Some(command) = command {
            self.invoke_registered_extension_command(command);
        }
        true
    }

    fn handle_extension_pane_mouse(&mut self, event: &MouseEvent) -> bool {
        let mut runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(planned) = runtime.layout.panes.iter().find(|planned| {
                    planned.divider.is_some_and(|divider| {
                        event.column >= divider.x
                            && event.column < divider.right()
                            && event.row >= divider.y
                            && event.row < divider.bottom()
                    })
                }) else {
                    return false;
                };
                let vertical = matches!(
                    planned.pane.placement,
                    PanePlacement::Left | PanePlacement::Right
                );
                let requested = extension_pane_size(&planned.pane, None);
                let current_size = if vertical {
                    planned.bounds.width
                } else {
                    planned.bounds.height
                };
                let review_size = if vertical {
                    runtime.layout.review_bounds.width
                } else {
                    runtime.layout.review_bounds.height
                };
                let review_minimum = if vertical {
                    20
                } else {
                    MIN_EXTENSION_REVIEW_HEIGHT
                };
                let max_size = current_size
                    .saturating_add(review_size.saturating_sub(review_minimum))
                    .min(requested.max.unwrap_or(u16::MAX));
                let resize = PaneResizeState {
                    placement: planned.pane.placement,
                    origin: if vertical { event.column } else { event.row },
                    start_size: current_size,
                    min_size: requested.min.unwrap_or(1),
                    max_size,
                };
                let key = planned.key.clone();
                runtime.resize = Some((key, resize));
                true
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let Some((key, resize)) = runtime.resize.clone() else {
                    return false;
                };
                let position =
                    if matches!(resize.placement, PanePlacement::Left | PanePlacement::Right) {
                        event.column
                    } else {
                        event.row
                    };
                let delta = if matches!(
                    resize.placement,
                    PanePlacement::Right | PanePlacement::Bottom
                ) {
                    i32::from(resize.origin) - i32::from(position)
                } else {
                    i32::from(position) - i32::from(resize.origin)
                };
                let next = (i32::from(resize.start_size) + delta)
                    .clamp(i32::from(resize.min_size), i32::from(resize.max_size))
                    as u16;
                runtime.size_overrides.insert(key.clone(), next);
                runtime.cached_renders.remove(&key);
                true
            }
            MouseEventKind::Up(MouseButton::Left) if runtime.resize.is_some() => {
                runtime.resize = None;
                true
            }
            _ => false,
        }
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

fn rect_contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x && column < area.right() && row >= area.y && row < area.bottom()
}

fn to_live_extension_key_event(key: &KeyEvent) -> ExtensionKeyEvent {
    let (name, sequence) = match key.code {
        KeyCode::Char(character) => (
            character.to_ascii_lowercase().to_string(),
            character.to_string(),
        ),
        KeyCode::Enter => ("return".into(), "\r".into()),
        KeyCode::Tab => ("tab".into(), "\t".into()),
        KeyCode::BackTab => ("tab".into(), "\t".into()),
        KeyCode::Backspace => ("backspace".into(), String::new()),
        KeyCode::Esc => ("escape".into(), String::new()),
        KeyCode::Left => ("left".into(), String::new()),
        KeyCode::Right => ("right".into(), String::new()),
        KeyCode::Up => ("up".into(), String::new()),
        KeyCode::Down => ("down".into(), String::new()),
        KeyCode::Home => ("home".into(), String::new()),
        KeyCode::End => ("end".into(), String::new()),
        KeyCode::PageUp => ("pageup".into(), String::new()),
        KeyCode::PageDown => ("pagedown".into(), String::new()),
        KeyCode::Delete => ("delete".into(), String::new()),
        KeyCode::Insert => ("insert".into(), String::new()),
        KeyCode::F(number) => (format!("f{number}"), String::new()),
        KeyCode::Null
        | KeyCode::CapsLock
        | KeyCode::ScrollLock
        | KeyCode::NumLock
        | KeyCode::PrintScreen
        | KeyCode::Pause
        | KeyCode::Menu
        | KeyCode::KeypadBegin
        | KeyCode::Media(_)
        | KeyCode::Modifier(_) => (String::new(), String::new()),
    };
    ExtensionKeyEvent {
        name,
        sequence,
        ctrl: key.modifiers.contains(KeyModifiers::CONTROL),
        meta: key.modifiers.contains(KeyModifiers::SUPER),
        option: key.modifiers.contains(KeyModifiers::ALT),
        shift: key.modifiers.contains(KeyModifiers::SHIFT),
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
        let next = THEMES
            .iter()
            .position(|theme| theme.id == self.requested)
            .map_or(0, |index| (index + 1) % THEMES.len());
        let next = THEMES[next].id.clone();
        let generation = self.request_preview(&next);
        self.commit_preview(generation);
        self.set_cursor_palette(Some(next.clone()));
        next
    }
}

pub fn run_review(changeset: Changeset, options: ReviewOptions) -> Result<()> {
    run_review_inner(changeset, options, Vec::new(), None, None)
}

pub fn run_review_with_extensions(
    changeset: Changeset,
    options: ReviewOptions,
    extensions: Vec<LoadedExtension>,
) -> Result<()> {
    run_review_inner(changeset, options, extensions, None, None)
}

pub fn run_review_with_reload<F>(
    changeset: Changeset,
    options: ReviewOptions,
    reload: &mut F,
) -> Result<()>
where
    F: FnMut() -> Result<Changeset>,
{
    run_review_inner(changeset, options, Vec::new(), None, Some(reload))
}

pub fn run_review_with_extensions_reload<F>(
    changeset: Changeset,
    options: ReviewOptions,
    extensions: Vec<LoadedExtension>,
    reload: &mut F,
) -> Result<()>
where
    F: FnMut() -> Result<Changeset>,
{
    run_review_inner(changeset, options, extensions, None, Some(reload))
}

/// Run a reloadable review while retaining the provider-neutral input needed
/// for native watch planning and signatures.
pub fn run_review_with_input_reload<F>(
    changeset: Changeset,
    options: ReviewOptions,
    input: workdeck_core::CliInput,
    input_cwd: PathBuf,
    reload: &mut F,
) -> Result<()>
where
    F: FnMut() -> Result<Changeset>,
{
    run_review_inner(
        changeset,
        options,
        Vec::new(),
        Some((input, input_cwd)),
        Some(reload),
    )
}

pub fn run_review_with_extensions_input_reload<F>(
    changeset: Changeset,
    options: ReviewOptions,
    extensions: Vec<LoadedExtension>,
    input: workdeck_core::CliInput,
    input_cwd: PathBuf,
    reload: &mut F,
) -> Result<()>
where
    F: FnMut() -> Result<Changeset>,
{
    run_review_inner(
        changeset,
        options,
        extensions,
        Some((input, input_cwd)),
        Some(reload),
    )
}

fn run_review_inner(
    changeset: Changeset,
    options: ReviewOptions,
    extensions: Vec<LoadedExtension>,
    watch_input: Option<(workdeck_core::CliInput, PathBuf)>,
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
    let mut app = ReviewApp::new_with_extensions(changeset, options, extensions);
    let mut watched_input = watch_input
        .filter(|_| app.options.watch)
        .and_then(|(input, cwd)| {
            let runtime: Arc<dyn WatchedInputRuntime> = Arc::new(NativeWatchedInputRuntime::new(
                cwd,
                Some(workdeck_vcs::bundled_vcs_catalog().clone()),
            ));
            match WatchedInputDriver::start(
                true,
                input,
                runtime,
                None,
                Instant::now(),
                workdeck_vcs::WatchControllerConfig::default(),
            ) {
                Ok(driver) => driver,
                Err(error) => {
                    app.status = Some(format!("failed to initialize watch mode: {error}"));
                    None
                }
            }
        });
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
        &mut watched_input,
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
    watched_input: &mut Option<WatchedInputDriver>,
) -> Result<()> {
    let mut next_reload = Instant::now() + Duration::from_millis(250);
    while !app.should_quit && !session_stop.is_some_and(|stop| stop.load(Ordering::Relaxed)) {
        app.tick_extension_notifications(Instant::now());
        terminal.draw(|frame| render(frame.area(), frame.buffer_mut(), app))?;
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => app.handle_key(key),
                Event::Mouse(mouse) => app.handle_mouse_event(mouse),
                Event::Resize(_, _) | Event::FocusGained | Event::FocusLost | Event::Paste(_) => {}
            }
        }
        let session_requested =
            session_reload.is_some_and(|reload| reload.swap(false, Ordering::Relaxed));
        let timer_requested =
            app.options.watch && watched_input.is_none() && Instant::now() >= next_reload;
        let manual_requested = app.reload_requested;
        let reload_requested = manual_requested || session_requested || timer_requested;
        if reload_requested {
            app.reload_requested = false;
            next_reload = Instant::now() + Duration::from_millis(250);
            reload_current_review(app, reloader, manual_requested || session_requested);
        }
        if let Some(driver) = watched_input {
            let outcome = driver.poll(Instant::now(), &mut || {
                let Some(reload) = reloader.as_deref_mut() else {
                    anyhow::bail!("this review input cannot be reloaded");
                };
                let changeset = reload()?;
                apply_reloaded_changeset(app, changeset);
                Ok::<(), anyhow::Error>(())
            });
            if outcome.reload_pending {
                app.status = Some("review reload pending".into());
            }
            if let Some(error) = outcome.errors.last() {
                app.status = Some(format!("auto-reload failed: {error}"));
            }
        }
    }
    Ok(())
}

fn reload_current_review(
    app: &mut ReviewApp,
    reloader: &mut Option<&mut dyn FnMut() -> Result<Changeset>>,
    report_unavailable: bool,
) {
    match reloader.as_deref_mut() {
        Some(reload) => match reload() {
            Ok(changeset) => apply_reloaded_changeset(app, changeset),
            Err(error) => app.status = Some(format!("reload failed: {error:#}")),
        },
        None if report_unavailable => {
            app.status = Some("this review input cannot be reloaded".into());
        }
        None => {}
    }
}

fn apply_reloaded_changeset(app: &mut ReviewApp, changeset: Changeset) {
    app.reload(changeset);
}

pub fn render(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    let background = if app.options.transparent_background {
        Color::Reset
    } else {
        ratatui_theme_color(&app.options.theme.background)
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
    render_extension_command_menu(area, buffer, app);
    if app.show_help {
        render_help(area, buffer);
    }
    render_extension_input_dialog(area, buffer, app);
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
    let theme = &app.options.theme;
    let title = Line::from(vec![
        Span::styled(
            " Workdeck ",
            Style::default()
                .fg(ratatui_theme_color(&theme.background))
                .bg(ratatui_theme_color(&theme.accent))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {} ", state.changeset().title),
            Style::default()
                .fg(ratatui_theme_color(&theme.text))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{} files", state.changeset().files.len()),
            Style::default().fg(ratatui_theme_color(&theme.muted)),
        ),
        Span::styled(
            format!("  +{}", stats.additions),
            Style::default().fg(ratatui_theme_color(&theme.badge_added)),
        ),
        Span::styled(
            format!(" -{}", stats.deletions),
            Style::default().fg(ratatui_theme_color(&theme.badge_removed)),
        ),
        Span::styled(
            format!("  {:?}", layout).to_lowercase(),
            Style::default().fg(ratatui_theme_color(&theme.muted)),
        ),
    ]);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(area);
    Paragraph::new(title).render(rows[0], buffer);
    render_extension_menu_button(rows[1], buffer, app);
}

/// Render the Extensions menu trigger and publish its hit rectangle to input routing.
pub fn render_extension_menu_button(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    let mut runtime = app
        .extension_pane_runtime
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if (runtime.commands.is_empty() && runtime.active_keyboard_mode.is_none())
        || area.width == 0
        || area.height == 0
    {
        runtime.menu_trigger = None;
        runtime.menu_open = false;
        return;
    }
    let width = 12.min(area.width);
    let trigger = Rect::new(area.right().saturating_sub(width), area.y, width, 1);
    runtime.menu_trigger = Some(trigger);
    let style = if runtime.menu_open {
        Style::default()
            .fg(ratatui_theme_color(&app.options.theme.background))
            .bg(ratatui_theme_color(&app.options.theme.accent))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(ratatui_theme_color(&app.options.theme.muted))
    };
    drop(runtime);
    Paragraph::new(Line::styled(" Extensions ", style)).render(trigger, buffer);
}

/// Draw the active command dropdown above the finished host surface.
pub fn render_extension_command_menu(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    let mut runtime = app
        .extension_pane_runtime
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !runtime.menu_open || (runtime.commands.is_empty() && runtime.active_keyboard_mode.is_none())
    {
        runtime.menu_bounds = None;
        return;
    }
    let Some(trigger) = runtime.menu_trigger else {
        runtime.menu_open = false;
        runtime.menu_bounds = None;
        return;
    };
    let mut labels = runtime
        .commands
        .iter()
        .map(|registration| {
            let title = sanitize_terminal_line(&registration.command.title);
            let keys = registration.command.default_keys.join(", ");
            if keys.is_empty() {
                title
            } else {
                format!("{title}  {keys}")
            }
        })
        .collect::<Vec<_>>();
    if let Some(active) = &runtime.active_keyboard_mode {
        labels.insert(0, format!("Exit {}", active.mode.title));
    }
    let desired_width = labels
        .iter()
        .map(|label| label.width())
        .max()
        .unwrap_or(1)
        .saturating_add(2);
    let width = u16::try_from(desired_width)
        .unwrap_or(u16::MAX)
        .max(20)
        .min(area.width.max(1));
    let height = u16::try_from(labels.len().saturating_add(2))
        .unwrap_or(u16::MAX)
        .min(area.height.max(1));
    let x = trigger.x.min(area.right().saturating_sub(width));
    let y = if trigger.bottom().saturating_add(height) <= area.bottom() {
        trigger.bottom()
    } else {
        trigger.y.saturating_sub(height)
    };
    let bounds = Rect::new(x, y, width, height);
    runtime.menu_bounds = Some(bounds);
    let selected = runtime.menu_selected;
    drop(runtime);

    Clear.render(bounds, buffer);
    let block = Block::default()
        .title(" Extensions ")
        .borders(Borders::ALL)
        .style(Style::default().bg(ratatui_theme_color(&app.options.theme.panel)))
        .border_style(Style::default().fg(ratatui_theme_color(&app.options.theme.border)));
    let inner = block.inner(bounds);
    block.render(bounds, buffer);
    let items = labels.into_iter().enumerate().map(|(index, label)| {
        let style = if index == selected {
            Style::default()
                .fg(ratatui_theme_color(&app.options.theme.background))
                .bg(ratatui_theme_color(&app.options.theme.accent))
        } else {
            Style::default().fg(ratatui_theme_color(&app.options.theme.text))
        };
        ListItem::new(Line::styled(label, style))
    });
    List::new(items).render(inner, buffer);
}

/// Draw the clickable host-owned escape hatch for an active extension mode.
pub fn render_active_keyboard_mode_badge(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    let hint = app.active_keyboard_mode_status_hint();
    let mut runtime = app
        .extension_pane_runtime
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(hint) = hint else {
        runtime.mode_badge_bounds = None;
        return;
    };
    if area.width == 0 || area.height == 0 {
        runtime.mode_badge_bounds = None;
        return;
    }
    let maximum = (area.width / 2).max(6).min(area.width);
    let width = u16::try_from(hint.width().saturating_add(2))
        .unwrap_or(u16::MAX)
        .min(maximum);
    let bounds = Rect::new(area.right().saturating_sub(width), area.y, width, 1);
    runtime.mode_badge_bounds = Some(bounds);
    drop(runtime);
    Paragraph::new(Line::styled(
        format!(" {hint} "),
        Style::default()
            .fg(ratatui_theme_color(&app.options.theme.panel_alt))
            .bg(ratatui_theme_color(&app.options.theme.badge_neutral))
            .add_modifier(Modifier::BOLD),
    ))
    .render(bounds, buffer);
}

/// Draw the host-owned input modal requested by a native extension.
pub fn render_extension_input_dialog(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    let dialog = app
        .extension_pane_runtime
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .input_dialog
        .clone();
    let Some(dialog) = dialog else {
        return;
    };
    let desired_width = dialog
        .title
        .width()
        .max(dialog.placeholder.width())
        .max(dialog.value.width())
        .saturating_add(4);
    let width = u16::try_from(desired_width)
        .unwrap_or(u16::MAX)
        .max(24)
        .min(area.width.max(1));
    let height = 3.min(area.height.max(1));
    let bounds = Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    );
    Clear.render(bounds, buffer);
    let block = Block::default()
        .title(format!(" {} ", dialog.title))
        .borders(Borders::ALL)
        .style(Style::default().bg(ratatui_theme_color(&app.options.theme.panel)))
        .border_style(Style::default().fg(ratatui_theme_color(&app.options.theme.accent)));
    let inner = block.inner(bounds);
    block.render(bounds, buffer);
    let (value, style) = if dialog.value.is_empty() {
        (
            dialog.placeholder,
            Style::default().fg(ratatui_theme_color(&app.options.theme.muted)),
        )
    } else {
        (
            dialog.value,
            Style::default().fg(ratatui_theme_color(&app.options.theme.text)),
        )
    };
    Paragraph::new(Line::styled(value, style)).render(inner, buffer);
}

fn render_body(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    let static_specs = app
        .options
        .extension_panes
        .iter()
        .enumerate()
        .map(|(index, view)| ExtensionPaneSpec {
            key: format!("static:{index}:{}", view.pane.id),
            pane: view.pane.clone(),
        })
        .collect::<Vec<_>>();
    let theme = to_extension_paint_theme(&app.options.theme);
    let (generation, selection) = app.with_state(|state| (state.generation(), state.selection()));
    let mut runtime = app
        .extension_pane_runtime
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut specs = static_specs.clone();
    specs.extend(runtime.panes.iter().map(|registration| ExtensionPaneSpec {
        key: registration.key.clone(),
        pane: registration.pane.clone(),
    }));
    let mut open = runtime.open.clone();
    open.extend(static_specs.iter().map(|spec| spec.key.clone()));
    let plan = plan_extension_panes(
        &specs,
        &open,
        &runtime.size_overrides,
        area,
        20,
        MIN_EXTENSION_REVIEW_HEIGHT,
    );
    runtime.layout = plan.clone();

    let mut rendered_panes = Vec::new();
    let mut snapshot = None;
    for planned in &plan.panes {
        if let Some((index, _)) = static_specs
            .iter()
            .enumerate()
            .find(|(_, spec)| spec.key == planned.key)
        {
            rendered_panes.push((
                app.options.extension_panes[index].clone(),
                planned.bounds,
                planned.divider,
            ));
            continue;
        }
        let Some(registration) = runtime
            .panes
            .iter()
            .find(|registration| registration.key == planned.key)
            .cloned()
        else {
            continue;
        };
        let signature = PaneRenderSignature {
            generation,
            selection,
            placement: registration.pane.placement,
            width: planned.bounds.width,
            height: planned.bounds.height,
            theme: theme.clone(),
        };
        let cached = runtime
            .cached_renders
            .get(&planned.key)
            .filter(|cached| cached.signature == signature)
            .map(|cached| cached.view.clone());
        let view = cached.unwrap_or_else(|| {
            let current_snapshot = snapshot
                .get_or_insert_with(|| app.with_state(|state| state.snapshot()))
                .clone();
            let request = PaneRenderRequest {
                pane_id: registration.pane.id.clone(),
                snapshot: current_snapshot,
                placement: signature.placement,
                width: signature.width,
                height: signature.height,
                theme: signature.theme.clone(),
            };
            let result = runtime.extensions[registration.extension_index]
                .render_pane(request)
                .unwrap_or_else(|error| ExtensionPaneView {
                    extension_id: registration.extension_id.clone(),
                    pane: registration.pane.clone(),
                    content: ViewNode::Text {
                        text: format!("Pane unavailable: {error}"),
                        style: ViewStyle {
                            foreground: Some("danger".into()),
                            ..ViewStyle::default()
                        },
                    },
                });
            runtime.cached_renders.insert(
                planned.key.clone(),
                CachedPaneRender {
                    signature,
                    view: result.clone(),
                },
            );
            result
        });
        rendered_panes.push((view, planned.bounds, planned.divider));
    }
    drop(runtime);

    render_builtin_body(plan.review_bounds, buffer, app);
    for (pane, pane_area, divider) in rendered_panes {
        if let Some(divider) = divider {
            render_extension_pane_divider(divider, buffer, pane.pane.placement, app);
        }
        render_extension_pane(pane_area, buffer, &pane, app);
    }
}

fn render_builtin_body(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    let state = app
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if state.changeset().is_empty() {
        Paragraph::new("No changes to review")
            .style(Style::default().fg(ratatui_theme_color(&app.options.theme.muted)))
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

fn render_extension_pane(
    area: Rect,
    buffer: &mut Buffer,
    pane: &ExtensionPaneView,
    app: &ReviewApp,
) {
    let mut lines = Vec::new();
    flatten_view(&pane.content, 0, &mut lines);
    Block::default()
        .style(Style::default().bg(ratatui_theme_color(&app.options.theme.panel)))
        .render(area, buffer);
    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .render(area, buffer);
}

fn render_extension_pane_divider(
    area: Rect,
    buffer: &mut Buffer,
    placement: PanePlacement,
    app: &ReviewApp,
) {
    let style = Style::default().fg(ratatui_theme_color(&app.options.theme.border));
    if matches!(placement, PanePlacement::Left | PanePlacement::Right) {
        Paragraph::new(vec![Line::styled("│", style); usize::from(area.height)])
            .render(area, buffer);
    } else {
        Paragraph::new(Line::styled("─".repeat(usize::from(area.width)), style))
            .render(area, buffer);
    }
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
                Span::styled(
                    format!("{marker} "),
                    Style::default().fg(ratatui_theme_color(&app.options.theme.accent)),
                ),
                Span::styled(
                    truncate_start(&file.path, area.width.saturating_sub(10) as usize),
                    Style::default().fg(ratatui_theme_color(&app.options.theme.text)),
                ),
                Span::styled(
                    format!(" +{}", file.stats.additions),
                    Style::default().fg(ratatui_theme_color(&app.options.theme.badge_added)),
                ),
                Span::styled(
                    format!(" -{}", file.stats.deletions),
                    Style::default().fg(ratatui_theme_color(&app.options.theme.badge_removed)),
                ),
            ]))
        })
        .collect::<Vec<_>>();
    let border_style = if app.focus == Focus::Sidebar {
        Style::default().fg(ratatui_theme_color(&app.options.theme.accent))
    } else {
        Style::default().fg(ratatui_theme_color(&app.options.theme.border))
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
    app.review_height.set(area.height);
    let state = app
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let layout = state.resolved_layout(area.width);
    let mut highlights = app
        .highlights
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut rows = build_live_review_rows(
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
    let cursor_row = app.current_line_row.min(rows.lines.len().saturating_sub(1));
    if app.active_keyboard_mode_title().is_some()
        && let Some(line) = rows.lines.get_mut(cursor_row)
    {
        let background = ratatui_theme_color(&app.options.theme.accent_muted);
        line.style = line.style.bg(background);
        for span in &mut line.spans {
            span.style = span.style.bg(background);
        }
    }
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
        Style::default().fg(ratatui_theme_color(&app.options.theme.accent))
    } else {
        Style::default().fg(ratatui_theme_color(&app.options.theme.border))
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

#[derive(Debug, Clone, Copy)]
struct ReviewStreamChrome {
    show_file_headers: bool,
}

impl Default for ReviewStreamChrome {
    fn default() -> Self {
        Self {
            show_file_headers: true,
        }
    }
}

#[cfg(test)]
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
    build_review_rows_with_chrome(
        changeset,
        comments,
        selection,
        layout,
        options,
        width,
        highlight_cache,
        expanded_gaps,
        ReviewStreamChrome::default(),
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_live_review_rows(
    changeset: &Changeset,
    comments: &[ReviewComment],
    selection: ReviewSelection,
    layout: LayoutMode,
    options: &ReviewOptions,
    width: u16,
    highlight_cache: &mut HighlightCache,
    expanded_gaps: &BTreeSet<(String, usize)>,
) -> ReviewRows {
    build_review_rows_with_chrome(
        changeset,
        comments,
        selection,
        layout,
        options,
        width,
        highlight_cache,
        expanded_gaps,
        ReviewStreamChrome::default(),
        true,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_review_rows_with_chrome(
    changeset: &Changeset,
    comments: &[ReviewComment],
    selection: ReviewSelection,
    layout: LayoutMode,
    options: &ReviewOptions,
    width: u16,
    highlight_cache: &mut HighlightCache,
    expanded_gaps: &BTreeSet<(String, usize)>,
    chrome: ReviewStreamChrome,
    live: bool,
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
        if chrome.show_file_headers {
            rows.push(file_header(
                file,
                selection.file_index == file_index,
                usize::from(width),
                header_stats_width,
                &options.theme,
            ));
        }
        let file_selection = if selection.file_index == file_index {
            selection
        } else {
            ReviewSelection::default()
        };
        let highlighted = if options.highlight {
            let appearance = match options.theme.appearance {
                ThemeAppearance::Light => workdeck_diff::HighlightAppearance::Light,
                ThemeAppearance::Dark => workdeck_diff::HighlightAppearance::Dark,
            };
            if live {
                highlight_cache
                    .highlight_with_syntax_theme_live(
                        file,
                        appearance,
                        options.theme.syntax_theme.as_deref(),
                        &options.theme.syntax_scope_overrides,
                    )
                    .unwrap_or_default()
            } else {
                highlight_cache.highlight_with_syntax_theme(
                    file,
                    appearance,
                    options.theme.syntax_theme.as_deref(),
                    &options.theme.syntax_scope_overrides,
                )
            }
        } else {
            Vec::new()
        };
        let gap_source = review_gap_source_for_file(file);
        let expansion_side = review_expansion_side(file.change_kind);
        let selected_source = match expansion_side {
            ReviewSide::Old => file.sources.old.as_ref(),
            ReviewSide::New => file.sources.new.as_ref(),
        };
        let expanded_source = expanded_gaps
            .iter()
            .any(|(file_key, _)| file_key == &file.key)
            .then_some(selected_source)
            .flatten();
        let highlighted_source = expanded_source.and_then(|source| {
            if !options.highlight {
                return None;
            }
            let appearance = match options.theme.appearance {
                ThemeAppearance::Light => workdeck_diff::HighlightAppearance::Light,
                ThemeAppearance::Dark => workdeck_diff::HighlightAppearance::Dark,
            };
            if live {
                highlight_cache.highlight_source_with_syntax_theme_live(
                    file,
                    &source.content,
                    workdeck_diff::SourceHighlightTheme {
                        id: &options.theme.id,
                        appearance,
                        syntax_theme: options.theme.syntax_theme.as_deref(),
                        syntax_scope_overrides: &options.theme.syntax_scope_overrides,
                    },
                    true,
                )
            } else {
                Some(highlight_cache.highlight_source_with_syntax_theme(
                    file,
                    &source.content,
                    workdeck_diff::SourceHighlightTheme {
                        id: &options.theme.id,
                        appearance,
                        syntax_theme: options.theme.syntax_theme.as_deref(),
                        syntax_scope_overrides: &options.theme.syntax_scope_overrides,
                    },
                ))
            }
        });
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
        for (hunk_index, hunk) in file.hunks.iter().enumerate() {
            if let Some(address) = review_leading_gap(&gap_source, hunk_index) {
                rows.extend(source_gap_rows(
                    file,
                    address,
                    hunk_index,
                    layout,
                    options,
                    width,
                    expanded_gaps,
                    highlighted_source.as_ref(),
                ));
            }
            if hunk_index > 0 {
                rows.extend((0..options.hunk_gap).map(|_| Line::default()));
            }
            hunk_tops.insert((file_index, hunk_index), rows.len());
            let selected_hunk = selection.file_index == file_index
                && selection.hunk_index == Some(hunk_index)
                && selection.line.is_none();
            if options.hunk_headers {
                rows.push(Line::styled(
                    format!("▌{}", hunk.formatted_header()),
                    if selected_hunk {
                        Style::default()
                            .fg(ratatui_theme_color(&options.theme.accent))
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(ratatui_theme_color(&options.theme.muted))
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
                    selected_hunk,
                )),
                LayoutMode::Stack | LayoutMode::Auto => rows.extend(stack_hunk_rows(
                    file,
                    hunk,
                    options,
                    highlighted.get(hunk_index),
                    comments,
                    width,
                    file_selection,
                    selected_hunk,
                )),
            }
        }
        if let Some(address) = review_trailing_gap(&gap_source) {
            rows.extend(source_gap_rows(
                file,
                address,
                file.hunks.len(),
                layout,
                options,
                width,
                expanded_gaps,
                highlighted_source.as_ref(),
            ));
        }
    }
    ReviewRows {
        lines: rows,
        file_tops,
        hunk_tops,
    }
}

#[allow(clippy::too_many_arguments)]
fn source_gap_rows(
    file: &DiffFile,
    address: ReviewGapAddress,
    gap_slot: usize,
    layout: LayoutMode,
    options: &ReviewOptions,
    width: u16,
    expanded_gaps: &BTreeSet<(String, usize)>,
    highlighted_source: Option<&workdeck_diff::HighlightedSourceCode>,
) -> Vec<Line<'static>> {
    let side = review_expansion_side(file.change_kind);
    let source = match side {
        ReviewSide::Old => file.sources.old.as_ref(),
        ReviewSide::New => file.sources.new.as_ref(),
    };
    let expanded = expanded_gaps.contains(&(file.key.clone(), gap_slot));
    let status = source.map_or(
        ExpandedSourceStatus::Error(ExpandedSourceError::Unavailable),
        |source| ExpandedSourceStatus::Loaded(&source.content),
    );
    let plan = plan_expanded_gap(&file.key, address, expanded, status, side);
    let mut rows = Vec::with_capacity(plan.lines.len().saturating_add(1));
    rows.push(source_gap_label(&plan.label, width));
    for expanded_line in plan.lines {
        let highlighted = highlighted_source
            .and_then(|source| source.lines.get(expanded_line.source_line_index))
            .and_then(Option::as_ref)
            .map(|tokens| sanitized_syntax_tokens(tokens));
        let line = DiffLine {
            kind: DiffLineKind::Context,
            content: sanitize_terminal_line(&expanded_line.text),
            old_line: Some(expanded_line.old_line),
            new_line: Some(expanded_line.new_line),
            moved: false,
            no_newline_at_eof: false,
        };
        match layout {
            LayoutMode::Stack | LayoutMode::Auto => rows.extend(stack_line_rows(
                &line,
                options,
                highlighted.as_ref(),
                &[],
                false,
                width,
            )),
            LayoutMode::Split => {
                let available = usize::from(width.saturating_sub(1));
                let left_width = available / 2;
                let right_width = available.saturating_sub(left_width);
                rows.extend(split_pair_rows(
                    SplitCellInput {
                        line: Some(&line),
                        highlighted: highlighted.as_ref(),
                        emphasis: &[],
                    },
                    SplitCellInput {
                        line: Some(&line),
                        highlighted: highlighted.as_ref(),
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
    rows
}

fn sanitized_syntax_tokens(tokens: &[SyntaxToken]) -> Vec<SyntaxToken> {
    tokens
        .iter()
        .filter_map(|token| {
            let text = sanitize_terminal_line(&token.text);
            (!text.is_empty()).then_some(SyntaxToken {
                text,
                foreground: token.foreground,
                bold: token.bold,
                italic: token.italic,
                underline: token.underline,
            })
        })
        .collect()
}

fn source_gap_label(label: &str, width: u16) -> Line<'static> {
    let spans = clip_styled_spans(
        vec![Span::styled(
            format!("▾ {label}"),
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
    theme: &AppTheme,
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
                .fg(ratatui_theme_color(if selected {
                    &theme.accent
                } else {
                    &theme.text
                }))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            label.state_label.unwrap_or_default(),
            Style::default().fg(ratatui_theme_color(&theme.muted)),
        ),
        Span::raw(" ".repeat(gap.saturating_add(header_stats_width.saturating_sub(stats.width)))),
        Span::styled(
            stats.additions_text,
            Style::default().fg(ratatui_theme_color(&theme.badge_added)),
        ),
        Span::raw(" "),
        Span::styled(
            stats.deletions_text,
            Style::default().fg(ratatui_theme_color(&theme.badge_removed)),
        ),
        Span::raw("  "),
    ])
}

#[allow(clippy::too_many_arguments)]
fn stack_hunk_rows(
    file: &DiffFile,
    hunk: &workdeck_core::DiffHunk,
    options: &ReviewOptions,
    highlighted: Option<&Vec<HighlightedDiffLine>>,
    comments: &[ReviewComment],
    width: u16,
    selection: ReviewSelection,
    hunk_selected: bool,
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
            highlighted
                .and_then(|lines| lines.get(index))
                .and_then(|line_highlight| line_highlight.for_stack(line.kind)),
            &emphasis[index],
            hunk_selected || line_is_selected(line, selection),
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
    let (marker, fg, default_bg) = line_style(line.kind, &options.theme);
    let bg = moved_line_background(line, &options.theme).unwrap_or(default_bg);
    let row_bg = if selected && options.cursor_line == CursorLineMode::Row {
        ratatui_theme_color(&options.theme.selected_hunk)
    } else {
        bg
    };
    let gutter = if options.line_numbers {
        let digits = options.line_number_digits.unwrap_or(4).max(1);
        format!(
            "{:>digits$} {:>digits$} {marker}  ",
            line.old_line.map_or(String::new(), |line| line.to_string()),
            line.new_line.map_or(String::new(), |line| line.to_string()),
            digits = digits,
        )
    } else {
        format!("{marker} ")
    };
    let number_style = Style::default()
        .fg(
            if selected && options.cursor_line == CursorLineMode::Number {
                ratatui_theme_color(&options.theme.accent)
            } else {
                ratatui_theme_color(&options.theme.line_number_fg)
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
    code = emphasize_spans(
        code,
        emphasis,
        emphasis_background(line.kind, &options.theme),
    );
    let digits = options.line_number_digits.unwrap_or(4).max(1);
    let geometry = resolve_stack_cell_geometry(
        usize::from(width),
        digits,
        options.line_numbers,
        DIFF_RAIL_PREFIX_WIDTH,
    );
    let prefix_width = geometry.gutter_width + DIFF_RAIL_PREFIX_WIDTH;
    let content_width = geometry.content_width;
    let wrapped = if options.wrap_lines {
        wrap_styled_spans(code, content_width)
    } else {
        vec![clip_styled_spans(
            skip_styled_spans(code, options.horizontal_offset),
            content_width,
        )]
    };
    wrapped
        .into_iter()
        .enumerate()
        .map(|(index, mut code)| {
            let mut spans = if index == 0 {
                vec![
                    Span::styled("▌", marker_style),
                    Span::styled(gutter.clone(), number_style),
                ]
            } else {
                vec![
                    Span::styled("▌", marker_style),
                    Span::styled(
                        " ".repeat(prefix_width.saturating_sub(1)),
                        Style::default().bg(row_bg),
                    ),
                ]
            };
            spans.append(&mut code);
            spans = clip_styled_spans(spans, usize::from(width));
            pad_spans(&mut spans, usize::from(width), Style::default().bg(row_bg));
            Line::from(spans)
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn split_hunk_rows(
    file: &DiffFile,
    hunk: &workdeck_core::DiffHunk,
    options: &ReviewOptions,
    width: u16,
    highlighted: Option<&Vec<HighlightedDiffLine>>,
    comments: &[ReviewComment],
    selection: ReviewSelection,
    hunk_selected: bool,
) -> Vec<Line<'static>> {
    let mut rows = Vec::new();
    let pane_widths = resolve_diff_split_pane_widths(usize::from(width));
    let left_width = pane_widths.left_width;
    let right_width = pane_widths.right_width;
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
                    .and_then(|index| highlighted.and_then(|lines| lines.get(index)))
                    .and_then(|line| line.deletion.as_ref()),
                emphasis: emphasis.as_ref().map_or(&[], |ranges| &ranges.old),
            },
            SplitCellInput {
                line: new,
                highlighted: pair
                    .new_index
                    .and_then(|index| highlighted.and_then(|lines| lines.get(index)))
                    .and_then(|line| line.addition.as_ref()),
                emphasis: emphasis.as_ref().map_or(&[], |ranges| &ranges.new),
            },
            options,
            left_width,
            right_width,
            hunk_selected
                || old.is_some_and(|line| line_is_selected(line, selection))
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
        return vec![vec![
            Span::styled(
                "▌",
                Style::default().fg(ratatui_theme_color(&options.theme.muted)),
            ),
            Span::raw(" ".repeat(width.saturating_sub(1))),
        ]];
    };
    let (marker, fg, default_bg) = line_style(line.kind, &options.theme);
    let bg = moved_line_background(line, &options.theme).unwrap_or(default_bg);
    let row_bg = if selected && options.cursor_line == CursorLineMode::Row {
        ratatui_theme_color(&options.theme.selected_hunk)
    } else {
        bg
    };
    let gutter = if options.line_numbers {
        let digits = options.line_number_digits.unwrap_or(4).max(1);
        let number = if old { line.old_line } else { line.new_line };
        format!(
            "{:>digits$} {marker} ",
            number.map_or(String::new(), |line| line.to_string()),
            digits = digits,
        )
    } else {
        format!("{marker} ")
    };
    let number_style = Style::default()
        .fg(
            if selected && options.cursor_line == CursorLineMode::Number {
                ratatui_theme_color(&options.theme.accent)
            } else {
                ratatui_theme_color(&options.theme.line_number_fg)
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
    code = emphasize_spans(
        code,
        emphasis,
        emphasis_background(line.kind, &options.theme),
    );
    let digits = options.line_number_digits.unwrap_or(4).max(1);
    let geometry =
        resolve_split_cell_geometry(width, digits, options.line_numbers, DIFF_RAIL_PREFIX_WIDTH);
    let prefix_width = geometry.gutter_width + DIFF_RAIL_PREFIX_WIDTH;
    let content_width = geometry.content_width;
    let wrapped = if options.wrap_lines {
        wrap_styled_spans(code, content_width)
    } else {
        vec![clip_styled_spans(
            skip_styled_spans(code, options.horizontal_offset),
            content_width,
        )]
    };
    wrapped
        .into_iter()
        .enumerate()
        .map(|(index, mut code)| {
            let mut spans = if index == 0 {
                vec![
                    Span::styled("▌", marker_style),
                    Span::styled(gutter.clone(), number_style),
                ]
            } else {
                vec![
                    Span::styled("▌", marker_style),
                    Span::styled(
                        " ".repeat(prefix_width.saturating_sub(1)),
                        Style::default().bg(row_bg),
                    ),
                ]
            };
            spans.append(&mut code);
            spans = clip_styled_spans(spans, width);
            pad_spans(&mut spans, width, Style::default().bg(row_bg));
            spans
        })
        .collect()
}

fn line_style(kind: DiffLineKind, theme: &AppTheme) -> (char, Color, Color) {
    match kind {
        DiffLineKind::Context => (
            ' ',
            ratatui_theme_color(&theme.text),
            ratatui_theme_color(&theme.context_bg),
        ),
        DiffLineKind::Addition => (
            '+',
            ratatui_theme_color(&theme.added_sign_color),
            ratatui_theme_color(&theme.added_bg),
        ),
        DiffLineKind::Deletion => (
            '-',
            ratatui_theme_color(&theme.removed_sign_color),
            ratatui_theme_color(&theme.removed_bg),
        ),
    }
}

fn moved_line_background(line: &DiffLine, theme: &AppTheme) -> Option<Color> {
    if !line.moved {
        return None;
    }
    match line.kind {
        DiffLineKind::Addition => Some(ratatui_theme_color(&theme.moved_added_bg)),
        DiffLineKind::Deletion => Some(ratatui_theme_color(&theme.moved_removed_bg)),
        DiffLineKind::Context => None,
    }
}

fn expanded_line_content(line: &DiffLine, tab_width: u16) -> String {
    expand_tabs(&line.content, tab_width, &mut 0)
}

fn emphasis_background(kind: DiffLineKind, theme: &AppTheme) -> Color {
    match kind {
        DiffLineKind::Deletion => ratatui_theme_color(&resolve_word_diff_highlight_bg(
            &theme.removed_content_bg,
            &theme.removed_bg,
            &theme.removed_sign_color,
        )),
        DiffLineKind::Addition => ratatui_theme_color(&resolve_word_diff_highlight_bg(
            &theme.added_content_bg,
            &theme.added_bg,
            &theme.added_sign_color,
        )),
        DiffLineKind::Context => ratatui_theme_color(&theme.context_content_bg),
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

/// Drop complete display cells from the left while preserving span styles.
/// A wide glyph intersected by the boundary is omitted rather than split.
fn skip_styled_spans(spans: Vec<Span<'static>>, offset: usize) -> Vec<Span<'static>> {
    if offset == 0 {
        return spans;
    }
    let segments = spans_to_segments(spans);
    segments_to_spans(slice_segments_window(&segments, offset, usize::MAX).segments)
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
    let output = expand_diff_tabs(value, tab_width, *column);
    *column = column.saturating_add(output.width());
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
        render_active_keyboard_mode_badge(area, buffer, app);
        return;
    }
    let focus = match app.focus {
        Focus::Review => "review",
        Focus::Sidebar => "files",
    };
    let mut spans = vec![Span::styled(
        format!(" {focus} "),
        Style::default().fg(ratatui_theme_color(&app.options.theme.accent)),
    )];
    if let Some(status) = &app.status {
        spans.push(Span::styled(
            format!("  {status}"),
            Style::default().fg(ratatui_theme_color(&app.options.theme.file_modified)),
        ));
    }
    spans.push(Span::styled(
        "  j/k scroll  n/p hunk  [/ ] file  e source  r reload  s/u/a layout  ? help  q quit",
        Style::default().fg(ratatui_theme_color(&app.options.theme.muted)),
    ));
    Paragraph::new(Line::from(spans)).render(area, buffer);
    render_active_keyboard_mode_badge(area, buffer, app);
}

fn render_extension_toast(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    let Some(notification) = app.active_extension_notification() else {
        return;
    };
    let theme = ExtensionToastTheme {
        badge_removed: ratatui_theme_color(&app.options.theme.badge_removed),
        file_modified: ratatui_theme_color(&app.options.theme.file_modified),
        badge_neutral: ratatui_theme_color(&app.options.theme.badge_neutral),
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
    fn explicit_light_theme_reaches_the_complete_review_cell_buffer() {
        fn cells_matching_text(buffer: &Buffer, text: &str) -> Vec<ratatui::buffer::Cell> {
            let symbols = text
                .chars()
                .map(|value| value.to_string())
                .collect::<Vec<_>>();
            let mut matches = Vec::new();
            for y in buffer.area.y..buffer.area.bottom() {
                let max_x = buffer.area.right().saturating_sub(symbols.len() as u16);
                for x in buffer.area.x..=max_x {
                    if symbols.iter().enumerate().all(|(offset, symbol)| {
                        buffer
                            .cell((x.saturating_add(offset as u16), y))
                            .is_some_and(|cell| cell.symbol() == symbol)
                    }) {
                        matches.push(buffer.cell((x, y)).unwrap().clone());
                    }
                }
            }
            matches
        }

        let theme = resolve_theme(Some("github-light-default"), None, &[]);
        let backend = TestBackend::new(100, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                layout: LayoutMode::Stack,
                sidebar: false,
                line_numbers: false,
                highlight: false,
                theme: theme.clone(),
                ..ReviewOptions::default()
            },
        );
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let old = cells_matching_text(buffer, "old");
        let new = cells_matching_text(buffer, "new");
        assert_eq!(old.len(), 1);
        assert_eq!(new.len(), 1);
        assert_eq!(old[0].fg, ratatui_theme_color(&theme.removed_sign_color));
        assert_eq!(old[0].bg, ratatui_theme_color(&theme.removed_content_bg));
        assert_eq!(new[0].fg, ratatui_theme_color(&theme.added_sign_color));
        assert_eq!(new[0].bg, ratatui_theme_color(&theme.added_content_bg));
        assert_eq!(
            buffer.cell((99, 19)).unwrap().bg,
            ratatui_theme_color(&theme.background)
        );
        let workdeck = cells_matching_text(buffer, "Workdeck");
        assert_eq!(workdeck.len(), 1);
        assert_eq!(workdeck[0].fg, ratatui_theme_color(&theme.background));
        assert_eq!(workdeck[0].bg, ratatui_theme_color(&theme.accent));
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
        assert_eq!(wrapped.lines.len(), 8);
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
                    .filter(|span| span.content.as_ref() == "▌")
                    .count(),
                2
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
        changeset.files[0].flags.partial = false;
        let options = ReviewOptions {
            layout: LayoutMode::Stack,
            sidebar: false,
            line_numbers: false,
            highlight: false,
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
        assert!(collapsed_text.contains("▾ 2 unchanged lines"));
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
        assert!(expanded_text.contains("▾ Hide 2 unchanged lines"));
        assert!(expanded_text.contains("one"));
        assert!(expanded_text.contains("two"));
        assert!(expanded_text.contains("▾ 1 unchanged line"));
        let label_index = expanded_text.find("Hide 2 unchanged lines").unwrap();
        assert!(label_index < expanded_text.find("one").unwrap());

        let trailing = build_review_rows(
            &changeset,
            &[],
            ReviewSelection::default(),
            LayoutMode::Stack,
            &options,
            80,
            &mut highlights,
            &BTreeSet::from([(changeset.files[0].key.clone(), 1)]),
        );
        let trailing_text = trailing
            .lines
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(trailing_text.contains("▾ Hide 1 unchanged line"));
        assert!(trailing_text.contains("four"));

        let split = build_review_rows(
            &changeset,
            &[],
            ReviewSelection::default(),
            LayoutMode::Split,
            &ReviewOptions {
                layout: LayoutMode::Split,
                line_numbers: true,
                ..options.clone()
            },
            100,
            &mut highlights,
            &BTreeSet::from([(changeset.files[0].key.clone(), 0)]),
        );
        let first_context = split
            .lines
            .iter()
            .find(|line| line.to_string().matches("one").count() == 2)
            .expect("split expansion paints the source on both sides");
        assert_eq!(
            first_context
                .spans
                .iter()
                .filter(|span| span.content.as_ref() == "▌")
                .count(),
            2
        );
        assert_eq!(
            first_context
                .spans
                .iter()
                .filter(|span| span.content.trim() == "1")
                .count(),
            2
        );
    }

    #[test]
    fn expanded_source_rows_have_no_cap_and_sanitize_controls_and_tabs() {
        let controls = "\x1b]52;c;SGVsbG8=\x07\x1b[2J\x1bPqpayload\x1b\\\x07\rspoof\x08hidden\x1b";
        let mut source_lines = vec![format!("a\tb safe{controls}")];
        source_lines.extend((2..=250).map(|line| format!("line-{line}")));
        source_lines.push("new".into());
        let new_source = format!("{}\n", source_lines.join("\n"));
        source_lines[250] = "old".into();
        let old_source = format!("{}\n", source_lines.join("\n"));
        let mut changeset = parse_patch(
            "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -251 +251 @@\n-old\n+new\n",
            "source-safety",
            "Source safety",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap();
        let file_key = changeset.files[0].key.clone();
        changeset.files[0].set_sources(FileSourceSnapshots {
            old: Some(SourceSnapshot::new(
                old_source,
                SourceOrigin::Revision {
                    revision: "HEAD".into(),
                },
                true,
            )),
            new: Some(SourceSnapshot::new(
                new_source,
                SourceOrigin::WorkingTree,
                false,
            )),
        });
        let mut highlights = HighlightCache::default();
        let rows = build_review_rows(
            &changeset,
            &[],
            ReviewSelection::default(),
            LayoutMode::Stack,
            &ReviewOptions {
                layout: LayoutMode::Stack,
                sidebar: false,
                line_numbers: false,
                highlight: false,
                tab_width: 4,
                ..ReviewOptions::default()
            },
            100,
            &mut highlights,
            &BTreeSet::from([(file_key, 0)]),
        );
        let rendered = rows
            .lines
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("Hide 250 unchanged lines"));
        assert!(rendered.contains("a   b safespoofhidden"));
        assert!(rendered.contains("line-250"));
        assert!(!rendered.contains("expansion limit"));
        for control in ['\x1b', '\x07', '\r', '\x08'] {
            assert!(!rendered.contains(control));
        }

        let sanitized = sanitized_syntax_tokens(&[SyntaxToken {
            text: format!("safe{controls}visible"),
            foreground: workdeck_diff::SyntaxColor {
                red: 1,
                green: 2,
                blue: 3,
            },
            bold: true,
            italic: false,
            underline: false,
        }]);
        assert_eq!(sanitized.len(), 1);
        assert_eq!(sanitized[0].text, "safespoofhiddenvisible");
        assert!(sanitized[0].bold);
    }

    #[test]
    fn expanded_source_rows_use_full_source_syntax_spans() {
        let mut changeset = parse_patch(
            "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -3 +3 @@\n-old\n+new\n",
            "source-highlight",
            "Expanded source highlight",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap();
        let file_key = changeset.files[0].key.clone();
        changeset.files[0].set_sources(FileSourceSnapshots {
            old: Some(SourceSnapshot::new(
                "// hidden\npub fn marker() {}\nold\n".into(),
                SourceOrigin::Revision {
                    revision: "HEAD".into(),
                },
                true,
            )),
            new: Some(SourceSnapshot::new(
                "// hidden\npub fn marker() {}\nnew\n".into(),
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
        let rows = build_review_rows(
            &changeset,
            &[],
            ReviewSelection::default(),
            LayoutMode::Stack,
            &options,
            80,
            &mut highlights,
            &BTreeSet::from([(file_key, 0)]),
        );
        let source_row = rows
            .lines
            .iter()
            .find(|line| line.to_string().contains("pub fn marker"))
            .expect("expanded source row is rendered");
        let base_foreground = ratatui_theme_color(&options.theme.text);
        assert!(
            source_row.spans.iter().any(|span| {
                span.content.contains("pub") && span.style.fg != Some(base_foreground)
            }),
            "expanded source spans: {:?}; base foreground: {:?}",
            source_row.spans,
            base_foreground
        );
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
                    && span.style.bg
                        == Some(emphasis_background(
                            DiffLineKind::Addition,
                            &ReviewOptions::default().theme,
                        ))
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
                        default_open: true,
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
