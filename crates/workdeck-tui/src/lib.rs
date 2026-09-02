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
mod file_render_window;
mod file_section_layout;
mod file_view_geometry;
mod help_content;
mod hunk_scroll;
mod ids;
mod job_control;
mod key_routing;
mod keyboard;
mod line_highlights;
mod list_geometry;
mod menu;
mod mouse_capture;
mod mouse_scroll;
mod public_review;
mod review_state_helpers;
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
mod viewport_anchor;
mod viewport_geometry;
mod viewport_selection;
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
pub use file_render_window::*;
pub use file_section_layout::*;
pub use file_view_geometry::*;
pub use help_content::*;
pub use hunk_scroll::*;
pub use ids::*;
pub use job_control::*;
pub use key_routing::*;
pub use keyboard::*;
pub use line_highlights::*;
pub use list_geometry::*;
pub use menu::*;
pub use mouse_capture::*;
pub use mouse_scroll::*;
pub use public_review::*;
pub use review_state_helpers::*;
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
pub use viewport_anchor::*;
pub use viewport_geometry::*;
pub use viewport_selection::*;
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
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use workdeck_core::{
    AgentAnnotation, Changeset, ChangesetSource, DiffFile, DiffLine, DiffLineKind, ReviewSelection,
    ReviewSide, SourceOrigin,
};
use workdeck_diff::{
    DIFF_RAIL_PREFIX_WIDTH, HighlightCache, HighlightedDiffLine, SyntaxToken, TextSegment,
    clip_segments, expand_diff_tabs, plan_split_line_pairs, resolve_split_cell_geometry,
    resolve_split_pane_widths as resolve_diff_split_pane_widths, resolve_stack_cell_geometry,
    sanitize_terminal_line, slice_segments_window, word_diff_ranges, wrap_segments,
};
use workdeck_extension_api::{
    CommandRegistration, ExtensionFileViewSpan, ExtensionFileViewTone, ExtensionHostAction,
    ExtensionKeyEvent, ExtensionNotification, ExtensionNotificationHub,
    ExtensionNotificationSubscription, ExtensionNotifyType, ExtensionPaintTheme, ExtensionPaneView,
    ExtensionTextAttribute, ExtensionWorkspaceWriteCompletion, ExtensionWorkspaceWriteResult,
    FileViewModeKeyRequest, FileViewModeLifecycleRequest, KeyRoutingResult,
    KeyboardModeRegistration, PaneActionInvocation, PanePlacement, PaneRegistration,
    PaneRenderRequest, Registration, ReviewEvent, ValidatedFileViewLayout, ViewNode, ViewStyle,
    bundled_files_pane, extension_pane_size,
};
use workdeck_extension_host::{
    ExtensionRequestCancellation, FileViewSelectionState, LoadedExtension, RegisteredFileView,
    create_file_view_input, create_file_view_input_snapshot, reconcile_file_view_selections,
    registered_file_view_key, select_file_view,
};
use workdeck_review::{
    ExpandedSourceError, ExpandedSourceStatus, LayoutMode, PlannedFileViewRow, ReviewComment,
    ReviewGapAddress, ReviewState, VisibleFileViewNote, build_extension_review_snapshot,
    build_file_view_render_plan, plan_expanded_gap, review_expansion_side,
    review_gap_source_for_file, review_leading_gap, review_trailing_gap,
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
    pub command_cwd: Option<PathBuf>,
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
            command_cwd: None,
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
struct LiveFileViewRegistration {
    extension_index: usize,
    view: Arc<RegisteredFileView>,
}

#[derive(Debug, Clone)]
struct CachedFileViewLayout {
    view_key: String,
    content_identity: String,
    width: usize,
    layout: ValidatedFileViewLayout,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct FileViewComponentStateKey {
    file_id: String,
    row_id: String,
}

#[derive(Debug, Clone)]
struct FileViewComponentHit {
    state_key: FileViewComponentStateKey,
    bounds: Rect,
}

#[derive(Debug, Clone)]
struct FileViewComponentPointer {
    state_key: FileViewComponentStateKey,
    dragged: bool,
}

#[derive(Debug, Clone)]
struct FileViewComponentLogicalHit {
    state_key: FileViewComponentStateKey,
    top: usize,
    height: usize,
}

#[derive(Debug, Clone)]
struct ActiveKeyboardMode {
    extension_index: usize,
    extension_id: String,
    mode: KeyboardModeRegistration,
}

#[derive(Debug, Clone)]
struct ActiveFileViewModeRuntime {
    extension_index: usize,
    extension_id: String,
    view_id: String,
    view_key: String,
    file: Arc<workdeck_extension_api::ExtensionDiffFile>,
    review_generation: u64,
}

#[derive(Debug, Clone)]
struct ExtensionWorkspaceWriteDialog {
    extension_index: usize,
    extension_id: String,
    request_id: String,
    file_id: String,
    path: String,
    text: String,
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
struct ExtensionSelectDialog {
    extension_index: usize,
    extension_id: String,
    action_id: String,
    title: String,
    options: Vec<String>,
    selected: usize,
}

#[derive(Debug, Clone)]
struct ExtensionConfirmDialog {
    extension_index: usize,
    extension_id: String,
    action_id: String,
    title: String,
    body: String,
    confirm_label: String,
}

#[derive(Debug, Clone)]
struct ExtensionPaneActionHit {
    bounds: Rect,
    extension_index: usize,
    extension_id: String,
    pane_id: String,
    action_id: String,
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
    file_views: Vec<LiveFileViewRegistration>,
    file_view_selections: FileViewSelectionState,
    file_view_layouts: BTreeMap<String, CachedFileViewLayout>,
    file_view_component_expanded: BTreeSet<FileViewComponentStateKey>,
    file_view_component_hits: Vec<FileViewComponentHit>,
    file_view_component_pointer: MouseCapture<FileViewComponentPointer>,
    active_file_view_mode: Option<ActiveFileViewModeRuntime>,
    active_keyboard_mode: Option<ActiveKeyboardMode>,
    workspace_write_dialog: Option<ExtensionWorkspaceWriteDialog>,
    input_dialog: Option<ExtensionInputDialog>,
    select_dialog: Option<ExtensionSelectDialog>,
    confirm_dialog: Option<ExtensionConfirmDialog>,
    pane_action_hits: Vec<ExtensionPaneActionHit>,
    open: BTreeSet<String>,
    size_overrides: BTreeMap<String, u16>,
    cached_renders: BTreeMap<String, CachedPaneRender>,
    layout: ExtensionPaneLayoutPlan,
    resize: MouseCapture<(String, PaneResizeState)>,
    menu_open: bool,
    menu_selected: usize,
    menu_trigger: Option<Rect>,
    menu_bounds: Option<Rect>,
    mode_badge_bounds: Option<Rect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SidebarFileHit {
    bounds: Rect,
    file_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SidebarRevealKey {
    generation: u64,
    selected_file_id: String,
    mode: FileSidebarMode,
}

impl ExtensionPaneRuntime {
    fn new(extensions: Vec<LoadedExtension>) -> Self {
        let mut panes = Vec::new();
        let mut commands = Vec::new();
        let mut keyboard_modes = Vec::new();
        let mut file_views = Vec::new();
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
                    Registration::FileView {
                        id,
                        interactive_mode,
                        ..
                    } => file_views.push(LiveFileViewRegistration {
                        extension_index,
                        view: Arc::new(RegisteredFileView {
                            extension_id: extension.manifest.id.clone(),
                            view_id: id.clone(),
                            interactive_mode: *interactive_mode,
                        }),
                    }),
                    _ => {}
                }
            }
        }
        Self {
            extensions,
            panes,
            commands,
            keyboard_modes,
            file_views,
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
    sidebar_bounds: Cell<Option<Rect>>,
    sidebar_scroll_top: Cell<usize>,
    sidebar_reveal_key: Mutex<Option<SidebarRevealKey>>,
    sidebar_file_hits: Mutex<Vec<SidebarFileHit>>,
    current_line_row: usize,
    expanded_gaps: BTreeSet<(String, usize)>,
    highlights: Mutex<HighlightCache>,
    themes: ThemeController,
    extension_toasts: Arc<Mutex<ExtensionNotificationSurface>>,
    extension_notification_subscription: Option<ExtensionNotificationSubscription>,
    mouse_scroll_acceleration: ReviewMouseWheelScrollAcceleration,
    mouse_scroll_accumulator: f64,
    extension_pane_runtime: Mutex<ExtensionPaneRuntime>,
    extension_event_dispatch_depth: usize,
    extension_known_note_ids: BTreeSet<String>,
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
        let extension_known_note_ids = state
            .comments()
            .iter()
            .map(|comment| comment.id.clone())
            .collect();
        let mut app = Self {
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
            sidebar_bounds: Cell::new(None),
            sidebar_scroll_top: Cell::new(0),
            sidebar_reveal_key: Mutex::new(None),
            sidebar_file_hits: Mutex::new(Vec::new()),
            current_line_row: 0,
            expanded_gaps: BTreeSet::new(),
            highlights: Mutex::new(HighlightCache::default()),
            themes,
            extension_toasts,
            extension_notification_subscription,
            mouse_scroll_acceleration: ReviewMouseWheelScrollAcceleration::default(),
            mouse_scroll_accumulator: 0.0,
            extension_pane_runtime: Mutex::new(ExtensionPaneRuntime::new(extensions)),
            extension_event_dispatch_depth: 0,
            extension_known_note_ids,
        };
        app.publish_extension_event("changeset_loaded", serde_json::json!({}));
        app.publish_extension_selection_events();
        app
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

    /// Consume a reload requested by a host-mediated extension write.
    pub fn take_reload_requested(&mut self) -> bool {
        std::mem::take(&mut self.reload_requested)
    }

    pub fn reload(&mut self, changeset: Changeset) {
        self.exit_active_keyboard_mode();
        self.exit_active_file_view_mode();
        {
            let mut runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            runtime.input_dialog = None;
            runtime.select_dialog = None;
            runtime.confirm_dialog = None;
            runtime.workspace_write_dialog = None;
            runtime.pane_action_hits.clear();
            let file_ids = changeset
                .files
                .iter()
                .map(|file| file.runtime_id.clone())
                .collect::<Vec<_>>();
            let view_keys = runtime
                .file_views
                .iter()
                .map(|registration| registered_file_view_key(&registration.view))
                .collect::<BTreeSet<_>>();
            runtime.file_view_selections = reconcile_file_view_selections(
                &runtime.file_view_selections,
                &file_ids,
                &view_keys,
            );
            runtime.file_view_layouts.clear();
            runtime.file_view_component_expanded.clear();
            runtime.file_view_component_hits.clear();
            runtime.file_view_component_pointer.release();
        }
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
        self.extension_known_note_ids = self.with_state(|state| {
            state
                .comments()
                .iter()
                .map(|comment| comment.id.clone())
                .collect()
        });
        self.publish_extension_event("session_reload", serde_json::json!({}));
        self.publish_extension_selection_events();
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
        self.sync_extension_note_events();
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
        let runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        runtime
            .active_keyboard_mode
            .as_ref()
            .map(|active| active.mode.title.clone())
            .or_else(|| {
                runtime
                    .active_file_view_mode
                    .as_ref()
                    .map(|active| active.view_id.clone())
            })
    }

    #[must_use]
    pub fn active_keyboard_mode_status_hint(&self) -> Option<String> {
        let runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        runtime
            .active_keyboard_mode
            .as_ref()
            .map(|active| {
                format!(
                    "{} — ext {}:{} — Esc exits",
                    active.mode.title, active.extension_id, active.mode.id
                )
            })
            .or_else(|| {
                runtime.active_file_view_mode.as_ref().map(|active| {
                    format!(
                        "{}:{} mode — Esc exits",
                        active.extension_id, active.view_id
                    )
                })
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

    #[must_use]
    pub fn has_extension_select_dialog(&self) -> bool {
        self.extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .select_dialog
            .is_some()
    }

    #[must_use]
    pub fn has_extension_confirm_dialog(&self) -> bool {
        self.extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .confirm_dialog
            .is_some()
    }

    #[must_use]
    pub fn has_extension_dialog(&self) -> bool {
        self.has_extension_input_dialog()
            || self.has_extension_select_dialog()
            || self.has_extension_confirm_dialog()
            || self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .workspace_write_dialog
                .is_some()
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }
        if self.handle_workspace_write_key(&key)
            || self.handle_extension_confirm_key(&key)
            || self.handle_extension_select_key(&key)
            || self.handle_extension_input_key(&key)
        {
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
        if self.route_active_file_view_mode(&key) {
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
        let (snapshot, review) =
            self.with_state(|state| (state.snapshot(), build_extension_review_snapshot(state)));
        let cwd = self.extension_command_cwd();
        let execution = {
            let mut runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let open_panes = runtime.open.iter().cloned().collect();
            let active_keyboard_mode = runtime
                .active_keyboard_mode
                .as_ref()
                .map(|active| format!("{}:{}", active.extension_id, active.mode.id))
                .or_else(|| {
                    runtime
                        .active_file_view_mode
                        .as_ref()
                        .map(|active| format!("{}:{}", active.extension_id, active.view_id))
                });
            runtime.extensions[command.extension_index].invoke_command_with_review_context(
                &command.command.id,
                snapshot,
                open_panes,
                active_keyboard_mode,
                cwd,
                Some(review),
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

    fn invoke_extension_pane_action(&mut self, hit: ExtensionPaneActionHit) {
        let (snapshot, review) =
            self.with_state(|state| (state.snapshot(), build_extension_review_snapshot(state)));
        let cwd = self.extension_command_cwd();
        let execution = {
            let mut runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let open_panes = runtime.open.iter().cloned().collect();
            runtime.extensions[hit.extension_index].invoke_pane_action(PaneActionInvocation {
                pane_id: hit.pane_id,
                action_id: hit.action_id,
                snapshot,
                cwd,
                review: Some(review),
                open_panes,
            })
        };
        match execution {
            Ok(execution) => self.apply_extension_actions(
                hit.extension_index,
                &hit.extension_id,
                execution.actions,
            ),
            Err(error) => {
                self.status = Some(format!(
                    "extension {} pane action failed: {error}",
                    hit.extension_id
                ));
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
                ExtensionHostAction::RefreshPane { id } => {
                    let pane_key = if id.contains(':') {
                        id
                    } else {
                        format!("{extension_id}:{id}")
                    };
                    self.extension_pane_runtime
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .cached_renders
                        .remove(&pane_key);
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
                ExtensionHostAction::TryReviewCommand {
                    id,
                    count,
                    unavailable_message,
                } => {
                    if !self.execute_extension_review_command(&id, count.unwrap_or(1)) {
                        self.status = Some(format!(
                            "extension {extension_id}: warning: {}",
                            sanitize_terminal_line(&unavailable_message)
                        ));
                    }
                }
                ExtensionHostAction::SelectReviewFile { file_id } => {
                    self.select_extension_review_file(extension_id, &file_id);
                }
                ExtensionHostAction::SelectReviewHunk {
                    file_id,
                    hunk_index,
                } => {
                    self.select_extension_review_hunk(extension_id, &file_id, hunk_index);
                }
                ExtensionHostAction::RevealReviewLine {
                    file_id,
                    side,
                    line,
                } => {
                    self.reveal_extension_review_line(extension_id, &file_id, side, line);
                }
                ExtensionHostAction::ToggleFileView { id } => {
                    self.toggle_extension_file_view(extension_index, extension_id, &id);
                }
                ExtensionHostAction::EnterFileViewMode { id } => {
                    self.enter_file_view_mode(extension_index, extension_id, &id);
                }
                ExtensionHostAction::ExitFileViewMode => {
                    self.exit_file_view_mode_for_extension(extension_index);
                }
                ExtensionHostAction::RefreshFileView { id, file_id } => {
                    self.refresh_extension_file_view(
                        extension_index,
                        extension_id,
                        &id,
                        file_id.as_deref(),
                    );
                }
                ExtensionHostAction::RequestWorkspaceWrite {
                    request_id,
                    file_id,
                    text,
                } => {
                    self.open_workspace_write_dialog(
                        extension_index,
                        extension_id,
                        request_id,
                        file_id,
                        text,
                    );
                }
                ExtensionHostAction::OpenInputDialog {
                    id,
                    title,
                    placeholder,
                    initial,
                } => {
                    let mut runtime = self
                        .extension_pane_runtime
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    runtime.menu_open = false;
                    runtime.select_dialog = None;
                    runtime.confirm_dialog = None;
                    runtime.input_dialog = Some(ExtensionInputDialog {
                        extension_index,
                        extension_id: extension_id.into(),
                        action_id: id,
                        title: sanitize_terminal_line(&title),
                        placeholder: sanitize_terminal_line(&placeholder),
                        value: initial
                            .map(|value| sanitize_terminal_line(&value))
                            .unwrap_or_default(),
                    });
                }
                ExtensionHostAction::OpenSelectDialog { id, title, options } => {
                    let options = options
                        .into_iter()
                        .map(|option| {
                            let option = sanitize_terminal_line(&option);
                            if option.trim().is_empty() {
                                "(empty option)".into()
                            } else {
                                option
                            }
                        })
                        .collect();
                    let mut runtime = self
                        .extension_pane_runtime
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    runtime.menu_open = false;
                    runtime.input_dialog = None;
                    runtime.confirm_dialog = None;
                    runtime.select_dialog = Some(ExtensionSelectDialog {
                        extension_index,
                        extension_id: extension_id.into(),
                        action_id: id,
                        title: sanitize_terminal_line(&title),
                        options,
                        selected: 0,
                    });
                }
                ExtensionHostAction::OpenConfirmDialog {
                    id,
                    title,
                    body,
                    confirm_label,
                } => {
                    let mut runtime = self
                        .extension_pane_runtime
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    runtime.menu_open = false;
                    runtime.input_dialog = None;
                    runtime.select_dialog = None;
                    runtime.confirm_dialog = Some(ExtensionConfirmDialog {
                        extension_index,
                        extension_id: extension_id.into(),
                        action_id: id,
                        title: sanitize_terminal_line(&title),
                        body: sanitize_terminal_line(&body),
                        confirm_label: sanitize_terminal_line(&confirm_label),
                    });
                }
                ExtensionHostAction::EmitEvent { name, payload } => {
                    self.publish_extension_event(&name, payload);
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

    fn publish_extension_event(&mut self, name: &str, payload: serde_json::Value) {
        const MAX_EVENT_DISPATCH_DEPTH: usize = 16;
        if self.extension_event_dispatch_depth >= MAX_EVENT_DISPATCH_DEPTH {
            self.status = Some(format!(
                "extension event {name} exceeded the {MAX_EVENT_DISPATCH_DEPTH}-event recursion limit"
            ));
            return;
        }
        let targets = {
            let runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            runtime
                .extensions
                .iter()
                .enumerate()
                .filter(|(_, extension)| extension.subscribes_to_event(name))
                .map(|(index, extension)| (index, extension.manifest.id.clone()))
                .collect::<Vec<_>>()
        };
        if targets.is_empty() {
            return;
        }
        let (snapshot, review) =
            self.with_state(|state| (state.snapshot(), build_extension_review_snapshot(state)));
        self.extension_event_dispatch_depth += 1;
        for (extension_index, extension_id) in targets {
            let execution = {
                let mut runtime = self
                    .extension_pane_runtime
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                runtime.extensions[extension_index].deliver_event(ReviewEvent {
                    name: name.into(),
                    snapshot: snapshot.clone(),
                    payload: payload.clone(),
                    review: Some(review.clone()),
                })
            };
            match execution {
                Ok(execution) => {
                    self.apply_extension_actions(extension_index, &extension_id, execution.actions)
                }
                Err(error) => {
                    self.status = Some(format!("extension {extension_id} event failed: {error}"));
                }
            }
        }
        self.extension_event_dispatch_depth -= 1;
    }

    fn publish_extension_selection_events(&mut self) {
        let (file_id, hunk_index) = self.with_state(|state| {
            let selection = state.selection();
            (
                state
                    .changeset()
                    .files
                    .get(selection.file_index)
                    .map(|file| file.runtime_id.clone()),
                selection.hunk_index,
            )
        });
        self.publish_extension_event(
            "selection_changed",
            serde_json::json!({ "fileId": file_id, "hunkIndex": hunk_index }),
        );
        if let (Some(file_id), Some(hunk_index)) = (file_id, hunk_index) {
            self.publish_extension_event(
                "hunk_viewed",
                serde_json::json!({ "fileId": file_id, "hunkIndex": hunk_index }),
            );
        }
    }

    fn sync_extension_note_events(&mut self) {
        let (current_ids, created) = self.with_state(|state| {
            let current_ids = state
                .comments()
                .iter()
                .map(|comment| comment.id.clone())
                .collect::<BTreeSet<_>>();
            let created = state
                .comments()
                .iter()
                .filter(|comment| !self.extension_known_note_ids.contains(&comment.id))
                .map(|comment| {
                    let file_id = state
                        .changeset()
                        .files
                        .iter()
                        .find(|file| file.key == comment.anchor.file_key)
                        .map(|file| file.runtime_id.clone());
                    serde_json::json!({
                        "noteId": comment.id,
                        "fileId": file_id,
                        "hunkIndex": comment.anchor.owner_hunk_index,
                    })
                })
                .collect::<Vec<_>>();
            (current_ids, created)
        });
        self.extension_known_note_ids = current_ids;
        for payload in created {
            self.publish_extension_event("note_created", payload);
        }
    }

    /// Inform subscribed extensions that watch mode has observed a source change.
    pub fn notify_watch_reload_pending(&mut self) {
        self.publish_extension_event("watch_reload_pending", serde_json::json!({}));
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
        self.exit_active_file_view_mode();
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
            if let Some(active) = &active
                && runtime
                    .select_dialog
                    .as_ref()
                    .is_some_and(|dialog| dialog.extension_index == active.extension_index)
            {
                runtime.select_dialog = None;
            }
            if let Some(active) = &active
                && runtime
                    .confirm_dialog
                    .as_ref()
                    .is_some_and(|dialog| dialog.extension_index == active.extension_index)
            {
                runtime.confirm_dialog = None;
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

    fn open_workspace_write_dialog(
        &mut self,
        extension_index: usize,
        extension_id: &str,
        request_id: String,
        file_id: String,
        text: String,
    ) {
        let path = self.with_state(|state| {
            state
                .changeset()
                .files
                .iter()
                .find(|file| file.runtime_id == file_id)
                .map(|file| file.path.clone())
        });
        let Some(path) = path else {
            self.complete_extension_workspace_write(
                extension_index,
                extension_id,
                request_id,
                ExtensionWorkspaceWriteResult::Failed {
                    detail: "Reviewed file is no longer available".into(),
                },
            );
            return;
        };
        let active_owns_file = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_file_view_mode
            .as_ref()
            .is_some_and(|active| {
                active.extension_index == extension_index && active.file.id == file_id
            });
        if !active_owns_file {
            self.complete_extension_workspace_write(
                extension_index,
                extension_id,
                request_id,
                ExtensionWorkspaceWriteResult::Failed {
                    detail: format!("Failed to write {path} • its edit mode is no longer active"),
                },
            );
            return;
        }
        let mut runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        runtime.menu_open = false;
        runtime.input_dialog = None;
        runtime.select_dialog = None;
        runtime.confirm_dialog = None;
        runtime.workspace_write_dialog = Some(ExtensionWorkspaceWriteDialog {
            extension_index,
            extension_id: extension_id.into(),
            request_id,
            file_id,
            path: path.clone(),
            text,
        });
        self.status = Some(format!(
            "ext {extension_id}: write {path}? Enter confirms · Esc cancels"
        ));
    }

    fn handle_workspace_write_key(&mut self, key: &KeyEvent) -> bool {
        let has_dialog = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .workspace_write_dialog
            .is_some();
        if !has_dialog {
            return false;
        }
        let confirmed = match key.code {
            KeyCode::Enter | KeyCode::Char('y') => Some(true),
            KeyCode::Esc | KeyCode::Char('n') => Some(false),
            _ => None,
        };
        let Some(confirmed) = confirmed else {
            return true;
        };
        let dialog = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .workspace_write_dialog
            .take();
        let Some(dialog) = dialog else {
            return true;
        };
        let result = if confirmed {
            match self.write_extension_workspace_document(&dialog.file_id, &dialog.text) {
                Ok(()) => ExtensionWorkspaceWriteResult::Written,
                Err(detail) => ExtensionWorkspaceWriteResult::Failed { detail },
            }
        } else {
            ExtensionWorkspaceWriteResult::Cancelled
        };
        let written = result == ExtensionWorkspaceWriteResult::Written;
        self.complete_extension_workspace_write(
            dialog.extension_index,
            &dialog.extension_id,
            dialog.request_id,
            result,
        );
        if written {
            self.exit_file_view_mode_for_extension(dialog.extension_index);
            self.reload_requested = true;
        }
        true
    }

    fn complete_extension_workspace_write(
        &mut self,
        extension_index: usize,
        extension_id: &str,
        request_id: String,
        result: ExtensionWorkspaceWriteResult,
    ) {
        let execution = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extensions[extension_index]
            .complete_workspace_write(ExtensionWorkspaceWriteCompletion { request_id, result });
        match execution {
            Ok(execution) => {
                self.apply_extension_actions(extension_index, extension_id, execution.actions)
            }
            Err(error) => {
                self.status = Some(format!(
                    "extension {extension_id} workspace write completion failed: {error}"
                ));
            }
        }
    }

    fn write_extension_workspace_document(
        &self,
        file_id: &str,
        replacement: &str,
    ) -> Result<(), String> {
        let (source, file) = self.with_state(|state| {
            (
                state.changeset().source.clone(),
                state
                    .changeset()
                    .files
                    .iter()
                    .find(|file| file.runtime_id == file_id)
                    .cloned(),
            )
        });
        let Some(file) = file else {
            return Err("Reviewed file is no longer available".into());
        };
        if !matches!(source, ChangesetSource::WorkingTree { staged: false })
            || !file.sources.new.as_ref().is_some_and(|snapshot| {
                matches!(snapshot.origin, SourceOrigin::WorkingTree) && snapshot.attested
            })
        {
            return Err(format!(
                "Failed to write {} • this review is not an attested working-tree diff",
                file.path
            ));
        }
        let relative = Path::new(&file.path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(format!(
                "Failed to write {} • unsafe reviewed path",
                file.path
            ));
        }
        let root = self.options.repo.as_ref().ok_or_else(|| {
            format!(
                "Failed to write {} • repository root is unavailable",
                file.path
            )
        })?;
        let root = std::fs::canonicalize(root)
            .map_err(|error| format!("Failed to write {} • {error}", file.path))?;
        let target = root.join(relative);
        let parent = target
            .parent()
            .and_then(|parent| std::fs::canonicalize(parent).ok())
            .ok_or_else(|| {
                format!(
                    "Failed to write {} • parent directory is unavailable",
                    file.path
                )
            })?;
        if !parent.starts_with(&root)
            || std::fs::symlink_metadata(&target)
                .map(|metadata| metadata.file_type().is_symlink() || !metadata.is_file())
                .unwrap_or(true)
        {
            return Err(format!(
                "Failed to write {} • path leaves the repository",
                file.path
            ));
        }
        let expected = &file.sources.new.as_ref().expect("checked above").content;
        let current = std::fs::read_to_string(&target)
            .map_err(|error| format!("Failed to write {} • {error}", file.path))?;
        if &current != expected {
            return Err(format!(
                "Failed to write {} • file changed since the review was loaded",
                file.path
            ));
        }
        std::fs::write(&target, replacement)
            .map_err(|error| format!("Failed to write {} • {error}", file.path))
    }

    fn handle_extension_confirm_key(&mut self, key: &KeyEvent) -> bool {
        let has_dialog = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .confirm_dialog
            .is_some();
        if !has_dialog {
            return false;
        }
        let confirmed = match key.code {
            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => Some(true),
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => Some(false),
            _ => None,
        };
        if let Some(confirmed) = confirmed {
            let dialog = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .confirm_dialog
                .take();
            if let Some(dialog) = dialog {
                self.submit_extension_confirm(dialog, confirmed);
            }
        }
        true
    }

    fn handle_extension_select_key(&mut self, key: &KeyEvent) -> bool {
        let has_dialog = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .select_dialog
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
                    .select_dialog
                    .take();
                if let Some(dialog) = dialog {
                    let value = (key.code == KeyCode::Enter)
                        .then(|| dialog.options[dialog.selected].clone());
                    self.submit_extension_select(dialog, value);
                }
            }
            KeyCode::Down | KeyCode::Tab | KeyCode::Char('j') => {
                let mut runtime = self
                    .extension_pane_runtime
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if let Some(dialog) = &mut runtime.select_dialog {
                    dialog.selected = dialog
                        .selected
                        .saturating_add(1)
                        .min(dialog.options.len().saturating_sub(1));
                }
            }
            KeyCode::Up | KeyCode::BackTab | KeyCode::Char('k') => {
                let mut runtime = self
                    .extension_pane_runtime
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if let Some(dialog) = &mut runtime.select_dialog {
                    dialog.selected = dialog.selected.saturating_sub(1);
                }
            }
            KeyCode::Home => {
                if let Some(dialog) = &mut self
                    .extension_pane_runtime
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .select_dialog
                {
                    dialog.selected = 0;
                }
            }
            KeyCode::End => {
                if let Some(dialog) = &mut self
                    .extension_pane_runtime
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .select_dialog
                {
                    dialog.selected = dialog.options.len().saturating_sub(1);
                }
            }
            _ => {}
        }
        true
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
        let (snapshot, review) =
            self.with_state(|state| (state.snapshot(), build_extension_review_snapshot(state)));
        let cwd = self.extension_command_cwd();
        let execution = {
            let mut runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let active_keyboard_mode = runtime
                .active_keyboard_mode
                .as_ref()
                .map(|active| format!("{}:{}", active.extension_id, active.mode.id))
                .or_else(|| {
                    runtime
                        .active_file_view_mode
                        .as_ref()
                        .map(|active| format!("{}:{}", active.extension_id, active.view_id))
                });
            runtime.extensions[dialog.extension_index].submit_input_dialog_with_context(
                &dialog.action_id,
                value,
                snapshot,
                active_keyboard_mode,
                cwd,
                Some(review),
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

    fn submit_extension_select(&mut self, dialog: ExtensionSelectDialog, value: Option<String>) {
        let (snapshot, review) =
            self.with_state(|state| (state.snapshot(), build_extension_review_snapshot(state)));
        let cwd = self.extension_command_cwd();
        let execution = {
            let mut runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let active_keyboard_mode = runtime
                .active_keyboard_mode
                .as_ref()
                .map(|active| format!("{}:{}", active.extension_id, active.mode.id))
                .or_else(|| {
                    runtime
                        .active_file_view_mode
                        .as_ref()
                        .map(|active| format!("{}:{}", active.extension_id, active.view_id))
                });
            runtime.extensions[dialog.extension_index].submit_select_dialog_with_context(
                &dialog.action_id,
                value,
                snapshot,
                active_keyboard_mode,
                cwd,
                Some(review),
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
                    "extension {} selection failed: {error}",
                    dialog.extension_id
                ));
            }
        }
    }

    fn submit_extension_confirm(&mut self, dialog: ExtensionConfirmDialog, confirmed: bool) {
        let (snapshot, review) =
            self.with_state(|state| (state.snapshot(), build_extension_review_snapshot(state)));
        let cwd = self.extension_command_cwd();
        let execution = {
            let mut runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let active_keyboard_mode = runtime
                .active_keyboard_mode
                .as_ref()
                .map(|active| format!("{}:{}", active.extension_id, active.mode.id))
                .or_else(|| {
                    runtime
                        .active_file_view_mode
                        .as_ref()
                        .map(|active| format!("{}:{}", active.extension_id, active.view_id))
                });
            runtime.extensions[dialog.extension_index].submit_confirm_dialog_with_context(
                &dialog.action_id,
                confirmed,
                snapshot,
                active_keyboard_mode,
                cwd,
                Some(review),
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
                    "extension {} confirmation failed: {error}",
                    dialog.extension_id
                ));
            }
        }
    }

    fn execute_extension_review_command(&mut self, id: &str, count: u16) -> bool {
        if matches!(
            id,
            "workdeck.review.align-current-line-top"
                | "workdeck.review.align-current-line-center"
                | "workdeck.review.align-current-line-bottom"
        ) && self.options.cursor_line == CursorLineMode::Off
        {
            return false;
        }
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
            _ => return false,
        }
        true
    }

    fn extension_review_file_index(&self, file_id: &str) -> Option<usize> {
        self.with_state(|state| {
            state
                .changeset()
                .files
                .iter()
                .position(|file| file.runtime_id == file_id)
        })
    }

    fn select_extension_review_file(&mut self, extension_id: &str, file_id: &str) {
        let Some(file_index) = self.extension_review_file_index(file_id) else {
            self.status = Some(format!(
                "extension {extension_id}: warning: review file is no longer available"
            ));
            return;
        };
        if self
            .with_state(|state| state.select_file(file_index))
            .is_ok()
        {
            self.reconcile_active_file_view_mode();
            self.scroll_to_selection();
            self.publish_extension_selection_events();
        }
    }

    fn toggle_extension_file_view(
        &mut self,
        extension_index: usize,
        extension_id: &str,
        view_id: &str,
    ) {
        let file = self.with_state(|state| {
            state
                .changeset()
                .files
                .get(state.selection().file_index)
                .cloned()
        });
        let Some(file) = file else {
            self.status = Some(format!("extension {extension_id} has no selected file"));
            return;
        };
        let replaces_active_mode = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_file_view_mode
            .as_ref()
            .is_some_and(|active| active.file.id == file.runtime_id);
        if replaces_active_mode {
            self.exit_active_file_view_mode();
        }
        let mut runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(registration) = runtime.file_views.iter().find(|registration| {
            registration.extension_index == extension_index && registration.view.view_id == view_id
        }) else {
            self.status = Some(format!(
                "extension {extension_id} targeted unknown file view {view_id:?}"
            ));
            return;
        };
        let view_key = registered_file_view_key(&registration.view);
        if runtime.file_view_selections.get(&file.runtime_id) == Some(view_key.as_str()) {
            runtime.file_view_selections =
                select_file_view(&runtime.file_view_selections, &file.runtime_id, None);
            runtime.file_view_layouts.remove(&file.runtime_id);
            clear_file_view_component_state(&mut runtime, &file.runtime_id);
            self.status = Some("file presentation: raw diff".into());
            return;
        }
        let snapshot = create_file_view_input_snapshot(&file);
        match runtime.extensions[extension_index]
            .file_view_matches(view_id, snapshot.file.as_ref().clone())
        {
            Ok(true) => {
                runtime.file_view_selections = select_file_view(
                    &runtime.file_view_selections,
                    &file.runtime_id,
                    Some(&view_key),
                );
                runtime.file_view_layouts.remove(&file.runtime_id);
                clear_file_view_component_state(&mut runtime, &file.runtime_id);
                self.status = Some(format!("file presentation: {view_key}"));
            }
            Ok(false) => {
                self.status = Some(format!(
                    "file view {view_id:?} does not match {} • using raw diff",
                    file.path
                ));
            }
            Err(error) => {
                self.status = Some(format!(
                    "extension {extension_id} file view match failed: {error}"
                ));
            }
        }
    }

    fn enter_file_view_mode(&mut self, extension_index: usize, extension_id: &str, view_id: &str) {
        let (file, review_generation) = self.with_state(|state| {
            (
                state
                    .changeset()
                    .files
                    .get(state.selection().file_index)
                    .cloned(),
                state.generation(),
            )
        });
        let Some(file) = file else {
            self.status = Some(format!("extension {extension_id} has no selected file"));
            return;
        };
        let registration = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .file_views
            .iter()
            .find(|registration| {
                registration.extension_index == extension_index
                    && registration.view.view_id == view_id
            })
            .cloned();
        let Some(registration) = registration else {
            self.status = Some(format!(
                "extension {extension_id} targeted unknown file view {view_id:?}"
            ));
            return;
        };
        if !registration.view.interactive_mode {
            self.status = Some(format!(
                "extension {extension_id} file view {view_id:?} has no interactive mode"
            ));
            return;
        }
        let snapshot = create_file_view_input_snapshot(&file);
        let matches = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extensions[extension_index]
            .file_view_matches(view_id, snapshot.file.as_ref().clone());
        match matches {
            Ok(true) => {}
            Ok(false) => {
                self.status = Some(format!(
                    "file view {view_id:?} does not match {} • using raw diff",
                    file.path
                ));
                return;
            }
            Err(error) => {
                self.status = Some(format!(
                    "extension {extension_id} file view match failed: {error}"
                ));
                return;
            }
        }

        self.exit_active_keyboard_mode();
        self.exit_active_file_view_mode();
        let view_key = registered_file_view_key(&registration.view);
        let active = ActiveFileViewModeRuntime {
            extension_index,
            extension_id: extension_id.into(),
            view_id: view_id.into(),
            view_key: view_key.clone(),
            file: snapshot.file,
            review_generation,
        };
        {
            let mut runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            runtime.file_view_selections = select_file_view(
                &runtime.file_view_selections,
                &file.runtime_id,
                Some(&view_key),
            );
            runtime.file_view_layouts.remove(&file.runtime_id);
            clear_file_view_component_state(&mut runtime, &file.runtime_id);
            runtime.active_file_view_mode = Some(active.clone());
        }
        let request = self.file_view_mode_lifecycle_request(&active);
        let execution = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extensions[extension_index]
            .file_view_mode_lifecycle("workdeck/file-view-mode/enter", request);
        match execution {
            Ok(execution) => {
                self.status = Some(format!("{extension_id}:{view_id} mode — Esc exits"));
                self.apply_extension_actions(extension_index, extension_id, execution.actions);
            }
            Err(error) => {
                self.extension_pane_runtime
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .active_file_view_mode = None;
                self.status = Some(format!(
                    "extension {extension_id} could not enter file-view mode: {error}"
                ));
            }
        }
    }

    fn file_view_mode_lifecycle_request(
        &self,
        active: &ActiveFileViewModeRuntime,
    ) -> FileViewModeLifecycleRequest {
        FileViewModeLifecycleRequest {
            view_id: active.view_id.clone(),
            file: active.file.as_ref().clone(),
            cwd: self.extension_command_cwd(),
            review_generation: active.review_generation,
        }
    }

    fn route_active_file_view_mode(&mut self, key: &KeyEvent) -> bool {
        let active = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_file_view_mode
            .clone();
        let Some(active) = active else {
            return false;
        };
        if key.code == KeyCode::Esc {
            self.exit_active_file_view_mode();
            return true;
        }
        let still_valid = self.with_state(|state| {
            state.generation() == active.review_generation
                && state
                    .selected_file()
                    .is_some_and(|file| file.runtime_id == active.file.id)
        }) && self
            .selected_extension_file_view(&active.file.id)
            .as_deref()
            == Some(active.view_key.as_str());
        if !still_valid {
            self.exit_active_file_view_mode();
            return false;
        }
        let request = FileViewModeKeyRequest {
            view_id: active.view_id.clone(),
            file: active.file.as_ref().clone(),
            key: to_live_extension_key_event(key),
            cwd: self.extension_command_cwd(),
            review_generation: active.review_generation,
        };
        let result = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extensions[active.extension_index]
            .route_file_view_mode_key(request);
        match result {
            Ok(execution) => {
                let routing = execution.result;
                self.apply_extension_actions(
                    active.extension_index,
                    &active.extension_id,
                    execution.actions,
                );
                if routing == KeyRoutingResult::Exit {
                    let unchanged = self
                        .extension_pane_runtime
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .active_file_view_mode
                        .as_ref()
                        .is_some_and(|current| {
                            current.extension_index == active.extension_index
                                && current.view_id == active.view_id
                                && current.file.id == active.file.id
                        });
                    if unchanged {
                        self.exit_active_file_view_mode();
                    }
                }
                routing != KeyRoutingResult::Pass
            }
            Err(error) => {
                self.exit_active_file_view_mode();
                self.status = Some(format!(
                    "extension {} file-view mode failed: {error}",
                    active.extension_id
                ));
                true
            }
        }
    }

    fn exit_active_file_view_mode(&mut self) {
        let active = {
            let mut runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let active = runtime.active_file_view_mode.take();
            if let Some(active) = &active
                && runtime
                    .workspace_write_dialog
                    .as_ref()
                    .is_some_and(|dialog| {
                        dialog.extension_index == active.extension_index
                            && dialog.file_id == active.file.id
                    })
            {
                runtime.workspace_write_dialog = None;
            }
            active
        };
        let Some(active) = active else {
            return;
        };
        let request = self.file_view_mode_lifecycle_request(&active);
        let execution = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extensions[active.extension_index]
            .file_view_mode_lifecycle("workdeck/file-view-mode/exit", request);
        match execution {
            Ok(execution) => self.apply_extension_actions(
                active.extension_index,
                &active.extension_id,
                execution.actions,
            ),
            Err(error) => {
                self.status = Some(format!(
                    "extension {} file-view mode exit failed: {error}",
                    active.extension_id
                ));
            }
        }
    }

    fn exit_file_view_mode_for_extension(&mut self, extension_index: usize) {
        let owns = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_file_view_mode
            .as_ref()
            .is_some_and(|active| active.extension_index == extension_index);
        if owns {
            self.exit_active_file_view_mode();
        }
    }

    fn refresh_extension_file_view(
        &mut self,
        extension_index: usize,
        extension_id: &str,
        view_id: &str,
        file_id: Option<&str>,
    ) {
        let mut runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let owned = runtime.file_views.iter().any(|registration| {
            registration.extension_index == extension_index && registration.view.view_id == view_id
        });
        if !owned {
            self.status = Some(format!(
                "extension {extension_id} targeted unknown file view {view_id:?}"
            ));
            return;
        }
        if let Some(file_id) = file_id {
            runtime.file_view_layouts.remove(file_id);
            clear_file_view_component_state(&mut runtime, file_id);
        } else {
            let view_key = format!("{extension_id}:{view_id}");
            let invalidated = runtime
                .file_view_layouts
                .iter()
                .filter_map(|(file_id, layout)| {
                    (layout.view_key == view_key).then_some(file_id.clone())
                })
                .collect::<Vec<_>>();
            for file_id in invalidated {
                runtime.file_view_layouts.remove(&file_id);
                clear_file_view_component_state(&mut runtime, &file_id);
            }
        }
    }

    #[must_use]
    pub fn selected_extension_file_view(&self, file_id: &str) -> Option<String> {
        self.extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .file_view_selections
            .get(file_id)
            .map(str::to_owned)
    }

    fn prepare_extension_file_view_layouts(
        &self,
        changeset: &Changeset,
        width: u16,
    ) -> BTreeMap<String, ValidatedFileViewLayout> {
        let width = usize::from(width.max(1));
        let mut runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let selections = runtime.file_view_selections.entries().clone();
        let registrations = runtime.file_views.clone();
        let mut prepared = BTreeMap::new();
        for file in &changeset.files {
            let Some(view_key) = selections.get(&file.runtime_id) else {
                continue;
            };
            if let Some(cached) = runtime.file_view_layouts.get(&file.runtime_id)
                && cached.view_key == *view_key
                && cached.content_identity == file.content_identity
                && cached.width == width
            {
                prepared.insert(file.runtime_id.clone(), cached.layout.clone());
                continue;
            }
            clear_file_view_component_state(&mut runtime, &file.runtime_id);
            let Some(registration) = registrations
                .iter()
                .find(|registration| registered_file_view_key(&registration.view) == *view_key)
            else {
                continue;
            };
            let input =
                create_file_view_input(file, width, ExtensionRequestCancellation::default(), None);
            match runtime.extensions[registration.extension_index]
                .layout_file_view(&registration.view.view_id, input)
            {
                Ok(Some(layout)) => {
                    runtime.file_view_layouts.insert(
                        file.runtime_id.clone(),
                        CachedFileViewLayout {
                            view_key: view_key.clone(),
                            content_identity: file.content_identity.clone(),
                            width,
                            layout: layout.clone(),
                        },
                    );
                    prepared.insert(file.runtime_id.clone(), layout);
                }
                Ok(None) | Err(_) => {
                    runtime.file_view_layouts.remove(&file.runtime_id);
                }
            }
        }
        prepared
    }

    /// Expose ephemeral component paint state for executable extension parity tests.
    #[must_use]
    pub fn extension_file_view_component_expanded(&self, file_id: &str, row_id: &str) -> bool {
        self.extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .file_view_component_expanded
            .contains(&FileViewComponentStateKey {
                file_id: file_id.into(),
                row_id: row_id.into(),
            })
    }

    /// Return the currently mounted component rectangle for terminal-level parity tests.
    #[doc(hidden)]
    #[must_use]
    pub fn extension_file_view_component_bounds(
        &self,
        file_id: &str,
        row_id: &str,
    ) -> Option<(u16, u16, u16, u16)> {
        self.extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .file_view_component_hits
            .iter()
            .find(|hit| hit.state_key.file_id == file_id && hit.state_key.row_id == row_id)
            .map(|hit| {
                (
                    hit.bounds.x,
                    hit.bounds.y,
                    hit.bounds.width,
                    hit.bounds.height,
                )
            })
    }

    fn select_extension_review_hunk(
        &mut self,
        extension_id: &str,
        file_id: &str,
        hunk_index: usize,
    ) {
        let Some(file_index) = self.extension_review_file_index(file_id) else {
            self.status = Some(format!(
                "extension {extension_id}: warning: review file is no longer available"
            ));
            return;
        };
        if let Err(error) = self.with_state(|state| state.select_hunk(file_index, hunk_index)) {
            self.status = Some(format!(
                "extension {extension_id}: warning: review target is unavailable: {error}"
            ));
            return;
        }
        self.scroll_to_selection();
        self.publish_extension_selection_events();
    }

    fn reveal_extension_review_line(
        &mut self,
        extension_id: &str,
        file_id: &str,
        side: ReviewSide,
        line: u32,
    ) {
        let Some(file_index) = self.extension_review_file_index(file_id) else {
            self.status = Some(format!(
                "extension {extension_id}: warning: review file is no longer available"
            ));
            return;
        };
        if let Err(error) = self.with_state(|state| state.reveal_line(file_index, side, line)) {
            self.status = Some(format!(
                "extension {extension_id}: warning: review line is unavailable: {error}"
            ));
            return;
        }
        self.scroll_to_selection();
        self.publish_extension_selection_events();
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
        let file_view_layouts = self.prepare_extension_file_view_layouts(state.changeset(), width);
        let component_expanded = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .file_view_component_expanded
            .clone();
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
            &file_view_layouts,
            &component_expanded,
        )
    }

    fn extension_command_cwd(&self) -> PathBuf {
        self.options
            .command_cwd
            .clone()
            .or_else(|| self.options.repo.clone())
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default()
    }

    fn handle_extension_menu_key(&mut self, key: &KeyEvent) -> bool {
        let mut runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let menu_shortcut = key.code == KeyCode::Menu
            || (key.code == KeyCode::Char('e') && key.modifiers == KeyModifiers::ALT);
        let has_exit =
            runtime.active_keyboard_mode.is_some() || runtime.active_file_view_mode.is_some();
        let entry_count = runtime.commands.len() + usize::from(has_exit);
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
                let has_exit = runtime.active_keyboard_mode.is_some()
                    || runtime.active_file_view_mode.is_some();
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
                    self.exit_active_extension_mode();
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
        let file_view_layouts = self.prepare_extension_file_view_layouts(state.changeset(), width);
        let component_expanded = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .file_view_component_expanded
            .clone();
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
            &file_view_layouts,
            &component_expanded,
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
            self.reconcile_active_file_view_mode();
            self.scroll_to_selection();
            self.publish_extension_selection_events();
        }
    }

    fn reconcile_active_file_view_mode(&mut self) {
        let active_file_id = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_file_view_mode
            .as_ref()
            .map(|active| active.file.id.clone());
        if active_file_id.is_some_and(|file_id| {
            !self.with_state(|state| {
                state
                    .selected_file()
                    .is_some_and(|file| file.runtime_id == file_id)
            })
        }) {
            self.exit_active_file_view_mode();
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
        if self.has_extension_dialog() {
            return;
        }
        if self.handle_extension_mode_badge_mouse(&event)
            || self.handle_extension_menu_mouse(&event)
            || self.handle_extension_pane_mouse(&event)
            || self.handle_extension_file_view_mouse(&event)
            || self.handle_sidebar_mouse(&event)
        {
            return;
        }
        self.handle_mouse_at(event.kind, Instant::now());
    }

    fn handle_sidebar_mouse(&mut self, event: &MouseEvent) -> bool {
        if !self
            .sidebar_bounds
            .get()
            .is_some_and(|area| rect_contains(area, event.column, event.row))
        {
            return false;
        }
        match event.kind {
            MouseEventKind::ScrollDown => {
                self.sidebar_scroll_top
                    .set(self.sidebar_scroll_top.get().saturating_add(1));
                true
            }
            MouseEventKind::ScrollUp => {
                self.sidebar_scroll_top
                    .set(self.sidebar_scroll_top.get().saturating_sub(1));
                true
            }
            MouseEventKind::Up(MouseButton::Left) => {
                let target = self
                    .sidebar_file_hits
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .iter()
                    .rev()
                    .find(|hit| rect_contains(hit.bounds, event.column, event.row))
                    .map(|hit| hit.file_index);
                if let Some(file_index) = target
                    && self
                        .with_state(|state| state.select_file(file_index))
                        .is_ok()
                {
                    self.reconcile_active_file_view_mode();
                    self.scroll_to_selection();
                    self.publish_extension_selection_events();
                }
                true
            }
            _ => false,
        }
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
            self.exit_active_extension_mode();
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
        let has_exit =
            runtime.active_keyboard_mode.is_some() || runtime.active_file_view_mode.is_some();
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
            self.exit_active_extension_mode();
        } else if let Some(command) = command {
            self.invoke_registered_extension_command(command);
        }
        true
    }

    fn exit_active_extension_mode(&mut self) {
        let has_file_view_mode = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_file_view_mode
            .is_some();
        if has_file_view_mode {
            self.exit_active_file_view_mode();
        } else {
            self.exit_active_keyboard_mode();
        }
    }

    fn handle_extension_pane_mouse(&mut self, event: &MouseEvent) -> bool {
        let mut runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if event.kind == MouseEventKind::Down(MouseButton::Left)
            && let Some(hit) = runtime
                .pane_action_hits
                .iter()
                .rev()
                .find(|hit| rect_contains(hit.bounds, event.column, event.row))
                .cloned()
        {
            drop(runtime);
            self.invoke_extension_pane_action(hit);
            return true;
        }
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
                runtime.resize.capture((key, resize));
                true
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let Some((key, resize)) = runtime.resize.as_ref().cloned() else {
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
                runtime.resize.release();
                true
            }
            _ => false,
        }
    }

    fn handle_extension_file_view_mouse(&mut self, event: &MouseEvent) -> bool {
        let mut runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let pointer = runtime
                    .file_view_component_hits
                    .iter()
                    .find(|hit| rect_contains(hit.bounds, event.column, event.row))
                    .map(|hit| FileViewComponentPointer {
                        state_key: hit.state_key.clone(),
                        dragged: false,
                    });
                runtime.file_view_component_pointer.set_captured(pointer);
                false
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(pointer) = runtime.file_view_component_pointer.as_mut() {
                    pointer.dragged = true;
                }
                false
            }
            MouseEventKind::Up(MouseButton::Left) => {
                let Some(pointer) = runtime.file_view_component_pointer.take() else {
                    return false;
                };
                if pointer.dragged
                    || !runtime.file_view_component_hits.iter().any(|hit| {
                        hit.state_key == pointer.state_key
                            && rect_contains(hit.bounds, event.column, event.row)
                    })
                {
                    return false;
                }
                if !runtime
                    .file_view_component_expanded
                    .remove(&pointer.state_key)
                {
                    runtime
                        .file_view_component_expanded
                        .insert(pointer.state_key);
                }
                true
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                runtime.file_view_component_pointer.release();
                false
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

fn clear_file_view_component_state(runtime: &mut ExtensionPaneRuntime, file_id: &str) {
    runtime
        .file_view_component_expanded
        .retain(|key| key.file_id != file_id);
    runtime
        .file_view_component_hits
        .retain(|hit| hit.state_key.file_id != file_id);
    if runtime
        .file_view_component_pointer
        .as_ref()
        .is_some_and(|pointer| pointer.state_key.file_id == file_id)
    {
        runtime.file_view_component_pointer.release();
    }
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
                app.notify_watch_reload_pending();
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
    render_extension_select_dialog(area, buffer, app);
    render_extension_confirm_dialog(area, buffer, app);
    render_extension_workspace_write_dialog(area, buffer, app);
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
    if (runtime.commands.is_empty()
        && runtime.active_keyboard_mode.is_none()
        && runtime.active_file_view_mode.is_none())
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
    if !runtime.menu_open
        || (runtime.commands.is_empty()
            && runtime.active_keyboard_mode.is_none()
            && runtime.active_file_view_mode.is_none())
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
    } else if let Some(active) = &runtime.active_file_view_mode {
        labels.insert(0, format!("Exit {}", active.view_id));
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

/// Draw the host-owned selection modal requested by a native extension.
pub fn render_extension_select_dialog(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    let dialog = app
        .extension_pane_runtime
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .select_dialog
        .clone();
    let Some(dialog) = dialog else {
        return;
    };
    let desired_width = dialog
        .options
        .iter()
        .map(|option| option.width().saturating_add(4))
        .max()
        .unwrap_or(24)
        .max(dialog.title.width().saturating_add(4));
    let width = u16::try_from(desired_width)
        .unwrap_or(u16::MAX)
        .max(24)
        .min(area.width.max(1));
    let desired_height = u16::try_from(dialog.options.len())
        .unwrap_or(u16::MAX)
        .saturating_add(2);
    let height = desired_height.max(3).min(area.height.max(1));
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
    let visible = usize::from(inner.height);
    let first = dialog
        .selected
        .saturating_add(1)
        .saturating_sub(visible)
        .min(dialog.options.len().saturating_sub(visible));
    let lines = dialog
        .options
        .iter()
        .enumerate()
        .skip(first)
        .take(visible)
        .map(|(index, option)| {
            let selected = index == dialog.selected;
            Line::styled(
                format!("{} {option}", if selected { "›" } else { " " }),
                if selected {
                    Style::default()
                        .fg(ratatui_theme_color(&app.options.theme.background))
                        .bg(ratatui_theme_color(&app.options.theme.accent))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(ratatui_theme_color(&app.options.theme.text))
                },
            )
        })
        .collect::<Vec<_>>();
    Paragraph::new(lines).render(inner, buffer);
}

/// Draw the host-owned confirmation modal requested by a native extension.
pub fn render_extension_confirm_dialog(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    let dialog = app
        .extension_pane_runtime
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .confirm_dialog
        .clone();
    let Some(dialog) = dialog else {
        return;
    };
    let help = format!("Enter/y {} · Esc/n cancels", dialog.confirm_label);
    let desired_width = dialog
        .title
        .width()
        .max(dialog.body.width())
        .max(help.width())
        .saturating_add(4);
    let width = u16::try_from(desired_width)
        .unwrap_or(u16::MAX)
        .max(30)
        .min(area.width.max(1));
    let height = 4.min(area.height.max(1));
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
    Paragraph::new(vec![
        Line::styled(
            dialog.body,
            Style::default().fg(ratatui_theme_color(&app.options.theme.text)),
        ),
        Line::styled(
            help,
            Style::default().fg(ratatui_theme_color(&app.options.theme.muted)),
        ),
    ])
    .render(inner, buffer);
}

/// Draw the host-owned consent prompt for a native extension workspace write.
pub fn render_extension_workspace_write_dialog(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    let dialog = app
        .extension_pane_runtime
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .workspace_write_dialog
        .clone();
    let Some(dialog) = dialog else {
        return;
    };
    let title = format!(" ext {} ", dialog.extension_id);
    let question = format!("Write {}?", dialog.path);
    let help = "Enter/y confirms · Esc/n cancels";
    let desired_width = title
        .width()
        .max(question.width())
        .max(help.width())
        .saturating_add(4);
    let width = u16::try_from(desired_width)
        .unwrap_or(u16::MAX)
        .max(30)
        .min(area.width.max(1));
    let height = 4.min(area.height.max(1));
    let bounds = Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    );
    Clear.render(bounds, buffer);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .style(Style::default().bg(ratatui_theme_color(&app.options.theme.panel)))
        .border_style(Style::default().fg(ratatui_theme_color(&app.options.theme.accent)));
    let inner = block.inner(bounds);
    block.render(bounds, buffer);
    Paragraph::new(vec![
        Line::styled(
            question,
            Style::default().fg(ratatui_theme_color(&app.options.theme.text)),
        ),
        Line::styled(
            help,
            Style::default().fg(ratatui_theme_color(&app.options.theme.muted)),
        ),
    ])
    .render(inner, buffer);
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
    runtime.pane_action_hits.clear();
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
                None,
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
        rendered_panes.push((
            view,
            planned.bounds,
            planned.divider,
            Some((
                registration.extension_index,
                registration.extension_id,
                registration.pane.id,
            )),
        ));
    }
    drop(runtime);

    render_builtin_body(plan.review_bounds, buffer, app);
    for (pane, pane_area, divider, owner) in rendered_panes {
        if let Some(divider) = divider {
            render_extension_pane_divider(divider, buffer, pane.pane.placement, app);
        }
        render_extension_pane(pane_area, buffer, &pane, owner, app);
    }
}

fn render_builtin_body(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    let state = app
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if state.changeset().is_empty() {
        app.sidebar_bounds.set(None);
        app.sidebar_file_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        let mut runtime = app
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        runtime.file_view_component_hits.clear();
        runtime.file_view_component_pointer.release();
        drop(runtime);
        Paragraph::new("No changes to review")
            .style(Style::default().fg(ratatui_theme_color(&app.options.theme.muted)))
            .block(Block::default().borders(Borders::TOP))
            .render(area, buffer);
        return;
    }
    let responsive = state.responsive_layout(area.width);
    drop(state);
    let sidebar_width = if app.options.sidebar && responsive.show_sidebar {
        bundled_sidebar_width(area.width, 30)
    } else {
        0
    };
    let chunks = if app.options.sidebar && responsive.show_sidebar {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(sidebar_width), Constraint::Min(30)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(0), Constraint::Min(1)])
            .split(area)
    };
    if chunks[0].width > 0 {
        render_sidebar(chunks[0], buffer, app);
    } else {
        app.sidebar_bounds.set(None);
        app.sidebar_file_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }
    render_review(chunks[1], buffer, app);
}

fn bundled_sidebar_width(total_width: u16, minimum_review_width: u16) -> u16 {
    let requested = extension_pane_size(bundled_files_pane(), None);
    let automatic = requested.fraction.map_or(requested.preferred, |fraction| {
        (f64::from(total_width) * fraction)
            .round()
            .clamp(0.0, f64::from(u16::MAX)) as u16
    });
    let minimum = requested.min.unwrap_or(1);
    let maximum = requested.max.unwrap_or(u16::MAX);
    let available = total_width.saturating_sub(minimum_review_width);
    if available < minimum {
        0
    } else {
        automatic.max(minimum).min(maximum).min(available)
    }
}

fn render_extension_pane(
    area: Rect,
    buffer: &mut Buffer,
    pane: &ExtensionPaneView,
    owner: Option<(usize, String, String)>,
    app: &ReviewApp,
) {
    let mut lines = Vec::new();
    let mut actions = Vec::new();
    flatten_view(&pane.content, 0, None, &mut lines, &mut actions);
    Block::default()
        .style(Style::default().bg(ratatui_theme_color(&app.options.theme.panel)))
        .render(area, buffer);
    if let Some((extension_index, extension_id, pane_id)) = owner {
        let mut y = area.y;
        let mut hits = Vec::new();
        for (line, action_id) in lines.iter().zip(&actions) {
            let height = extension_pane_line_height(line, area.width);
            let visible_height = height.min(area.bottom().saturating_sub(y));
            if let Some(action_id) = action_id
                && visible_height > 0
            {
                hits.push(ExtensionPaneActionHit {
                    bounds: Rect::new(area.x, y, area.width, visible_height),
                    extension_index,
                    extension_id: extension_id.clone(),
                    pane_id: pane_id.clone(),
                    action_id: action_id.clone(),
                });
            }
            y = y.saturating_add(height);
            if y >= area.bottom() {
                break;
            }
        }
        app.extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pane_action_hits
            .extend(hits);
    }
    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .render(area, buffer);
}

fn extension_pane_line_height(line: &Line<'_>, width: u16) -> u16 {
    let height = Paragraph::new(line.clone())
        .wrap(Wrap { trim: false })
        .line_count(width.max(1))
        .max(1);
    u16::try_from(height).unwrap_or(u16::MAX)
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

fn flatten_view(
    node: &ViewNode,
    indent: usize,
    action_id: Option<&str>,
    lines: &mut Vec<Line<'static>>,
    actions: &mut Vec<Option<String>>,
) {
    match node {
        ViewNode::Text { text, style } => {
            lines.push(Line::from(vec![
                Span::raw(" ".repeat(indent)),
                Span::styled(text.clone(), extension_style(style)),
            ]));
            actions.push(action_id.map(str::to_owned));
        }
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
            actions.push(action_id.map(str::to_owned));
        }
        ViewNode::Column { children, gap } => {
            for (index, child) in children.iter().enumerate() {
                if index > 0 {
                    for _ in 0..*gap {
                        lines.push(Line::default());
                        actions.push(action_id.map(str::to_owned));
                    }
                }
                flatten_view(child, indent, action_id, lines, actions);
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
                actions.push(action_id.map(str::to_owned));
                flatten_view(item, indent + 2, action_id, lines, actions);
            }
        }
        ViewNode::Action { id, child } => {
            flatten_view(child, indent, Some(id), lines, actions);
        }
        ViewNode::Divider => {
            lines.push(Line::styled(
                format!("{}────────", " ".repeat(indent)),
                Style::default().fg(Color::DarkGray),
            ));
            actions.push(action_id.map(str::to_owned));
        }
        ViewNode::Empty => {}
    }
}

fn flatten_file_view_component(
    node: &ViewNode,
    indent: usize,
    theme: &AppTheme,
    lines: &mut Vec<Line<'static>>,
) {
    match node {
        ViewNode::Text { text, style } => lines.push(Line::from(vec![
            Span::raw(" ".repeat(indent)),
            Span::styled(text.clone(), extension_file_view_style(style, theme)),
        ])),
        ViewNode::Row { children, gap } => {
            let mut spans = vec![Span::raw(" ".repeat(indent))];
            for (index, child) in children.iter().enumerate() {
                if index > 0 {
                    spans.push(Span::raw(" ".repeat(usize::from(*gap))));
                }
                match child {
                    ViewNode::Text { text, style } => spans.push(Span::styled(
                        text.clone(),
                        extension_file_view_style(style, theme),
                    )),
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
                flatten_file_view_component(child, indent, theme, lines);
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
                    Style::default().fg(ratatui_theme_color(&theme.accent)),
                ));
                flatten_file_view_component(item, indent + 2, theme, lines);
            }
        }
        ViewNode::Action { child, .. } => {
            flatten_file_view_component(child, indent, theme, lines);
        }
        ViewNode::Divider => lines.push(Line::styled(
            format!("{}────────", " ".repeat(indent)),
            Style::default().fg(ratatui_theme_color(&theme.border)),
        )),
        ViewNode::Empty => {}
    }
}

fn extension_file_view_style(style: &ViewStyle, theme: &AppTheme) -> Style {
    let mut result = Style::default();
    if let Some(color) = style
        .foreground
        .as_deref()
        .and_then(|value| extension_file_view_color(value, theme))
    {
        result = result.fg(color);
    }
    if let Some(color) = style
        .background
        .as_deref()
        .and_then(|value| extension_file_view_color(value, theme))
    {
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

fn extension_file_view_color(value: &str, theme: &AppTheme) -> Option<Color> {
    let themed = match value {
        "background" => Some(&theme.background),
        "panel" => Some(&theme.panel),
        "panel-alt" => Some(&theme.panel_alt),
        "border" => Some(&theme.border),
        "accent" | "info" => Some(&theme.accent),
        "accent-muted" => Some(&theme.accent_muted),
        "text" | "heading" => Some(&theme.text),
        "muted" | "subtle" => Some(&theme.muted),
        "selected-hunk" => Some(&theme.selected_hunk),
        "badge-added" | "success" => Some(&theme.badge_added),
        "badge-removed" | "danger" | "error" => Some(&theme.badge_removed),
        "badge-neutral" => Some(&theme.badge_neutral),
        "file-new" => Some(&theme.file_new),
        "file-deleted" => Some(&theme.file_deleted),
        "file-renamed" => Some(&theme.file_renamed),
        "file-modified" | "warning" => Some(&theme.file_modified),
        "file-untracked" => Some(&theme.file_untracked),
        "note-border" => Some(&theme.note_border),
        _ => None,
    };
    themed
        .map(|value| ratatui_theme_color(value))
        .or_else(|| parse_extension_color(value))
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
    let files = &state.changeset().files;
    let generation = state.generation();
    let selected_file_id = files.get(selected).map(public_file_id).map(str::to_owned);
    let border_style = if app.focus == Focus::Sidebar {
        Style::default().fg(ratatui_theme_color(&app.options.theme.accent))
    } else {
        Style::default().fg(ratatui_theme_color(&app.options.theme.border))
    };
    let block = Block::default()
        .title(format!(" {} ", bundled_files_pane().title))
        .borders(Borders::TOP | Borders::RIGHT)
        .border_style(border_style);
    let inner = block.inner(area);
    block.render(area, buffer);
    app.sidebar_bounds.set(Some(inner));

    let mode = resolve_file_sidebar_mode(inner.width.saturating_sub(1));
    let entries = match mode {
        FileSidebarMode::Flat => build_flat_sidebar_entries(files),
        FileSidebarMode::Tree => build_tree_sidebar_entries(files),
    };
    let selected_entry = selected_file_id.as_deref().and_then(|selected_id| {
        entries.iter().position(
            |entry| matches!(entry, FileSidebarEntry::File(file) if file.id == selected_id),
        )
    });
    let max_scroll = entries.len().saturating_sub(usize::from(inner.height));
    let reveal_key = selected_file_id
        .as_ref()
        .map(|selected_file_id| SidebarRevealKey {
            generation,
            selected_file_id: selected_file_id.clone(),
            mode,
        });
    let mut previous_reveal_key = app
        .sidebar_reveal_key
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let reveal_changed = *previous_reveal_key != reveal_key;
    *previous_reveal_key = reveal_key;
    drop(previous_reveal_key);

    let mut scroll_top = app.sidebar_scroll_top.get().min(max_scroll);
    if reveal_changed && let Some(selected_entry) = selected_entry {
        if selected_entry < scroll_top {
            scroll_top = selected_entry;
        } else if selected_entry >= scroll_top.saturating_add(usize::from(inner.height)) {
            scroll_top = selected_entry
                .saturating_add(1)
                .saturating_sub(usize::from(inner.height));
        }
    }
    scroll_top = scroll_top.min(max_scroll);
    app.sidebar_scroll_top.set(scroll_top);

    let map = render_workdeck_file_nav_window(
        inner,
        buffer,
        files,
        &WorkdeckFileNavOptions {
            selected_file_id,
            theme: app.options.theme.id.clone(),
        },
        scroll_top,
    );
    let hits = map
        .file_rows
        .into_iter()
        .filter_map(|hit| {
            let file_index = files
                .iter()
                .position(|file| public_file_id(file) == hit.file_id)?;
            Some(SidebarFileHit {
                bounds: Rect::new(inner.x, inner.y.saturating_add(hit.row), inner.width, 1),
                file_index,
            })
        })
        .collect();
    *app.sidebar_file_hits
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = hits;
}

fn render_review(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    app.review_width.set(area.width);
    app.review_height.set(area.height);
    let state = app
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let layout = state.resolved_layout(area.width);
    let file_view_layouts = app.prepare_extension_file_view_layouts(state.changeset(), area.width);
    let component_expanded = app
        .extension_pane_runtime
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .file_view_component_expanded
        .clone();
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
        &file_view_layouts,
        &component_expanded,
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
    let viewport_bottom = scroll.saturating_add(viewport);
    let component_hits = rows
        .file_view_component_hits
        .into_iter()
        .filter_map(|hit| {
            let top = hit.top.max(scroll);
            let bottom = hit.top.saturating_add(hit.height).min(viewport_bottom);
            (top < bottom).then(|| FileViewComponentHit {
                state_key: hit.state_key,
                bounds: Rect::new(
                    area.x,
                    area.y.saturating_add(1).saturating_add(
                        u16::try_from(top.saturating_sub(scroll)).unwrap_or(u16::MAX),
                    ),
                    area.width,
                    u16::try_from(bottom.saturating_sub(top)).unwrap_or(u16::MAX),
                ),
            })
        })
        .collect::<Vec<_>>();
    {
        let mut runtime = app
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let visible_state_keys = component_hits
            .iter()
            .map(|hit| hit.state_key.clone())
            .collect::<BTreeSet<_>>();
        runtime
            .file_view_component_expanded
            .retain(|key| visible_state_keys.contains(key));
        runtime.file_view_component_hits = component_hits;
    }
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
    file_view_component_hits: Vec<FileViewComponentLogicalHit>,
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
        &BTreeMap::new(),
        &BTreeSet::new(),
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
    file_view_layouts: &BTreeMap<String, ValidatedFileViewLayout>,
    component_expanded: &BTreeSet<FileViewComponentStateKey>,
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
        file_view_layouts,
        component_expanded,
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
    file_view_layouts: &BTreeMap<String, ValidatedFileViewLayout>,
    component_expanded: &BTreeSet<FileViewComponentStateKey>,
) -> ReviewRows {
    let mut rows = Vec::new();
    let mut file_tops = Vec::with_capacity(changeset.files.len());
    let mut hunk_tops = std::collections::HashMap::new();
    let mut file_view_component_hits = Vec::new();
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
        if options.agent_notes {
            rows.extend(agent_rows(file, layout, usize::from(width)));
        }
        if let Some(resolved) = file_view_layouts.get(&file.runtime_id)
            && append_extension_file_view_rows(
                &mut rows,
                &mut hunk_tops,
                file,
                file_index,
                file_selection,
                comments,
                resolved,
                options,
                usize::from(width),
                component_expanded,
                &mut file_view_component_hits,
            )
        {
            continue;
        }
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
        file_view_component_hits,
    }
}

#[allow(clippy::too_many_arguments)]
fn append_extension_file_view_rows(
    rows: &mut Vec<Line<'static>>,
    hunk_tops: &mut std::collections::HashMap<(usize, usize), usize>,
    file: &DiffFile,
    file_index: usize,
    selection: ReviewSelection,
    comments: &[ReviewComment],
    resolved: &ValidatedFileViewLayout,
    options: &ReviewOptions,
    width: usize,
    component_expanded: &BTreeSet<FileViewComponentStateKey>,
    component_hits: &mut Vec<FileViewComponentLogicalHit>,
) -> bool {
    let notes = comments
        .iter()
        .filter(|comment| comment.anchor.file_key == file.key)
        .filter(|comment| comment.resolution != workdeck_review::ReviewNoteResolution::Orphaned)
        .map(|comment| {
            let preferred = comment
                .anchor
                .preferred_side
                .zip(comment.anchor.preferred_line);
            let old_range = comment.anchor.old_range.or_else(|| {
                preferred
                    .filter(|(side, _)| *side == ReviewSide::Old)
                    .map(|(_, line)| workdeck_core::LineRange {
                        start: line,
                        end: line,
                    })
            });
            let new_range = comment.anchor.new_range.or_else(|| {
                preferred
                    .filter(|(side, _)| *side == ReviewSide::New)
                    .map(|(_, line)| workdeck_core::LineRange {
                        start: line,
                        end: line,
                    })
            });
            VisibleFileViewNote {
                id: comment.id.clone(),
                annotation: AgentAnnotation {
                    id: Some(comment.id.clone()),
                    old_range,
                    new_range,
                    summary: comment.summary.clone(),
                    rationale: comment.rationale.clone(),
                    markup: comment.markup.clone(),
                    tags: comment.tags.clone(),
                    confidence: comment.confidence,
                    source: Some(comment.source.clone()),
                    title: comment.title.clone(),
                    author: comment.author.clone(),
                    created_at: comment.created_at.clone(),
                    updated_at: comment.updated_at.clone(),
                    editable: comment.editable,
                },
                thread_depth: review_comment_thread_depth(comment, comments),
                has_actions: comment.editable,
            }
        })
        .collect::<Vec<_>>();
    let plan = build_file_view_render_plan(&resolved.layout, &notes);
    if !plan.unresolved_note_ids.is_empty() {
        return false;
    }
    let starts = resolved.layout.hunk_rows.iter().enumerate().fold(
        BTreeMap::<usize, Vec<usize>>::new(),
        |mut starts, (index, bounds)| {
            starts.entry(bounds.start_row).or_default().push(index);
            starts
        },
    );
    for planned in &plan.rows {
        match planned {
            PlannedFileViewRow::FileViewRow { row, row_index, .. } => {
                if let Some(hunks) = starts.get(row_index) {
                    for hunk_index in hunks {
                        hunk_tops.insert((file_index, *hunk_index), rows.len());
                    }
                }
                let selected = selection.file_index == file_index
                    && selection.hunk_index.is_some_and(|selected| {
                        resolved
                            .layout
                            .hunk_rows
                            .get(selected)
                            .is_some_and(|bounds| {
                                *row_index >= bounds.start_row && *row_index <= bounds.end_row
                            })
                    });
                let state_key = FileViewComponentStateKey {
                    file_id: file.runtime_id.clone(),
                    row_id: row.id.clone(),
                };
                if row
                    .component
                    .as_ref()
                    .is_some_and(|component| component.toggle_expanded_on_left_mouse_up)
                {
                    component_hits.push(FileViewComponentLogicalHit {
                        state_key: state_key.clone(),
                        top: rows.len(),
                        height: resolved.row_heights[*row_index],
                    });
                }
                rows.extend(extension_file_view_row_lines(
                    row,
                    resolved.row_heights[*row_index],
                    &options.theme,
                    width,
                    selected,
                    component_expanded.contains(&state_key),
                ));
            }
            PlannedFileViewRow::InlineNote { note, .. } => {
                rows.extend(extension_file_view_note_lines(note, width));
            }
        }
    }
    true
}

fn review_comment_thread_depth(comment: &ReviewComment, comments: &[ReviewComment]) -> usize {
    let mut depth = 0;
    let mut parent = comment.parent_id.as_deref();
    while let Some(parent_id) = parent {
        let Some(parent_comment) = comments.iter().find(|candidate| candidate.id == parent_id)
        else {
            break;
        };
        depth += 1;
        if depth >= 64 {
            break;
        }
        parent = parent_comment.parent_id.as_deref();
    }
    depth
}

fn extension_file_view_row_lines(
    row: &workdeck_extension_api::ExtensionFileViewRow,
    declared_height: usize,
    theme: &AppTheme,
    width: usize,
    selected: bool,
    expanded: bool,
) -> Vec<Line<'static>> {
    let mut lines = if let Some(component) = &row.component {
        let mut lines = Vec::new();
        let content = if selected && expanded {
            component
                .selected_expanded_content
                .as_ref()
                .or(component.expanded_content.as_ref())
                .or(component.selected_content.as_ref())
                .unwrap_or(&component.content)
        } else if expanded {
            component
                .expanded_content
                .as_ref()
                .unwrap_or(&component.content)
        } else if selected {
            component
                .selected_content
                .as_ref()
                .unwrap_or(&component.content)
        } else {
            &component.content
        };
        flatten_file_view_component(content, 0, theme, &mut lines);
        if lines.is_empty() {
            extension_file_view_symbolic_lines(&row.spans, theme, width)
        } else {
            if let Some(prefix) = &component.selection_prefix
                && let Some(line) = lines.first_mut()
            {
                let style = line
                    .spans
                    .iter()
                    .find(|span| !span.content.is_empty())
                    .map_or_else(Style::default, |span| span.style);
                line.spans.insert(
                    0,
                    Span::styled(
                        if selected {
                            prefix.selected.clone()
                        } else {
                            prefix.unselected.clone()
                        },
                        style,
                    ),
                );
            }
            lines
        }
    } else {
        extension_file_view_symbolic_lines(&row.spans, theme, width)
    };
    lines.truncate(declared_height);
    lines.resize_with(declared_height, Line::default);
    let component_owns_selection_paint = row.component.as_ref().is_some_and(|component| {
        component.selected_content.is_some() || component.selected_expanded_content.is_some()
    });
    if selected && !component_owns_selection_paint {
        let background = ratatui_theme_color(&theme.selected_hunk);
        for line in &mut lines {
            line.style = line.style.bg(background);
            for span in &mut line.spans {
                span.style = span.style.bg(background);
            }
        }
    }
    lines
}

fn extension_file_view_symbolic_lines(
    spans: &[ExtensionFileViewSpan],
    theme: &AppTheme,
    width: usize,
) -> Vec<Line<'static>> {
    let spans = spans
        .iter()
        .map(|span| {
            let foreground = match span.tone {
                None | Some(ExtensionFileViewTone::Syntax) => &theme.text,
                Some(ExtensionFileViewTone::Muted) => &theme.muted,
                Some(ExtensionFileViewTone::Accent) => &theme.accent,
                Some(ExtensionFileViewTone::AccentMuted) => &theme.accent_muted,
                Some(ExtensionFileViewTone::Added) => &theme.badge_added,
                Some(ExtensionFileViewTone::Removed) => &theme.badge_removed,
            };
            let mut style = Style::default().fg(ratatui_theme_color(foreground));
            for attribute in &span.attributes {
                style = style.add_modifier(match attribute {
                    ExtensionTextAttribute::Bold => Modifier::BOLD,
                    ExtensionTextAttribute::Italic => Modifier::ITALIC,
                    ExtensionTextAttribute::Underline => Modifier::UNDERLINED,
                    ExtensionTextAttribute::Strikethrough => Modifier::CROSSED_OUT,
                });
            }
            Span::styled(span.text.clone(), style)
        })
        .collect::<Vec<_>>();
    wrap_styled_spans(spans, width.max(1))
        .into_iter()
        .map(Line::from)
        .collect()
}

fn extension_file_view_note_lines(note: &VisibleFileViewNote, width: usize) -> Vec<Line<'static>> {
    let author = note
        .annotation
        .author
        .as_deref()
        .or(note.annotation.source.as_deref())
        .unwrap_or("note");
    let indent = "  ".repeat(note.thread_depth.min(8));
    let mut rows = vec![Line::from(vec![
        Span::styled(
            format!("{indent}  │ note "),
            Style::default().fg(Color::Magenta),
        ),
        Span::styled(
            author.to_owned(),
            Style::default()
                .fg(Color::LightMagenta)
                .add_modifier(Modifier::BOLD),
        ),
    ])];
    let body = note
        .annotation
        .markup
        .as_deref()
        .unwrap_or(&note.annotation.summary);
    let rendered = workdeck_markup::render(body, width.saturating_sub(indent.len() + 8).max(1));
    rows.extend(rendered.lines.into_iter().map(|text| {
        Line::styled(
            format!("{indent}  │   {text}"),
            Style::default().fg(Color::LightMagenta),
        )
    }));
    if let Some(rationale) = &note.annotation.rationale {
        rows.push(Line::styled(
            format!("{indent}  │   {rationale}"),
            Style::default().fg(Color::DarkGray),
        ));
    }
    rows
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
    let sections = build_help_sections(&default_help_commands());
    let mut lines = Vec::new();
    for (section_index, section) in sections.into_iter().enumerate() {
        if section_index > 0 {
            lines.push(Line::default());
        }
        lines.push(Line::styled(
            section.title,
            Style::default().add_modifier(Modifier::BOLD),
        ));
        let key_width = section
            .rows
            .iter()
            .map(|row| row.keys.width())
            .max()
            .unwrap_or_default();
        lines.extend(
            section
                .rows
                .into_iter()
                .map(|row| Line::from(format!("  {:key_width$}  {}", row.keys, row.description))),
        );
    }
    let requested_height = u16::try_from(lines.len().saturating_add(2)).unwrap_or(u16::MAX);
    let geometry = resolve_modal_geometry(76, requested_height, area.width, area.height);
    let popup = Rect {
        x: area.x.saturating_add(geometry.left),
        y: area.y.saturating_add(geometry.top),
        width: geometry.width,
        height: geometry.height,
    };
    Clear.render(popup, buffer);
    Paragraph::new(lines)
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
                title: None,
                tags: Vec::new(),
                confidence: None,
                updated_at: None,
                resolution: workdeck_review::ReviewNoteResolution::Active,
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
    fn pane_action_hit_heights_follow_ratatuis_word_wrapping() {
        let line = Line::raw("aaaaa bbbbb ccccc");
        assert_eq!(extension_pane_line_height(&line, 10), 3);
        assert_eq!(extension_pane_line_height(&line, 17), 1);
        assert_eq!(extension_pane_line_height(&Line::default(), 0), 1);
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
    fn review_help_overlay_renders_the_command_derived_sections_and_rows() {
        let backend = TestBackend::new(100, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_help(frame.area(), frame.buffer_mut()))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        for expected in [
            "Navigation",
            "PageDown / Space / f",
            "Mouse",
            "Shift+Wheel",
            "View",
            "1 / 2 / 0",
            "Review",
            "create review note",
        ] {
            assert!(rendered.contains(expected), "missing {expected:?}");
        }
    }

    #[test]
    fn built_in_files_pane_uses_registered_responsive_widths() {
        assert_eq!(bundled_sidebar_width(40, 30), 0);
        assert_eq!(bundled_sidebar_width(100, 30), 22);
        assert_eq!(bundled_sidebar_width(220, 30), 35);
        assert_eq!(bundled_sidebar_width(500, 30), 56);
    }

    #[test]
    fn built_in_sidebar_mouse_up_selects_the_rendered_file_row() {
        let mut changes = changeset();
        changes.files[0].runtime_id = "first".into();
        let mut second = changes.files[0].clone();
        second.key = "second-key".into();
        second.runtime_id = "second".into();
        second.path = "b.rs".into();
        changes.files.push(second);
        let backend = TestBackend::new(220, 16);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = ReviewApp::new(changes, ReviewOptions::default());
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        let second = app
            .sidebar_file_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .find(|hit| hit.file_index == 1)
            .copied()
            .expect("second file hit");
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: second.bounds.x,
            row: second.bounds.y,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(
            app.shared_state()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .selection()
                .file_index,
            1
        );
    }

    #[test]
    fn built_in_sidebar_scroll_is_independent_and_selection_reveals_once() {
        let mut changes = changeset();
        changes.files[0].runtime_id = "file-0".into();
        for index in 1..20 {
            let mut file = changes.files[0].clone();
            file.key = format!("file-key-{index}");
            file.runtime_id = format!("file-{index}");
            file.path = format!("src/file-{index}.rs");
            changes.files.push(file);
        }
        let backend = TestBackend::new(220, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = ReviewApp::new(changes, ReviewOptions::default());
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        let bounds = app.sidebar_bounds.get().expect("sidebar bounds");
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: bounds.x,
            row: bounds.y,
            modifiers: KeyModifiers::NONE,
        });
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        assert_eq!(app.sidebar_scroll_top.get(), 1);
        assert_eq!(app.review_scroll(), 0);

        app.with_state(|state| state.select_file(19).unwrap());
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        assert!(app.sidebar_scroll_top.get() > 1);
        assert!(
            app.sidebar_file_hits
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .iter()
                .any(|hit| hit.file_index == 19)
        );
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
