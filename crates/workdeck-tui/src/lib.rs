//! Ratatui review canvas.

mod agent_annotations;
mod agent_note_geometry;
mod agent_popover;
mod app_commands;
mod app_menus;
mod code_row_layout;
mod color;
mod command_keymap;
mod command_keys;
mod current_review_controller;
mod current_review_refresh;
mod cursor_highlight;
mod diff_section_geometry;
mod diff_section_row_plan;
mod extension_command_controls;
mod extension_commands;
mod extension_current_line;
mod extension_dialogs;
mod extension_navigation;
mod extension_notifications;
mod extension_pane_controller;
mod extension_panes;
mod extension_review_events;
mod extension_trust_controller;
mod extension_trust_prompt;
mod extension_workspace;
mod file_header;
mod file_render_window;
mod file_section_layout;
mod file_view_geometry;
mod help_content;
mod highlighted_diff_runtime;
mod hunk_scroll;
mod ids;
mod job_control;
mod key_routing;
mod keyboard;
mod line_cursors;
mod line_highlight_paint;
mod line_highlights;
mod list_geometry;
mod menu;
mod mouse_capture;
mod mouse_scroll;
mod open_in_editor;
mod planned_review_row;
mod planned_row_text;
mod public_review;
mod review_render_plan;
mod review_row_geometry;
mod review_state_helpers;
mod row_style;
mod shutdown;
mod spatial;
mod startup_notices;
mod static_diff_pager;
mod status_bar;
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

use extension_dialogs::{
    ExtensionConfirmDialog, ExtensionDialogAnswer, ExtensionDialogError, ExtensionDialogQueue,
    ExtensionDialogRequest, ExtensionDialogSettlement, ExtensionInputDialog, ExtensionSelectDialog,
    ExtensionWorkspaceWriteDialog,
};

pub use agent_annotations::*;
pub use agent_note_geometry::*;
pub use agent_popover::*;
pub use app_commands::*;
pub use app_menus::*;
pub use code_row_layout::*;
pub use color::*;
pub use command_keymap::*;
pub use command_keys::*;
pub use current_review_controller::*;
pub use current_review_refresh::*;
pub use cursor_highlight::*;
pub use diff_section_geometry::*;
pub use diff_section_row_plan::*;
pub use extension_commands::*;
pub use extension_current_line::*;
pub use extension_navigation::*;
pub use extension_notifications::*;
pub use extension_pane_controller::*;
pub use extension_panes::*;
pub use extension_review_events::*;
pub use extension_trust_controller::*;
pub use extension_trust_prompt::*;
pub use extension_workspace::*;
pub use file_header::*;
pub use file_render_window::*;
pub use file_section_layout::*;
pub use file_view_geometry::*;
pub use help_content::*;
pub use highlighted_diff_runtime::*;
pub use hunk_scroll::*;
pub use ids::*;
pub use job_control::*;
pub use key_routing::*;
pub use keyboard::*;
pub use line_cursors::*;
pub use line_highlight_paint::*;
pub use line_highlights::*;
pub use list_geometry::*;
pub use menu::*;
pub use mouse_capture::*;
pub use mouse_scroll::*;
pub use open_in_editor::*;
pub use planned_review_row::*;
pub use planned_row_text::*;
pub use public_review::*;
pub use review_render_plan::*;
pub use review_row_geometry::*;
pub use review_state_helpers::*;
pub use row_style::*;
pub use shutdown::*;
pub use spatial::*;
pub use startup_notices::*;
pub use static_diff_pager::*;
pub use status_bar::*;
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
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{self, IsTerminal, Stdout};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use workdeck_core::{
    AgentAnnotation, Changeset, ChangesetSource, DiffFile, DiffLine, DiffLineKind, ReviewSelection,
    ReviewSide, SourceOrigin, StartupNotice,
};
use workdeck_diff::{
    DIFF_RAIL_PREFIX_WIDTH, HighlightedDiffLine, LanguageMatcher, LanguageRegistration,
    LanguageRegistry, SyntaxToken, TextSegment, clip_segments, expand_diff_tabs,
    plan_split_line_pairs, resolve_split_cell_geometry,
    resolve_split_pane_widths as resolve_diff_split_pane_widths, resolve_stack_cell_geometry,
    sanitize_terminal_line, slice_segments_window, word_diff_ranges, wrap_segments,
};
use workdeck_extension_api::{
    ExtensionCommandAvailability, ExtensionFileViewSpan, ExtensionFileViewTone,
    ExtensionHostAction, ExtensionKeyEvent, ExtensionLayoutMode, ExtensionLifecycleEvent,
    ExtensionNotification, ExtensionNotificationHub, ExtensionNotificationSubscription,
    ExtensionNotifyType, ExtensionPaintTheme, ExtensionPaneView, ExtensionResolvedLayout,
    ExtensionReviewNote, ExtensionTextAttribute, ExtensionWorkspaceWriteCompletion,
    ExtensionWorkspaceWriteResult, FileLanguageGlobTarget, FileLanguageMatcher,
    FileViewModeKeyRequest, FileViewModeLifecycleRequest, KeyRoutingResult,
    KeyboardModeRegistration, PaneActionInvocation, PanePlacement, PaneRegistration,
    PaneRenderRequest, Registration, ReviewEvent, SessionReloadReason, ValidatedFileViewLayout,
    ViewNode, ViewStyle, WORKDECK_FILES_PANE_KEY, bundled_files_pane, extension_pane_size,
};
use workdeck_extension_host::{
    ActiveSessionKeyboardMode, EXTENSION_SHUTDOWN_TIMEOUT,
    ExtensionEventContextProviderInstallation, ExtensionEventContextProviderSlot,
    ExtensionRequestCancellation, FileViewSelectionState, HostError, KeyboardModeActionAuthority,
    KeyboardModeControllerState, LineHighlightRefreshResult, LineHighlightsController,
    LoadedExtension, RegisteredFileView, RegisteredKeyboardMode, RegisteredLineHighlighter,
    create_file_view_input, create_file_view_input_snapshot, format_keyboard_mode_failure,
    project_extension_changeset, project_extension_diff_file, reconcile_file_view_selections,
    registered_file_view_key, resolve_loaded_extension_registrations, select_file_view,
    session_keyboard_mode_display_title, session_keyboard_mode_status_hint,
    session_keyboard_mode_still_valid,
};
use workdeck_review::{
    CommentAnchor, ExpandedSourceError, ExpandedSourceStatus, LayoutMode, PlannedFileViewRow,
    ReviewComment, ReviewGapAddress, ReviewLineTarget, ReviewNavigationFile, ReviewNavigationModel,
    ReviewNoteResolution, ReviewSelectionMove, ReviewSelectionScope, ReviewState,
    SemanticReviewAnnotationIndex, SemanticReviewSelection, VisibleFileViewNote,
    build_extension_review_snapshot, build_file_view_render_plan, plan_expanded_gap,
    plan_review_selection_move, project_extension_review_notes, review_annotated_hunk_indices,
    review_default_hunk_line_target, review_expansion_side, review_gap_source_for_file,
    review_leading_gap, review_line_anchor, review_trailing_gap,
};
use workdeck_session::{ReviewSessionServer, default_discovery_directory};
use workdeck_vcs::bundled_vcs_catalog;

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
    pub show_menu_bar: bool,
    pub copy_decorations: bool,
    pub theme: AppTheme,
    pub repo: Option<PathBuf>,
    pub command_cwd: Option<PathBuf>,
    /// Provider-neutral invocation retained for workspace-write policy decisions.
    pub review_input: Option<workdeck_core::CliInput>,
    /// Ordered user command bindings; ordering decides conflicting explicit claims.
    pub keybindings: Vec<UserKeyBindingEntry>,
    /// Non-fatal diagnostics produced while reading the user's binding table.
    pub keybinding_notices: Vec<String>,
    /// Ordered, deduplicated notices shown transiently on the startup footer row.
    pub startup_notices: Vec<StartupNotice>,
    pub extension_panes: Vec<ExtensionPaneView>,
    pub extension_notifications: Option<ExtensionNotificationHub>,
    /// Repository whose native extensions are waiting on an explicit trust decision.
    pub pending_extension_trust_repo_root: Option<PathBuf>,
    /// Composition-root authority for persisting a decision and loading newly trusted code.
    pub extension_trust_handler: Option<ExtensionTrustHandler>,
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
            show_menu_bar: true,
            copy_decorations: false,
            theme: resolve_theme(Some(DEFAULT_DARK_THEME_ID), None, &[]),
            repo: None,
            command_cwd: None,
            review_input: None,
            keybindings: Vec::new(),
            keybinding_notices: Vec::new(),
            startup_notices: Vec::new(),
            extension_panes: Vec::new(),
            extension_notifications: None,
            pending_extension_trust_repo_root: None,
            extension_trust_handler: None,
        }
    }
}

/// Host-owned persistence request produced by the repository-extension trust modal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionTrustRequest {
    pub repo_root: PathBuf,
    pub decision: workdeck_extension_host::TrustDecision,
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
    Filter,
}

#[derive(Debug, Clone)]
struct LivePaneRegistration {
    key: String,
    extension_index: usize,
    extension_id: String,
    pane: PaneRegistration,
}

#[derive(Debug, Clone)]
struct LiveKeyboardModeRegistration {
    extension_index: usize,
    extension_id: String,
    mode: KeyboardModeRegistration,
    registered: Arc<RegisteredKeyboardMode>,
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
    activation_id: u64,
    session: ActiveSessionKeyboardMode,
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
struct PendingExtensionCommand {
    extension_index: usize,
    extension_id: String,
    command_id: String,
    title: String,
    review_generation: u64,
}

#[derive(Debug, Clone)]
struct QueuedExtensionEvent {
    event: ReviewEvent,
    dispatch_depth: usize,
}

#[derive(Debug, Clone)]
struct QueuedExtensionCommand {
    pending: PendingExtensionCommand,
    snapshot: workdeck_core::ReviewSnapshot,
    open_panes: Vec<String>,
    active_keyboard_mode: Option<String>,
    cwd: PathBuf,
    review: workdeck_extension_api::ExtensionReviewSnapshot,
    commands: ExtensionCommandAvailability,
    workspace: Option<workdeck_extension_api::ExtensionWorkspaceSnapshot>,
}

#[derive(Debug, Clone)]
enum QueuedExtensionRequest {
    Command(QueuedExtensionCommand),
    Event(QueuedExtensionEvent),
}

#[derive(Debug, Clone)]
struct PendingExtensionEvent {
    extension_index: usize,
    extension_id: String,
    event_name: String,
    dispatch_depth: usize,
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
    pending_commands: BTreeMap<usize, PendingExtensionCommand>,
    pending_events: BTreeMap<usize, PendingExtensionEvent>,
    request_queues: BTreeMap<usize, VecDeque<QueuedExtensionRequest>>,
    panes: Vec<LivePaneRegistration>,
    session_panes: Vec<SessionPane>,
    commands: Vec<RegisteredExtensionCommand>,
    app_commands: Vec<ExtensionAppCommand>,
    command_conflicts: Vec<ExtensionCommandConflict>,
    keyboard_modes: Vec<LiveKeyboardModeRegistration>,
    file_views: Vec<LiveFileViewRegistration>,
    line_highlights: LineHighlightsController,
    line_highlight_preparation: LineHighlightPreparationController,
    file_languages: Vec<LanguageRegistration>,
    file_view_selections: FileViewSelectionState,
    file_view_layouts: BTreeMap<String, CachedFileViewLayout>,
    file_view_component_expanded: BTreeSet<FileViewComponentStateKey>,
    file_view_component_hits: Vec<FileViewComponentHit>,
    file_view_component_pointer: MouseCapture<FileViewComponentPointer>,
    active_file_view_mode: Option<ActiveFileViewModeRuntime>,
    active_keyboard_mode: Option<ActiveKeyboardMode>,
    keyboard_mode_controller: KeyboardModeControllerState,
    dialogs: ExtensionDialogQueue,
    pane_action_hits: Vec<ExtensionPaneActionHit>,
    open: BTreeSet<String>,
    size_overrides: BTreeMap<String, u16>,
    cached_renders: BTreeMap<String, CachedPaneRender>,
    layout: ExtensionPaneLayoutPlan,
    resize: MouseCapture<(String, PaneResizeState)>,
    menu: MenuController,
    menu_triggers: Vec<(MenuId, Rect)>,
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

#[derive(Debug, Clone, Copy)]
struct ExtensionTrustPromptHits {
    bounds: Rect,
    close: Rect,
    trust: Rect,
    dismiss: Rect,
    deny: Rect,
}

impl ExtensionPaneRuntime {
    fn new(extensions: Vec<LoadedExtension>, files: &[DiffFile]) -> Self {
        let resolution = resolve_loaded_extension_registrations(&extensions, bundled_vcs_catalog());
        let mut pane_candidates = Vec::new();
        let mut commands = Vec::new();
        let mut keyboard_modes = Vec::new();
        let mut file_views = Vec::new();
        let mut line_highlighters = Vec::new();
        let mut file_languages = Vec::new();
        let mut open = BTreeSet::new();
        for (extension_index, extension) in extensions.iter().enumerate() {
            for (registration_index, registration) in
                extension.handshake.registrations.iter().enumerate()
            {
                if !resolution.accepts(extension_index, registration_index) {
                    continue;
                }
                match registration {
                    Registration::Pane(pane) => {
                        pane_candidates.push((
                            extension_index,
                            RegisteredExtensionPane::new(
                                extension.manifest.id.clone(),
                                pane.clone(),
                            ),
                        ));
                    }
                    Registration::Command(command) => commands.push(RegisteredExtensionCommand {
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
                        let registered = Arc::new(RegisteredKeyboardMode {
                            extension_id: extension.manifest.id.clone(),
                            mode: mode.clone(),
                        });
                        keyboard_modes.push(LiveKeyboardModeRegistration {
                            extension_index,
                            extension_id: extension.manifest.id.clone(),
                            mode,
                            registered,
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
                    Registration::LineHighlighter { id } => {
                        line_highlighters.push(RegisteredLineHighlighter {
                            extension_index,
                            extension_id: extension.manifest.id.clone(),
                            highlighter_id: id.clone(),
                        });
                    }
                    Registration::FileLanguage(registration) => {
                        file_languages.push(LanguageRegistration {
                            matcher: match &registration.matcher {
                                FileLanguageMatcher::Extension { value } => {
                                    LanguageMatcher::Extension(value.clone())
                                }
                                FileLanguageMatcher::Filename { value } => {
                                    LanguageMatcher::Filename(value.clone())
                                }
                                FileLanguageMatcher::Glob { value, target } => {
                                    LanguageMatcher::Glob {
                                        value: value.clone(),
                                        target_path: *target == FileLanguageGlobTarget::Path,
                                    }
                                }
                            },
                            language: registration.language.clone(),
                            reserved: false,
                        });
                    }
                    _ => {}
                }
            }
        }
        let registered = pane_candidates
            .iter()
            .map(|(_, registered)| Arc::clone(registered))
            .collect::<Vec<_>>();
        let session_panes = build_session_panes(&registered);
        let initial_open_state = initial_pane_open_state(&session_panes);
        let accepted = session_panes
            .iter()
            .map(|pane| pane.registered.identity)
            .collect::<BTreeSet<_>>();
        let panes = pane_candidates
            .into_iter()
            .filter(|(_, registered)| accepted.contains(&registered.identity))
            .map(|(extension_index, registered)| LivePaneRegistration {
                key: registered.key(),
                extension_index,
                extension_id: registered.extension_id.clone(),
                pane: registered.pane.clone(),
            })
            .collect::<Vec<_>>();
        open.extend(
            initial_open_state
                .open
                .iter()
                .filter(|key| key.as_str() != WORKDECK_FILES_PANE_KEY)
                .cloned(),
        );
        Self {
            extensions,
            panes,
            session_panes,
            commands,
            keyboard_modes,
            file_views,
            line_highlights: LineHighlightsController::new(
                files.iter().map(|file| file.runtime_id.clone()),
                line_highlighters,
            ),
            file_languages,
            open,
            ..Self::default()
        }
    }

    fn reconcile_panes_from(&mut self, previous: &Self, files_pane_open: bool) -> bool {
        let mut previous_open = previous.open.iter().cloned().collect::<Vec<_>>();
        if files_pane_open {
            previous_open.insert(0, WORKDECK_FILES_PANE_KEY.into());
        }
        let previous_state = Arc::new(PaneOpenState {
            known: previous
                .session_panes
                .iter()
                .map(|pane| pane.key.clone())
                .collect(),
            open: previous_open,
        });
        let reconciled = reconcile_pane_open_state(&self.session_panes, &previous_state);
        self.open = reconciled
            .open
            .iter()
            .filter(|key| key.as_str() != WORKDECK_FILES_PANE_KEY)
            .cloned()
            .collect();
        reconciled
            .open
            .iter()
            .any(|key| key == WORKDECK_FILES_PANE_KEY)
    }

    fn retire_extensions(&mut self) {
        self.pending_commands.clear();
        self.pending_events.clear();
        self.request_queues.clear();
        for extension in &mut self.extensions {
            let _ = extension.begin_retirement();
        }
        let deadline = Instant::now() + EXTENSION_SHUTDOWN_TIMEOUT;
        for extension in &mut self.extensions {
            extension.finish_retirement(deadline);
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
    extension_command_epoch: u64,
    editor_requested: bool,
    resolved_command_keys: ResolvedKeymap,
    show_menu_bar: bool,
    copy_decorations: bool,
    status: Option<String>,
    note_composer: Option<ReviewNoteComposer>,
    note_composer_bounds: Cell<Option<Rect>>,
    note_sequence: u64,
    filter: String,
    filter_cursor: usize,
    filter_scroll: Cell<usize>,
    review_width: Cell<u16>,
    review_height: Cell<u16>,
    sidebar_bounds: Cell<Option<Rect>>,
    sidebar_scroll_top: Cell<usize>,
    sidebar_reveal_key: Mutex<Option<SidebarRevealKey>>,
    sidebar_file_hits: Mutex<Vec<SidebarFileHit>>,
    review_file_header_hits: Mutex<Vec<SidebarFileHit>>,
    current_line_row: usize,
    expanded_gaps: BTreeSet<(String, usize)>,
    highlights: Mutex<HighlightedDiffRuntime>,
    themes: ThemeController,
    startup_notices: StartupNoticeQueue,
    extension_toasts: Arc<Mutex<ExtensionNotificationSurface>>,
    extension_notification_subscription: Option<ExtensionNotificationSubscription>,
    mouse_scroll_acceleration: ReviewMouseWheelScrollAcceleration,
    mouse_scroll_accumulator: f64,
    extension_pane_runtime: Mutex<ExtensionPaneRuntime>,
    extension_event_context_provider: ExtensionEventContextProviderSlot,
    extension_event_context_installation: Option<ExtensionEventContextProviderInstallation>,
    extension_event_dispatch_depth: usize,
    extension_review_events: ExtensionReviewEventController,
    extension_registry_generation: u64,
    review_projection_generation: u64,
    #[cfg(test)]
    observed_extension_events: Vec<(u64, String, serde_json::Value)>,
    extension_trust_controller: ExtensionTrustController,
    extension_trust_request: Option<ExtensionTrustRequest>,
    extension_trust_prompt_hits: Cell<Option<ExtensionTrustPromptHits>>,
}

impl ReviewApp {
    pub fn new(changeset: Changeset, options: ReviewOptions) -> Self {
        Self::new_with_extensions(changeset, options, Vec::new())
    }

    pub fn new_with_extensions(
        changeset: Changeset,
        mut options: ReviewOptions,
        mut extensions: Vec<LoadedExtension>,
    ) -> Self {
        // Native factories encode events emitted during handshake as provisional declarations.
        // Drain them only after every extension has registered, matching Hunk's bind-and-replay
        // boundary and preventing a second ReviewApp from replaying the same factory event.
        let mut pending_custom_events = Vec::new();
        for extension in &mut extensions {
            extension.handshake.registrations.retain(|registration| {
                if let Registration::PendingCustomEvent { name, payload } = registration {
                    pending_custom_events.push((name.clone(), payload.clone()));
                    false
                } else {
                    true
                }
            });
        }
        let mut state = ReviewState::new(changeset);
        state.set_layout(options.layout);
        let themes = ThemeController::new(options.theme.id.clone());
        let mut startup_notices = StartupNoticeQueue::new(true, DEFAULT_STARTUP_NOTICE_DURATION);
        startup_notices.restart(
            true,
            DEFAULT_STARTUP_NOTICE_DURATION,
            options.startup_notices.iter().cloned(),
            Instant::now(),
        );
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
        let mut extension_pane_runtime =
            ExtensionPaneRuntime::new(extensions, state.changeset().files.as_slice());
        if !extension_pane_runtime
            .session_panes
            .iter()
            .find(|pane| pane.key == WORKDECK_FILES_PANE_KEY)
            .is_some_and(|pane| pane.default_open)
        {
            options.sidebar = false;
        }
        let mut extension_trust_controller = ExtensionTrustController::default();
        extension_trust_controller.reconcile(
            options.pager,
            options.pending_extension_trust_repo_root.as_deref(),
        );
        let mut command_defaults = builtin_command_key_defaults();
        command_defaults.extend(extension_command_key_defaults(
            &extension_pane_runtime.commands,
        ));
        let resolved_command_keys = resolve_command_keys(&command_defaults, &options.keybindings);
        let extension_command_table = build_extension_app_commands(
            &extension_pane_runtime.commands,
            &builtin_command_match_probes(Some(&resolved_command_keys)),
            Some(&resolved_command_keys),
        );
        extension_pane_runtime.app_commands = extension_command_table.commands;
        extension_pane_runtime.command_conflicts = extension_command_table.conflicts;
        let keymap_status = {
            let mut notices = options.keybinding_notices.clone();
            notices.extend(
                resolved_command_keys
                    .issues
                    .iter()
                    .map(|issue| issue.message.clone()),
            );
            notices.extend(
                extension_pane_runtime
                    .command_conflicts
                    .iter()
                    .map(ExtensionCommandConflict::warning),
            );
            (!notices.is_empty()).then(|| notices.join(" • "))
        };
        let extension_event_context_provider = ExtensionEventContextProviderSlot::default();
        let show_menu_bar = options.show_menu_bar;
        let copy_decorations = options.copy_decorations;
        let mut app = Self {
            state: Arc::new(Mutex::new(state)),
            options,
            focus: Focus::Review,
            scroll: 0,
            show_help: false,
            should_quit: false,
            reload_requested: false,
            extension_command_epoch: 1,
            editor_requested: false,
            resolved_command_keys,
            show_menu_bar,
            copy_decorations,
            status: keymap_status,
            note_composer: None,
            note_composer_bounds: Cell::new(None),
            note_sequence: 0,
            filter: String::new(),
            filter_cursor: 0,
            filter_scroll: Cell::new(0),
            review_width: Cell::new(120),
            review_height: Cell::new(20),
            sidebar_bounds: Cell::new(None),
            sidebar_scroll_top: Cell::new(0),
            sidebar_reveal_key: Mutex::new(None),
            sidebar_file_hits: Mutex::new(Vec::new()),
            review_file_header_hits: Mutex::new(Vec::new()),
            current_line_row: 0,
            expanded_gaps: BTreeSet::new(),
            highlights: Mutex::new(HighlightedDiffRuntime::default()),
            themes,
            startup_notices,
            extension_toasts,
            extension_notification_subscription,
            mouse_scroll_acceleration: ReviewMouseWheelScrollAcceleration::default(),
            mouse_scroll_accumulator: 0.0,
            extension_pane_runtime: Mutex::new(extension_pane_runtime),
            extension_event_context_provider,
            extension_event_context_installation: None,
            extension_event_dispatch_depth: 0,
            extension_review_events: ExtensionReviewEventController::default(),
            extension_registry_generation: 1,
            review_projection_generation: 1,
            #[cfg(test)]
            observed_extension_events: Vec::new(),
            extension_trust_controller,
            extension_trust_request: None,
            extension_trust_prompt_hits: Cell::new(None),
        };
        app.install_extension_event_context_provider();
        let initial_events = app.update_extension_review_events(Instant::now());
        debug_assert!(initial_events.is_empty());
        for (name, payload) in pending_custom_events {
            app.publish_extension_event(&name, payload);
        }
        app.publish_extension_lifecycle_event(ExtensionLifecycleEvent::Startup {
            cwd: app.extension_command_cwd(),
        });
        app.publish_current_changeset_event(false, SessionReloadReason::Manual);
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

    pub fn take_quit_requested(&mut self) -> bool {
        std::mem::take(&mut self.should_quit)
    }

    /// Replace the discovery result without remounting the review application.
    pub fn reconcile_extension_trust_repo_root(&mut self, repo_root: Option<PathBuf>) {
        self.options.pending_extension_trust_repo_root = repo_root;
        self.extension_trust_controller.reconcile(
            self.options.pager,
            self.options.pending_extension_trust_repo_root.as_deref(),
        );
    }

    #[must_use]
    pub fn extension_trust_prompt_root(&self) -> Option<&Path> {
        self.extension_trust_controller.prompt_root()
    }

    /// Consume one trust decision for persistence and extension-aware refresh by the host.
    pub fn take_extension_trust_request(&mut self) -> Option<ExtensionTrustRequest> {
        self.extension_trust_request.take()
    }

    /// Persist one queued security decision and atomically install extensions after a fresh reload.
    pub fn process_extension_trust_request(
        &mut self,
        reloader: &mut Option<&mut dyn FnMut() -> Result<Changeset>>,
    ) {
        let Some(request) = self.take_extension_trust_request() else {
            return;
        };
        let Some(handler) = self.options.extension_trust_handler.clone() else {
            self.status = Some("Failed to record the trust decision.".into());
            return;
        };
        let can_reload = reloader.is_some();
        let load_extensions =
            can_reload && request.decision == workdeck_extension_host::TrustDecision::Trusted;
        let extensions = match handler.run(&request.repo_root, request.decision, load_extensions) {
            Ok(extensions) => extensions,
            Err(ExtensionTrustHostError::Write(error)) => {
                self.status = Some(error.notice());
                return;
            }
            Err(ExtensionTrustHostError::Reload) => {
                self.status =
                    Some("Failed to reload after trusting this repository's extensions.".into());
                return;
            }
        };
        self.reconcile_extension_trust_repo_root(None);
        if request.decision == workdeck_extension_host::TrustDecision::Denied {
            self.status = Some("Won't run this repository's extensions".into());
            return;
        }
        let Some(reload) = reloader.as_deref_mut() else {
            self.status =
                Some("Trusted this repository • restart Workdeck to load its extensions".into());
            return;
        };
        match reload() {
            Ok(changeset) => self.replace_extensions_and_reload(changeset, extensions),
            Err(_) => {
                self.status =
                    Some("Failed to reload after trusting this repository's extensions.".into());
            }
        }
    }

    #[must_use]
    pub fn take_editor_request(&mut self) -> Option<ReviewEditorRequest> {
        if !std::mem::take(&mut self.editor_requested) {
            return None;
        }
        let (file, line_cursor, selected_hunk) = self.with_state(|state| {
            let selection = state.selection();
            let file = state.selected_file().cloned();
            let selected_hunk = file
                .as_ref()
                .and_then(|file| selection.hunk_index.and_then(|index| file.hunks.get(index)))
                .cloned();
            let line_cursor = file.as_ref().and_then(|file| {
                Some(EditorLineCursor {
                    file_id: if file.runtime_id.is_empty() {
                        file.key.clone()
                    } else {
                        file.runtime_id.clone()
                    },
                    hunk_index: selection.hunk_index?,
                    target: EditorLineTarget {
                        side: selection.side?,
                        line: selection.line?,
                    },
                })
            });
            (file, line_cursor, selected_hunk)
        });
        Some(ReviewEditorRequest {
            base_path: self
                .options
                .repo
                .clone()
                .or_else(|| self.options.command_cwd.clone())
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_default(),
            file,
            line_cursor,
            selected_hunk,
        })
    }

    pub fn reload(&mut self, changeset: Changeset) {
        self.reload_with_reason(changeset, SessionReloadReason::Manual, false);
    }

    fn reload_with_reason(
        &mut self,
        changeset: Changeset,
        reason: SessionReloadReason,
        emit_startup: bool,
    ) {
        self.extension_command_epoch = self.extension_command_epoch.saturating_add(1);
        self.review_projection_generation = self.review_projection_generation.saturating_add(1);
        self.cancel_extension_dialogs_for_reload();
        self.exit_active_keyboard_mode();
        self.exit_active_file_view_mode();
        {
            let mut runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
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
        let mut changeset = changeset;
        self.apply_extension_file_languages(&mut changeset);
        let changeset = self.apply_extension_transforms(changeset);
        self.extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .line_highlights
            .reconcile_files(changeset.files.iter().map(|file| file.runtime_id.clone()));
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
        let immediate = self.update_extension_review_events(Instant::now());
        self.publish_extension_lifecycle_events(immediate);
        if emit_startup {
            self.publish_extension_lifecycle_event(ExtensionLifecycleEvent::Startup {
                cwd: self.extension_command_cwd(),
            });
        }
        self.publish_current_changeset_event(true, reason);
    }

    fn replace_extensions_and_reload(
        &mut self,
        changeset: Changeset,
        extensions: Vec<LoadedExtension>,
    ) {
        self.cancel_extension_dialogs_for_reload();
        self.exit_active_keyboard_mode();
        self.exit_active_file_view_mode();
        let mut replacement = ExtensionPaneRuntime::new(extensions, &changeset.files);
        let mut command_defaults = builtin_command_key_defaults();
        command_defaults.extend(extension_command_key_defaults(&replacement.commands));
        self.resolved_command_keys =
            resolve_command_keys(&command_defaults, &self.options.keybindings);
        let extension_command_table = build_extension_app_commands(
            &replacement.commands,
            &builtin_command_match_probes(Some(&self.resolved_command_keys)),
            Some(&self.resolved_command_keys),
        );
        replacement.app_commands = extension_command_table.commands;
        replacement.command_conflicts = extension_command_table.conflicts;
        let mut previous = {
            let mut runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            self.options.sidebar = replacement.reconcile_panes_from(&runtime, self.options.sidebar);
            replacement
                .line_highlights
                .retain_epochs_from(&runtime.line_highlights);
            std::mem::replace(&mut *runtime, replacement)
        };
        previous.retire_extensions();
        drop(previous);
        self.extension_registry_generation = self.extension_registry_generation.saturating_add(1);
        self.install_extension_event_context_provider();
        self.reload_with_reason(changeset, SessionReloadReason::Manual, true);
    }

    fn apply_extension_file_languages(&self, changeset: &mut Changeset) {
        let runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let file_languages = runtime.file_languages.clone();
        drop(runtime);
        let mut language_registry = LanguageRegistry::default();
        language_registry.replace_extensions(file_languages);
        for file in &mut changeset.files {
            let language = language_registry.language_for_path(&file.path);
            file.language = (language != "text").then_some(language);
        }
    }

    fn apply_extension_transforms(&self, mut changeset: Changeset) -> Changeset {
        let mut runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for extension in &mut runtime.extensions {
            changeset = extension.apply_changeset_transforms(changeset);
        }
        changeset.refresh_review_identities();
        changeset
    }

    pub fn set_status(&mut self, status: impl Into<String>) {
        self.status = Some(status.into());
    }

    pub fn tick_extension_notifications(&mut self, now: Instant) {
        let events = self.update_extension_review_events(now);
        self.publish_extension_lifecycle_events(events);
        self.startup_notices.tick(now);
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
    pub fn active_startup_notice(&self) -> Option<&str> {
        self.startup_notices.text()
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
            .map(|active| session_keyboard_mode_display_title(&active.session))
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
            .map(|active| session_keyboard_mode_status_hint(&active.session))
            .or_else(|| {
                runtime.active_file_view_mode.as_ref().map(|active| {
                    format!(
                        "{}:{} mode — Esc exits",
                        active.extension_id, active.view_id
                    )
                })
            })
    }

    /// Cursor requested by the focused status-bar filter input.
    #[must_use]
    pub fn status_filter_cursor_position(&self, area: Rect) -> Option<Position> {
        if let (Some(composer), Some(bounds)) =
            (self.note_composer.as_ref(), self.note_composer_bounds.get())
        {
            let input_width = usize::from(bounds.width.saturating_sub(2).max(1));
            let input_height = usize::from(bounds.height.saturating_sub(2).max(1));
            let (row, column) = note_composer_cursor_cell(
                &composer.body,
                composer.cursor,
                input_width,
                input_height,
            );
            return Some(Position::new(
                bounds
                    .x
                    .saturating_add(1)
                    .saturating_add(u16::try_from(column).unwrap_or(u16::MAX))
                    .min(bounds.right().saturating_sub(2)),
                bounds
                    .y
                    .saturating_add(1)
                    .saturating_add(u16::try_from(row).unwrap_or(u16::MAX))
                    .min(bounds.bottom().saturating_sub(2)),
            ));
        }
        if self.focus != Focus::Filter || area.width < 10 || area.height == 0 {
            return None;
        }
        let mode_width = status_bar_mode_width(
            self.active_keyboard_mode_status_hint().as_deref(),
            area.width,
        )
        .min(area.width.saturating_sub(2));
        let input_width = usize::from(
            area.width
                .saturating_sub(mode_width)
                .saturating_sub(11)
                .max(4),
        );
        let view = status_bar_input_view(
            &self.filter,
            self.filter_cursor,
            input_width,
            self.filter_scroll.get(),
        );
        self.filter_scroll.set(view.scroll);
        Some(Position::new(
            area.x
                .saturating_add(9)
                .saturating_add(u16::try_from(view.cursor_column).unwrap_or(u16::MAX))
                .min(area.right().saturating_sub(1)),
            area.y,
        ))
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
    pub const fn show_menu_bar(&self) -> bool {
        self.show_menu_bar
    }

    #[must_use]
    pub fn has_extension_input_dialog(&self) -> bool {
        matches!(
            self.extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .dialogs
                .current(),
            Some(ExtensionDialogRequest::Input(_))
        )
    }

    #[must_use]
    pub fn has_extension_select_dialog(&self) -> bool {
        matches!(
            self.extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .dialogs
                .current(),
            Some(ExtensionDialogRequest::Select(_))
        )
    }

    #[must_use]
    pub fn has_extension_confirm_dialog(&self) -> bool {
        matches!(
            self.extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .dialogs
                .current(),
            Some(ExtensionDialogRequest::Confirm(_))
        )
    }

    #[must_use]
    pub fn has_extension_dialog(&self) -> bool {
        self.extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .dialogs
            .current()
            .is_some()
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.handle_extension_trust_prompt_key(&key) {
            return;
        }
        if self.handle_note_composer_key(&key) {
            return;
        }
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
        if self.handle_filter_key(&key) {
            return;
        }
        if self.handle_app_menu_key(&key) {
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
        let live_key = to_live_extension_key_event(&key);
        let commands = self.builtin_commands();
        if vertical_command_direction(&commands, &live_key).is_some() {
            self.mouse_scroll_acceleration.reset();
            self.mouse_scroll_accumulator = 0.0;
        }
        if let Some(dispatch) = dispatch_app_command(&commands, &live_key) {
            if dispatch.closes_menu {
                self.close_app_menu();
            }
            let command_id = dispatch.command_id;
            self.apply_builtin_command_action(dispatch.action);
            self.publish_extension_lifecycle_event(ExtensionLifecycleEvent::CommandExecuted {
                command_id: command_id.into(),
            });
            return;
        }
        if self.invoke_extension_command(&key) {
            self.close_app_menu();
            return;
        }
        if key.code == KeyCode::Enter && self.focus == Focus::Sidebar {
            self.focus = Focus::Review;
            self.scroll_to_selection();
        }
    }

    fn handle_note_composer_key(&mut self, key: &KeyEvent) -> bool {
        if self.note_composer.is_none() {
            return false;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s') {
            self.save_note_composer();
            return true;
        }
        if key.code == KeyCode::Esc {
            self.note_composer = None;
            self.note_composer_bounds.set(None);
            self.status = Some("review note cancelled".into());
            return true;
        }
        let previous_body = self
            .note_composer
            .as_ref()
            .expect("composer was checked")
            .body
            .clone();
        {
            let composer = self.note_composer.as_mut().expect("composer was checked");
            match key.code {
                KeyCode::Left => composer.cursor = composer.cursor.saturating_sub(1),
                KeyCode::Right => {
                    composer.cursor = composer
                        .cursor
                        .saturating_add(1)
                        .min(composer.body.chars().count());
                }
                KeyCode::Home => composer.cursor = 0,
                KeyCode::End => composer.cursor = composer.body.chars().count(),
                KeyCode::Backspace => {
                    remove_filter_character_before(&mut composer.body, &mut composer.cursor);
                }
                KeyCode::Delete => {
                    remove_filter_character_at(&mut composer.body, &mut composer.cursor);
                }
                KeyCode::Enter => {
                    insert_filter_character(&mut composer.body, &mut composer.cursor, '\n');
                }
                KeyCode::Tab => {
                    for _ in 0..4 {
                        insert_filter_character(&mut composer.body, &mut composer.cursor, ' ');
                    }
                }
                KeyCode::Char(character)
                    if !key.modifiers.intersects(
                        KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                    ) =>
                {
                    insert_filter_character(&mut composer.body, &mut composer.cursor, character);
                }
                _ => {}
            }
            if composer.body.len() > workdeck_review::MAX_REVIEW_NOTE_BYTES {
                let mut end = workdeck_review::MAX_REVIEW_NOTE_BYTES;
                while !composer.body.is_char_boundary(end) {
                    end = end.saturating_sub(1);
                }
                composer.body.truncate(end);
                composer.cursor = composer.body.chars().count();
                self.status = Some("review note reached the size limit".into());
            }
        }
        let composer = self.note_composer.as_ref().expect("composer was checked");
        if composer.body != previous_body {
            self.publish_note_composer_edited(composer.clone());
        }
        true
    }

    fn handle_paste(&mut self, text: &str) {
        let Some(composer) = self.note_composer.as_mut() else {
            return;
        };
        let previous_body = composer.body.clone();
        let available = workdeck_review::MAX_REVIEW_NOTE_BYTES.saturating_sub(composer.body.len());
        let end = text
            .char_indices()
            .map(|(index, _)| index)
            .chain(std::iter::once(text.len()))
            .take_while(|index| *index <= available)
            .last()
            .unwrap_or_default();
        let pasted = &text[..end];
        for character in pasted.chars() {
            insert_filter_character(&mut composer.body, &mut composer.cursor, character);
        }
        let composer = composer.clone();
        if composer.body != previous_body {
            self.publish_note_composer_edited(composer);
        }
    }

    fn open_note_composer(&mut self) {
        let Some(target) = self.current_note_target() else {
            self.status = Some("select a changed review line before adding a note".into());
            return;
        };
        self.note_sequence = self.note_sequence.saturating_add(1);
        self.note_composer = Some(ReviewNoteComposer {
            id: format!("user-note-{}", self.note_sequence),
            target,
            body: String::new(),
            cursor: 0,
        });
        self.status = None;
    }

    fn extension_note_from_composer(
        &self,
        composer: &ReviewNoteComposer,
        body: String,
        draft: bool,
    ) -> Option<ExtensionReviewNote> {
        self.with_state(|state| {
            state
                .changeset()
                .files
                .get(composer.target.file_index)
                .map(|file| ExtensionReviewNote {
                    id: composer.id.clone(),
                    parent_id: None,
                    file_id: file.runtime_id.clone(),
                    file_path: file.path.clone(),
                    hunk_index: composer.target.hunk_index,
                    side: match composer.target.side {
                        ReviewSide::Old => workdeck_extension_api::ExtensionFileSide::Old,
                        ReviewSide::New => workdeck_extension_api::ExtensionFileSide::New,
                    },
                    line: composer.target.line,
                    body,
                    draft,
                })
        })
    }

    fn publish_note_composer_edited(&mut self, composer: ReviewNoteComposer) {
        if let Some(note) =
            self.extension_note_from_composer(&composer, composer.body.clone(), true)
        {
            self.publish_extension_lifecycle_event(ExtensionLifecycleEvent::NoteEdited { note });
        }
    }

    fn current_note_target(&self) -> Option<ReviewNoteTarget> {
        let rows = self.current_review_rows();
        rows.note_targets
            .get(&self.current_line_row)
            .copied()
            .or_else(|| {
                self.with_state(|state| {
                    let selection = state.selection();
                    let file = state.changeset().files.get(selection.file_index)?;
                    let hunk_index = selection.hunk_index?;
                    let hunk = file.hunks.get(hunk_index)?;
                    let (side, line) = selection.side.zip(selection.line).unwrap_or_else(|| {
                        let target = review_default_hunk_line_target(hunk);
                        (target.side, target.line)
                    });
                    Some(ReviewNoteTarget {
                        file_index: selection.file_index,
                        hunk_index,
                        side,
                        line,
                    })
                })
            })
    }

    fn save_note_composer(&mut self) {
        let Some(composer) = self.note_composer.take() else {
            return;
        };
        let body = composer.body.trim();
        if body.is_empty() {
            self.note_composer_bounds.set(None);
            self.status = Some("empty review note discarded".into());
            return;
        }
        let id = composer.id.clone();
        let extension_note = self.extension_note_from_composer(&composer, body.to_owned(), false);
        let result = self.with_state(|state| {
            let file = state
                .changeset()
                .files
                .get(composer.target.file_index)
                .ok_or(workdeck_review::ReviewError::FileOutOfRange(
                    composer.target.file_index,
                ))?;
            let anchor = review_line_anchor(
                &file.hunks,
                ReviewLineTarget {
                    hunk_index: composer.target.hunk_index,
                    side: composer.target.side,
                    line: composer.target.line,
                },
            );
            let comment = ReviewComment {
                id: id.clone(),
                parent_id: None,
                source: "user".into(),
                author: None,
                created_at: None,
                file_path: Some(file.path.clone()),
                hunk_index: Some(composer.target.hunk_index),
                side: Some(composer.target.side),
                line: Some(composer.target.line),
                summary: body.to_owned(),
                rationale: None,
                markup: None,
                title: None,
                tags: vec!["user".into()],
                confidence: None,
                updated_at: None,
                resolution: ReviewNoteResolution::Active,
                anchor: CommentAnchor {
                    file_key: file.key.clone(),
                    old_range: anchor.old_range,
                    new_range: anchor.new_range,
                    preferred_side: anchor.preferred.map(|target| target.side),
                    preferred_line: anchor.preferred.map(|target| target.line),
                    intersecting_hunk_indices: anchor.intersecting_hunk_indices,
                    owner_hunk_index: anchor.owner_hunk_index,
                },
                editable: true,
            };
            state.add_comment(comment)
        });
        self.note_composer_bounds.set(None);
        match result {
            Ok(()) => {
                self.status = Some("review note saved".into());
                if let Some(note) = extension_note {
                    self.publish_extension_lifecycle_event(ExtensionLifecycleEvent::NoteCreated {
                        note,
                    });
                }
                let events = self.update_extension_review_events(Instant::now());
                self.publish_extension_lifecycle_events(events);
            }
            Err(error) => self.status = Some(format!("failed to save review note: {error}")),
        }
    }

    /// The trust prompt is a security decision and owns every key while visible.
    fn handle_extension_trust_prompt_key(&mut self, key: &KeyEvent) -> bool {
        if !self.extension_trust_controller.prompt_open() {
            return false;
        }
        let decision = match key.code {
            KeyCode::Enter | KeyCode::Char('t') => {
                Some(workdeck_extension_host::TrustDecision::Trusted)
            }
            KeyCode::Char('n') => Some(workdeck_extension_host::TrustDecision::Denied),
            KeyCode::Esc => {
                self.extension_trust_controller.close();
                None
            }
            _ => None,
        };
        if let Some(decision) = decision {
            self.queue_extension_trust_decision(decision);
        }
        true
    }

    fn queue_extension_trust_decision(&mut self, decision: workdeck_extension_host::TrustDecision) {
        let Some(repo_root) = self
            .extension_trust_controller
            .prompt_root()
            .map(Path::to_owned)
        else {
            return;
        };
        self.extension_trust_controller.close();
        self.extension_trust_prompt_hits.set(None);
        self.extension_trust_request = Some(ExtensionTrustRequest {
            repo_root,
            decision,
        });
    }

    fn handle_filter_key(&mut self, key: &KeyEvent) -> bool {
        if self.focus != Focus::Filter {
            return false;
        }
        match key.code {
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Enter => self.focus = Focus::Review,
            KeyCode::Esc if self.filter.is_empty() => self.focus = Focus::Review,
            KeyCode::Esc => {
                self.filter.clear();
                self.filter_cursor = 0;
                self.filter_scroll.set(0);
            }
            KeyCode::Left => {
                self.filter_cursor = self.filter_cursor.saturating_sub(1);
            }
            KeyCode::Right => {
                self.filter_cursor = self
                    .filter_cursor
                    .saturating_add(1)
                    .min(self.filter.chars().count());
            }
            KeyCode::Home => self.filter_cursor = 0,
            KeyCode::End => self.filter_cursor = self.filter.chars().count(),
            KeyCode::Backspace => {
                remove_filter_character_before(&mut self.filter, &mut self.filter_cursor);
            }
            KeyCode::Delete => {
                remove_filter_character_at(&mut self.filter, &mut self.filter_cursor);
            }
            KeyCode::Char(character)
                if !key.modifiers.intersects(
                    KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                ) =>
            {
                insert_filter_character(&mut self.filter, &mut self.filter_cursor, character);
            }
            _ => {}
        }
        true
    }

    fn builtin_command_availability(&self) -> BuiltinCommandAvailability {
        let (can_edit_active_note, can_reply_to_active_note) = self.with_state(|state| {
            let selection = state.selection();
            let file_key = state
                .changeset()
                .files
                .get(selection.file_index)
                .map(|file| file.key.as_str());
            let active = state.comments().iter().filter(|comment| {
                comment.resolution == workdeck_review::ReviewNoteResolution::Active
                    && file_key == Some(comment.anchor.file_key.as_str())
                    && selection.hunk_index.is_some_and(|hunk_index| {
                        comment.anchor.owner_hunk_index == Some(hunk_index)
                            || comment
                                .anchor
                                .intersecting_hunk_indices
                                .contains(&hunk_index)
                    })
                    && (self.options.agent_notes || comment.source == "user")
            });
            let comments = active.collect::<Vec<_>>();
            (
                comments
                    .iter()
                    .any(|comment| comment.source == "user" && comment.editable),
                !comments.is_empty(),
            )
        });
        BuiltinCommandAvailability {
            can_align_current_line: self.options.cursor_line != CursorLineMode::Off,
            can_apply_file_presentation_to_all_matching: false,
            can_edit_active_note,
            can_reply_to_active_note,
            can_refresh_current_input: true,
        }
    }

    fn builtin_commands(&self) -> Vec<AppCommand> {
        build_app_commands(
            Some(&self.resolved_command_keys),
            self.builtin_command_availability(),
        )
    }

    fn extension_command_availability(&self) -> ExtensionCommandAvailability {
        ExtensionCommandAvailability {
            enabled: self
                .builtin_commands()
                .into_iter()
                .filter(|command| command.enabled && command.public_to_extensions)
                .flat_map(|command| {
                    std::iter::once(command.id.to_owned())
                        .chain(command.aliases.iter().map(|alias| (*alias).to_owned()))
                        .chain(
                            extension_command_controls::native_compatibility_aliases(command.id)
                                .map(str::to_owned),
                        )
                })
                .collect(),
        }
    }

    fn app_menus(&self) -> AppMenus {
        let builtins = self.builtin_commands();
        let mut commands = builtins
            .iter()
            .map(AppMenuCommand::from)
            .collect::<Vec<_>>();
        let (extension_commands, keyboard_mode_exit_entry) = {
            let runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let extension_commands = runtime
                .app_commands
                .iter()
                .map(|command| AppMenuCommand {
                    id: command.id.clone(),
                    title: sanitize_terminal_line(&command.title),
                    key_labels: command.key_labels.clone(),
                    enabled: true,
                })
                .collect::<Vec<_>>();
            let keyboard_mode_exit_entry = runtime
                .active_keyboard_mode
                .as_ref()
                .map(|active| active.mode.title.as_str())
                .or_else(|| {
                    runtime
                        .active_file_view_mode
                        .as_ref()
                        .map(|active| active.view_id.as_str())
                })
                .map(|title| MenuEntry::Item {
                    label: format!("Exit {title}"),
                    command_id: Some("workdeck.extensions.exitKeyboardMode".into()),
                    hint: None,
                    checked: None,
                });
            (extension_commands, keyboard_mode_exit_entry)
        };
        commands.extend(extension_commands.iter().cloned());
        build_app_menus(BuildAppMenusOptions {
            commands,
            extension_commands,
            file_view_entries: Vec::new(),
            keyboard_mode_exit_entry,
            file_view_apply_all_label: None,
            copy_decorations: self.copy_decorations,
            cursor_line: match self.options.cursor_line {
                CursorLineMode::Row => CommandCursorLine::Row,
                CursorLineMode::Number => CommandCursorLine::Number,
                CursorLineMode::Off => CommandCursorLine::Off,
            },
            layout_mode: self.layout(),
            files_pane_visible: self.options.sidebar,
            show_agent_notes: self.options.agent_notes,
            show_help: self.show_help,
            show_hunk_headers: self.options.hunk_headers,
            show_line_numbers: self.options.line_numbers,
            show_menu_bar: self.show_menu_bar,
            wrap_lines: self.options.wrap_lines,
        })
    }

    fn close_app_menu(&self) {
        self.extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .menu
            .close();
    }

    fn help_commands(&self) -> Vec<HelpCommand> {
        self.builtin_commands()
            .into_iter()
            .map(|command| HelpCommand {
                id: command.id.into(),
                key_labels: command.key_labels,
                enabled: command.enabled,
            })
            .collect()
    }

    fn apply_builtin_command_action(&mut self, action: AppCommandAction) {
        match action {
            AppCommandAction::ScrollDiff { delta, unit } => self.scroll_diff(delta, unit),
            AppCommandAction::RequestQuit => self.should_quit = true,
            AppCommandAction::ToggleHelp => self.show_help = !self.show_help,
            AppCommandAction::OpenAgentSkill => {
                self.status = Some("agent skill is available through `workdeck skill`".into());
            }
            AppCommandAction::ToggleFocusArea => {
                if self.focus == Focus::Filter {
                    self.focus = Focus::Review;
                } else {
                    self.focus = Focus::Filter;
                    self.filter_cursor = self.filter.chars().count();
                    self.filter_scroll.set(0);
                }
            }
            AppCommandAction::FocusFilter => {
                self.focus = Focus::Filter;
                self.filter_cursor = self.filter.chars().count();
                self.filter_scroll.set(0);
            }
            AppCommandAction::StartUserNote => {
                self.open_note_composer();
            }
            AppCommandAction::EditActiveNote => {
                self.status = Some("active review note editor is not active".into());
            }
            AppCommandAction::ReplyToActiveNote => {
                self.status = Some("review note reply composer is not active".into());
            }
            AppCommandAction::StepDiffLine(delta) => self.step_diff_line(delta),
            AppCommandAction::ScrollCodeHorizontally(delta) => {
                self.options.horizontal_offset =
                    self.options.horizontal_offset.saturating_add_signed(delta);
            }
            AppCommandAction::AlignCurrentLine(alignment) => {
                self.align_current_line(alignment);
            }
            AppCommandAction::SelectCursorLine(cursor) => {
                self.options.cursor_line = match cursor {
                    CommandCursorLine::Row => CursorLineMode::Row,
                    CommandCursorLine::Number => CursorLineMode::Number,
                    CommandCursorLine::Off => CursorLineMode::Off,
                };
            }
            AppCommandAction::SelectLayoutMode(layout) => {
                self.with_state(|state| state.set_layout(layout));
            }
            AppCommandAction::ApplyFilePresentationToAllMatching => {}
            AppCommandAction::ToggleFilesPane => self.toggle_files_pane_role(),
            AppCommandAction::RefreshCurrentInput => self.reload_requested = true,
            AppCommandAction::OpenThemeSelector => self.cycle_theme_preview(),
            AppCommandAction::ToggleAgentNotes => {
                self.options.agent_notes = !self.options.agent_notes;
            }
            AppCommandAction::ToggleLineNumbers => {
                self.options.line_numbers = !self.options.line_numbers;
            }
            AppCommandAction::ToggleLineWrap => {
                self.options.wrap_lines = !self.options.wrap_lines;
            }
            AppCommandAction::ToggleMenuBar => self.show_menu_bar = !self.show_menu_bar,
            AppCommandAction::ToggleHunkHeaders => {
                self.options.hunk_headers = !self.options.hunk_headers;
            }
            AppCommandAction::ToggleCopyDecorations => {
                self.copy_decorations = !self.copy_decorations;
            }
            AppCommandAction::ToggleGapForSelectedHunk => self.toggle_source_gap(),
            AppCommandAction::EditSelectedFile => self.editor_requested = true,
            AppCommandAction::MoveSelection { scope, delta } => {
                self.move_selection(scope, delta);
            }
        }
    }

    fn scroll_diff(&mut self, delta: isize, unit: ScrollUnit) {
        let rows = self.current_review_rows();
        let last = rows.lines.len().saturating_sub(1);
        let viewport = usize::from(self.review_height.get().saturating_sub(1).max(1));
        if unit == ScrollUnit::Content {
            if delta < 0 {
                self.scroll = 0;
                self.current_line_row = 0;
            } else {
                self.current_line_row = last;
                self.scroll = last.saturating_sub(viewport.saturating_sub(1));
            }
            return;
        }
        let rows_per_step = match unit {
            ScrollUnit::Step => 1,
            ScrollUnit::Viewport => viewport,
            ScrollUnit::Half => (viewport / 2).max(1),
            ScrollUnit::Content => unreachable!(),
        };
        let movement = isize::try_from(rows_per_step)
            .unwrap_or(isize::MAX)
            .saturating_mul(delta);
        self.scroll = self.scroll.saturating_add_signed(movement).min(last);
        self.current_line_row = self.scroll.min(last);
    }

    fn toggle_files_pane_role(&mut self) {
        let mut runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut open = runtime.open.clone();
        if self.options.sidebar {
            open.insert(WORKDECK_FILES_PANE_KEY.into());
        }
        let key = resolve_pane_slot_key(
            &runtime.session_panes,
            WORKDECK_FILES_PANE_KEY,
            &open,
            &BTreeSet::new(),
        );
        if key == WORKDECK_FILES_PANE_KEY {
            self.options.sidebar = !self.options.sidebar;
        } else if !runtime.open.remove(&key) {
            runtime.open.insert(key.clone());
        }
        runtime.cached_renders.remove(&key);
    }

    fn step_diff_line(&mut self, delta: isize) {
        let last = self.current_review_rows().lines.len().saturating_sub(1);
        let viewport = usize::from(self.review_height.get().saturating_sub(1).max(1));
        self.current_line_row = self.current_line_row.saturating_add_signed(delta).min(last);
        self.keep_current_line_visible(viewport, last);
    }

    fn align_current_line(&mut self, alignment: AppCommandLineAlignment) {
        let viewport = usize::from(self.review_height.get().saturating_sub(1).max(1));
        self.scroll = match alignment {
            AppCommandLineAlignment::Top => self.current_line_row,
            AppCommandLineAlignment::Center => self.current_line_row.saturating_sub(viewport / 2),
            AppCommandLineAlignment::Bottom => self
                .current_line_row
                .saturating_sub(viewport.saturating_sub(1)),
        };
    }

    fn cycle_theme_preview(&mut self) {
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

    fn move_selection(&mut self, scope: ReviewSelectionScope, delta: isize) {
        if delta == 0 {
            return;
        }
        let (selection, model) = self.with_state(|state| {
            let selection = state.selection();
            let mut annotations = SemanticReviewAnnotationIndex::default();
            let files = state
                .changeset()
                .files
                .iter()
                .map(|file| {
                    let mut annotated_hunks = review_annotated_hunk_indices(Some(file));
                    let mut annotated_file = file.agent.is_some();
                    for comment in state.comments().iter().filter(|comment| {
                        comment.resolution == workdeck_review::ReviewNoteResolution::Active
                            && comment.anchor.file_key == file.key
                            && (self.options.agent_notes || comment.source == "user")
                    }) {
                        annotated_file = true;
                        if let Some(owner) = comment.anchor.owner_hunk_index {
                            annotated_hunks.insert(owner);
                        }
                        annotated_hunks
                            .extend(comment.anchor.intersecting_hunk_indices.iter().copied());
                    }
                    if annotated_file {
                        annotations.annotated_file_keys.insert(file.key.clone());
                    }
                    if !annotated_hunks.is_empty() {
                        annotations
                            .annotated_hunk_indices_by_file_key
                            .insert(file.key.clone(), annotated_hunks);
                    }
                    ReviewNavigationFile {
                        file_key: file.key.clone(),
                        hunk_count: file.hunks.len(),
                    }
                })
                .collect();
            (
                SemanticReviewSelection {
                    file_key: state
                        .changeset()
                        .files
                        .get(selection.file_index)
                        .map(|file| file.key.clone()),
                    hunk_index: selection.hunk_index.unwrap_or(0),
                },
                ReviewNavigationModel { files, annotations },
            )
        });
        let Some(target) =
            plan_review_selection_move(&model, &selection, ReviewSelectionMove { scope, delta })
        else {
            return;
        };
        let Some(file_index) = self.with_state(|state| {
            state
                .changeset()
                .files
                .iter()
                .position(|file| file.key == target.file_key)
        }) else {
            return;
        };
        self.navigate(|state| {
            let previous = state.selection();
            let changed = if state
                .changeset()
                .files
                .get(file_index)
                .is_some_and(|file| target.hunk_index < file.hunks.len())
            {
                state.select_hunk(file_index, target.hunk_index).is_ok()
            } else {
                state.select_file(file_index).is_ok()
            };
            changed && state.selection() != previous
        });
    }

    fn invoke_extension_command(&mut self, key: &KeyEvent) -> bool {
        let key = to_live_extension_key_event(key);
        let command = {
            let runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            dispatch_extension_app_command(&runtime.app_commands, &key).cloned()
        };
        let Some(command) = command else {
            return false;
        };
        self.invoke_registered_extension_command(command);
        true
    }

    fn invoke_registered_extension_command(&mut self, command: RegisteredExtensionCommand) {
        let command_id = command.full_id();
        let command_epoch = self.extension_command_epoch;
        let (snapshot, review, workspace) = self.with_state(|state| {
            let workspace = self
                .options
                .review_input
                .as_ref()
                .zip(self.options.repo.as_deref())
                .map(|(input, root)| {
                    build_extension_workspace_snapshot(
                        &state.changeset().files,
                        input,
                        root,
                        command_epoch,
                    )
                });
            (
                state.snapshot(),
                build_extension_review_snapshot(state),
                workspace,
            )
        });
        let cwd = self.extension_command_cwd();
        let commands = self.extension_command_availability();
        {
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
            runtime
                .request_queues
                .entry(command.extension_index)
                .or_default()
                .push_back(QueuedExtensionRequest::Command(QueuedExtensionCommand {
                    pending: PendingExtensionCommand {
                        extension_index: command.extension_index,
                        extension_id: command.extension_id.clone(),
                        command_id: command.command.id.clone(),
                        title: command.command.title.clone(),
                        review_generation: command_epoch,
                    },
                    snapshot,
                    open_panes,
                    active_keyboard_mode,
                    cwd,
                    review,
                    commands,
                    workspace,
                }));
        }
        self.status = Some(command.command.title);
        self.start_queued_extension_requests(Some(command.extension_index));
        self.publish_extension_lifecycle_event(ExtensionLifecycleEvent::CommandExecuted {
            command_id,
        });
    }

    /// Apply every ready native command/event result without blocking the Ratatui event loop.
    pub fn poll_extension_commands(&mut self) {
        let (command_completions, event_completions) = {
            let mut runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let pending_indices = runtime.pending_commands.keys().copied().collect::<Vec<_>>();
            let mut command_completions = Vec::new();
            for extension_index in pending_indices {
                let Some(outcome) = runtime.extensions[extension_index].poll_command() else {
                    continue;
                };
                if let Some(pending) = runtime.pending_commands.remove(&extension_index) {
                    command_completions.push((pending, outcome));
                }
            }
            let pending_indices = runtime.pending_events.keys().copied().collect::<Vec<_>>();
            let mut event_completions = Vec::new();
            for extension_index in pending_indices {
                let Some(outcome) = runtime.extensions[extension_index].poll_event() else {
                    continue;
                };
                if let Some(pending) = runtime.pending_events.remove(&extension_index) {
                    event_completions.push((pending, outcome));
                }
            }
            (command_completions, event_completions)
        };

        for (pending, outcome) in command_completions {
            // Requests observed during a command retain their order and begin before any custom
            // event returned by that command is appended.
            self.start_queued_extension_requests(Some(pending.extension_index));
            match outcome {
                Ok(execution) => {
                    self.status = Some(pending.title.clone());
                    self.apply_extension_command_actions(pending, execution.actions);
                }
                Err(error) => {
                    let detail = extension_command_error_detail(&error);
                    self.report_extension_command_failure(
                        &pending.extension_id,
                        &pending.command_id,
                        &detail,
                    );
                }
            }
        }
        for (pending, outcome) in event_completions {
            match outcome {
                Ok(execution) => {
                    let previous_depth = self.extension_event_dispatch_depth;
                    self.extension_event_dispatch_depth = pending.dispatch_depth.saturating_add(1);
                    self.apply_extension_actions(
                        pending.extension_index,
                        &pending.extension_id,
                        execution.actions,
                    );
                    self.extension_event_dispatch_depth = previous_depth;
                }
                Err(error) => {
                    self.status = Some(format!(
                        "extension {} event {} failed: {error}",
                        pending.extension_id, pending.event_name
                    ));
                }
            }
            self.start_queued_extension_requests(Some(pending.extension_index));
        }
    }

    fn start_queued_extension_requests(&mut self, only_extension: Option<usize>) {
        let (command_failures, event_failures) = {
            let mut runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let extension_indices = only_extension.map_or_else(
                || runtime.request_queues.keys().copied().collect::<Vec<_>>(),
                |extension_index| vec![extension_index],
            );
            let mut command_failures = Vec::new();
            let mut event_failures = Vec::new();
            for extension_index in extension_indices {
                if runtime.pending_commands.contains_key(&extension_index)
                    || runtime.pending_events.contains_key(&extension_index)
                    || runtime.extensions[extension_index].request_pending()
                {
                    continue;
                }
                let queued = runtime
                    .request_queues
                    .get_mut(&extension_index)
                    .and_then(VecDeque::pop_front);
                let Some(queued) = queued else {
                    runtime.request_queues.remove(&extension_index);
                    continue;
                };
                if runtime
                    .request_queues
                    .get(&extension_index)
                    .is_some_and(VecDeque::is_empty)
                {
                    runtime.request_queues.remove(&extension_index);
                }
                match queued {
                    QueuedExtensionRequest::Command(queued) => {
                        let started = runtime.extensions[extension_index]
                            .begin_command_with_workspace_context(
                                &queued.pending.command_id,
                                queued.snapshot,
                                queued.open_panes,
                                queued.active_keyboard_mode,
                                queued.cwd,
                                Some(queued.review),
                                queued.commands,
                                queued.workspace,
                            );
                        match started {
                            Ok(()) => {
                                runtime
                                    .pending_commands
                                    .insert(extension_index, queued.pending);
                            }
                            Err(error) => {
                                runtime.request_queues.remove(&extension_index);
                                command_failures.push((queued.pending, error.to_string()));
                            }
                        }
                    }
                    QueuedExtensionRequest::Event(queued) => {
                        let extension_id = runtime.extensions[extension_index].manifest.id.clone();
                        let event_name = queued.event.name.clone();
                        match runtime.extensions[extension_index].begin_event(queued.event) {
                            Ok(()) => {
                                if runtime.extensions[extension_index].event_pending() {
                                    runtime.pending_events.insert(
                                        extension_index,
                                        PendingExtensionEvent {
                                            extension_index,
                                            extension_id,
                                            event_name,
                                            dispatch_depth: queued.dispatch_depth,
                                        },
                                    );
                                } else {
                                    runtime.request_queues.remove(&extension_index);
                                }
                            }
                            Err(error) => {
                                runtime.request_queues.remove(&extension_index);
                                event_failures.push((extension_id, event_name, error.to_string()))
                            }
                        }
                    }
                }
            }
            (command_failures, event_failures)
        };
        for (pending, error) in command_failures {
            self.report_extension_command_failure(
                &pending.extension_id,
                &pending.command_id,
                &error,
            );
        }
        if let Some((extension_id, event_name, error)) = event_failures.into_iter().last() {
            self.status = Some(format!(
                "extension {extension_id} event {event_name} failed: {error}"
            ));
        }
    }

    #[must_use]
    pub fn has_pending_extension_commands(&self) -> bool {
        let runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        !runtime.pending_commands.is_empty()
            || runtime.request_queues.values().any(|queue| {
                queue
                    .iter()
                    .any(|request| matches!(request, QueuedExtensionRequest::Command(_)))
            })
    }

    #[must_use]
    pub fn has_pending_extension_events(&self) -> bool {
        let runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        !runtime.pending_events.is_empty()
            || runtime.request_queues.values().any(|queue| {
                queue
                    .iter()
                    .any(|request| matches!(request, QueuedExtensionRequest::Event(_)))
            })
    }

    fn apply_extension_command_actions(
        &mut self,
        pending: PendingExtensionCommand,
        actions: Vec<ExtensionHostAction>,
    ) {
        let current_generation = self.extension_command_epoch;
        let mut live_actions = Vec::with_capacity(actions.len());
        for action in actions {
            if current_generation != pending.review_generation {
                let stale_method = match &action {
                    ExtensionHostAction::SelectReviewFile { .. } => Some("selectFile"),
                    ExtensionHostAction::SelectReviewHunk { .. } => Some("selectHunk"),
                    ExtensionHostAction::RevealReviewLine { .. } => Some("revealLine"),
                    _ => None,
                };
                if let Some(method) = stale_method {
                    self.status = Some(
                        extension_navigation_reloaded_warning(&pending.extension_id, method).0,
                    );
                    continue;
                }
            }
            if current_generation != pending.review_generation
                && let ExtensionHostAction::RequestWorkspaceWrite { request_id, .. } = action
            {
                self.complete_extension_workspace_write(
                    pending.extension_index,
                    &pending.extension_id,
                    request_id,
                    ExtensionWorkspaceWriteResult::Unavailable {
                        detail: "The review reloaded before this extension operation could finish."
                            .into(),
                    },
                );
                continue;
            }
            live_actions.push(action);
        }
        self.apply_extension_actions(pending.extension_index, &pending.extension_id, live_actions);
    }

    /// Contain native construction, protocol, process, timeout, and handler failures at the
    /// command boundary with Hunk's attributed warning shape.
    fn report_extension_command_failure(
        &mut self,
        extension_id: &str,
        command_id: &str,
        detail: &str,
    ) {
        let message = extension_command_failure_message(extension_id, command_id, detail);
        if let Some(notifications) = self.options.extension_notifications.as_ref() {
            notifications.notify(message.clone(), ExtensionNotifyType::Warning);
        }
        self.status = Some(message);
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
        let (active, still_valid) = {
            let runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let active = runtime.active_keyboard_mode.clone();
            let still_valid = active.as_ref().is_some_and(|active| {
                let registry = runtime
                    .extensions
                    .get(active.extension_index)
                    .map(LoadedExtension::registry);
                let registrations = runtime
                    .keyboard_modes
                    .iter()
                    .map(|mode| Arc::clone(&mode.registered))
                    .collect::<Vec<_>>();
                session_keyboard_mode_still_valid(
                    &active.session,
                    registry.as_ref(),
                    &registrations,
                )
            });
            (active, still_valid)
        };
        let Some(active) = active else {
            return false;
        };
        if !still_valid {
            self.exit_active_keyboard_mode();
            return false;
        }
        if key.code == KeyCode::Esc {
            self.exit_active_keyboard_mode();
            return true;
        }
        let snapshot = self.with_state(|state| state.snapshot());
        let commands = self.extension_command_availability();
        let result = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extensions[active.extension_index]
            .route_keyboard_mode_key_with_commands(
                &active.mode.id,
                to_live_extension_key_event(key),
                snapshot,
                commands,
            );
        match result {
            Ok(execution) => {
                self.apply_extension_actions_with_keyboard_authority(
                    active.extension_index,
                    &active.extension_id,
                    execution.actions,
                    KeyboardModeActionAuthority::ActiveKey(active.activation_id),
                );
                let result = self
                    .extension_pane_runtime
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .keyboard_mode_controller
                    .normalize_key_result(active.activation_id, execution.result);
                if result == KeyRoutingResult::Exit {
                    self.exit_active_keyboard_mode();
                }
                result != KeyRoutingResult::Pass
            }
            Err(error) => {
                self.exit_active_keyboard_mode();
                self.status = Some(format_keyboard_mode_failure(
                    &active.session,
                    "onKey",
                    &error.to_string(),
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
        self.apply_extension_actions_with_keyboard_authority(
            extension_index,
            extension_id,
            actions,
            KeyboardModeActionAuthority::Unscoped,
        );
    }

    fn apply_extension_actions_with_keyboard_authority(
        &mut self,
        extension_index: usize,
        extension_id: &str,
        actions: Vec<ExtensionHostAction>,
        keyboard_authority: KeyboardModeActionAuthority,
    ) {
        for action in actions {
            match action {
                ExtensionHostAction::OpenPane { id } => {
                    let mut runtime = self
                        .extension_pane_runtime
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    let Some(pane_key) =
                        resolve_pane_key(&runtime.session_panes, extension_id, &id)
                    else {
                        drop(runtime);
                        self.status = Some(format!(
                            "extension {extension_id}: warning: unknown pane {id:?}"
                        ));
                        continue;
                    };
                    runtime.open.insert(pane_key.clone());
                    runtime.cached_renders.remove(&pane_key);
                }
                ExtensionHostAction::ClosePane { id } => {
                    let mut runtime = self
                        .extension_pane_runtime
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    let Some(pane_key) =
                        resolve_pane_key(&runtime.session_panes, extension_id, &id)
                    else {
                        drop(runtime);
                        self.status = Some(format!(
                            "extension {extension_id}: warning: unknown pane {id:?}"
                        ));
                        continue;
                    };
                    runtime.open.remove(&pane_key);
                    runtime.cached_renders.remove(&pane_key);
                }
                ExtensionHostAction::RefreshPane { id } => {
                    let mut runtime = self
                        .extension_pane_runtime
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    let Some(pane_key) =
                        resolve_pane_key(&runtime.session_panes, extension_id, &id)
                    else {
                        drop(runtime);
                        self.status = Some(format!(
                            "extension {extension_id}: warning: unknown pane {id:?}"
                        ));
                        continue;
                    };
                    runtime.cached_renders.remove(&pane_key);
                }
                ExtensionHostAction::EnterKeyboardMode { id } => {
                    let allowed = self
                        .extension_pane_runtime
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .keyboard_mode_controller
                        .ownership_change_allowed(keyboard_authority);
                    if allowed {
                        self.enter_keyboard_mode(extension_index, extension_id, &id);
                    }
                }
                ExtensionHostAction::ExitKeyboardMode => {
                    let allowed = self
                        .extension_pane_runtime
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .keyboard_mode_controller
                        .ownership_change_allowed(keyboard_authority);
                    if allowed {
                        self.exit_keyboard_mode_for_extension(extension_index);
                    }
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
                ExtensionHostAction::RefreshLineHighlights { id, file_id } => {
                    let result = self
                        .extension_pane_runtime
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .line_highlights
                        .refresh(extension_id, &id, file_id.as_deref());
                    match result {
                        LineHighlightRefreshResult::InvalidHighlighterId => {
                            self.status = Some(format!(
                                "Extension {extension_id} targeted an invalid line highlighter id"
                            ));
                        }
                        LineHighlightRefreshResult::UnknownHighlighter => {
                            self.status = Some(format!(
                                "Extension {extension_id} targeted unknown line highlighter {id:?}"
                            ));
                        }
                        LineHighlightRefreshResult::Refreshed
                        | LineHighlightRefreshResult::StaleFile => {}
                    }
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
                    let queued = {
                        let mut runtime = self
                            .extension_pane_runtime
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        runtime.menu.close();
                        runtime.dialogs.enqueue_input(
                            extension_index,
                            extension_id,
                            id,
                            &title,
                            &placeholder,
                            initial.as_deref(),
                        )
                    };
                    self.finish_extension_dialog_enqueue(extension_id, queued);
                }
                ExtensionHostAction::OpenSelectDialog { id, title, options } => {
                    let queued = {
                        let mut runtime = self
                            .extension_pane_runtime
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        runtime.menu.close();
                        runtime.dialogs.enqueue_select(
                            extension_index,
                            extension_id,
                            id,
                            &title,
                            options,
                        )
                    };
                    self.finish_extension_dialog_enqueue(extension_id, queued);
                }
                ExtensionHostAction::OpenConfirmDialog {
                    id,
                    title,
                    body,
                    confirm_label,
                    cancel_label,
                } => {
                    let queued = {
                        let mut runtime = self
                            .extension_pane_runtime
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        runtime.menu.close();
                        runtime.dialogs.enqueue_confirm(
                            extension_index,
                            extension_id,
                            id,
                            &title,
                            &body,
                            &confirm_label,
                            cancel_label.as_deref(),
                        )
                    };
                    self.finish_extension_dialog_enqueue(extension_id, queued);
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

    fn finish_extension_dialog_enqueue(
        &mut self,
        extension_id: &str,
        queued: Result<Option<ExtensionDialogSettlement>, ExtensionDialogError>,
    ) {
        match queued {
            Ok(Some(settlement)) => self.settle_extension_dialog(settlement),
            Ok(None) => {}
            Err(error) => {
                self.status = Some(format!("extension {extension_id} dialog failed: {error}"));
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
        #[cfg(test)]
        self.observed_extension_events.push((
            self.extension_registry_generation,
            name.to_owned(),
            payload.clone(),
        ));
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
                .filter_map(|(index, extension)| {
                    let extension_id = extension.manifest.id.clone();
                    let prefix = format!("{extension_id}:");
                    let open_panes = runtime
                        .open
                        .iter()
                        .filter_map(|key| key.strip_prefix(&prefix).map(str::to_owned))
                        .collect();
                    let context = self.extension_event_context_provider.context(open_panes)?;
                    Some((index, extension_id, context))
                })
                .collect::<Vec<_>>()
        };
        if targets.is_empty() {
            return;
        }
        let (snapshot, review) =
            self.with_state(|state| (state.snapshot(), build_extension_review_snapshot(state)));
        for (extension_index, _extension_id, context) in targets {
            let event = ReviewEvent {
                name: name.into(),
                snapshot: snapshot.clone(),
                payload: payload.clone(),
                review: Some(review.clone()),
                context,
            };
            self.extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .request_queues
                .entry(extension_index)
                .or_default()
                .push_back(QueuedExtensionRequest::Event(QueuedExtensionEvent {
                    event,
                    dispatch_depth: self.extension_event_dispatch_depth,
                }));
        }
        self.start_queued_extension_requests(None);
    }

    fn publish_extension_lifecycle_event(&mut self, event: ExtensionLifecycleEvent) {
        let (name, payload) = event.into_parts();
        self.publish_extension_event(&name, payload);
    }

    fn publish_extension_lifecycle_events(
        &mut self,
        events: impl IntoIterator<Item = ExtensionLifecycleEvent>,
    ) {
        for event in events {
            self.publish_extension_lifecycle_event(event);
        }
    }

    fn publish_current_changeset_event(&mut self, reloaded: bool, reason: SessionReloadReason) {
        let changeset = self.with_state(|state| project_extension_changeset(state.changeset()));
        self.publish_extension_lifecycle_event(ExtensionLifecycleEvent::ChangesetLoaded {
            changeset: changeset.clone(),
        });
        if reloaded {
            self.publish_extension_lifecycle_event(ExtensionLifecycleEvent::SessionReload {
                changeset,
                reason,
            });
        }
    }

    fn extension_review_event_facts(&self) -> ExtensionReviewEventFacts {
        let (
            review_generation,
            review_notes,
            selected_file,
            selected_hunk_index,
            layout_mode,
            resolved_layout,
        ) = self.with_state(|state| {
            let selection = state.selection();
            (
                format!(
                    "generation:workdeck-tui:{}:{}",
                    state.generation(),
                    self.review_projection_generation
                ),
                project_extension_review_notes(state),
                state
                    .changeset()
                    .files
                    .get(selection.file_index)
                    .map(project_extension_diff_file),
                selection.hunk_index,
                state.layout(),
                state.resolved_layout(self.review_width.get()),
            )
        });
        let selected_file_id = selected_file.as_ref().map(|file| file.id.clone());
        ExtensionReviewEventFacts {
            registry_generation: self.extension_registry_generation,
            review_projection_generation: self.review_projection_generation,
            review_generation,
            review_notes,
            filter: self.filter.clone(),
            layout_mode: extension_layout_mode(layout_mode),
            resolved_layout: extension_resolved_layout(resolved_layout),
            selected_file,
            selected_file_id,
            selected_hunk_index,
            theme_id: self.options.theme.id.clone(),
        }
    }

    fn update_extension_review_events(&mut self, now: Instant) -> Vec<ExtensionLifecycleEvent> {
        let facts = self.extension_review_event_facts();
        let mut events = self.extension_review_events.update(&facts, now);
        events.extend(self.extension_review_events.settle_due(now));
        events
    }

    fn publish_extension_selection_events(&mut self) {
        let events = self.update_extension_review_events(Instant::now());
        self.publish_extension_lifecycle_events(events);
    }

    /// Inform subscribed extensions that watch mode has observed a source change.
    pub fn notify_watch_reload_pending(&mut self) {
        self.publish_extension_lifecycle_event(ExtensionLifecycleEvent::WatchReloadPending);
    }

    fn enter_keyboard_mode(&mut self, extension_index: usize, extension_id: &str, id: &str) {
        let local_id = id.strip_prefix(&format!("{extension_id}:")).unwrap_or(id);
        if local_id.trim().is_empty() {
            self.status = Some(format!(
                "Extension {extension_id} targeted an invalid keyboard mode id"
            ));
            return;
        }
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
                "Extension {extension_id} targeted unknown keyboard mode {local_id:?}"
            ));
            return;
        };
        self.exit_active_keyboard_mode();
        self.exit_active_file_view_mode();
        let activation_id = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .keyboard_mode_controller
            .activate();
        let Some(activation_id) = activation_id else {
            return;
        };
        let active = ActiveKeyboardMode {
            extension_index,
            extension_id: extension_id.into(),
            mode: registration.mode,
            activation_id,
            session: ActiveSessionKeyboardMode {
                extension_id: registration.extension_id,
                mode_id: registration.registered.mode.id.clone(),
                registered: registration.registered,
                registry: self
                    .extension_pane_runtime
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .extensions[extension_index]
                    .registry(),
            },
        };
        self.extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_keyboard_mode = Some(active.clone());
        let snapshot = self.with_state(|state| state.snapshot());
        let commands = self.extension_command_availability();
        let execution = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extensions[extension_index]
            .enter_keyboard_mode_with_commands(&active.mode.id, snapshot, commands);
        match execution {
            Ok(execution) => {
                self.status = Some(session_keyboard_mode_status_hint(&active.session));
                self.apply_extension_actions_with_keyboard_authority(
                    extension_index,
                    extension_id,
                    execution.actions,
                    KeyboardModeActionAuthority::Lifecycle,
                );
            }
            Err(error) => {
                self.exit_active_keyboard_mode();
                self.status = Some(format_keyboard_mode_failure(
                    &active.session,
                    "onEnter",
                    &error.to_string(),
                ));
            }
        }
    }

    fn exit_active_keyboard_mode(&mut self) {
        let (active, dialog) = {
            let mut runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let active = runtime.active_keyboard_mode.take();
            if let Some(active) = active.as_ref() {
                runtime
                    .keyboard_mode_controller
                    .retire(active.activation_id);
            }
            let dialog = active.as_ref().and_then(|active| {
                runtime
                    .dialogs
                    .cancel_current_for_extension(active.extension_index)
            });
            (active, dialog)
        };
        if let Some(dialog) = dialog {
            self.retire_extension_dialog(dialog);
        }
        let Some(active) = active else {
            return;
        };
        let snapshot = self.with_state(|state| state.snapshot());
        let commands = self.extension_command_availability();
        let execution = {
            let mut runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(extension) = runtime.extensions.get_mut(active.extension_index) else {
                return;
            };
            extension.exit_keyboard_mode_with_commands(&active.mode.id, snapshot, commands)
        };
        match execution {
            Ok(execution) => {
                self.status = Some(format!("{} exited", active.mode.title));
                self.apply_extension_actions_with_keyboard_authority(
                    active.extension_index,
                    &active.extension_id,
                    execution.actions,
                    KeyboardModeActionAuthority::Lifecycle,
                );
            }
            Err(error) => {
                self.status = Some(format_keyboard_mode_failure(
                    &active.session,
                    "onExit",
                    &error.to_string(),
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
        let Some(input) = self.options.review_input.as_ref() else {
            self.complete_extension_workspace_write(
                extension_index,
                extension_id,
                request_id,
                ExtensionWorkspaceWriteResult::Unavailable {
                    detail: "Workspace writes need a session that can reload; this review has no reloadable input.".into(),
                },
            );
            return;
        };
        let Some(root) = self.options.repo.as_ref() else {
            self.complete_extension_workspace_write(
                extension_index,
                extension_id,
                request_id,
                ExtensionWorkspaceWriteResult::Unavailable {
                    detail: "Workspace writes need the reviewed repository root.".into(),
                },
            );
            return;
        };
        let (review_generation, target) = self.with_state(|state| {
            (
                state.generation(),
                resolve_extension_workspace_write_target(
                    &file_id,
                    &state.changeset().files,
                    input,
                    root,
                ),
            )
        });
        let ExtensionWorkspaceWriteTarget::Writable {
            path,
            absolute_path,
        } = target
        else {
            self.complete_extension_workspace_write(
                extension_index,
                extension_id,
                request_id,
                ExtensionWorkspaceWriteResult::Unavailable {
                    detail: target
                        .detail()
                        .expect("unavailable workspace target has detail")
                        .to_owned(),
                },
            );
            return;
        };
        if let Some(detail) = verify_workspace_write_target(&absolute_path, &path, root) {
            self.complete_extension_workspace_write(
                extension_index,
                extension_id,
                request_id,
                ExtensionWorkspaceWriteResult::Unavailable { detail },
            );
            return;
        }
        let mut runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        runtime.menu.close();
        let refused = runtime
            .dialogs
            .enqueue_workspace(ExtensionWorkspaceWriteDialog {
                extension_index,
                extension_id: extension_id.into(),
                request_id,
                file_id,
                path: path.clone(),
                absolute_path,
                root: root.clone(),
                text,
                review_generation,
            });
        let visible = matches!(
            runtime.dialogs.current(),
            Some(ExtensionDialogRequest::Workspace { dialog, .. }) if dialog.path == path
        );
        drop(runtime);
        if let Some(settlement) = refused {
            self.settle_extension_dialog(settlement);
        } else if visible {
            self.status = Some(format!(
                "ext {extension_id}: write {path}? Enter confirms · Esc cancels"
            ));
        }
    }

    fn handle_workspace_write_key(&mut self, key: &KeyEvent) -> bool {
        let has_dialog = matches!(
            self.extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .dialogs
                .current(),
            Some(ExtensionDialogRequest::Workspace { .. })
        );
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
        let settlement = {
            let mut runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let request_id = runtime
                .dialogs
                .current()
                .map(ExtensionDialogRequest::request_id);
            request_id.and_then(|request_id| {
                if confirmed {
                    runtime.dialogs.accept(request_id, None)
                } else {
                    runtime.dialogs.cancel(request_id)
                }
            })
        };
        if let Some(settlement) = settlement {
            self.settle_extension_dialog(settlement);
        }
        true
    }

    fn settle_extension_dialog(&mut self, settlement: ExtensionDialogSettlement) {
        match (settlement.request, settlement.answer) {
            (ExtensionDialogRequest::Input(dialog), ExtensionDialogAnswer::Input(value)) => {
                self.submit_extension_input(dialog, value);
            }
            (ExtensionDialogRequest::Select(dialog), ExtensionDialogAnswer::Select(value)) => {
                self.submit_extension_select(dialog, value);
            }
            (
                ExtensionDialogRequest::Confirm(dialog),
                ExtensionDialogAnswer::Confirm(confirmed),
            ) => {
                self.submit_extension_confirm(dialog, confirmed);
            }
            (
                ExtensionDialogRequest::Workspace { dialog, .. },
                ExtensionDialogAnswer::Workspace(confirmed),
            ) => {
                let result = if confirmed {
                    match self.write_extension_workspace_document(&dialog) {
                        Ok(()) => ExtensionWorkspaceWriteResult::Written,
                        Err(failure) => failure.into_extension_result(),
                    }
                } else {
                    ExtensionWorkspaceWriteResult::Cancelled {
                        detail: format!("The write to {} was declined.", dialog.path),
                    }
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
            }
            _ => {
                self.status = Some("extension dialog queue returned a mismatched answer".into());
            }
        }
    }

    fn cancel_extension_dialogs_for_reload(&mut self) {
        let settlements = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .dialogs
            .cancel_all();
        for settlement in settlements {
            self.retire_extension_dialog(settlement);
        }
    }

    fn retire_extension_dialog(&mut self, settlement: ExtensionDialogSettlement) {
        let extension_id = settlement.request.extension_id().to_owned();
        let extension_index = settlement.request.extension_index();
        if let ExtensionDialogRequest::Workspace { dialog, .. } = settlement.request {
            let completion = ExtensionWorkspaceWriteCompletion {
                request_id: dialog.request_id,
                result: ExtensionWorkspaceWriteResult::Unavailable {
                    detail: "The review reloaded before this extension operation could finish."
                        .into(),
                },
            };
            let outcome = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .extensions
                .get_mut(extension_index)
                .map(|extension| extension.complete_workspace_write(completion));
            if let Some(Err(error)) = outcome {
                self.status = Some(format!(
                    "extension {extension_id} workspace write retirement failed: {error}"
                ));
            }
            return;
        }

        let (snapshot, review) =
            self.with_state(|state| (state.snapshot(), build_extension_review_snapshot(state)));
        let cwd = self.extension_command_cwd();
        let commands = self.extension_command_availability();
        let outcome = {
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
            let Some(extension) = runtime.extensions.get_mut(extension_index) else {
                return;
            };
            match (settlement.request, settlement.answer) {
                (ExtensionDialogRequest::Input(dialog), ExtensionDialogAnswer::Input(value)) => {
                    extension.submit_input_dialog_with_context(
                        &dialog.action_id,
                        value,
                        snapshot,
                        active_keyboard_mode,
                        cwd,
                        Some(review),
                        commands.clone(),
                    )
                }
                (ExtensionDialogRequest::Select(dialog), ExtensionDialogAnswer::Select(value)) => {
                    extension.submit_select_dialog_with_context(
                        &dialog.action_id,
                        value,
                        snapshot,
                        active_keyboard_mode,
                        cwd,
                        Some(review),
                        commands.clone(),
                    )
                }
                (
                    ExtensionDialogRequest::Confirm(dialog),
                    ExtensionDialogAnswer::Confirm(confirmed),
                ) => extension.submit_confirm_dialog_with_context(
                    &dialog.action_id,
                    confirmed,
                    snapshot,
                    active_keyboard_mode,
                    cwd,
                    Some(review),
                    commands,
                ),
                _ => return,
            }
        };
        if let Err(error) = outcome {
            self.status = Some(format!(
                "extension {extension_id} dialog retirement failed: {error}"
            ));
        }
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
        dialog: &ExtensionWorkspaceWriteDialog,
    ) -> Result<(), WorkspaceWriteFailure> {
        self.write_extension_workspace_document_with(dialog, |path, text| {
            std::fs::write(path, text)
        })
    }

    /// Cross the irreversible filesystem boundary synchronously on the Ratatui event loop.
    ///
    /// No reload or shutdown event can interleave after `writer` starts. Deliberately do not
    /// recheck the generation after it returns: like Hunk's tracked async runner, a started write
    /// reports its actual result and the caller schedules exactly one reconciliation.
    fn write_extension_workspace_document_with(
        &self,
        dialog: &ExtensionWorkspaceWriteDialog,
        writer: impl FnOnce(&std::path::Path, &str) -> std::io::Result<()>,
    ) -> Result<(), WorkspaceWriteFailure> {
        let generation = self.with_state(|state| state.generation());
        if generation != dialog.review_generation {
            return Err(WorkspaceWriteFailure::Unavailable(
                "The review reloaded before this extension operation could finish.".into(),
            ));
        }
        if let Some(detail) =
            verify_workspace_write_target(&dialog.absolute_path, &dialog.path, &dialog.root)
        {
            return Err(WorkspaceWriteFailure::Unavailable(detail));
        }
        let (source, file) = self.with_state(|state| {
            (
                state.changeset().source.clone(),
                state
                    .changeset()
                    .files
                    .iter()
                    .find(|file| file.runtime_id == dialog.file_id)
                    .cloned(),
            )
        });
        let Some(file) = file else {
            return Err(WorkspaceWriteFailure::Unavailable(
                "The review reloaded before this extension operation could finish.".into(),
            ));
        };
        if !matches!(source, ChangesetSource::WorkingTree { staged: false })
            || !file.sources.new.as_ref().is_some_and(|snapshot| {
                matches!(snapshot.origin, SourceOrigin::WorkingTree) && snapshot.attested
            })
        {
            return Err(WorkspaceWriteFailure::Unavailable(format!(
                "Failed to write {} • this review is not an attested working-tree diff",
                file.path
            )));
        }
        let expected = &file.sources.new.as_ref().expect("checked above").content;
        let current = std::fs::read_to_string(&dialog.absolute_path).map_err(|error| {
            WorkspaceWriteFailure::Failed(format!("Failed to write {} • {error}", file.path))
        })?;
        if &current != expected {
            return Err(WorkspaceWriteFailure::Unavailable(format!(
                "Failed to write {} • file changed since the review was loaded",
                file.path
            )));
        }
        writer(&dialog.absolute_path, &dialog.text).map_err(|error| {
            WorkspaceWriteFailure::Failed(format!("Failed to write {} • {error}", file.path))
        })
    }

    fn handle_extension_confirm_key(&mut self, key: &KeyEvent) -> bool {
        let has_dialog = matches!(
            self.extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .dialogs
                .current(),
            Some(ExtensionDialogRequest::Confirm(_))
        );
        if !has_dialog {
            return false;
        }
        let confirmed = match key.code {
            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => Some(true),
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => Some(false),
            _ => None,
        };
        if let Some(confirmed) = confirmed {
            let settlement = {
                let mut runtime = self
                    .extension_pane_runtime
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let request_id = runtime
                    .dialogs
                    .current()
                    .map(ExtensionDialogRequest::request_id);
                request_id.and_then(|request_id| {
                    if confirmed {
                        runtime.dialogs.accept(request_id, None)
                    } else {
                        runtime.dialogs.cancel(request_id)
                    }
                })
            };
            if let Some(settlement) = settlement {
                self.settle_extension_dialog(settlement);
            }
        }
        true
    }

    fn handle_extension_select_key(&mut self, key: &KeyEvent) -> bool {
        let has_dialog = matches!(
            self.extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .dialogs
                .current(),
            Some(ExtensionDialogRequest::Select(_))
        );
        if !has_dialog {
            return false;
        }
        match key.code {
            KeyCode::Enter | KeyCode::Esc => {
                let settlement = {
                    let mut runtime = self
                        .extension_pane_runtime
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    let selected = runtime.dialogs.current().map(|request| {
                        let value = match request {
                            ExtensionDialogRequest::Select(dialog) => {
                                dialog.options.get(dialog.selected).cloned()
                            }
                            _ => None,
                        };
                        (request.request_id(), value)
                    });
                    selected.and_then(|(request_id, value)| {
                        if key.code == KeyCode::Enter {
                            runtime.dialogs.accept(request_id, value)
                        } else {
                            runtime.dialogs.cancel(request_id)
                        }
                    })
                };
                if let Some(settlement) = settlement {
                    self.settle_extension_dialog(settlement);
                }
            }
            KeyCode::Down | KeyCode::Tab | KeyCode::Char('j') => {
                self.extension_pane_runtime
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .dialogs
                    .move_selection(1);
            }
            KeyCode::Up | KeyCode::BackTab | KeyCode::Char('k') => {
                self.extension_pane_runtime
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .dialogs
                    .move_selection(-1);
            }
            KeyCode::Home => {
                self.extension_pane_runtime
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .dialogs
                    .pick_option(0);
            }
            KeyCode::End => {
                let mut runtime = self
                    .extension_pane_runtime
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let last = match runtime.dialogs.current() {
                    Some(ExtensionDialogRequest::Select(dialog)) => {
                        dialog.options.len().saturating_sub(1)
                    }
                    _ => 0,
                };
                runtime.dialogs.pick_option(last);
            }
            _ => {}
        }
        true
    }

    fn handle_extension_input_key(&mut self, key: &KeyEvent) -> bool {
        let has_dialog = matches!(
            self.extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .dialogs
                .current(),
            Some(ExtensionDialogRequest::Input(_))
        );
        if !has_dialog {
            return false;
        }
        match key.code {
            KeyCode::Enter | KeyCode::Esc => {
                let settlement = {
                    let mut runtime = self
                        .extension_pane_runtime
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    let input = runtime.dialogs.current().map(|request| {
                        let value = match request {
                            ExtensionDialogRequest::Input(dialog) => Some(dialog.value.clone()),
                            _ => None,
                        };
                        (request.request_id(), value)
                    });
                    input.and_then(|(request_id, value)| {
                        if key.code == KeyCode::Enter {
                            runtime.dialogs.accept(request_id, value)
                        } else {
                            runtime.dialogs.cancel(request_id)
                        }
                    })
                };
                if let Some(settlement) = settlement {
                    self.settle_extension_dialog(settlement);
                }
            }
            KeyCode::Backspace => {
                let mut runtime = self
                    .extension_pane_runtime
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let next = match runtime.dialogs.current() {
                    Some(ExtensionDialogRequest::Input(dialog)) => {
                        let mut next = dialog.value.clone();
                        if let Some(grapheme) = next.graphemes(true).next_back() {
                            next.truncate(next.len() - grapheme.len());
                        }
                        Some(next)
                    }
                    _ => None,
                };
                if let Some(next) = next {
                    runtime.dialogs.update_input(next);
                }
            }
            KeyCode::Char(character)
                if !key.modifiers.intersects(
                    KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                ) =>
            {
                let mut runtime = self
                    .extension_pane_runtime
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let next = match runtime.dialogs.current() {
                    Some(ExtensionDialogRequest::Input(dialog)) => {
                        let mut next = dialog.value.clone();
                        next.push(character);
                        Some(next)
                    }
                    _ => None,
                };
                if let Some(next) = next {
                    runtime.dialogs.update_input(next);
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
        let commands = self.extension_command_availability();
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
                commands,
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
        let commands = self.extension_command_availability();
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
                commands,
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
        let commands = self.extension_command_availability();
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
                commands,
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
        let id = canonical_extension_review_command_id(id);
        let dispatch = extension_command_controls::execute_extension_command_with_count(
            &self.builtin_commands(),
            true,
            id,
            usize::from(count),
        );
        let Some(dispatch) = dispatch else {
            return false;
        };
        let command_id = dispatch.command_id;
        self.apply_builtin_command_action(dispatch.action);
        self.publish_extension_lifecycle_event(ExtensionLifecycleEvent::CommandExecuted {
            command_id: command_id.into(),
        });
        true
    }

    fn select_extension_review_file(&mut self, extension_id: &str, file_id: &str) {
        let target = self.with_state(|state| {
            let files = state
                .changeset()
                .files
                .iter()
                .map(|file| NavigableFile {
                    id: &file.runtime_id,
                    hunk_count: file.hunks.len(),
                })
                .collect::<Vec<_>>();
            guard_extension_select_file(extension_id, &files, true, file_id)
        });
        let target = match target {
            Ok(target) => target,
            Err(warning) => {
                self.status = Some(warning.0);
                return;
            }
        };
        if let Err(error) = self.with_state(|state| state.select_file(target.file_index)) {
            self.status =
                Some(extension_navigation_callback_warning(extension_id, "selectFile", error).0);
            return;
        }
        self.reconcile_active_file_view_mode();
        self.scroll_to_selection();
        self.publish_extension_selection_events();
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
        let (active, dialogs) = {
            let mut runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let active = runtime.active_file_view_mode.take();
            let dialogs = active.as_ref().map_or_else(Vec::new, |active| {
                runtime
                    .dialogs
                    .remove_workspace_for_file(active.extension_index, &active.file.id)
            });
            (active, dialogs)
        };
        for dialog in dialogs {
            self.retire_extension_dialog(dialog);
        }
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
            let Some(registration) = registrations
                .iter()
                .find(|registration| registered_file_view_key(&registration.view) == *view_key)
            else {
                continue;
            };
            if runtime.extensions[registration.extension_index].request_pending() {
                continue;
            }
            clear_file_view_component_state(&mut runtime, &file.runtime_id);
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

    fn prepare_extension_line_highlights(&self, changeset: &Changeset) -> LineHighlightMap {
        let mut runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let extensions = runtime
            .extensions
            .iter()
            .cloned()
            .map(|extension| Arc::new(extension) as Arc<dyn LineHighlightRuntime>)
            .collect::<Vec<_>>();
        let registrations = runtime.line_highlights.registrations().to_vec();
        let epochs = runtime.line_highlights.epochs().clone();
        runtime.line_highlight_preparation.reconcile(
            &extensions,
            &registrations,
            &epochs,
            &changeset.files,
        );
        runtime.line_highlight_preparation.resolved().clone()
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
        let target = self.with_state(|state| {
            let files = state
                .changeset()
                .files
                .iter()
                .map(|file| NavigableFile {
                    id: &file.runtime_id,
                    hunk_count: file.hunks.len(),
                })
                .collect::<Vec<_>>();
            guard_extension_select_hunk(
                extension_id,
                &files,
                true,
                file_id,
                Some(hunk_index as f64),
            )
        });
        let target = match target {
            Ok(target) => target,
            Err(warning) => {
                self.status = Some(warning.0);
                return;
            }
        };
        if let Err(error) =
            self.with_state(|state| state.select_hunk(target.file_index, target.hunk_index))
        {
            self.status =
                Some(extension_navigation_callback_warning(extension_id, "selectHunk", error).0);
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
        let target = self.with_state(|state| {
            let files = state
                .changeset()
                .files
                .iter()
                .map(|file| NavigableFile {
                    id: &file.runtime_id,
                    hunk_count: file.hunks.len(),
                })
                .collect::<Vec<_>>();
            guard_extension_reveal_line(
                extension_id,
                &files,
                true,
                file_id,
                match side {
                    ReviewSide::Old => "old",
                    ReviewSide::New => "new",
                },
                Some(f64::from(line)),
            )
        });
        let target = match target {
            Ok(target) => target,
            Err(warning) => {
                self.status = Some(warning.0);
                return;
            }
        };
        if self
            .with_state(|state| state.reveal_line(target.file_index, target.side, target.line))
            .is_err()
        {
            self.status = Some(
                extension_reveal_line_missing_warning(
                    extension_id,
                    file_id,
                    target.side,
                    target.line,
                )
                .0,
            );
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
        let line_highlights = self.prepare_extension_line_highlights(state.changeset());
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
            &line_highlights,
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

    fn install_extension_event_context_provider(&mut self) {
        let successor = self
            .extension_event_context_provider
            .install(self.extension_command_cwd());
        let predecessor = self.extension_event_context_installation.replace(successor);
        // The predecessor performs identity-checked cleanup. Dropping it after
        // installing the successor exercises the same stale-cleanup boundary
        // as a committed UI runtime replacement.
        drop(predecessor);
    }

    fn handle_app_menu_key(&mut self, key: &KeyEvent) -> bool {
        let menus = self.app_menus();
        let shortcut_target = if key.code == KeyCode::F(10) {
            Some(MenuId::File)
        } else if key.code == KeyCode::Menu
            || (key.code == KeyCode::Char('e') && key.modifiers == KeyModifiers::ALT)
        {
            Some(if menus.contains_key(&MenuId::Extensions) {
                MenuId::Extensions
            } else {
                MenuId::File
            })
        } else {
            None
        };
        let mut runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(target) = shortcut_target {
            runtime.menu.toggle(&menus, target);
            return true;
        }
        if runtime.menu.active_menu_id(&menus).is_none() {
            return false;
        }
        match key.code {
            KeyCode::Esc => {
                runtime.menu.close();
                return true;
            }
            KeyCode::Left => {
                runtime.menu.switch(&menus, -1);
                return true;
            }
            KeyCode::Right | KeyCode::Tab => {
                runtime.menu.switch(&menus, 1);
                return true;
            }
            KeyCode::Enter => {
                let command_id = runtime.menu.activate(&menus);
                drop(runtime);
                if let Some(command_id) = command_id {
                    self.execute_app_menu_command(&command_id);
                }
                return true;
            }
            _ => {}
        }
        drop(runtime);

        let direction =
            vertical_command_direction(&self.builtin_commands(), &to_live_extension_key_event(key))
                .map(|direction| direction.delta())
                .or(match key.code {
                    KeyCode::Up => Some(-1),
                    KeyCode::Down => Some(1),
                    _ => None,
                });
        if let Some(direction) = direction {
            self.extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .menu
                .move_item(&menus, direction);
            return true;
        }

        // An advertised single-key accelerator continues to the ordinary
        // command dispatcher, which closes the menu after a successful effect.
        false
    }

    fn execute_app_menu_command(&mut self, command_id: &str) {
        if command_id == "workdeck.extensions.exitKeyboardMode" {
            self.exit_active_extension_mode();
            self.publish_extension_lifecycle_event(ExtensionLifecycleEvent::CommandExecuted {
                command_id: command_id.into(),
            });
            return;
        }
        if let Some(dispatch) =
            execute_app_command_with_count(&self.builtin_commands(), command_id, 1)
        {
            let command_id = dispatch.command_id;
            self.apply_builtin_command_action(dispatch.action);
            self.publish_extension_lifecycle_event(ExtensionLifecycleEvent::CommandExecuted {
                command_id: command_id.into(),
            });
            return;
        }
        let command = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .app_commands
            .iter()
            .find(|command| command.id == command_id)
            .map(|command| command.registration.clone());
        if let Some(command) = command {
            self.invoke_registered_extension_command(command);
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
        let line_highlights = self.prepare_extension_line_highlights(state.changeset());
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
            &line_highlights,
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
        if self.extension_trust_controller.prompt_open() {
            if event.kind == MouseEventKind::Up(MouseButton::Left) {
                let hits = self.extension_trust_prompt_hits.get();
                match hits {
                    Some(hits) if rect_contains(hits.trust, event.column, event.row) => {
                        self.queue_extension_trust_decision(
                            workdeck_extension_host::TrustDecision::Trusted,
                        );
                    }
                    Some(hits) if rect_contains(hits.deny, event.column, event.row) => {
                        self.queue_extension_trust_decision(
                            workdeck_extension_host::TrustDecision::Denied,
                        );
                    }
                    Some(hits)
                        if rect_contains(hits.dismiss, event.column, event.row)
                            || rect_contains(hits.close, event.column, event.row)
                            || !rect_contains(hits.bounds, event.column, event.row) =>
                    {
                        self.extension_trust_controller.close();
                        self.extension_trust_prompt_hits.set(None);
                    }
                    _ => {}
                }
            }
            return;
        }
        if self.has_extension_dialog() {
            return;
        }
        if self.handle_extension_mode_badge_mouse(&event)
            || self.handle_app_menu_mouse(&event)
            || self.handle_extension_pane_mouse(&event)
            || self.handle_extension_file_view_mouse(&event)
            || self.handle_sidebar_mouse(&event)
            || self.handle_review_file_header_mouse(&event)
            || self.handle_horizontal_mouse_scroll(&event)
        {
            return;
        }
        self.handle_mouse_at(event.kind, Instant::now());
    }

    /// Route horizontal wheel gestures without leaving a fractional vertical
    /// remainder that can move the review on the next ordinary wheel event.
    fn handle_horizontal_mouse_scroll(&mut self, event: &MouseEvent) -> bool {
        if self.options.wrap_lines {
            return false;
        }
        let delta = match event.kind {
            MouseEventKind::ScrollLeft => -1,
            MouseEventKind::ScrollRight => 1,
            MouseEventKind::ScrollUp if event.modifiers.contains(KeyModifiers::SHIFT) => -1,
            MouseEventKind::ScrollDown if event.modifiers.contains(KeyModifiers::SHIFT) => 1,
            _ => return false,
        };
        self.options.horizontal_offset =
            self.options.horizontal_offset.saturating_add_signed(delta);
        self.mouse_scroll_acceleration.reset();
        self.mouse_scroll_accumulator = 0.0;
        true
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

    fn handle_review_file_header_mouse(&mut self, event: &MouseEvent) -> bool {
        if event.kind != MouseEventKind::Up(MouseButton::Left) {
            return false;
        }
        let target = self
            .review_file_header_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .rev()
            .find(|hit| rect_contains(hit.bounds, event.column, event.row))
            .map(|hit| hit.file_index);
        let Some(file_index) = target else {
            return false;
        };
        if self
            .with_state(|state| state.select_file(file_index))
            .is_ok()
        {
            self.reconcile_active_file_view_mode();
            self.publish_extension_selection_events();
        }
        true
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

    fn handle_app_menu_mouse(&mut self, event: &MouseEvent) -> bool {
        let menus = self.app_menus();
        let mut runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let trigger = runtime
            .menu_triggers
            .iter()
            .find(|(_, bounds)| rect_contains(*bounds, event.column, event.row))
            .map(|(id, _)| *id);
        if let Some(id) = trigger {
            match event.kind {
                MouseEventKind::Up(MouseButton::Left) => runtime.menu.toggle(&menus, id),
                MouseEventKind::Moved if runtime.menu.active_menu_id(&menus).is_some() => {
                    runtime.menu.open(&menus, id);
                }
                _ => return false,
            }
            return true;
        }
        if runtime.menu.active_menu_id(&menus).is_none() {
            return false;
        }
        let row = runtime.menu_bounds.and_then(|area| {
            if !rect_contains(area, event.column, event.row)
                || event.row == area.y
                || event.row + 1 == area.bottom()
            {
                return None;
            }
            Some(usize::from(event.row.saturating_sub(area.y + 1)))
        });
        if let Some(row) = row {
            let selectable = matches!(
                runtime.menu.active_entries(&menus).get(row),
                Some(MenuEntry::Item { .. })
            );
            if selectable && matches!(event.kind, MouseEventKind::Moved) {
                runtime.menu.set_selected_index(&menus, row);
                return true;
            }
            if selectable && event.kind == MouseEventKind::Up(MouseButton::Left) {
                runtime.menu.set_selected_index(&menus, row);
                let command_id = runtime.menu.activate(&menus);
                drop(runtime);
                if let Some(command_id) = command_id {
                    self.execute_app_menu_command(&command_id);
                }
                return true;
            }
            return true;
        }
        if event.kind == MouseEventKind::Up(MouseButton::Left) {
            runtime.menu.close();
        }
        false
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
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let planned = runtime.layout.panes.iter().find(|planned| {
                    planned.divider.is_some_and(|divider| {
                        rect_contains(
                            pane_divider_hit_area(divider, planned.pane.placement),
                            event.column,
                            event.row,
                        )
                    })
                });
                if let Some(planned) = planned {
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
                    return true;
                }
                let hit = runtime
                    .pane_action_hits
                    .iter()
                    .rev()
                    .find(|hit| rect_contains(hit.bounds, event.column, event.row))
                    .cloned();
                drop(runtime);
                if let Some(hit) = hit {
                    self.invoke_extension_pane_action(hit);
                    true
                } else {
                    false
                }
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

impl Drop for ReviewApp {
    fn drop(&mut self) {
        self.exit_active_keyboard_mode();
        self.exit_active_file_view_mode();
        let mut runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        runtime.keyboard_mode_controller.shutdown();
        let _settlements = runtime.dialogs.shutdown();
        runtime.retire_extensions();
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

fn canonical_extension_review_command_id(id: &str) -> &str {
    extension_command_controls::canonical_extension_review_command_id(id)
}

fn extension_command_failure_message(extension_id: &str, command_id: &str, detail: &str) -> String {
    format!("Extension {extension_id} failed command \"{command_id}\" • {detail}")
}

fn extension_command_error_detail(error: &HostError) -> std::borrow::Cow<'_, str> {
    match error {
        HostError::Remote { message, .. } => std::borrow::Cow::Borrowed(message),
        _ => std::borrow::Cow::Owned(error.to_string()),
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

const fn extension_layout_mode(layout: LayoutMode) -> ExtensionLayoutMode {
    match layout {
        LayoutMode::Auto => ExtensionLayoutMode::Auto,
        LayoutMode::Split => ExtensionLayoutMode::Split,
        LayoutMode::Stack => ExtensionLayoutMode::Stack,
    }
}

const fn extension_resolved_layout(layout: LayoutMode) -> ExtensionResolvedLayout {
    match layout {
        LayoutMode::Split => ExtensionResolvedLayout::Split,
        LayoutMode::Stack | LayoutMode::Auto => ExtensionResolvedLayout::Stack,
    }
}

pub fn run_review(changeset: Changeset, options: ReviewOptions) -> Result<()> {
    run_review_inner(changeset, options, Vec::new(), None, None, None)
}

pub fn run_review_with_extensions(
    changeset: Changeset,
    options: ReviewOptions,
    extensions: Vec<LoadedExtension>,
) -> Result<()> {
    run_review_inner(changeset, options, extensions, None, None, None)
}

pub fn run_review_with_reload<F>(
    changeset: Changeset,
    options: ReviewOptions,
    reload: &mut F,
) -> Result<()>
where
    F: FnMut() -> Result<Changeset>,
{
    run_review_inner(changeset, options, Vec::new(), None, None, Some(reload))
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
    run_review_inner(changeset, options, extensions, None, None, Some(reload))
}

/// Provider-neutral input and the signature captured before its initial content load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewWatchInput {
    pub input: workdeck_core::CliInput,
    pub cwd: PathBuf,
    pub initial_signature: Option<String>,
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
        Some((input, input_cwd, None)),
        None,
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
    run_review_with_extensions_input_reload_with_signature(
        changeset,
        options,
        extensions,
        ReviewWatchInput {
            input,
            cwd: input_cwd,
            initial_signature: None,
        },
        reload,
    )
}

/// Run a reloadable review using the signature captured before initial content I/O.
pub fn run_review_with_extensions_input_reload_with_signature<F>(
    changeset: Changeset,
    options: ReviewOptions,
    extensions: Vec<LoadedExtension>,
    watch_input: ReviewWatchInput,
    reload: &mut F,
) -> Result<()>
where
    F: FnMut() -> Result<Changeset>,
{
    run_review_inner(
        changeset,
        options,
        extensions,
        Some((
            watch_input.input,
            watch_input.cwd,
            watch_input.initial_signature,
        )),
        None,
        Some(reload),
    )
}

/// Run a native-extension-backed review with the exact catalog used for initial loading.
pub fn run_review_with_extensions_catalog_input_reload<F>(
    changeset: Changeset,
    options: ReviewOptions,
    extensions: Vec<LoadedExtension>,
    input: workdeck_core::CliInput,
    input_cwd: PathBuf,
    vcs_catalog: workdeck_vcs::VcsCatalog,
    reload: &mut F,
) -> Result<()>
where
    F: FnMut() -> Result<Changeset>,
{
    run_review_with_extensions_catalog_input_reload_with_signature(
        changeset,
        options,
        extensions,
        ReviewWatchInput {
            input,
            cwd: input_cwd,
            initial_signature: None,
        },
        vcs_catalog,
        reload,
    )
}

/// Catalog-aware variant retaining the pre-load watch signature.
pub fn run_review_with_extensions_catalog_input_reload_with_signature<F>(
    changeset: Changeset,
    options: ReviewOptions,
    extensions: Vec<LoadedExtension>,
    watch_input: ReviewWatchInput,
    vcs_catalog: workdeck_vcs::VcsCatalog,
    reload: &mut F,
) -> Result<()>
where
    F: FnMut() -> Result<Changeset>,
{
    run_review_inner(
        changeset,
        options,
        extensions,
        Some((
            watch_input.input,
            watch_input.cwd,
            watch_input.initial_signature,
        )),
        Some(vcs_catalog),
        Some(reload),
    )
}

fn run_review_inner(
    changeset: Changeset,
    mut options: ReviewOptions,
    extensions: Vec<LoadedExtension>,
    watch_input: Option<(workdeck_core::CliInput, PathBuf, Option<String>)>,
    watch_vcs_catalog: Option<workdeck_vcs::VcsCatalog>,
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
    options.review_input = watch_input.as_ref().map(|(input, _, _)| input.clone());
    let mut app = ReviewApp::new_with_extensions(changeset, options, extensions);
    let watch_vcs_catalog =
        watch_vcs_catalog.unwrap_or_else(|| workdeck_vcs::bundled_vcs_catalog().clone());
    let mut watched_input =
        watch_input
            .filter(|_| app.options.watch)
            .and_then(|(input, cwd, initial_signature)| {
                let runtime: Arc<dyn WatchedInputRuntime> = Arc::new(
                    NativeWatchedInputRuntime::new(cwd, Some(watch_vcs_catalog.clone())),
                );
                match WatchedInputDriver::start(
                    true,
                    input,
                    runtime,
                    initial_signature,
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

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut ReviewApp,
    session_stop: Option<&AtomicBool>,
    session_reload: Option<&AtomicBool>,
    reloader: &mut Option<&mut dyn FnMut() -> Result<Changeset>>,
    watched_input: &mut Option<WatchedInputDriver>,
) -> Result<()> {
    let mut next_reload = Instant::now() + Duration::from_millis(250);
    while !app.should_quit && !session_stop.is_some_and(|stop| stop.load(Ordering::Relaxed)) {
        app.poll_extension_commands();
        app.tick_extension_notifications(Instant::now());
        terminal.draw(|frame| {
            let area = frame.area();
            render(area, frame.buffer_mut(), app);
            let footer = Rect::new(
                area.x,
                area.bottom().saturating_sub(1),
                area.width,
                u16::from(area.height > 0),
            );
            if let Some(position) = app.status_filter_cursor_position(footer) {
                frame.set_cursor_position(position);
            }
        })?;
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => {
                    app.handle_key(key);
                    if let Some(request) = app.take_editor_request()
                        && let Some(message) = open_review_editor_in_crossterm(terminal, &request)
                    {
                        app.status = Some(message);
                    }
                }
                Event::Mouse(mouse) => app.handle_mouse_event(mouse),
                Event::Paste(text) => app.handle_paste(&text),
                Event::Resize(_, _) | Event::FocusGained | Event::FocusLost => {}
            }
            app.process_extension_trust_request(reloader);
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
            let reason = reload_request_reason(manual_requested, session_requested);
            reload_current_review(app, reloader, manual_requested || session_requested, reason);
        }
        if let Some(driver) = watched_input {
            let outcome = driver.poll(Instant::now(), &mut || {
                let Some(reload) = reloader.as_deref_mut() else {
                    anyhow::bail!("this review input cannot be reloaded");
                };
                let changeset = reload()?;
                apply_reloaded_changeset(app, changeset, SessionReloadReason::Watch);
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

const fn reload_request_reason(
    manual_requested: bool,
    session_requested: bool,
) -> SessionReloadReason {
    if manual_requested {
        SessionReloadReason::Manual
    } else if session_requested {
        SessionReloadReason::Daemon
    } else {
        SessionReloadReason::Watch
    }
}

fn reload_current_review(
    app: &mut ReviewApp,
    reloader: &mut Option<&mut dyn FnMut() -> Result<Changeset>>,
    report_unavailable: bool,
    reason: SessionReloadReason,
) {
    match reloader.as_deref_mut() {
        Some(reload) => match reload() {
            Ok(changeset) => apply_reloaded_changeset(app, changeset, reason),
            Err(error) => app.status = Some(format!("reload failed: {error:#}")),
        },
        None if report_unavailable => {
            app.status = Some("this review input cannot be reloaded".into());
        }
        None => {}
    }
}

fn apply_reloaded_changeset(
    app: &mut ReviewApp,
    changeset: Changeset,
    reason: SessionReloadReason,
) {
    app.reload_with_reason(changeset, reason, false);
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
            Constraint::Length(u16::from(app.show_menu_bar)),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);
    render_app_menu_bar(outer[0], buffer, app);
    render_body(outer[1], buffer, app);
    render_footer(outer[2], buffer, app);
    render_app_menu_dropdown(area, buffer, app);
    if app.show_help {
        let commands = app.help_commands();
        render_help(area, buffer, &commands);
    }
    render_note_composer(area, buffer, app);
    render_extension_input_dialog(area, buffer, app);
    render_extension_select_dialog(area, buffer, app);
    render_extension_confirm_dialog(area, buffer, app);
    render_extension_workspace_write_dialog(area, buffer, app);
    render_extension_trust_prompt(area, buffer, app);
}

/// Draw the host-owned repository-extension security decision above the review.
pub fn render_extension_trust_prompt(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    let Some(repo_root) = app.extension_trust_controller.prompt_root() else {
        app.extension_trust_prompt_hits.set(None);
        return;
    };
    let width = 72.min(area.width.saturating_sub(2).max(1));
    let height = 12.min(area.height.saturating_sub(2).max(1));
    let bounds = Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    );
    Clear.render(bounds, buffer);
    let panel = ratatui_theme_color(&app.options.theme.panel);
    let text = ratatui_theme_color(&app.options.theme.text);
    let muted = ratatui_theme_color(&app.options.theme.muted);
    let accent = ratatui_theme_color(&app.options.theme.accent);
    let neutral = ratatui_theme_color(&app.options.theme.badge_neutral);
    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().bg(panel))
        .border_style(Style::default().fg(accent));
    let inner = block.inner(bounds);
    block.render(bounds, buffer);
    if inner.height == 0 || inner.width == 0 {
        app.extension_trust_prompt_hits
            .set(Some(ExtensionTrustPromptHits {
                bounds,
                close: Rect::default(),
                trust: Rect::default(),
                dismiss: Rect::default(),
                deny: Rect::default(),
            }));
        return;
    }
    let content = Rect::new(
        inner.x.saturating_add(u16::from(inner.width > 1)),
        inner.y,
        inner.width.saturating_sub(2),
        inner.height,
    );
    let row = |offset: u16| {
        Rect::new(
            content.x,
            content.y.saturating_add(offset),
            content.width,
            u16::from(offset < content.height),
        )
    };
    let close_width = content.width.min(5);
    let close = Rect::new(
        content.right().saturating_sub(close_width),
        content.y.saturating_add(1),
        close_width,
        u16::from(content.height > 1),
    );
    let title = row(1);
    Paragraph::new(Line::styled(
        "Run this repository's extensions?",
        Style::default().fg(text),
    ))
    .render(
        Rect::new(
            title.x,
            title.y,
            title.width.saturating_sub(close_width.saturating_add(1)),
            title.height,
        ),
        buffer,
    );
    Paragraph::new(Line::styled("[Esc]", Style::default().fg(neutral)))
        .alignment(Alignment::Right)
        .render(close, buffer);
    Paragraph::new(Line::styled(
        "This repository contains extensions in .agents/workdeck/extensions.",
        Style::default().fg(muted),
    ))
    .render(row(2), buffer);
    Paragraph::new(Line::styled(
        "Extensions run with your user permissions.",
        Style::default().fg(muted),
    ))
    .render(row(3), buffer);
    Paragraph::new(Line::styled(
        repo_root.display().to_string(),
        Style::default().fg(neutral),
    ))
    .render(row(5), buffer);
    Paragraph::new(Line::styled(
        "Trust runs them now and remembers this repo; never won't ask again.",
        Style::default().fg(muted),
    ))
    .render(row(6), buffer);
    Paragraph::new(Line::from(vec![
        Span::raw(" "),
        Span::styled("enter/t", Style::default().fg(accent)),
        Span::styled(" trust ", Style::default().fg(muted)),
        Span::styled("·", Style::default().fg(neutral)),
        Span::raw(" "),
        Span::styled("esc", Style::default().fg(accent)),
        Span::styled(" not now ", Style::default().fg(muted)),
        Span::styled("·", Style::default().fg(neutral)),
        Span::raw(" "),
        Span::styled("n", Style::default().fg(accent)),
        Span::styled(" never ", Style::default().fg(muted)),
    ]))
    .render(row(8), buffer);
    let action_y = content.y.saturating_add(8);
    let trust = Rect::new(content.x, action_y, content.width.min(15), 1);
    let dismiss_x = content.x.saturating_add(18);
    let dismiss = Rect::new(
        dismiss_x,
        action_y,
        content.right().saturating_sub(dismiss_x).min(13),
        1,
    );
    let deny_x = content.x.saturating_add(34);
    let deny = Rect::new(
        deny_x,
        action_y,
        content.right().saturating_sub(deny_x).min(9),
        1,
    );
    app.extension_trust_prompt_hits
        .set(Some(ExtensionTrustPromptHits {
            bounds,
            close,
            trust,
            dismiss,
            deny,
        }));
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
    render_extension_input_dialog(area, buffer, app);
    render_extension_select_dialog(area, buffer, app);
    render_extension_confirm_dialog(area, buffer, app);
    render_extension_workspace_write_dialog(area, buffer, app);
    render_extension_trust_prompt(area, buffer, app);
}

/// Render Hunk's desktop-style top menu bar with a one-cell outer gutter.
pub fn render_app_menu_bar(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    let menus = app.app_menus();
    let specs = build_menu_specs(&menus);
    let mut runtime = app
        .extension_pane_runtime
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if area.width == 0 || area.height == 0 {
        runtime.menu_triggers.clear();
        return;
    }
    let active = runtime.menu.active_menu_id(&menus);
    runtime.menu_triggers = specs
        .iter()
        .filter_map(|spec| {
            let left = u16::try_from(spec.left).ok()?;
            let width = u16::try_from(spec.width).ok()?;
            (left < area.width).then_some((
                spec.id,
                Rect::new(
                    area.x.saturating_add(left),
                    area.y,
                    width.min(area.width.saturating_sub(left)),
                    1,
                ),
            ))
        })
        .collect();
    let triggers = runtime.menu_triggers.clone();
    drop(runtime);

    let theme = &app.options.theme;
    if area.width > 2 {
        Block::default()
            .style(Style::default().bg(ratatui_theme_color(&theme.panel_alt)))
            .render(Rect::new(area.x + 1, area.y, area.width - 2, 1), buffer);
    }
    for (id, trigger) in triggers {
        let is_active = active == Some(id);
        let style = Style::default()
            .fg(ratatui_theme_color(if is_active {
                &theme.text
            } else {
                &theme.muted
            }))
            .bg(ratatui_theme_color(if is_active {
                &theme.accent_muted
            } else {
                &theme.panel_alt
            }));
        Paragraph::new(Line::styled(format!(" {} ", id.label()), style)).render(trigger, buffer);
    }

    let title_width = menu_bar_title_width(&specs, usize::from(area.width));
    if title_width > 0 {
        let title = app
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .changeset()
            .title
            .clone();
        let width = u16::try_from(title_width)
            .unwrap_or(u16::MAX)
            .min(area.width.saturating_sub(1));
        let title_area = Rect::new(
            area.right().saturating_sub(width.saturating_add(1)),
            area.y,
            width,
            1,
        );
        let badge = " Workdeck ";
        let context_width = title_width.saturating_sub(badge.width() + 1);
        Paragraph::new(Line::from(vec![
            Span::styled(
                badge,
                Style::default()
                    .fg(ratatui_theme_color(&theme.background))
                    .bg(ratatui_theme_color(&theme.accent))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {}", fit_text(&title, context_width, None)),
                Style::default()
                    .fg(ratatui_theme_color(&theme.muted))
                    .bg(ratatui_theme_color(&theme.panel_alt)),
            ),
        ]))
        .alignment(Alignment::Right)
        .render(title_area, buffer);
    }
}

/// Compatibility entrypoint retained for the unified shell while it migrates
/// from the extension-only button to the complete application menu bar.
pub fn render_extension_menu_button(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    render_app_menu_bar(area, buffer, app);
}

/// Draw the dropdown belonging to the currently active top-level menu.
pub fn render_app_menu_dropdown(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    let menus = app.app_menus();
    let specs = build_menu_specs(&menus);
    let mut runtime = app
        .extension_pane_runtime
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(active_id) = runtime.menu.active_menu_id(&menus) else {
        runtime.menu_bounds = None;
        return;
    };
    let Some(spec) = specs.iter().find(|spec| spec.id == active_id) else {
        runtime.menu.close();
        runtime.menu_bounds = None;
        return;
    };
    let entries = runtime.menu.active_entries(&menus).to_vec();
    if entries.is_empty() || area.width == 0 || area.height == 0 {
        runtime.menu.close();
        runtime.menu_bounds = None;
        return;
    }
    let desired_width = menu_width(&entries).saturating_add(2);
    let maximum_width = usize::from(area.width.saturating_sub(2)).max(22);
    let width = u16::try_from(desired_width.min(maximum_width))
        .unwrap_or(u16::MAX)
        .min(area.width.max(1));
    let top = area.y.saturating_add(u16::from(app.show_menu_bar));
    let available_height = area.bottom().saturating_sub(top);
    let height = u16::try_from(menu_box_height(&entries))
        .unwrap_or(u16::MAX)
        .min(available_height.max(1));
    let desired_left = u16::try_from(spec.left).unwrap_or(u16::MAX);
    let maximum_left = area.width.saturating_sub(width).saturating_sub(1);
    let minimum_left = if area.width > 1 { 1 } else { 0 };
    let x = area
        .x
        .saturating_add(desired_left.min(maximum_left).max(minimum_left));
    let bounds = Rect::new(x, top, width, height);
    runtime.menu_bounds = Some(bounds);
    let selected = runtime.menu.selected_index(&menus);
    drop(runtime);

    Clear.render(bounds, buffer);
    let block = Block::default()
        .title(format!(" {} ", active_id.label()))
        .borders(Borders::ALL)
        .style(Style::default().bg(ratatui_theme_color(&app.options.theme.panel)))
        .border_style(Style::default().fg(ratatui_theme_color(&app.options.theme.border)));
    let inner = block.inner(bounds);
    block.render(bounds, buffer);
    for (index, entry) in entries.iter().enumerate().take(usize::from(inner.height)) {
        let row = Rect::new(inner.x, inner.y + index as u16, inner.width, 1);
        let style = if index == selected {
            Style::default()
                .fg(ratatui_theme_color(&app.options.theme.text))
                .bg(ratatui_theme_color(&app.options.theme.accent_muted))
        } else {
            Style::default()
                .fg(ratatui_theme_color(&app.options.theme.text))
                .bg(ratatui_theme_color(&app.options.theme.panel))
        };
        match entry {
            MenuEntry::Separator => {
                Paragraph::new(Line::styled(
                    pad_text(
                        &"─".repeat(usize::from(inner.width.saturating_sub(2))),
                        usize::from(inner.width),
                    ),
                    Style::default()
                        .fg(ratatui_theme_color(&app.options.theme.border))
                        .bg(ratatui_theme_color(&app.options.theme.panel)),
                ))
                .render(row, buffer);
            }
            MenuEntry::Item {
                label,
                hint,
                checked,
                ..
            } => {
                let prefix = match checked {
                    None => format!("  {label}"),
                    Some(true) => format!("[x] {label}"),
                    Some(false) => format!("[ ] {label}"),
                };
                let hint = hint.as_deref().unwrap_or("");
                let hint_width = hint.width();
                let gap = usize::from(hint_width > 0);
                let left_width = usize::from(inner.width).saturating_sub(hint_width + gap);
                let left = pad_text(&fit_text(&prefix, left_width, None), left_width);
                let mut spans = vec![Span::styled(left, style)];
                if gap > 0 {
                    spans.push(Span::styled(" ", style));
                    spans.push(Span::styled(
                        hint.to_owned(),
                        if index == selected {
                            style
                        } else {
                            style.fg(ratatui_theme_color(&app.options.theme.muted))
                        },
                    ));
                }
                Paragraph::new(Line::from(spans)).render(row, buffer);
            }
        }
    }
}

/// Legacy name retained for callers compiled against the extension-only menu.
pub fn render_extension_command_menu(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    render_app_menu_dropdown(area, buffer, app);
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
    let width = status_bar_mode_width(Some(&hint), area.width).min(area.width.saturating_sub(2));
    let bounds = Rect::new(
        area.right().saturating_sub(width.saturating_add(1)),
        area.y,
        width,
        1,
    );
    runtime.mode_badge_bounds = Some(bounds);
    drop(runtime);
    Block::default()
        .style(Style::default().bg(ratatui_theme_color(&app.options.theme.badge_neutral)))
        .render(bounds, buffer);
    Paragraph::new(Line::styled(
        format!(" {hint} "),
        Style::default()
            .fg(ratatui_theme_color(&app.options.theme.panel_alt))
            .bg(ratatui_theme_color(&app.options.theme.badge_neutral)),
    ))
    .wrap(Wrap { trim: false })
    .render(bounds, buffer);
}

fn extension_dialog_title(title: &str, extension_id: &str, show_attribution: bool) -> String {
    if show_attribution {
        format!("{title} · {extension_id}")
    } else {
        title.into()
    }
}

/// Draw the host-owned input modal requested by a native extension.
pub fn render_extension_input_dialog(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    let dialog = app
        .extension_pane_runtime
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .dialogs
        .current()
        .and_then(|request| match request {
            ExtensionDialogRequest::Input(dialog) => Some(dialog.clone()),
            _ => None,
        });
    let Some(dialog) = dialog else {
        return;
    };
    let title =
        extension_dialog_title(&dialog.title, &dialog.extension_id, dialog.show_attribution);
    let desired_width = dialog
        .title
        .width()
        .max(title.width())
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
        .title(format!(" {title} "))
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
        .dialogs
        .current()
        .and_then(|request| match request {
            ExtensionDialogRequest::Select(dialog) => Some(dialog.clone()),
            _ => None,
        });
    let Some(dialog) = dialog else {
        return;
    };
    let title =
        extension_dialog_title(&dialog.title, &dialog.extension_id, dialog.show_attribution);
    let desired_width = dialog
        .options
        .iter()
        .map(|option| option.width().saturating_add(4))
        .max()
        .unwrap_or(24)
        .max(title.width().saturating_add(4));
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
        .title(format!(" {title} "))
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
        .dialogs
        .current()
        .and_then(|request| match request {
            ExtensionDialogRequest::Confirm(dialog) => Some(dialog.clone()),
            _ => None,
        });
    let Some(dialog) = dialog else {
        return;
    };
    let title =
        extension_dialog_title(&dialog.title, &dialog.extension_id, dialog.show_attribution);
    let help = format!(
        "Enter/y {} · Esc/n {}",
        dialog.confirm_label, dialog.cancel_label
    );
    let body_width = dialog
        .body_lines
        .iter()
        .map(|line| line.width())
        .max()
        .unwrap_or_default();
    let desired_width = title
        .width()
        .max(body_width)
        .max(help.width())
        .saturating_add(4);
    let width = u16::try_from(desired_width)
        .unwrap_or(u16::MAX)
        .max(30)
        .min(area.width.max(1));
    let height = u16::try_from(dialog.body_lines.len())
        .unwrap_or(u16::MAX)
        .saturating_add(3)
        .max(3)
        .min(area.height.max(1));
    let bounds = Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    );
    Clear.render(bounds, buffer);
    let block = Block::default()
        .title(format!(" {title} "))
        .borders(Borders::ALL)
        .style(Style::default().bg(ratatui_theme_color(&app.options.theme.panel)))
        .border_style(Style::default().fg(ratatui_theme_color(&app.options.theme.accent)));
    let inner = block.inner(bounds);
    block.render(bounds, buffer);
    let mut lines = dialog
        .body_lines
        .into_iter()
        .map(|body| {
            Line::styled(
                body,
                Style::default().fg(ratatui_theme_color(&app.options.theme.text)),
            )
        })
        .collect::<Vec<_>>();
    lines.push(Line::styled(
        help,
        Style::default().fg(ratatui_theme_color(&app.options.theme.muted)),
    ));
    Paragraph::new(lines).render(inner, buffer);
}

/// Draw the host-owned consent prompt for a native extension workspace write.
pub fn render_extension_workspace_write_dialog(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    let dialog = app
        .extension_pane_runtime
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .dialogs
        .current()
        .and_then(|request| match request {
            ExtensionDialogRequest::Workspace { dialog, .. } => Some(dialog.clone()),
            _ => None,
        });
    let Some(dialog) = dialog else {
        return;
    };
    let title = format!(" ext {} ", dialog.extension_id);
    let question = format!("Write {}?", dialog.path);
    let body = format!(
        "Extension {} will replace this file's contents on disk.",
        dialog.extension_id
    );
    let help = "Enter/y confirms · Esc/n cancels";
    let desired_width = title
        .width()
        .max(question.width())
        .max(body.width())
        .max(help.width())
        .saturating_add(4);
    let width = u16::try_from(desired_width)
        .unwrap_or(u16::MAX)
        .max(30)
        .min(area.width.max(1));
    let height = 5.min(area.height.max(1));
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
            body,
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
                planned.key.clone(),
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
        let view = if let Some(cached) = cached {
            cached
        } else if runtime.extensions[registration.extension_index].request_pending() {
            let message = if runtime.extensions[registration.extension_index].command_pending() {
                "Command running…"
            } else {
                "Event handler running…"
            };
            ExtensionPaneView {
                extension_id: registration.extension_id.clone(),
                pane: registration.pane.clone(),
                content: ViewNode::Text {
                    text: message.into(),
                    style: ViewStyle {
                        foreground: Some("muted".into()),
                        ..ViewStyle::default()
                    },
                },
            }
        } else {
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
        };
        rendered_panes.push((
            planned.key.clone(),
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
    for (key, pane, pane_area, divider, owner) in rendered_panes {
        if let Some(divider) = divider {
            render_extension_pane_divider(divider, buffer, &key, pane.pane.placement, app);
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
    pane_key: &str,
    placement: PanePlacement,
    app: &ReviewApp,
) {
    let is_resizing = app
        .extension_pane_runtime
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .resize
        .as_ref()
        .is_some_and(|(key, _)| key == pane_key);
    let style = Style::default()
        .fg(ratatui_theme_color(if is_resizing {
            &app.options.theme.accent
        } else {
            &app.options.theme.border
        }))
        .bg(ratatui_theme_color(if is_resizing {
            &app.options.theme.accent_muted
        } else {
            &app.options.theme.panel
        }));
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

fn cursor_highlighted_style(style: Style, fallback: &str, theme: &AppTheme) -> Style {
    let base = match style.bg {
        Some(Color::Rgb(red, green, blue)) => format!("#{red:02x}{green:02x}{blue:02x}"),
        Some(Color::Reset) => TRANSPARENT_BACKGROUND.to_owned(),
        _ => fallback.to_owned(),
    };
    style.bg(ratatui_theme_color(&cursor_line_highlight_background(
        &base, theme,
    )))
}

fn paint_cursor_line(line: &mut Line<'_>, mode: CursorLineMode, theme: &AppTheme) {
    match mode {
        CursorLineMode::Row => {
            line.style = cursor_highlighted_style(line.style, &theme.panel, theme);
            for span in &mut line.spans {
                span.style = cursor_highlighted_style(span.style, &theme.panel, theme);
            }
        }
        CursorLineMode::Number => {
            for prefix_or_gutter in line.spans.iter_mut().take(2) {
                prefix_or_gutter.style =
                    cursor_highlighted_style(prefix_or_gutter.style, &theme.panel, theme);
            }
        }
        CursorLineMode::Off => {}
    }
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
    let line_highlights = app.prepare_extension_line_highlights(state.changeset());
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
        &line_highlights,
        &file_view_layouts,
        &component_expanded,
    );
    drop(state);
    let viewport = area.height.saturating_sub(1) as usize;
    let max_scroll = rows.lines.len().saturating_sub(viewport);
    let scroll = if app.scroll == usize::MAX {
        max_scroll
    } else {
        app.scroll.min(max_scroll)
    };
    let viewport_bottom = scroll.saturating_add(viewport);
    *app.review_file_header_hits
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = rows
        .file_header_rows
        .iter()
        .filter_map(|(file_index, top)| {
            (*top >= scroll && *top < viewport_bottom).then_some(SidebarFileHit {
                bounds: Rect::new(
                    area.x,
                    area.y.saturating_add(1).saturating_add(
                        u16::try_from(top.saturating_sub(scroll)).unwrap_or(u16::MAX),
                    ),
                    area.width,
                    1,
                ),
                file_index: *file_index,
            })
        })
        .collect();
    let cursor_row = app.current_line_row.min(rows.lines.len().saturating_sub(1));
    if app.focus == Focus::Review
        && app.options.cursor_line != CursorLineMode::Off
        && let Some(line) = rows.lines.get_mut(cursor_row)
    {
        paint_cursor_line(line, app.options.cursor_line, &app.options.theme);
    }
    let visible = rows
        .lines
        .into_iter()
        .skip(scroll)
        .take(viewport)
        .collect::<Vec<_>>();
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
    note_targets: BTreeMap<usize, ReviewNoteTarget>,
    file_tops: Vec<usize>,
    file_header_rows: Vec<(usize, usize)>,
    hunk_tops: std::collections::HashMap<(usize, usize), usize>,
    file_view_component_hits: Vec<FileViewComponentLogicalHit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReviewNoteTarget {
    file_index: usize,
    hunk_index: usize,
    side: ReviewSide,
    line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReviewNoteComposer {
    id: String,
    target: ReviewNoteTarget,
    body: String,
    cursor: usize,
}

#[derive(Debug)]
struct TargetedHunkRows {
    lines: Vec<Line<'static>>,
    targets: Vec<Option<ReviewNoteTarget>>,
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
    highlight_cache: &mut HighlightedDiffRuntime,
    expanded_gaps: &BTreeSet<(String, usize)>,
) -> ReviewRows {
    let line_highlights = LineHighlightMap::default();
    build_review_rows_with_chrome(
        changeset,
        comments,
        selection,
        layout,
        options,
        width,
        highlight_cache,
        expanded_gaps,
        &line_highlights,
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
    highlight_cache: &mut HighlightedDiffRuntime,
    expanded_gaps: &BTreeSet<(String, usize)>,
    line_highlights: &LineHighlightMap,
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
        line_highlights,
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
    highlight_cache: &mut HighlightedDiffRuntime,
    expanded_gaps: &BTreeSet<(String, usize)>,
    line_highlights: &LineHighlightMap,
    chrome: ReviewStreamChrome,
    live: bool,
    file_view_layouts: &BTreeMap<String, ValidatedFileViewLayout>,
    component_expanded: &BTreeSet<FileViewComponentStateKey>,
) -> ReviewRows {
    let mut rows = Vec::new();
    let mut file_tops = Vec::with_capacity(changeset.files.len());
    let mut file_header_rows = Vec::with_capacity(changeset.files.len());
    let mut hunk_tops = std::collections::HashMap::new();
    let mut file_view_component_hits = Vec::new();
    let mut note_targets = BTreeMap::new();
    let header_stats_width = max_file_header_stats_width(&changeset.files);
    for (file_index, file) in changeset.files.iter().enumerate() {
        if file_index > 0 {
            rows.extend((0..options.file_gap).map(|_| Line::default()));
        }
        file_tops.push(rows.len());
        if chrome.show_file_headers {
            file_header_rows.push((file_index, rows.len()));
            rows.push(file_header(
                file,
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
            highlight_cache
                .prefetch_highlighted_diff(file, &options.theme, live)
                .map(|highlighted| highlighted.highlighted)
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let gap_source = review_gap_source_for_file(file);
        let expansion_side = review_expansion_side(file.change_kind);
        let selected_source = match expansion_side {
            ReviewSide::Old => file.sources.old.as_ref(),
            ReviewSide::New => file.sources.new.as_ref(),
        };
        let line_highlight_paint = line_highlights.get(&file.runtime_id).and_then(|marks| {
            build_line_highlight_paint_index(
                file,
                marks,
                options.tab_width,
                selected_source.map(|source| source.content.as_str()),
            )
        });
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
                    line_highlight_paint.as_ref(),
                ));
            }
            if hunk_index > 0 {
                rows.extend((0..options.hunk_gap).map(|_| Line::default()));
            }
            hunk_tops.insert((file_index, hunk_index), rows.len());
            let selected_hunk =
                selection.file_index == file_index && selection.hunk_index == Some(hunk_index);
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
            let rendered = match layout {
                LayoutMode::Split => split_hunk_rows(
                    file,
                    file_index,
                    hunk,
                    hunk_index,
                    options,
                    width,
                    highlighted.get(hunk_index),
                    comments,
                    file_selection,
                    selected_hunk,
                    line_highlight_paint.as_ref(),
                ),
                LayoutMode::Stack | LayoutMode::Auto => stack_hunk_rows(
                    file,
                    file_index,
                    hunk,
                    hunk_index,
                    options,
                    highlighted.get(hunk_index),
                    comments,
                    width,
                    file_selection,
                    selected_hunk,
                    line_highlight_paint.as_ref(),
                ),
            };
            let row_start = rows.len();
            note_targets.extend(
                rendered
                    .targets
                    .into_iter()
                    .enumerate()
                    .filter_map(|(offset, target)| {
                        target.map(|target| (row_start + offset, target))
                    }),
            );
            rows.extend(rendered.lines);
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
                line_highlight_paint.as_ref(),
            ));
        }
    }
    ReviewRows {
        lines: rows,
        note_targets,
        file_tops,
        file_header_rows,
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
                rows.extend(extension_file_view_note_lines(note, &options.theme, width));
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

fn stml_theme_colors(theme: &AppTheme) -> workdeck_markup::StmlThemeColors {
    workdeck_markup::StmlThemeColors {
        accent: theme.accent.clone(),
        accent_muted: theme.accent_muted.clone(),
        added_sign_color: theme.added_sign_color.clone(),
        removed_sign_color: theme.removed_sign_color.clone(),
        file_modified: theme.file_modified.clone(),
        muted: theme.muted.clone(),
        panel_alt: theme.panel_alt.clone(),
        text: theme.text.clone(),
        panel: theme.panel.clone(),
        note_border: theme.note_border.clone(),
        background: theme.background.clone(),
    }
}

fn stml_ratatui_lines(body: &str, width: usize, theme: &AppTheme) -> Vec<Line<'static>> {
    let colors = stml_theme_colors(theme);
    workdeck_markup::layout_stml_cached(body, width)
        .lines
        .iter()
        .map(|line| {
            Line::from(
                line.spans
                    .iter()
                    .map(|span| {
                        let foreground =
                            workdeck_markup::resolve_stml_color(span.style.fg.as_deref(), &colors)
                                .unwrap_or_else(|| theme.text.clone());
                        let background =
                            workdeck_markup::resolve_stml_color(span.style.bg.as_deref(), &colors)
                                .unwrap_or_else(|| theme.panel.clone());
                        let mut style = Style::default()
                            .fg(ratatui_theme_color(&foreground))
                            .bg(ratatui_theme_color(&background));
                        for (enabled, modifier) in [
                            (span.style.bold, Modifier::BOLD),
                            (span.style.dim, Modifier::DIM),
                            (span.style.italic, Modifier::ITALIC),
                            (span.style.underline, Modifier::UNDERLINED),
                            (span.style.strike, Modifier::CROSSED_OUT),
                        ] {
                            if enabled == Some(true) {
                                style = style.add_modifier(modifier);
                            }
                        }
                        Span::styled(span.text.clone(), style)
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}

fn note_body_ratatui_lines(
    markup: Option<&str>,
    summary: &str,
    width: usize,
    theme: &AppTheme,
) -> Vec<Line<'static>> {
    let markup_lines = markup
        .map(|markup| stml_ratatui_lines(markup, width, theme))
        .unwrap_or_default();
    if !markup_lines.is_empty() {
        return markup_lines;
    }
    wrap_text(summary, width)
        .into_iter()
        .map(|text| {
            Line::styled(
                text,
                Style::default()
                    .fg(ratatui_theme_color(&theme.text))
                    .bg(ratatui_theme_color(&theme.panel)),
            )
        })
        .collect()
}

fn extension_file_view_note_lines(
    note: &VisibleFileViewNote,
    theme: &AppTheme,
    width: usize,
) -> Vec<Line<'static>> {
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
    let markup_width = width.saturating_sub(indent.len() + 8).max(1);
    rows.extend(
        note_body_ratatui_lines(
            (note.annotation.source.as_deref() != Some("user-draft"))
                .then_some(note.annotation.markup.as_deref())
                .flatten(),
            &note.annotation.summary,
            markup_width,
            theme,
        )
        .into_iter()
        .map(|mut line| {
            line.spans.insert(
                0,
                Span::styled(
                    format!("{indent}  │   "),
                    Style::default().fg(ratatui_theme_color(&theme.note_border)),
                ),
            );
            line
        }),
    );
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
    line_highlights: Option<&LineHighlightPaintIndex>,
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
                line_highlight_ranges(line_highlights, &line),
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
                        line_highlights: line_highlight_ranges(line_highlights, &line),
                    },
                    SplitCellInput {
                        line: Some(&line),
                        highlighted: highlighted.as_ref(),
                        emphasis: &[],
                        line_highlights: line_highlight_ranges(line_highlights, &line),
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
            Style::default().fg(ratatui_theme_color(&theme.text)),
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
        Span::styled(" ", Style::default().fg(ratatui_theme_color(&theme.muted))),
        Span::styled(
            stats.deletions_text,
            Style::default().fg(ratatui_theme_color(&theme.badge_removed)),
        ),
        Span::styled(" ", Style::default().fg(ratatui_theme_color(&theme.muted))),
        Span::raw(" "),
    ])
    .style(
        Style::default()
            .fg(Color::Rgb(255, 255, 255))
            .bg(ratatui_theme_color(&theme.panel)),
    )
}

#[allow(clippy::too_many_arguments)]
fn stack_hunk_rows(
    file: &DiffFile,
    file_index: usize,
    hunk: &workdeck_core::DiffHunk,
    hunk_index: usize,
    options: &ReviewOptions,
    highlighted: Option<&Vec<HighlightedDiffLine>>,
    comments: &[ReviewComment],
    width: u16,
    selection: ReviewSelection,
    hunk_selected: bool,
    line_highlights: Option<&LineHighlightPaintIndex>,
) -> TargetedHunkRows {
    let mut rows = Vec::new();
    let mut targets = Vec::new();
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
        let line_rows = stack_line_rows(
            line,
            options,
            highlighted
                .and_then(|lines| lines.get(index))
                .and_then(|line_highlight| line_highlight.for_stack(line.kind)),
            &emphasis[index],
            hunk_selected || line_is_selected(line, selection),
            width,
            line_highlight_ranges(line_highlights, line),
        );
        let target = diff_line_note_target(file_index, hunk_index, line);
        targets.extend(std::iter::repeat_n(Some(target), line_rows.len()));
        rows.extend(line_rows);
        let note_rows = comment_rows(file, line, comments, &options.theme, width);
        targets.extend(std::iter::repeat_n(Some(target), note_rows.len()));
        rows.extend(note_rows);
    }
    TargetedHunkRows {
        lines: rows,
        targets,
    }
}

fn stack_line_rows(
    line: &DiffLine,
    options: &ReviewOptions,
    highlighted: Option<&Vec<SyntaxToken>>,
    emphasis: &[Range<usize>],
    selected: bool,
    width: u16,
    line_highlights: Option<&LineHighlightRangeList>,
) -> Vec<Line<'static>> {
    let kind = row_cell_kind(line.kind);
    let palette = stack_cell_palette(kind, &options.theme, line.moved);
    let base_background = palette.content_background;
    let row_bg = ratatui_theme_color(palette.content_background);
    let gutter_bg = ratatui_theme_color(palette.gutter_background);
    let digits = options.line_number_digits.unwrap_or(4).max(1);
    let geometry = resolve_stack_cell_geometry(
        usize::from(width),
        digits,
        options.line_numbers,
        DIFF_RAIL_PREFIX_WIDTH,
    );
    let gutter = format!(
        "{:<width$}",
        stack_gutter_text(
            line_sign(line.kind),
            line.old_line,
            line.new_line,
            digits,
            options.line_numbers,
        ),
        width = geometry.gutter_width,
    );
    let number_style = Style::default()
        .fg(ratatui_theme_color(palette.number_color))
        .bg(gutter_bg);
    let marker_style = Style::default()
        .fg(ratatui_theme_color(&stack_rail_color(
            kind,
            &options.theme,
            selected,
        )))
        .bg(ratatui_theme_color(&options.theme.panel));
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
            Style::default()
                .fg(ratatui_theme_color(&options.theme.syntax_colors.default))
                .bg(row_bg),
        ));
    }
    code = emphasize_spans(
        code,
        emphasis,
        emphasis_background(line.kind, &options.theme),
    );
    if let Some(line_highlights) = line_highlights {
        code = apply_prepared_line_highlights_to_ratatui_spans(
            code,
            line_highlights,
            base_background,
            &options.theme,
        );
    }
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
                    Span::styled(diff_rail_marker(), marker_style),
                    Span::styled(gutter.clone(), number_style),
                ]
            } else {
                vec![
                    Span::styled(diff_rail_marker(), marker_style),
                    Span::styled(
                        " ".repeat(prefix_width.saturating_sub(1)),
                        Style::default().bg(gutter_bg),
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
    file_index: usize,
    hunk: &workdeck_core::DiffHunk,
    hunk_index: usize,
    options: &ReviewOptions,
    width: u16,
    highlighted: Option<&Vec<HighlightedDiffLine>>,
    comments: &[ReviewComment],
    selection: ReviewSelection,
    hunk_selected: bool,
    line_highlights: Option<&LineHighlightPaintIndex>,
) -> TargetedHunkRows {
    let mut rows = Vec::new();
    let mut targets = Vec::new();
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
        let pair_rows = split_pair_rows(
            SplitCellInput {
                line: old,
                highlighted: pair
                    .old_index
                    .and_then(|index| highlighted.and_then(|lines| lines.get(index)))
                    .and_then(|line| line.deletion.as_ref()),
                emphasis: emphasis.as_ref().map_or(&[], |ranges| &ranges.old),
                line_highlights: old.and_then(|line| line_highlight_ranges(line_highlights, line)),
            },
            SplitCellInput {
                line: new,
                highlighted: pair
                    .new_index
                    .and_then(|index| highlighted.and_then(|lines| lines.get(index)))
                    .and_then(|line| line.addition.as_ref()),
                emphasis: emphasis.as_ref().map_or(&[], |ranges| &ranges.new),
                line_highlights: new.and_then(|line| line_highlight_ranges(line_highlights, line)),
            },
            options,
            left_width,
            right_width,
            hunk_selected
                || old.is_some_and(|line| line_is_selected(line, selection))
                || new.is_some_and(|line| line_is_selected(line, selection)),
        );
        let pair_target = new
            .or(old)
            .map(|line| diff_line_note_target(file_index, hunk_index, line));
        targets.extend(std::iter::repeat_n(pair_target, pair_rows.len()));
        rows.extend(pair_rows);
        if let Some(line) = old {
            let note_rows = comment_rows(file, line, comments, &options.theme, width);
            targets.extend(std::iter::repeat_n(
                Some(diff_line_note_target(file_index, hunk_index, line)),
                note_rows.len(),
            ));
            rows.extend(note_rows);
        }
        if pair.new_index != pair.old_index
            && let Some(line) = new
        {
            let note_rows = comment_rows(file, line, comments, &options.theme, width);
            targets.extend(std::iter::repeat_n(
                Some(diff_line_note_target(file_index, hunk_index, line)),
                note_rows.len(),
            ));
            rows.extend(note_rows);
        }
    }
    TargetedHunkRows {
        lines: rows,
        targets,
    }
}

fn diff_line_note_target(
    file_index: usize,
    hunk_index: usize,
    line: &DiffLine,
) -> ReviewNoteTarget {
    let (side, line) = line
        .new_line
        .map(|line| (ReviewSide::New, line))
        .or_else(|| line.old_line.map(|line| (ReviewSide::Old, line)))
        .expect("every rendered diff line has an old or new line number");
    ReviewNoteTarget {
        file_index,
        hunk_index,
        side,
        line,
    }
}

fn comment_rows(
    file: &DiffFile,
    line: &DiffLine,
    comments: &[ReviewComment],
    theme: &AppTheme,
    width: u16,
) -> Vec<Line<'static>> {
    let mut rows = Vec::new();
    for comment in comments
        .iter()
        .filter(|comment| comment.anchor.file_key == file.key)
        .filter(|comment| comment_matches_line(comment, line))
    {
        let title = if comment.source == "user" {
            "Your note".to_owned()
        } else {
            let author = comment.author.as_deref().unwrap_or(&comment.source);
            format!("note {author}")
        };
        rows.push(Line::from(Span::styled(
            format!("  │ {title}"),
            Style::default()
                .fg(Color::LightMagenta)
                .add_modifier(Modifier::BOLD),
        )));
        for mut line in note_body_ratatui_lines(
            comment.markup.as_deref(),
            &comment.summary,
            usize::from(width.saturating_sub(8).max(1)),
            theme,
        ) {
            line.spans.insert(
                0,
                Span::styled(
                    "  │   ",
                    Style::default().fg(ratatui_theme_color(&theme.note_border)),
                ),
            );
            rows.push(line);
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
    line_highlights: Option<&'a LineHighlightRangeList>,
}

fn split_pair_rows(
    old: SplitCellInput<'_>,
    new: SplitCellInput<'_>,
    options: &ReviewOptions,
    left_width: usize,
    right_width: usize,
    selected: bool,
) -> Vec<Line<'static>> {
    let mut old_lines = split_cell_lines(old, true, options, left_width, selected);
    let mut new_lines = split_cell_lines(new, false, options, right_width, selected);
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
    input: SplitCellInput<'_>,
    old: bool,
    options: &ReviewOptions,
    width: usize,
    selected: bool,
) -> Vec<Vec<Span<'static>>> {
    let kind = input
        .line
        .map_or(RowCellKind::Empty, |line| row_cell_kind(line.kind));
    let palette = split_cell_palette(
        kind,
        &options.theme,
        input.line.is_some_and(|line| line.moved),
    );
    let base_background = palette.content_background;
    let row_bg = ratatui_theme_color(palette.content_background);
    let gutter_bg = ratatui_theme_color(palette.gutter_background);
    let digits = options.line_number_digits.unwrap_or(4).max(1);
    let geometry =
        resolve_split_cell_geometry(width, digits, options.line_numbers, DIFF_RAIL_PREFIX_WIDTH);
    let sign = input.line.map_or(' ', |line| line_sign(line.kind));
    let number = input
        .line
        .and_then(|line| if old { line.old_line } else { line.new_line });
    let gutter = format!(
        "{:<width$}",
        split_gutter_text(sign, number, digits, options.line_numbers),
        width = geometry.gutter_width,
    );
    let rail = if old {
        split_left_rail_color(kind, &options.theme, selected)
    } else {
        split_right_rail_color(kind, &options.theme, selected)
    };
    let number_style = Style::default()
        .fg(ratatui_theme_color(palette.number_color))
        .bg(gutter_bg);
    let marker_style = Style::default()
        .fg(ratatui_theme_color(&rail))
        .bg(ratatui_theme_color(&options.theme.panel));
    let mut code = Vec::new();
    if let Some(line) = input.line {
        if let Some(tokens) = input.highlighted.filter(|tokens| !tokens.is_empty()) {
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
                Style::default()
                    .fg(ratatui_theme_color(&options.theme.syntax_colors.default))
                    .bg(row_bg),
            ));
        }
        code = emphasize_spans(
            code,
            input.emphasis,
            emphasis_background(line.kind, &options.theme),
        );
        if let Some(line_highlights) = input.line_highlights {
            code = apply_prepared_line_highlights_to_ratatui_spans(
                code,
                line_highlights,
                base_background,
                &options.theme,
            );
        }
    }
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
                    Span::styled(diff_rail_marker(), marker_style),
                    Span::styled(gutter.clone(), number_style),
                ]
            } else {
                vec![
                    Span::styled(diff_rail_marker(), marker_style),
                    Span::styled(
                        " ".repeat(prefix_width.saturating_sub(1)),
                        Style::default().bg(gutter_bg),
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

const fn row_cell_kind(kind: DiffLineKind) -> RowCellKind {
    match kind {
        DiffLineKind::Context => RowCellKind::Context,
        DiffLineKind::Addition => RowCellKind::Addition,
        DiffLineKind::Deletion => RowCellKind::Deletion,
    }
}

const fn line_sign(kind: DiffLineKind) -> char {
    match kind {
        DiffLineKind::Context => ' ',
        DiffLineKind::Addition => '+',
        DiffLineKind::Deletion => '-',
    }
}

fn line_highlight_ranges<'a>(
    index: Option<&'a LineHighlightPaintIndex>,
    line: &DiffLine,
) -> Option<&'a LineHighlightRangeList> {
    line.new_line
        .and_then(|number| index?.get(ReviewSide::New, u64::from(number)))
        .or_else(|| {
            line.old_line
                .and_then(|number| index?.get(ReviewSide::Old, u64::from(number)))
        })
        .map(Arc::as_ref)
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
    if app.active_startup_notice().is_none() && app.active_extension_notification().is_some() {
        render_extension_toast(area, buffer, app);
        render_active_keyboard_mode_badge(area, buffer, app);
        return;
    }
    let background = ratatui_theme_color(&app.options.theme.panel_alt);
    Block::default()
        .style(
            Style::default()
                .fg(Color::Rgb(255, 255, 255))
                .bg(background),
        )
        .render(area, buffer);
    let mode_width = status_bar_mode_width(
        app.active_keyboard_mode_status_hint().as_deref(),
        area.width,
    )
    .min(area.width.saturating_sub(2));
    let content = Rect::new(
        area.x.saturating_add(1),
        area.y,
        area.width.saturating_sub(2).saturating_sub(mode_width),
        area.height.min(1),
    );
    if content.width > 0 && content.height > 0 {
        let line = if app.focus == Focus::Filter {
            let input_width = usize::from(
                area.width
                    .saturating_sub(mode_width)
                    .saturating_sub(11)
                    .max(4),
            );
            let (input, input_color) = if app.filter.is_empty() {
                (
                    fit_text("type to filter files", input_width, None),
                    Color::Rgb(102, 102, 102),
                )
            } else {
                let view = status_bar_input_view(
                    &app.filter,
                    app.filter_cursor,
                    input_width,
                    app.filter_scroll.get(),
                );
                app.filter_scroll.set(view.scroll);
                (view.text, Color::Rgb(255, 255, 255))
            };
            Line::from(vec![
                Span::styled(
                    "filter:",
                    Style::default()
                        .fg(ratatui_theme_color(&app.options.theme.badge_neutral))
                        .bg(background),
                ),
                Span::styled(
                    " ",
                    Style::default()
                        .fg(ratatui_theme_color(&app.options.theme.muted))
                        .bg(background),
                ),
                Span::styled(input, Style::default().fg(input_color).bg(background)),
            ])
        } else {
            let text = if app.filter.is_empty() {
                app.active_startup_notice()
                    .or(app.status.as_deref())
                    .unwrap_or_default()
                    .to_owned()
            } else {
                format!("filter={}", app.filter)
            };
            Line::styled(
                sanitize_terminal_line(&text),
                Style::default()
                    .fg(ratatui_theme_color(&app.options.theme.muted))
                    .bg(background),
            )
        };
        Paragraph::new(line).render(content, buffer);
    }
    render_active_keyboard_mode_badge(area, buffer, app);
}

fn render_extension_toast(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    let Some(notification) = app.active_extension_notification() else {
        return;
    };
    let background = ratatui_theme_color(&app.options.theme.panel_alt);
    Block::default()
        .style(
            Style::default()
                .fg(Color::Rgb(255, 255, 255))
                .bg(background),
        )
        .render(area, buffer);
    let content = Rect::new(
        area.x.saturating_add(1),
        area.y,
        area.width.saturating_sub(2),
        area.height.min(1),
    );
    if content.width == 0 || content.height == 0 {
        return;
    }
    let theme = ExtensionToastTheme {
        badge_removed: ratatui_theme_color(&app.options.theme.badge_removed),
        file_modified: ratatui_theme_color(&app.options.theme.file_modified),
        badge_neutral: ratatui_theme_color(&app.options.theme.badge_neutral),
    };
    let color = extension_toast_color(notification.notification_type, theme);
    Paragraph::new(Line::from(vec![
        Span::styled(
            extension_toast_prefix(),
            Style::default().fg(color).bg(background),
        ),
        Span::styled(
            format!(
                " {}",
                extension_toast_message(&notification.message, area.width)
            ),
            Style::default()
                .fg(ratatui_theme_color(&app.options.theme.muted))
                .bg(background),
        ),
    ]))
    .render(content, buffer);
}

fn render_help(area: Rect, buffer: &mut Buffer, commands: &[HelpCommand]) {
    let sections = build_help_sections(commands);
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

fn render_note_composer(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    let Some(composer) = app.note_composer.as_ref() else {
        app.note_composer_bounds.set(None);
        return;
    };
    let requested_height =
        u16::try_from(composer.body.lines().count().saturating_add(4).clamp(7, 14)).unwrap_or(14);
    let geometry = resolve_modal_geometry(78, requested_height, area.width, area.height);
    let popup = Rect {
        x: area.x.saturating_add(geometry.left),
        y: area.y.saturating_add(geometry.top),
        width: geometry.width,
        height: geometry.height,
    };
    app.note_composer_bounds.set(Some(popup));
    Clear.render(popup, buffer);
    Paragraph::new(composer.body.as_str())
        .wrap(Wrap { trim: false })
        .style(
            Style::default()
                .fg(ratatui_theme_color(&app.options.theme.text))
                .bg(ratatui_theme_color(&app.options.theme.panel)),
        )
        .block(
            Block::default()
                .title(" Draft note ")
                .title_bottom(" Ctrl+S save · Esc cancel ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(ratatui_theme_color(&app.options.theme.accent))),
        )
        .render(popup, buffer);
}

fn note_composer_cursor_cell(
    body: &str,
    cursor: usize,
    width: usize,
    height: usize,
) -> (usize, usize) {
    let mut row: usize = 0;
    let mut column: usize = 0;
    for character in body.chars().take(cursor) {
        if character == '\n' {
            row = row.saturating_add(1);
            column = 0;
            continue;
        }
        let cell_width = unicode_width::UnicodeWidthChar::width(character)
            .unwrap_or_default()
            .max(1);
        if column.saturating_add(cell_width) > width {
            row = row.saturating_add(1);
            column = 0;
        }
        column = column.saturating_add(cell_width);
        if column >= width {
            row = row.saturating_add(column / width);
            column %= width;
        }
    }
    (
        row.min(height.saturating_sub(1)),
        column.min(width.saturating_sub(1)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use workdeck_core::{
        ChangesetSource, CliInput, CommonOptions, FileSourceSnapshots, LineRange, SourceOrigin,
        SourceSnapshot, VcsDiffCommandInput,
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

    fn saved_comment(file_key: &str, id: &str, summary: &str) -> ReviewComment {
        ReviewComment {
            id: id.into(),
            parent_id: None,
            source: "agent".into(),
            author: None,
            created_at: None,
            file_path: None,
            hunk_index: Some(0),
            side: Some(ReviewSide::New),
            line: Some(1),
            summary: summary.into(),
            rationale: None,
            markup: None,
            title: None,
            tags: Vec::new(),
            confidence: None,
            updated_at: None,
            resolution: workdeck_review::ReviewNoteResolution::Active,
            anchor: CommentAnchor {
                file_key: file_key.into(),
                old_range: None,
                new_range: Some(LineRange { start: 1, end: 1 }),
                preferred_side: Some(ReviewSide::New),
                preferred_line: Some(1),
                intersecting_hunk_indices: vec![0],
                owner_hunk_index: Some(0),
            },
            editable: false,
        }
    }

    #[test]
    fn extension_note_sync_tracks_complete_values_and_reseeds_review_generations() {
        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
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
            .add_comment(saved_comment(&file_key, "note:1", "before"))
            .unwrap();
        let events = app.update_extension_review_events(Instant::now());
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].clone().into_parts().1["note"]["summary"],
            "before"
        );

        {
            let mut state = state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.remove_comment("note:1").unwrap();
            state
                .add_comment(saved_comment(&file_key, "note:1", "after"))
                .unwrap();
        }
        let events = app.update_extension_review_events(Instant::now());
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].clone().into_parts().1["kind"], "updated");
        assert_eq!(events[0].clone().into_parts().1["note"]["summary"], "after");

        {
            let mut state = state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.reload(changeset());
            state
                .add_comment(saved_comment(&file_key, "note:2", "new generation"))
                .unwrap();
        }
        assert!(
            app.update_extension_review_events(Instant::now())
                .is_empty()
        );

        state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove_comment("note:1")
            .unwrap();
        let events = app.update_extension_review_events(Instant::now());
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].clone().into_parts().1["kind"], "removed");
        assert_eq!(events[0].clone().into_parts().1["note"]["id"], "note:1");
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

    fn two_file_changeset() -> Changeset {
        parse_patch(
            "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\ndiff --git a/b.rs b/b.rs\n--- a/b.rs\n+++ b/b.rs\n@@ -1 +1 @@\n-before\n+after\n",
            "two-files",
            "Two files",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap()
    }

    fn two_hunk_changeset() -> Changeset {
        parse_patch(
            "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n@@ -10 +10 @@\n-before\n+after\n",
            "two-hunks",
            "Two hunks",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap()
    }

    fn writable_input() -> CliInput {
        CliInput::Vcs(VcsDiffCommandInput {
            range: None,
            range_endpoints: None,
            staged: false,
            pathspecs: Vec::new(),
            options: CommonOptions::default(),
        })
    }

    fn attested_writable_changeset() -> Changeset {
        let mut review = changeset();
        review.files[0].set_sources(FileSourceSnapshots {
            old: Some(SourceSnapshot::new(
                "old\n".into(),
                SourceOrigin::Revision {
                    revision: "HEAD".into(),
                },
                true,
            )),
            new: Some(SourceSnapshot::new(
                "new\n".into(),
                SourceOrigin::WorkingTree,
                true,
            )),
        });
        review
    }

    #[test]
    fn committed_event_context_provider_is_installed_before_observation_and_cleans_up() {
        let app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                command_cwd: Some(PathBuf::from("/repo")),
                ..ReviewOptions::default()
            },
        );
        let slot = app.extension_event_context_provider.clone();
        assert!(slot.has_provider());
        assert_eq!(
            slot.context(vec!["summary".into()]).unwrap().cwd,
            PathBuf::from("/repo")
        );
        drop(app);
        assert!(!slot.has_provider());
    }

    #[test]
    fn extension_runtime_replacement_installs_a_successor_event_context() {
        let mut app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                command_cwd: Some(PathBuf::from("/repo/first")),
                ..ReviewOptions::default()
            },
        );
        let slot = app.extension_event_context_provider.clone();
        app.options.command_cwd = Some(PathBuf::from("/repo/second"));
        app.replace_extensions_and_reload(changeset(), Vec::new());
        assert_eq!(
            slot.context(Vec::new()).unwrap().cwd,
            PathBuf::from("/repo/second")
        );
    }

    #[test]
    fn review_app_publishes_imperative_lifecycle_payloads_to_the_current_registry() {
        let mut app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                command_cwd: Some(PathBuf::from("/repo/first")),
                ..ReviewOptions::default()
            },
        );
        assert_eq!(
            app.observed_extension_events
                .iter()
                .map(|(_, name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["startup", "changeset_loaded"]
        );
        app.observed_extension_events.clear();

        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        app.notify_watch_reload_pending();

        let first = app.observed_extension_events.clone();
        assert!(first.iter().all(|(registry, _, _)| *registry == 1));
        assert_eq!(
            first
                .iter()
                .map(|(_, name, _)| name.as_str())
                .collect::<Vec<_>>(),
            [
                "command_executed",
                "note_edited",
                "note_created",
                "note_changed",
                "watch_reload_pending",
            ]
        );
        assert_eq!(
            first[0].2,
            serde_json::json!({ "commandId": "workdeck.review.startNote" })
        );
        assert_eq!(first[1].2["note"]["body"], "x");
        assert_eq!(first[1].2["note"]["draft"], true);
        assert_eq!(first[2].2["note"]["body"], "x");
        assert_eq!(first[2].2["note"]["draft"], false);
        assert_eq!(first[3].2["kind"], "created");

        app.observed_extension_events.clear();
        app.options.command_cwd = Some(PathBuf::from("/repo/second"));
        app.replace_extensions_and_reload(changeset(), Vec::new());
        app.notify_watch_reload_pending();
        assert!(
            app.observed_extension_events
                .iter()
                .all(|(registry, _, _)| *registry == 2)
        );
        assert_eq!(
            app.observed_extension_events
                .iter()
                .map(|(_, name, _)| name.as_str())
                .collect::<Vec<_>>(),
            [
                "startup",
                "changeset_loaded",
                "session_reload",
                "watch_reload_pending",
            ]
        );
        assert_eq!(app.observed_extension_events[0].2["cwd"], "/repo/second");
        assert_eq!(app.observed_extension_events[2].2["reason"], "manual");
    }

    #[test]
    fn reload_request_provenance_distinguishes_user_daemon_and_watch_sources() {
        assert_eq!(
            reload_request_reason(true, false),
            SessionReloadReason::Manual
        );
        assert_eq!(
            reload_request_reason(true, true),
            SessionReloadReason::Manual
        );
        assert_eq!(
            reload_request_reason(false, true),
            SessionReloadReason::Daemon
        );
        assert_eq!(
            reload_request_reason(false, false),
            SessionReloadReason::Watch
        );
    }

    #[test]
    fn extension_event_publication_is_a_noop_without_subscribers_and_bounds_recursion() {
        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        let initial_status = app.status.clone();
        app.publish_extension_event("selection_changed", serde_json::json!({ "fileId": null }));
        assert_eq!(app.status, initial_status);
        assert!(!app.has_pending_extension_events());

        app.extension_event_dispatch_depth = 16;
        app.publish_extension_event("review-triage:loop", serde_json::json!({}));
        assert_eq!(
            app.status.as_deref(),
            Some("extension event review-triage:loop exceeded the 16-event recursion limit")
        );
    }

    #[test]
    fn soft_reload_drains_the_dialog_controller_without_closing_its_queue() {
        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        {
            let mut runtime = app
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            runtime
                .dialogs
                .enqueue_confirm(usize::MAX, "retired", "old", "Old review?", "", "", None)
                .unwrap();
            runtime
                .dialogs
                .enqueue_input(
                    usize::MAX,
                    "retired",
                    "queued",
                    "Queued behind it",
                    "",
                    None,
                )
                .unwrap();
        }

        app.reload(changeset());
        let mut runtime = app
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(runtime.dialogs.current().is_none());
        assert!(
            runtime
                .dialogs
                .enqueue_confirm(
                    usize::MAX,
                    "replacement",
                    "new",
                    "New review?",
                    "",
                    "",
                    None,
                )
                .unwrap()
                .is_none()
        );
        assert!(runtime.dialogs.current().is_some());
    }

    #[test]
    fn extension_dialog_actions_enter_one_cross_kind_fifo_without_overwriting() {
        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        app.apply_extension_actions(
            0,
            "probe",
            vec![
                ExtensionHostAction::OpenConfirmDialog {
                    id: "first".into(),
                    title: "First?".into(),
                    body: "one\ntwo".into(),
                    confirm_label: String::new(),
                    cancel_label: None,
                },
                ExtensionHostAction::OpenSelectDialog {
                    id: "second".into(),
                    title: "Second?".into(),
                    options: vec!["one".into(), "two".into()],
                },
                ExtensionHostAction::OpenInputDialog {
                    id: "third".into(),
                    title: "Third?".into(),
                    placeholder: "value".into(),
                    initial: Some("initial".into()),
                },
            ],
        );
        let mut runtime = app
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let first = runtime.dialogs.current().unwrap().request_id();
        assert!(matches!(
            runtime.dialogs.current(),
            Some(ExtensionDialogRequest::Confirm(dialog))
                if dialog.title == "First?" && dialog.body_lines == ["one", "two"]
        ));
        runtime.dialogs.cancel(first).unwrap();
        let second = runtime.dialogs.current().unwrap().request_id();
        assert!(matches!(
            runtime.dialogs.current(),
            Some(ExtensionDialogRequest::Select(dialog))
                if dialog.title == "Second?" && dialog.selected == 0
        ));
        runtime.dialogs.cancel(second).unwrap();
        assert!(matches!(
            runtime.dialogs.current(),
            Some(ExtensionDialogRequest::Input(dialog))
                if dialog.title == "Third?" && dialog.value == "initial"
        ));
    }

    #[test]
    fn keyboard_mode_lifecycle_and_stale_key_actions_cannot_change_ownership() {
        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        let status = app.status.clone();
        for authority in [
            KeyboardModeActionAuthority::Lifecycle,
            KeyboardModeActionAuthority::ActiveKey(99),
        ] {
            app.apply_extension_actions_with_keyboard_authority(
                0,
                "probe",
                vec![ExtensionHostAction::EnterKeyboardMode {
                    id: "normal".into(),
                }],
                authority,
            );
            assert!(app.active_keyboard_mode_title().is_none());
            assert_eq!(app.status, status);
        }

        app.apply_extension_actions(
            0,
            "probe",
            vec![ExtensionHostAction::EnterKeyboardMode {
                id: "normal".into(),
            }],
        );
        assert_eq!(
            app.status.as_deref(),
            Some("Extension probe targeted unknown keyboard mode \"normal\"")
        );
    }

    #[test]
    fn extension_workspace_write_rechecks_the_captured_target_and_replaces_exact_source() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::write(root.path().join("a.rs"), "new\n").unwrap();
        let review = attested_writable_changeset();
        let file_id = review.files[0].runtime_id.clone();
        let app = ReviewApp::new(
            review,
            ReviewOptions {
                repo: Some(root.path().to_owned()),
                review_input: Some(writable_input()),
                ..ReviewOptions::default()
            },
        );
        let review_generation = app.with_state(|state| state.generation());
        let dialog = ExtensionWorkspaceWriteDialog {
            extension_index: 0,
            extension_id: "rewrite".into(),
            request_id: "write-1".into(),
            file_id,
            path: "a.rs".into(),
            absolute_path: root.path().join("a.rs"),
            root: root.path().to_owned(),
            text: "rewritten\n".into(),
            review_generation,
        };
        app.write_extension_workspace_document(&dialog).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.path().join("a.rs")).unwrap(),
            "rewritten\n"
        );

        std::fs::remove_file(root.path().join("a.rs")).unwrap();
        let error = app.write_extension_workspace_document(&dialog).unwrap_err();
        assert!(matches!(
            error,
            WorkspaceWriteFailure::Unavailable(detail)
                if detail.contains("no longer in the working tree")
        ));
    }

    #[test]
    fn extension_workspace_write_reports_io_failure_and_started_result_across_reload() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::write(root.path().join("a.rs"), "new\n").unwrap();
        let review = attested_writable_changeset();
        let file_id = review.files[0].runtime_id.clone();
        let app = ReviewApp::new(
            review,
            ReviewOptions {
                repo: Some(root.path().to_owned()),
                review_input: Some(writable_input()),
                ..ReviewOptions::default()
            },
        );
        let review_generation = app.with_state(|state| state.generation());
        let dialog = ExtensionWorkspaceWriteDialog {
            extension_index: 0,
            extension_id: "rewrite".into(),
            request_id: "write-boundary".into(),
            file_id,
            path: "a.rs".into(),
            absolute_path: root.path().join("a.rs"),
            root: root.path().to_owned(),
            text: "rewritten\n".into(),
            review_generation,
        };

        let failure = app
            .write_extension_workspace_document_with(&dialog, |_, _| {
                Err(std::io::Error::other("disk full"))
            })
            .unwrap_err();
        assert_eq!(
            failure,
            WorkspaceWriteFailure::Failed("Failed to write a.rs • disk full".into())
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("a.rs")).unwrap(),
            "new\n"
        );

        let shared_state = app.shared_state();
        app.write_extension_workspace_document_with(&dialog, |path, text| {
            shared_state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .reload(attested_writable_changeset());
            std::fs::write(path, text)
        })
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(root.path().join("a.rs")).unwrap(),
            "rewritten\n"
        );
        assert_ne!(
            app.with_state(|state| state.generation()),
            review_generation
        );
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

    fn rendered_review_text(terminal: &mut Terminal<TestBackend>, app: &ReviewApp) -> String {
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), app))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn repository_extension_trust_prompt_reconciles_across_in_place_reloads() {
        let backend = TestBackend::new(140, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                pending_extension_trust_repo_root: Some(PathBuf::from("/repo/alpha")),
                ..ReviewOptions::default()
            },
        );

        let initial = rendered_review_text(&mut terminal, &app);
        assert!(initial.contains("Run this repository's extensions?"));
        assert!(initial.contains("/repo/alpha"));
        assert!(initial.contains(".agents/workdeck/extensions"));

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(
            !rendered_review_text(&mut terminal, &app)
                .contains("Run this repository's extensions?")
        );

        app.reconcile_extension_trust_repo_root(Some(PathBuf::from("/repo/alpha")));
        assert!(
            !rendered_review_text(&mut terminal, &app)
                .contains("Run this repository's extensions?")
        );

        app.reconcile_extension_trust_repo_root(Some(PathBuf::from("/repo/beta")));
        let reasked = rendered_review_text(&mut terminal, &app);
        assert!(reasked.contains("Run this repository's extensions?"));
        assert!(reasked.contains("/repo/beta"));

        app.reconcile_extension_trust_repo_root(None);
        assert!(
            !rendered_review_text(&mut terminal, &app)
                .contains("Run this repository's extensions?")
        );
    }

    #[test]
    fn repository_extension_trust_prompt_owns_keys_and_queues_typed_decisions() {
        let mut app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                pending_extension_trust_repo_root: Some(PathBuf::from("/repo/alpha")),
                ..ReviewOptions::default()
            },
        );

        app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(!app.take_quit_requested());
        assert_eq!(
            app.extension_trust_prompt_root(),
            Some(Path::new("/repo/alpha"))
        );

        app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
        assert_eq!(
            app.take_extension_trust_request(),
            Some(ExtensionTrustRequest {
                repo_root: PathBuf::from("/repo/alpha"),
                decision: workdeck_extension_host::TrustDecision::Trusted,
            })
        );
        assert!(app.extension_trust_prompt_root().is_none());

        app.reconcile_extension_trust_repo_root(Some(PathBuf::from("/repo/beta")));
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        assert_eq!(
            app.take_extension_trust_request(),
            Some(ExtensionTrustRequest {
                repo_root: PathBuf::from("/repo/beta"),
                decision: workdeck_extension_host::TrustDecision::Denied,
            })
        );
    }

    #[test]
    fn repository_extension_trust_decisions_run_host_authority_and_reload_atomically() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&calls);
        let mut app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                pending_extension_trust_repo_root: Some(PathBuf::from("/repo/alpha")),
                extension_trust_handler: Some(ExtensionTrustHandler::new(
                    move |root, decision, load_extensions| {
                        observed
                            .lock()
                            .unwrap()
                            .push((root.to_owned(), decision, load_extensions));
                        Ok(Vec::new())
                    },
                )),
                ..ReviewOptions::default()
            },
        );
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let reloads = Arc::new(Mutex::new(0));
        let observed_reloads = Arc::clone(&reloads);
        let mut reload = move || {
            *observed_reloads.lock().unwrap() += 1;
            Ok(two_file_changeset())
        };
        let mut reload: Option<&mut dyn FnMut() -> Result<Changeset>> = Some(&mut reload);
        app.process_extension_trust_request(&mut reload);

        assert_eq!(
            *calls.lock().unwrap(),
            [(
                PathBuf::from("/repo/alpha"),
                workdeck_extension_host::TrustDecision::Trusted,
                true,
            )]
        );
        assert_eq!(*reloads.lock().unwrap(), 1);
        assert_eq!(app.with_state(|state| state.changeset().files.len()), 2);
        assert_eq!(app.status.as_deref(), Some("review reloaded"));
        assert!(app.options.pending_extension_trust_repo_root.is_none());
    }

    #[test]
    fn repository_extension_denial_and_nonreloadable_trust_never_load_code() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&calls);
        let handler = ExtensionTrustHandler::new(move |root, decision, load_extensions| {
            observed
                .lock()
                .unwrap()
                .push((root.to_owned(), decision, load_extensions));
            assert!(!load_extensions);
            Ok(Vec::new())
        });
        let mut denied = ReviewApp::new(
            changeset(),
            ReviewOptions {
                pending_extension_trust_repo_root: Some(PathBuf::from("/repo/deny")),
                extension_trust_handler: Some(handler.clone()),
                ..ReviewOptions::default()
            },
        );
        denied.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        let mut unavailable = None;
        denied.process_extension_trust_request(&mut unavailable);
        assert_eq!(
            denied.status.as_deref(),
            Some("Won't run this repository's extensions")
        );

        let mut deferred = ReviewApp::new(
            changeset(),
            ReviewOptions {
                pending_extension_trust_repo_root: Some(PathBuf::from("/repo/defer")),
                extension_trust_handler: Some(handler),
                ..ReviewOptions::default()
            },
        );
        deferred.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
        deferred.process_extension_trust_request(&mut unavailable);
        assert_eq!(
            deferred.status.as_deref(),
            Some("Trusted this repository • restart Workdeck to load its extensions")
        );
        assert_eq!(calls.lock().unwrap().len(), 2);
    }

    #[test]
    fn repository_extension_trust_prompt_renders_embedded_and_owns_mouse_actions() {
        let backend = TestBackend::new(140, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                pending_extension_trust_repo_root: Some(PathBuf::from("/repo/alpha")),
                ..ReviewOptions::default()
            },
        );
        terminal
            .draw(|frame| render_embedded(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Run this repository's extensions?"));

        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 36,
            row: 15,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(
            app.take_extension_trust_request(),
            Some(ExtensionTrustRequest {
                repo_root: PathBuf::from("/repo/alpha"),
                decision: workdeck_extension_host::TrustDecision::Trusted,
            })
        );

        app.reconcile_extension_trust_repo_root(Some(PathBuf::from("/repo/beta")));
        terminal
            .draw(|frame| render_embedded(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
        assert!(app.extension_trust_prompt_root().is_none());
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
        assert_eq!(old[0].fg, ratatui_theme_color(&theme.syntax_colors.default));
        assert_eq!(old[0].bg, ratatui_theme_color(&theme.removed_content_bg));
        assert_eq!(new[0].fg, ratatui_theme_color(&theme.syntax_colors.default));
        assert_eq!(new[0].bg, ratatui_theme_color(&theme.added_content_bg));
        assert_eq!(
            buffer.cell((99, 19)).unwrap().bg,
            ratatui_theme_color(&theme.panel_alt)
        );
        let workdeck = cells_matching_text(buffer, "Workdeck");
        assert_eq!(workdeck.len(), 1);
        assert_eq!(workdeck[0].fg, ratatui_theme_color(&theme.background));
        assert_eq!(workdeck[0].bg, ratatui_theme_color(&theme.accent));
    }

    #[test]
    fn explicit_row_plan_wraps_unicode_and_keeps_nowrap_to_one_physical_row() {
        let changeset = long_changeset();
        let mut highlights = HighlightedDiffRuntime::default();
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
        let mut highlights = HighlightedDiffRuntime::default();
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
        let mut highlights = HighlightedDiffRuntime::default();

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
        let mut highlights = HighlightedDiffRuntime::default();
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
        let mut highlights = HighlightedDiffRuntime::default();
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
        let mut highlights = HighlightedDiffRuntime::default();
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
    fn live_diff_cells_use_shared_palettes_and_selection_only_activates_rails() {
        let changeset = changeset();
        let line = &changeset.files[0].hunks[0].lines[0];
        let options = ReviewOptions {
            sidebar: false,
            line_numbers: true,
            highlight: false,
            cursor_line: CursorLineMode::Off,
            ..ReviewOptions::default()
        };
        let kind = RowCellKind::Deletion;
        let palette = stack_cell_palette(kind, &options.theme, false);

        let inactive = stack_line_rows(line, &options, None, &[], false, 80, None);
        let active = stack_line_rows(line, &options, None, &[], true, 80, None);
        assert_eq!(inactive.len(), 1);
        assert_eq!(active.len(), 1);
        assert_eq!(inactive[0].spans[0].content, diff_rail_marker());
        assert_eq!(
            inactive[0].spans[0].style.fg,
            Some(ratatui_theme_color(&stack_rail_color(
                kind,
                &options.theme,
                false,
            )))
        );
        assert_eq!(
            active[0].spans[0].style.fg,
            Some(ratatui_theme_color(&stack_rail_color(
                kind,
                &options.theme,
                true,
            )))
        );
        assert_eq!(
            inactive[0].spans[1].style.bg,
            Some(ratatui_theme_color(palette.gutter_background))
        );
        assert_eq!(
            inactive[0].spans[2].style.bg,
            Some(ratatui_theme_color(palette.content_background))
        );
        assert_eq!(inactive[0].spans[1].style, active[0].spans[1].style);
        assert_eq!(inactive[0].spans[2].style, active[0].spans[2].style);

        let empty = split_cell_lines(
            SplitCellInput {
                line: None,
                highlighted: None,
                emphasis: &[],
                line_highlights: None,
            },
            true,
            &options,
            40,
            false,
        );
        let empty_palette = split_cell_palette(RowCellKind::Empty, &options.theme, false);
        assert_eq!(empty[0][0].content, diff_rail_marker());
        assert_eq!(
            empty[0][1].style.bg,
            Some(ratatui_theme_color(empty_palette.gutter_background))
        );
        assert_eq!(
            empty[0].last().unwrap().style.bg,
            Some(ratatui_theme_color(empty_palette.content_background))
        );
    }

    #[test]
    fn native_cursor_paint_blends_each_ratatui_surface_and_number_mode_stops_at_the_gutter() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let prefix = Style::default().bg(ratatui_theme_color(&theme.panel));
        let gutter = Style::default().bg(ratatui_theme_color(&theme.line_number_bg));
        let code = Style::default().bg(ratatui_theme_color(&theme.added_bg));
        let mut number = Line::from(vec![
            Span::styled(diff_rail_marker(), prefix),
            Span::styled(" 1 + ", gutter),
            Span::styled("new", code),
        ]);
        paint_cursor_line(&mut number, CursorLineMode::Number, &theme);
        assert_eq!(
            number.spans[0].style.bg,
            Some(ratatui_theme_color(&cursor_line_highlight_background(
                &theme.panel,
                &theme,
            )))
        );
        assert_eq!(
            number.spans[1].style.bg,
            Some(ratatui_theme_color(&cursor_line_highlight_background(
                &theme.line_number_bg,
                &theme,
            )))
        );
        assert_eq!(number.spans[2].style.bg, code.bg);

        let mut row = Line::from(vec![
            Span::styled(diff_rail_marker(), prefix),
            Span::styled(" 1 + ", gutter),
            Span::styled("new", code),
        ]);
        paint_cursor_line(&mut row, CursorLineMode::Row, &theme);
        assert_eq!(
            row.spans[2].style.bg,
            Some(ratatui_theme_color(&cursor_line_highlight_background(
                &theme.added_bg,
                &theme,
            )))
        );
    }

    #[test]
    fn native_line_highlights_reach_stack_and_split_cell_buffers_after_word_paint() {
        use ratatui::widgets::{Paragraph, Widget};
        use workdeck_extension_api::{HighlightTone, ValidatedLineHighlight};

        let changeset = changeset();
        let file_id = changeset.files[0].runtime_id.clone();
        let line_highlights = LineHighlightMap::from_entries([(
            file_id,
            vec![ValidatedLineHighlight {
                side: ReviewSide::New,
                line: 1,
                start: 0,
                end: 2,
                tone: HighlightTone::Current,
            }],
        )]);
        let options = ReviewOptions {
            sidebar: false,
            line_numbers: false,
            highlight: false,
            ..ReviewOptions::default()
        };

        for layout in [LayoutMode::Stack, LayoutMode::Split] {
            let mut syntax = HighlightedDiffRuntime::default();
            let rows = build_review_rows_with_chrome(
                &changeset,
                &[],
                ReviewSelection::default(),
                layout,
                &options,
                80,
                &mut syntax,
                &BTreeSet::new(),
                &line_highlights,
                ReviewStreamChrome::default(),
                false,
                &BTreeMap::new(),
                &BTreeSet::new(),
            );
            let area = Rect::new(0, 0, 80, rows.lines.len() as u16);
            let mut buffer = Buffer::empty(area);
            Paragraph::new(rows.lines).render(area, &mut buffer);
            let mut new_start = None;
            for y in area.y..area.bottom() {
                for x in area.x..area.right().saturating_sub(2) {
                    if buffer.cell((x, y)).is_some_and(|cell| cell.symbol() == "n")
                        && buffer
                            .cell((x + 1, y))
                            .is_some_and(|cell| cell.symbol() == "e")
                        && buffer
                            .cell((x + 2, y))
                            .is_some_and(|cell| cell.symbol() == "w")
                    {
                        new_start = Some((x, y));
                    }
                }
            }
            let (x, y) = new_start.expect("the added line is present in the terminal buffer");
            for offset in 0..2 {
                let cell = buffer.cell((x + offset, y)).unwrap();
                assert_eq!(cell.bg, ratatui_theme_color(&options.theme.text));
                assert_eq!(cell.fg, ratatui_theme_color(&options.theme.background));
            }
            assert_ne!(
                buffer.cell((x + 2, y)).unwrap().bg,
                ratatui_theme_color(&options.theme.text)
            );
        }
    }

    #[test]
    fn stml_note_rows_preserve_styles_and_fall_back_when_markup_is_empty() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let styled = note_body_ratatui_lines(
            Some("<b><c fg=\"success\">ok</c></b>"),
            "fallback",
            20,
            &theme,
        );
        assert_eq!(styled.len(), 1);
        assert_eq!(styled[0].spans[0].content, "ok");
        assert!(
            styled[0].spans[0]
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
        assert_eq!(
            styled[0].spans[0].style.fg,
            Some(ratatui_theme_color(&theme.added_sign_color))
        );

        let fallback = note_body_ratatui_lines(
            Some("<!-- only a comment -->"),
            "Fallback summary",
            20,
            &theme,
        );
        assert_eq!(fallback.len(), 1);
        assert_eq!(fallback[0].spans[0].content, "Fallback summary");
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
                markup: Some("<box border title=\"flow\"><b>shape</b></box>".into()),
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
    fn user_note_composer_targets_the_cursor_line_and_saves_inline() {
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                layout: LayoutMode::Stack,
                ..ReviewOptions::default()
            },
        );
        let rows = app.current_review_rows();
        let (&cursor_row, &target) = rows.note_targets.iter().next().unwrap();
        app.current_line_row = cursor_row;
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        assert_eq!(app.note_composer.as_ref().unwrap().target, target);
        for character in "unicode note 😀".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        let draft = rendered_review_text(&mut terminal, &app);
        assert!(draft.contains("Draft note"));
        assert!(draft.contains("unicode note 😀"));

        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert!(app.note_composer.is_none());
        let comment = app.with_state(|state| state.comments()[0].clone());
        assert_eq!(comment.summary, "unicode note 😀");
        assert_eq!(comment.side, Some(target.side));
        assert_eq!(comment.line, Some(target.line));
        assert_eq!(comment.hunk_index, Some(target.hunk_index));
        assert!(comment.editable);
        assert!(rendered_review_text(&mut terminal, &app).contains("Your note"));
    }

    #[test]
    fn note_composer_supports_editing_paste_cancel_and_safe_cursor_geometry() {
        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        app.open_note_composer();
        app.handle_paste("ab😀");
        app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
        assert_eq!(app.note_composer.as_ref().unwrap().body, "a\nz😀");
        assert_eq!(note_composer_cursor_cell("ab\n😀", 4, 3, 4), (1, 2));
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.note_composer.is_none());
        assert!(app.with_state(|state| state.comments().is_empty()));

        app.open_note_composer();
        let composer = app.note_composer.as_mut().unwrap();
        composer.body = "x".repeat(workdeck_review::MAX_REVIEW_NOTE_BYTES - 1);
        composer.cursor = composer.body.len();
        app.handle_key(KeyEvent::new(KeyCode::Char('😀'), KeyModifiers::NONE));
        let composer = app.note_composer.as_ref().unwrap();
        assert_eq!(
            composer.body.len(),
            workdeck_review::MAX_REVIEW_NOTE_BYTES - 1
        );
        assert!(composer.body.is_char_boundary(composer.body.len()));
        assert_eq!(
            app.status.as_deref(),
            Some("review note reached the size limit")
        );
    }

    #[test]
    fn renders_and_resizes_declarative_native_extension_panes() {
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = ReviewApp::new(
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
                        replaces: None,
                        current_line: false,
                        available: false,
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

        let (key, divider) = {
            let runtime = app
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let planned = runtime.layout.panes.first().expect("planned pane");
            (planned.key.clone(), planned.divider.expect("pane divider"))
        };
        let inactive = terminal
            .backend()
            .buffer()
            .cell((divider.x, divider.y))
            .unwrap();
        assert_eq!(inactive.symbol(), "│");
        assert_eq!(inactive.fg, ratatui_theme_color(&app.options.theme.border));
        assert_eq!(inactive.fg, Color::Rgb(52, 57, 63));
        assert_eq!(inactive.bg, ratatui_theme_color(&app.options.theme.panel));
        assert_eq!(inactive.bg, Color::Rgb(30, 35, 41));

        let hit_column = divider.x.saturating_sub(PANE_DIVIDER_HIT_AREA_OFFSET);
        let hit_row = divider.y.saturating_add(1);
        app.extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pane_action_hits
            .push(ExtensionPaneActionHit {
                bounds: pane_divider_hit_area(divider, PanePlacement::Right),
                extension_index: usize::MAX,
                extension_id: "must-not-run".into(),
                pane_id: "must-not-run".into(),
                action_id: "must-not-run".into(),
            });
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: hit_column,
            row: hit_row,
            modifiers: KeyModifiers::NONE,
        });
        assert!(
            app.extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .resize
                .is_some()
        );
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        let active = terminal
            .backend()
            .buffer()
            .cell((divider.x, divider.y))
            .unwrap();
        assert_eq!(active.fg, ratatui_theme_color(&app.options.theme.accent));
        assert_eq!(active.fg, Color::Rgb(187, 128, 9));
        assert_eq!(
            active.bg,
            ratatui_theme_color(&app.options.theme.accent_muted)
        );
        assert_eq!(active.bg, Color::Rgb(57, 45, 20));

        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: hit_column.saturating_sub(3),
            row: hit_row,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(
            app.extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .size_overrides
                .get(&key),
            Some(&31)
        );
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: hit_column.saturating_sub(3),
            row: hit_row,
            modifiers: KeyModifiers::NONE,
        });
        assert!(
            !app.extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .resize
                .is_some()
        );
    }

    #[test]
    fn active_horizontal_extension_divider_uses_the_pinned_glyph_and_paint() {
        let app = ReviewApp::new(changeset(), ReviewOptions::default());
        app.extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .resize
            .capture((
                "probe".into(),
                PaneResizeState {
                    placement: PanePlacement::Bottom,
                    origin: 2,
                    start_size: 8,
                    min_size: 3,
                    max_size: 20,
                },
            ));
        let area = Rect::new(0, 0, 9, 1);
        let mut buffer = Buffer::empty(area);
        render_extension_pane_divider(area, &mut buffer, "probe", PanePlacement::Bottom, &app);

        assert_eq!(
            buffer
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>(),
            "─────────"
        );
        assert!(buffer.content().iter().all(|cell| {
            cell.fg == Color::Rgb(187, 128, 9) && cell.bg == Color::Rgb(57, 45, 20)
        }));
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
        app.handle_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE));
        assert_eq!(app.layout(), LayoutMode::Split);
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert!(!app.options.sidebar);
        app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
        assert!(app.show_help);
    }

    #[test]
    fn desktop_menu_bar_renders_and_dispatches_through_the_shared_command_table() {
        let backend = TestBackend::new(100, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
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
        for label in ["File", "View", "Navigate", "Agent", "Help", "Workdeck"] {
            assert!(rendered.contains(label), "missing menu label {label:?}");
        }

        app.handle_key(KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE));
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
        assert!(rendered.contains("Toggle files/filter focus"));
        assert!(rendered.contains("Open file in editor"));
        assert!(rendered.contains("Reload"));

        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.layout(), LayoutMode::Split);
        let menus = app.app_menus();
        assert_eq!(
            app.extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .menu
                .active_menu_id(&menus),
            None
        );

        let sidebar = app.options.sidebar;
        app.handle_key(KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert_eq!(app.options.sidebar, !sidebar);
        let menus = app.app_menus();
        assert_eq!(
            app.extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .menu
                .active_menu_id(&menus),
            None
        );
    }

    #[test]
    fn menu_mouse_hits_switch_highlight_and_activate_the_exact_row() {
        let backend = TestBackend::new(100, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 2,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        let bounds = app
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .menu_bounds
            .expect("open file menu bounds");
        let focus = app.focus;
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Moved,
            column: bounds.x + 1,
            row: bounds.y + 1,
            modifiers: KeyModifiers::NONE,
        });
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: bounds.x + 1,
            row: bounds.y + 1,
            modifiers: KeyModifiers::NONE,
        });
        assert_ne!(app.focus, focus);
    }

    #[test]
    fn menu_dropdown_renders_checks_hints_and_repositions_inside_a_narrow_terminal() {
        let backend = TestBackend::new(34, 22);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        app.handle_key(KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
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
        assert!(rendered.contains("[x] Automatic layout"));
        assert!(rendered.contains("[x] Files pane"));
        assert!(rendered.contains("[x] Line numbers"));
        assert!(rendered.contains("[ ] Line wrapping"));

        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        let bounds = app
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .menu_bounds
            .expect("agent menu bounds");
        assert!(bounds.right() <= 34);
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Next annotated file"));
        assert!(rendered.contains("Previous annotated file"));
    }

    #[test]
    fn resolved_command_keys_drive_runtime_help_and_unbind_the_shipped_chord() {
        let mut app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                keybindings: vec![UserKeyBindingEntry::new(
                    "workdeck.view.toggleFilesPane",
                    UserKeyBinding::Chord("x".into()),
                )],
                ..ReviewOptions::default()
            },
        );
        let sidebar = app.options.sidebar;
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert_eq!(app.options.sidebar, sidebar);
        app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(app.options.sidebar, !sidebar);
        assert_eq!(
            app.help_commands()
                .into_iter()
                .find(|command| command.id == "workdeck.view.toggleFilesPane")
                .unwrap()
                .key_labels,
            ["x"]
        );
    }

    #[test]
    fn extension_command_conflicts_are_absent_from_live_dispatch_and_menu_hints() {
        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        let registrations = vec![
            RegisteredExtensionCommand {
                extension_index: 0,
                extension_id: "meta".into(),
                command: workdeck_extension_api::CommandRegistration {
                    id: "steal-s".into(),
                    title: "Steal S".into(),
                    description: None,
                    default_keys: vec!["s".into()],
                },
            },
            RegisteredExtensionCommand {
                extension_index: 0,
                extension_id: "meta".into(),
                command: workdeck_extension_api::CommandRegistration {
                    id: "ok".into(),
                    title: "Safe command".into(),
                    description: None,
                    default_keys: vec!["y".into()],
                },
            },
        ];
        let table = build_extension_app_commands(
            &registrations,
            &builtin_command_match_probes(Some(&app.resolved_command_keys)),
            Some(&app.resolved_command_keys),
        );
        {
            let mut runtime = app
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            runtime.commands = registrations;
            runtime.app_commands = table.commands;
            runtime.command_conflicts = table.conflicts;
        }

        let menus = app.app_menus();
        let extension_items = menus.get(&MenuId::Extensions).unwrap();
        assert!(extension_items.iter().any(|entry| matches!(
            entry,
            MenuEntry::Item { label, hint: None, .. } if label == "Steal S"
        )));
        assert!(extension_items.iter().any(|entry| matches!(
            entry,
            MenuEntry::Item { label, hint: Some(hint), .. }
                if label == "Safe command" && hint == "y"
        )));
        assert!(
            !app.invoke_extension_command(&KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE,))
        );
    }

    #[test]
    fn config_keybinding_notices_share_the_runtime_diagnostic_surface() {
        let app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                keybinding_notices: vec![
                    "Ignored [keybindings] entries with unsupported values: bad.boolean.".into(),
                ],
                keybindings: vec![UserKeyBindingEntry::new(
                    "workdeck.app.quit",
                    UserKeyBinding::Chord("not-a-chord".into()),
                )],
                ..ReviewOptions::default()
            },
        );
        let status = app.status.as_deref().expect("keybinding diagnostics");
        assert!(status.contains("bad.boolean"));
        assert!(status.contains("not-a-chord"));
    }

    #[test]
    fn programmatic_extension_commands_share_the_catalog_and_keep_legacy_ids() {
        let mut app = ReviewApp::new(long_changeset(), ReviewOptions::default());
        assert!(app.execute_extension_review_command("workdeck.view.layoutStack", 1));
        assert_eq!(app.layout(), LayoutMode::Stack);
        assert!(app.execute_extension_review_command("workdeck.review.halfPageDown", 2));
        let moved = app.current_line_row;
        assert!(moved > 0);
        assert!(app.execute_extension_review_command("workdeck.review.half-page-up", 1));
        assert!(app.current_line_row < moved);
        assert!(!app.execute_extension_review_command("missing.command", 1));
    }

    #[test]
    fn native_command_context_projects_only_current_public_enablement_and_aliases() {
        let app = ReviewApp::new(changeset(), ReviewOptions::default());
        let commands = app.extension_command_availability();
        assert!(commands.is_enabled("workdeck.review.nextHunk"));
        assert!(commands.is_enabled("workdeck.review.next-hunk"));
        assert!(!commands.is_enabled("workdeck.missing"));

        let app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                cursor_line: CursorLineMode::Off,
                ..ReviewOptions::default()
            },
        );
        let commands = app.extension_command_availability();
        assert!(!commands.is_enabled("workdeck.review.alignCurrentLineCenter"));
        assert!(!commands.is_enabled("workdeck.review.align-current-line-center"));
    }

    #[test]
    fn native_navigation_actions_use_the_live_guard_and_clamped_hunk_target() {
        let mut app = ReviewApp::new(two_hunk_changeset(), ReviewOptions::default());
        let file_id = app.with_state(|state| state.changeset().files[0].runtime_id.clone());

        app.select_extension_review_hunk("triage", &file_id, 99);
        assert_eq!(
            app.with_state(|state| state.selection().hunk_index),
            Some(1)
        );

        app.reveal_extension_review_line("triage", &file_id, ReviewSide::New, 10);
        assert_eq!(app.with_state(|state| state.selection().line), Some(10));

        app.reveal_extension_review_line("triage", &file_id, ReviewSide::New, 9_001);
        assert_eq!(
            app.status.as_deref(),
            Some(
                format!("Extension triage revealLine found no new line 9001 in \"{file_id}\"")
                    .as_str()
            )
        );

        app.select_extension_review_file("triage", "hidden");
        assert_eq!(
            app.status.as_deref(),
            Some("Extension triage selectFile targeted unknown file id \"hidden\"")
        );
    }

    #[test]
    fn native_line_highlight_refresh_actions_update_live_epochs_and_notices() {
        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        let file_id = app.with_state(|state| state.changeset().files[0].runtime_id.clone());
        {
            let mut runtime = app
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            runtime.line_highlights = LineHighlightsController::new(
                [file_id.clone()],
                vec![RegisteredLineHighlighter {
                    extension_index: 0,
                    extension_id: "search".into(),
                    highlighter_id: "matches".into(),
                }],
            );
        }

        app.apply_extension_actions(
            0,
            "other-extension",
            vec![ExtensionHostAction::RefreshLineHighlights {
                id: "search:matches".into(),
                file_id: None,
            }],
        );
        let epoch = app
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .line_highlights
            .epochs()
            .clone();
        assert_eq!(
            workdeck_extension_host::scoped_epoch(&epoch, "search:matches", &file_id),
            1
        );

        app.status = None;
        app.apply_extension_actions(
            0,
            "search",
            vec![ExtensionHostAction::RefreshLineHighlights {
                id: "matches".into(),
                file_id: Some("gone".into()),
            }],
        );
        assert_eq!(app.status, None);

        app.apply_extension_actions(
            0,
            "search",
            vec![ExtensionHostAction::RefreshLineHighlights {
                id: "unknown".into(),
                file_id: None,
            }],
        );
        assert_eq!(
            app.status.as_deref(),
            Some("Extension search targeted unknown line highlighter \"unknown\"")
        );
    }

    #[test]
    fn async_navigation_from_a_retired_review_generation_is_discarded() {
        let mut app = ReviewApp::new(two_file_changeset(), ReviewOptions::default());
        let second_file_id = app.with_state(|state| state.changeset().files[1].runtime_id.clone());
        let pending = PendingExtensionCommand {
            extension_index: 0,
            extension_id: "triage".into(),
            command_id: "jump".into(),
            title: "Jump".into(),
            review_generation: app.extension_command_epoch,
        };
        app.extension_command_epoch = app.extension_command_epoch.saturating_add(1);

        app.apply_extension_command_actions(
            pending,
            vec![ExtensionHostAction::SelectReviewFile {
                file_id: second_file_id,
            }],
        );

        assert_eq!(app.with_state(|state| state.selection().file_index), 0);
        assert_eq!(
            app.status.as_deref(),
            Some("Extension triage selectFile ignored — the review session was reloaded")
        );
    }

    #[test]
    fn extension_command_failures_are_contained_and_reported_as_attributed_warnings() {
        let notifications = ExtensionNotificationHub::new();
        let mut app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                extension_notifications: Some(notifications),
                ..ReviewOptions::default()
            },
        );

        app.report_extension_command_failure("probe", "run", "context boom");
        let expected = "Extension probe failed command \"run\" • context boom";
        assert_eq!(app.status.as_deref(), Some(expected));
        let notification = app.active_extension_notification().unwrap();
        assert_eq!(notification.message, expected);
        assert_eq!(notification.notification_type, ExtensionNotifyType::Warning);
        assert_eq!(
            extension_command_failure_message("probe", "run", "async boom"),
            "Extension probe failed command \"run\" • async boom"
        );
    }

    #[test]
    fn editor_shortcut_prepares_an_owned_request_and_context_expansion_uses_z() {
        let mut app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                repo: Some(PathBuf::from("/repo")),
                ..ReviewOptions::default()
            },
        );
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        let request = app.take_editor_request().expect("editor request");
        assert_eq!(request.base_path, Path::new("/repo"));
        assert_eq!(request.file.unwrap().path, "a.rs");
        assert_eq!(request.selected_hunk.unwrap().new_start, 1);
        assert!(request.line_cursor.is_none());
        assert!(app.take_editor_request().is_none());

        app.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
        assert_eq!(
            app.status.as_deref(),
            Some("source expansion is unavailable for this file")
        );
        assert!(app.take_editor_request().is_none());
    }

    #[test]
    fn review_help_overlay_renders_the_command_derived_sections_and_rows() {
        let backend = TestBackend::new(100, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_help(frame.area(), frame.buffer_mut(), &default_help_commands());
            })
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
    fn review_file_header_mouse_up_selects_without_moving_the_visible_stream() {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = ReviewApp::new(
            two_file_changeset(),
            ReviewOptions {
                sidebar: false,
                ..ReviewOptions::default()
            },
        );
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        let second = app
            .review_file_header_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .find(|hit| hit.file_index == 1)
            .copied()
            .expect("second review file header hit");

        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: second.bounds.x + 5,
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
        assert_eq!(app.scroll, 0);
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
    fn shifted_and_native_horizontal_wheel_events_never_move_the_vertical_viewport() {
        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        let start = Instant::now();
        app.handle_mouse_at(MouseEventKind::ScrollDown, start);
        app.handle_mouse_at(
            MouseEventKind::ScrollDown,
            start + Duration::from_millis(50),
        );
        let vertical = app.scroll;
        assert!(app.mouse_scroll_accumulator > 0.0);

        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 20,
            row: 5,
            modifiers: KeyModifiers::SHIFT,
        });
        assert_eq!(app.options.horizontal_offset, 1);
        assert_eq!(app.scroll, vertical);
        assert_eq!(app.mouse_scroll_accumulator, 0.0);

        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::ScrollRight,
            column: 20,
            row: 5,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.options.horizontal_offset, 2);
        assert_eq!(app.scroll, vertical);

        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::ScrollLeft,
            column: 20,
            row: 5,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.options.horizontal_offset, 1);
        assert_eq!(app.scroll, vertical);
    }

    #[test]
    fn wrapped_review_leaves_shifted_wheel_available_for_vertical_scrolling() {
        let mut app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                wrap_lines: true,
                ..ReviewOptions::default()
            },
        );
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 20,
            row: 5,
            modifiers: KeyModifiers::SHIFT,
        });
        assert_eq!(app.options.horizontal_offset, 0);
        assert_eq!(app.scroll, 1);
    }

    #[test]
    fn nested_row_mouse_action_claims_parent_selection_event() {
        let mut app = ReviewApp::new(two_file_changeset(), ReviewOptions::default());
        let bounds = Rect::new(10, 4, 12, 1);
        let state_key = FileViewComponentStateKey {
            file_id: "file-0".into(),
            row_id: "nested-action".into(),
        };
        app.extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .file_view_component_hits
            .push(FileViewComponentHit {
                state_key: state_key.clone(),
                bounds,
            });
        app.review_file_header_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(SidebarFileHit {
                bounds,
                file_index: 1,
            });

        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: bounds.x,
            row: bounds.y,
            modifiers: KeyModifiers::NONE,
        });
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: bounds.x,
            row: bounds.y,
            modifiers: KeyModifiers::NONE,
        });

        assert!(
            app.extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .file_view_component_expanded
                .contains(&state_key)
        );
        assert_eq!(
            app.shared_state()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .selection()
                .file_index,
            0
        );
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
    fn extension_toast_matches_the_one_row_theme_and_padding_contract() {
        let hub = ExtensionNotificationHub::new();
        hub.notify("loaded", ExtensionNotifyType::Error);
        let mut app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                extension_notifications: Some(hub),
                ..ReviewOptions::default()
            },
        );
        app.tick_extension_notifications(Instant::now());

        let area = Rect::new(0, 0, 20, 1);
        let mut buffer = Buffer::empty(area);
        render_extension_toast(area, &mut buffer, &app);

        let panel_alt = ratatui_theme_color(&app.options.theme.panel_alt);
        let muted = ratatui_theme_color(&app.options.theme.muted);
        let error = ratatui_theme_color(&app.options.theme.badge_removed);
        assert_eq!(panel_alt, Color::Rgb(39, 43, 49));
        assert_eq!(muted, Color::Rgb(173, 174, 177));
        assert_eq!(error, Color::Rgb(250, 142, 137));
        let symbols = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert_eq!(symbols, " ext loaded         ");
        assert!(buffer.content().iter().all(|cell| cell.bg == panel_alt));
        for x in 1..=3 {
            let cell = buffer.cell((x, 0)).unwrap();
            assert_eq!(cell.fg, error);
            assert!(!cell.modifier.contains(Modifier::BOLD));
        }
        for x in 4..=10 {
            assert_eq!(buffer.cell((x, 0)).unwrap().fg, muted);
        }
        assert_eq!(buffer.cell((0, 0)).unwrap().symbol(), " ");
        assert_eq!(buffer.cell((0, 0)).unwrap().fg, Color::Rgb(255, 255, 255));
        assert_eq!(buffer.cell((19, 0)).unwrap().symbol(), " ");
        assert_eq!(buffer.cell((19, 0)).unwrap().fg, Color::Rgb(255, 255, 255));
    }

    #[test]
    fn status_bar_prioritizes_startup_notices_then_reveals_buffered_extension_output() {
        let hub = ExtensionNotificationHub::new();
        hub.notify("factory ready", ExtensionNotifyType::Info);
        let mut app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                startup_notices: vec![StartupNotice::new(
                    "extension:broken",
                    "Extension broken failed to load • boom",
                )],
                extension_notifications: Some(hub),
                ..ReviewOptions::default()
            },
        );
        app.set_status("ordinary status");
        let area = Rect::new(0, 0, 60, 1);
        let mut startup = Buffer::empty(area);
        render_footer(area, &mut startup, &app);
        let startup_text = startup
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(startup_text.contains("Extension broken failed to load • boom"));
        assert!(!startup_text.contains("factory ready"));

        app.tick_extension_notifications(Instant::now() + Duration::from_secs(8));
        let mut notification = Buffer::empty(area);
        render_footer(area, &mut notification, &app);
        let notification_text = notification
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(notification_text.contains("ext factory ready"));
        assert!(!notification_text.contains("ordinary status"));
    }

    #[test]
    fn status_bar_matches_notice_filter_and_mode_precedence_frames() {
        fn footer(app: &ReviewApp) -> Buffer {
            let area = Rect::new(0, 0, 40, 1);
            let mut buffer = Buffer::empty(area);
            render_footer(area, &mut buffer, app);
            buffer
        }
        fn text(buffer: &Buffer) -> String {
            buffer.content().iter().map(|cell| cell.symbol()).collect()
        }

        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        app.set_status("Update available");
        let notice = footer(&app);
        assert_eq!(text(&notice), " Update available                       ");
        assert!(
            notice
                .content()
                .iter()
                .all(|cell| cell.bg == Color::Rgb(39, 43, 49))
        );
        for x in 1..=16 {
            assert_eq!(notice.cell((x, 0)).unwrap().fg, Color::Rgb(173, 174, 177));
        }

        app.filter = "beta".into();
        app.filter_cursor = 4;
        app.focus = Focus::Filter;
        let focused = footer(&app);
        assert_eq!(text(&focused), " filter: beta                           ");
        for x in 1..=8 {
            assert_eq!(focused.cell((x, 0)).unwrap().fg, Color::Rgb(173, 174, 177));
        }
        for x in 9..=12 {
            assert_eq!(focused.cell((x, 0)).unwrap().fg, Color::Rgb(255, 255, 255));
        }

        app.filter = "0123456789abcdefghijklmnopqrstuvwxyz".into();
        app.filter_cursor = app.filter.chars().count();
        app.filter_scroll.set(0);
        let long_focused = footer(&app);
        assert_eq!(
            text(&long_focused),
            " filter: defghijklmnopqrstuvwxyz        "
        );
        assert_eq!(app.filter_scroll.get(), 13);
        assert_eq!(
            app.status_filter_cursor_position(Rect::new(0, 0, 40, 1)),
            Some(Position::new(32, 0))
        );

        app.filter.clear();
        app.filter_cursor = 0;
        app.filter_scroll.set(0);
        let empty_focused = footer(&app);
        assert_eq!(
            text(&empty_focused),
            " filter: type to filter files           "
        );
        for x in 9..=28 {
            assert_eq!(
                empty_focused.cell((x, 0)).unwrap().fg,
                Color::Rgb(102, 102, 102)
            );
        }

        app.filter = "beta".into();
        app.focus = Focus::Review;
        let summary = footer(&app);
        assert_eq!(text(&summary), " filter=beta                            ");
        assert!(!text(&summary).contains("Update available"));

        app.filter.clear();
        let registered = Arc::new(RegisteredKeyboardMode {
            extension_id: "vim".into(),
            mode: KeyboardModeRegistration {
                id: "normal".into(),
                title: "Vim navigation".into(),
            },
        });
        app.extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_keyboard_mode = Some(ActiveKeyboardMode {
            extension_index: 0,
            extension_id: "vim".into(),
            mode: registered.mode.clone(),
            activation_id: 1,
            session: ActiveSessionKeyboardMode {
                extension_id: "vim".into(),
                mode_id: "normal".into(),
                registered,
                registry: Arc::new(workdeck_extension_host::ExtensionRuntimeRegistry::new()),
            },
        });
        let mode = footer(&app);
        assert_eq!(text(&mode), " Update available   Vim navigation —    ");
        let bounds = app
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .mode_badge_bounds
            .expect("mode badge bounds");
        assert_eq!(bounds, Rect::new(19, 0, 20, 1));
        for x in bounds.x..bounds.right() {
            assert_eq!(mode.cell((x, 0)).unwrap().bg, Color::Rgb(173, 174, 177));
        }

        let mut light = ReviewApp::new(
            changeset(),
            ReviewOptions {
                theme: resolve_theme(Some("github-light-default"), None, &[]),
                ..ReviewOptions::default()
            },
        );
        light.filter = "beta".into();
        light.filter_cursor = 4;
        light.focus = Focus::Filter;
        let light_frame = footer(&light);
        assert_eq!(
            text(&light_frame),
            " filter: beta                           "
        );
        assert_eq!(
            light_frame.cell((0, 0)).unwrap().bg,
            Color::Rgb(237, 237, 238)
        );
        assert_eq!(light_frame.cell((1, 0)).unwrap().fg, Color::Rgb(90, 90, 90));
        assert_eq!(
            light_frame.cell((9, 0)).unwrap().fg,
            Color::Rgb(255, 255, 255)
        );
    }

    #[test]
    fn focused_status_filter_owns_editing_and_escape_clears_before_exit() {
        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert_eq!(app.focus, Focus::Filter);
        for character in ['b', '界'] {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert_eq!(app.filter, "ba界");
        app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
        assert_eq!(app.filter, "b");

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.filter.is_empty());
        assert_eq!(app.focus, Focus::Filter);
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.focus, Focus::Review);

        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.filter, "z");
        assert_eq!(app.focus, Focus::Review);
    }

    #[test]
    fn status_bar_mouse_up_closes_an_open_application_menu() {
        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        app.handle_key(KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE));
        let menus = app.app_menus();
        assert!(
            app.extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .menu
                .active_menu_id(&menus)
                .is_some()
        );

        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 39,
            row: 19,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(
            app.extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .menu
                .active_menu_id(&menus),
            None
        );
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
